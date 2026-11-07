//! Router port forwarding (UPnP IGD), so players outside the LAN can reach
//! the console.
//!
//! On `Start` the server thread asks the local router to forward the listen
//! port to this machine. The request runs in the background and never delays
//! or fails the server: the LAN URL is published at once and the internet URL
//! is added when (and if) the router grants the mapping. The background task
//! renews the lease and keeps retrying while no router answers; `Stop`
//! removes the mapping.
//!
//! # Why a small client of our own
//!
//! Discovery is an SSDP multicast. A socket bound to `0.0.0.0` lets Windows
//! pick the outgoing interface from the routing table, and with a VPN up
//! (`0.0.0.0/1` + `128.0.0.0/1` through the tunnel — the normal setup of most
//! VPN clients) the M-SEARCH goes into the tunnel and the router never sees
//! it. The generic crate used before did exactly that, so port forwarding
//! silently failed on every machine with a VPN. Here discovery is sent from
//! every private IPv4 address of the machine, with the multicast interface
//! pinned to it, plus a unicast probe to the usual gateway address.
//!
//! A granted mapping is not always reachable from the internet: behind a
//! second NAT (carrier-grade NAT, a provider modem in front of the router) the
//! router's external address is itself private. That case is reported as
//! [`UpnpStatus::DoubleNat`] instead of advertising a URL nobody can open.

use std::net::{IpAddr, Ipv4Addr, SocketAddr, SocketAddrV4};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpStream, UdpSocket};
use tokio::sync::oneshot;
use tokio::task::JoinHandle;

/// Lease asked of the router; the task renews it at half-time.
const LEASE: Duration = Duration::from_secs(2 * 60 * 60);

/// How long to wait before asking again when no router granted a mapping.
const RETRY_INTERVAL: Duration = Duration::from_secs(30);

/// How long SSDP answers are collected on each interface.
const DISCOVERY_WAIT: Duration = Duration::from_millis(1500);

/// Deadline for one HTTP exchange with the router.
const HTTP_TIMEOUT: Duration = Duration::from_secs(4);

/// Description shown in the router's port-forwarding table.
const DESCRIPTION: &str = "Bardwave";

const SSDP_ADDR: SocketAddrV4 = SocketAddrV4::new(Ipv4Addr::new(239, 255, 255, 250), 1900);

/// Search targets, most specific first. Some routers only answer the exact
/// device type they implement.
const SEARCH_TARGETS: [&str; 3] = [
    "urn:schemas-upnp-org:device:InternetGatewayDevice:1",
    "urn:schemas-upnp-org:device:InternetGatewayDevice:2",
    "urn:schemas-upnp-org:service:WANIPConnection:1",
];

/// UPnP error: another host already has this external port.
const CONFLICT_IN_MAPPING_ENTRY: u16 = 718;
/// UPnP error: the router only accepts permanent (lease 0) mappings.
const ONLY_PERMANENT_LEASES_SUPPORTED: u16 = 725;

/// Where port forwarding stands, as shown to the operator.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum UpnpStatus {
    /// Disabled in the config, not applicable to the bind address, or the
    /// server is stopped.
    #[default]
    Off,
    /// Looking for a router; no answer yet.
    Searching,
    /// The router forwards `external` to us and it is a public address.
    Mapped {
        external: SocketAddrV4,
        /// URL players outside the LAN open.
        url: String,
    },
    /// The router granted `external`, but that address is private: there is
    /// another NAT in front of the router and the port is not reachable from
    /// the internet.
    DoubleNat { external: SocketAddrV4 },
    /// No router granted a mapping; the reason of the last attempt. The
    /// background task keeps retrying.
    Unavailable(String),
}

impl UpnpStatus {
    /// Internet URL, when there is one to advertise.
    pub fn url(&self) -> Option<&str> {
        match self {
            UpnpStatus::Mapped { url, .. } => Some(url),
            _ => None,
        }
    }
}

/// What the operator should see for a mapping the router granted.
pub fn describe_external(external: SocketAddrV4) -> UpnpStatus {
    if is_public_ipv4(*external.ip()) {
        UpnpStatus::Mapped {
            external,
            url: format!("http://{external}"),
        }
    } else {
        UpnpStatus::DoubleNat { external }
    }
}

/// A running port-forwarding task. Keep it for as long as the server runs and
/// hand it to [`release`] on stop.
pub struct PortMapping {
    status: Arc<Mutex<UpnpStatus>>,
    stop: Option<oneshot::Sender<()>>,
    task: JoinHandle<()>,
}

impl PortMapping {
    /// Current state of the mapping.
    pub fn status(&self) -> UpnpStatus {
        self.status.lock().unwrap().clone()
    }
}

impl Drop for PortMapping {
    fn drop(&mut self) {
        // Dropped without `release`: still ask the task to remove the mapping.
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
    }
}

/// Starts forwarding `port` (TCP) on the router, if the bind address can be
/// forwarded at all.
///
/// Returns `None` for IPv6 and loopback binds: routers only forward IPv4, and
/// a loopback socket is unreachable from anywhere else anyway.
///
/// Must be called inside a Tokio runtime.
pub fn start(bind_addr: IpAddr, port: u16) -> Option<PortMapping> {
    let IpAddr::V4(v4) = bind_addr else {
        tracing::info!("Port forwarding skipped: routers forward IPv4 only ({bind_addr})");
        return None;
    };
    if v4.is_loopback() {
        tracing::info!("Port forwarding skipped: bound to loopback ({bind_addr})");
        return None;
    }
    if port == 0 {
        return None;
    }

    tracing::info!("Asking the router to forward TCP port {port} (UPnP IGD)");
    let status = Arc::new(Mutex::new(UpnpStatus::Searching));
    let (stop_tx, stop_rx) = oneshot::channel();
    // A specific bind address is the only interface the mapping may point at.
    let only_from = (!v4.is_unspecified()).then_some(v4);
    let task = tokio::spawn(run(port, only_from, Arc::clone(&status), stop_rx));
    Some(PortMapping {
        status,
        stop: Some(stop_tx),
        task,
    })
}

/// Removes the mapping from the router; returns within a few seconds.
pub async fn release(mut mapping: PortMapping) {
    if let Some(stop) = mapping.stop.take() {
        let _ = stop.send(());
    }
    let task = &mut mapping.task;
    if tokio::time::timeout(Duration::from_secs(5), task)
        .await
        .is_err()
    {
        tracing::warn!("Router did not confirm removing the port mapping in time");
        mapping.task.abort();
    }
}

/// The mapping task: discover, map, renew, and delete on stop.
async fn run(
    port: u16,
    only_from: Option<Ipv4Addr>,
    status: Arc<Mutex<UpnpStatus>>,
    mut stop: oneshot::Receiver<()>,
) {
    let mut granted: Option<(Gateway, SocketAddrV4)> = None;
    loop {
        let preferred_external = granted.as_ref().map(|(_, ext)| ext.port()).unwrap_or(port);
        let wait = match establish(port, preferred_external, only_from).await {
            Ok((gateway, external, lease)) => {
                let next = describe_external(external);
                if *status.lock().unwrap() != next {
                    tracing::info!(
                        "Router {} forwards {external} -> {}:{port}",
                        gateway.control.host,
                        gateway.local_ip
                    );
                }
                *status.lock().unwrap() = next;
                granted = Some((gateway, external));
                if lease == 0 {
                    // Permanent mapping: just check it is still there now and then.
                    Duration::from_secs(30 * 60)
                } else {
                    Duration::from_secs(u64::from(lease) / 2).max(Duration::from_secs(60))
                }
            }
            Err(reason) => {
                tracing::debug!("Port forwarding attempt failed: {reason}");
                *status.lock().unwrap() = UpnpStatus::Unavailable(reason);
                RETRY_INTERVAL
            }
        };

        tokio::select! {
            _ = tokio::time::sleep(wait) => {}
            _ = &mut stop => break,
        }
    }

    if let Some((gateway, external)) = granted {
        let args = [
            ("NewRemoteHost", String::new()),
            ("NewExternalPort", external.port().to_string()),
            ("NewProtocol", "TCP".to_string()),
        ];
        match soap(&gateway, "DeletePortMapping", &args).await {
            Ok(_) => tracing::info!("Router port mapping {external} removed"),
            Err(e) => tracing::warn!("Could not remove the router port mapping: {e}"),
        }
    }
    *status.lock().unwrap() = UpnpStatus::Off;
}

/// A router's WAN connection service, reached from one of our addresses.
#[derive(Debug, Clone)]
struct Gateway {
    service_type: String,
    control: HttpUrl,
    /// Our address on the router's LAN (the mapping's internal client).
    local_ip: Ipv4Addr,
}

/// Find a router on any LAN interface and ask it for the mapping.
///
/// Returns the gateway, the external address and the lease actually granted.
async fn establish(
    port: u16,
    preferred_external: u16,
    only_from: Option<Ipv4Addr>,
) -> Result<(Gateway, SocketAddrV4, u32), String> {
    let interfaces: Vec<Ipv4Addr> = match only_from {
        Some(ip) => vec![ip],
        None => lan_ipv4_addresses(),
    };
    if interfaces.is_empty() {
        return Err("no LAN IPv4 address on this machine".into());
    }

    let mut last_error = String::from("no UPnP router answered");
    for local in interfaces {
        let locations = match discover(local).await {
            Ok(found) => found,
            Err(e) => {
                last_error = format!("discovery on {local} failed: {e}");
                continue;
            }
        };
        for location in locations {
            let gateways = match gateways_at(&location, local).await {
                Ok(found) => found,
                Err(e) => {
                    last_error = format!("{location}: {e}");
                    continue;
                }
            };
            for gateway in gateways {
                match map_port(&gateway, port, preferred_external).await {
                    Ok((external, lease)) => return Ok((gateway, external, lease)),
                    Err(e) => last_error = format!("router refused the mapping: {e}"),
                }
            }
        }
    }
    Err(last_error)
}

/// Private IPv4 addresses of this machine, LAN-looking ones first.
fn lan_ipv4_addresses() -> Vec<Ipv4Addr> {
    let mut found: Vec<Ipv4Addr> = super::local_ip::candidates()
        .into_iter()
        .filter_map(|ip| match ip {
            IpAddr::V4(v4) if v4.is_private() => Some(v4),
            _ => None,
        })
        .collect();
    // Same preference as the LAN URL: 192.168/16, then 10/8, then 172.16/12
    // (where Docker, WSL and many VPN adapters live).
    found.sort_by_key(|ip| match ip.octets()[0] {
        192 => 0,
        10 => 1,
        _ => 2,
    });
    found.dedup();
    found
}

/// SSDP search from one local address; returns the description URLs heard.
async fn discover(local: Ipv4Addr) -> std::io::Result<Vec<String>> {
    use socket2::{Domain, Protocol, Socket, Type};

    let socket = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))?;
    socket.bind(&SocketAddr::V4(SocketAddrV4::new(local, 0)).into())?;
    // Pin the multicast interface: without this the M-SEARCH follows the
    // routing table, i.e. into a VPN tunnel when one is up.
    socket.set_multicast_if_v4(&local)?;
    socket.set_multicast_ttl_v4(2)?;
    socket.set_nonblocking(true)?;
    let socket = UdpSocket::from_std(socket.into())?;

    // The usual gateway address of a /24 LAN, for routers that ignore
    // multicast but answer a unicast M-SEARCH.
    let [a, b, c, _] = local.octets();
    let gateway_guess = SocketAddrV4::new(Ipv4Addr::new(a, b, c, 1), 1900);

    for target in SEARCH_TARGETS {
        let request = format!(
            "M-SEARCH * HTTP/1.1\r\nHOST: 239.255.255.250:1900\r\nMAN: \"ssdp:discover\"\r\nMX: 1\r\nST: {target}\r\n\r\n"
        );
        let _ = socket.send_to(request.as_bytes(), SSDP_ADDR).await;
        if gateway_guess.ip() != &local {
            let _ = socket.send_to(request.as_bytes(), gateway_guess).await;
        }
    }

    let mut found = Vec::new();
    let mut buf = [0u8; 2048];
    let deadline = tokio::time::Instant::now() + DISCOVERY_WAIT;
    while let Ok(Ok((len, _))) = tokio::time::timeout_at(deadline, socket.recv_from(&mut buf)).await
    {
        if let Some(location) = ssdp_location(&buf[..len]) {
            if !found.contains(&location) {
                found.push(location);
            }
        }
    }
    Ok(found)
}

/// The `LOCATION` header of an SSDP answer that comes from a gateway.
fn ssdp_location(answer: &[u8]) -> Option<String> {
    let text = std::str::from_utf8(answer).ok()?;
    let mut lines = text.split("\r\n");
    let status = lines.next()?;
    if !status.starts_with("HTTP/1.1 200") && !status.starts_with("HTTP/1.0 200") {
        return None;
    }
    let mut location = None;
    let mut is_gateway = false;
    for line in lines {
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        let value = value.trim();
        match name.trim().to_ascii_lowercase().as_str() {
            "location" => location = Some(value.to_string()),
            "st" | "usn"
                if (value.contains("InternetGatewayDevice")
                    || value.contains("WANIPConnection")
                    || value.contains("WANPPPConnection")) =>
            {
                is_gateway = true;
            }
            _ => {}
        }
    }
    location.filter(|_| is_gateway)
}

/// WAN connection services described at `location`.
async fn gateways_at(location: &str, local: Ipv4Addr) -> Result<Vec<Gateway>, String> {
    let url = HttpUrl::parse(location).ok_or("unsupported description URL")?;
    let (status, body) = http(&url, "GET", &[], "").await?;
    if status != 200 {
        return Err(format!("description returned HTTP {status}"));
    }
    let services = wan_services(&body, &url)?;
    if services.is_empty() {
        return Err("the router describes no WAN connection service".into());
    }
    Ok(services
        .into_iter()
        .map(|(service_type, control)| Gateway {
            service_type,
            control,
            local_ip: local,
        })
        .collect())
}

/// `(serviceType, controlURL)` of every WANIP/WANPPP connection service,
/// IP connections first.
fn wan_services(description: &str, location: &HttpUrl) -> Result<Vec<(String, HttpUrl)>, String> {
    let doc =
        roxmltree::Document::parse(description).map_err(|e| format!("bad description XML: {e}"))?;
    let base = doc
        .descendants()
        .find(|n| n.has_tag_name("URLBase"))
        .and_then(|n| n.text())
        .and_then(|t| HttpUrl::parse(t.trim()))
        .unwrap_or_else(|| location.clone());

    let mut services = Vec::new();
    for service in doc.descendants().filter(|n| n.has_tag_name("service")) {
        let child = |name: &str| {
            service
                .children()
                .find(|c| c.has_tag_name(name))
                .and_then(|c| c.text())
                .map(|t| t.trim().to_string())
        };
        let (Some(kind), Some(control)) = (child("serviceType"), child("controlURL")) else {
            continue;
        };
        if !(kind.contains(":WANIPConnection:") || kind.contains(":WANPPPConnection:")) {
            continue;
        }
        let control = if control.starts_with("http://") {
            match HttpUrl::parse(&control) {
                Some(url) => url,
                None => continue,
            }
        } else {
            base.join(&control)
        };
        services.push((kind, control));
    }
    services.sort_by_key(|(kind, _)| !kind.contains("WANIPConnection"));
    Ok(services)
}

/// Ask for the mapping; returns the external address and the lease granted.
async fn map_port(
    gateway: &Gateway,
    port: u16,
    preferred_external: u16,
) -> Result<(SocketAddrV4, u32), String> {
    let external_ip: Ipv4Addr = soap(gateway, "GetExternalIPAddress", &[])
        .await?
        .get("NewExternalIPAddress")
        .and_then(|ip| ip.parse().ok())
        .ok_or("the router reported no external address")?;
    if external_ip.is_unspecified() {
        return Err("the router has no external address (WAN down?)".into());
    }

    let mut lease = LEASE.as_secs() as u32;
    let mut external_port = preferred_external;
    let mut conflicts = 0;
    loop {
        let args = [
            ("NewRemoteHost", String::new()),
            ("NewExternalPort", external_port.to_string()),
            ("NewProtocol", "TCP".to_string()),
            ("NewInternalPort", port.to_string()),
            ("NewInternalClient", gateway.local_ip.to_string()),
            ("NewEnabled", "1".to_string()),
            ("NewPortMappingDescription", DESCRIPTION.to_string()),
            ("NewLeaseDuration", lease.to_string()),
        ];
        match soap(gateway, "AddPortMapping", &args).await {
            Ok(_) => return Ok((SocketAddrV4::new(external_ip, external_port), lease)),
            Err(SoapError::Refused(ONLY_PERMANENT_LEASES_SUPPORTED, _)) if lease != 0 => lease = 0,
            Err(SoapError::Refused(CONFLICT_IN_MAPPING_ENTRY, _)) if conflicts < 4 => {
                // Another machine already forwards this port: take another.
                conflicts += 1;
                external_port = 20_000 + (std::process::id() as u16 % 20_000) + conflicts * 977;
            }
            Err(e) => return Err(e.to_string()),
        }
    }
}

#[derive(Debug)]
enum SoapError {
    /// The router answered with a UPnP error code.
    Refused(u16, String),
    Other(String),
}

impl std::fmt::Display for SoapError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SoapError::Refused(code, text) => write!(f, "UPnP error {code} {text}"),
            SoapError::Other(text) => f.write_str(text),
        }
    }
}

impl From<String> for SoapError {
    fn from(text: String) -> Self {
        SoapError::Other(text)
    }
}

impl From<SoapError> for String {
    fn from(e: SoapError) -> Self {
        e.to_string()
    }
}

/// One SOAP action; returns the text of every leaf element of the reply.
async fn soap(
    gateway: &Gateway,
    action: &str,
    args: &[(&str, String)],
) -> Result<std::collections::HashMap<String, String>, SoapError> {
    let mut body = format!(
        "<?xml version=\"1.0\"?>\r\n<s:Envelope xmlns:s=\"http://schemas.xmlsoap.org/soap/envelope/\" \
         s:encodingStyle=\"http://schemas.xmlsoap.org/soap/encoding/\"><s:Body><u:{action} xmlns:u=\"{}\">",
        gateway.service_type
    );
    for (name, value) in args {
        body.push_str(&format!("<{name}>{}</{name}>", xml_escape(value)));
    }
    body.push_str(&format!("</u:{action}></s:Body></s:Envelope>\r\n"));

    let soap_action = format!("\"{}#{action}\"", gateway.service_type);
    let headers = [
        ("Content-Type", "text/xml; charset=\"utf-8\""),
        ("SOAPAction", soap_action.as_str()),
    ];
    let (status, reply) = http(&gateway.control, "POST", &headers, &body).await?;
    let fields = xml_leaves(&reply);
    if status == 200 {
        return Ok(fields);
    }
    match fields.get("errorCode").and_then(|c| c.parse().ok()) {
        Some(code) => Err(SoapError::Refused(
            code,
            fields.get("errorDescription").cloned().unwrap_or_default(),
        )),
        None => Err(SoapError::Other(format!("{action} returned HTTP {status}"))),
    }
}

/// Name -> text of every element that has text and no element children.
fn xml_leaves(xml: &str) -> std::collections::HashMap<String, String> {
    let mut out = std::collections::HashMap::new();
    if let Ok(doc) = roxmltree::Document::parse(xml) {
        for node in doc.descendants().filter(|n| n.is_element()) {
            if node.children().any(|c| c.is_element()) {
                continue;
            }
            out.insert(
                node.tag_name().name().to_string(),
                node.text().unwrap_or("").trim().to_string(),
            );
        }
    } else {
        // Some routers send sloppy XML (undeclared prefixes, junk after the
        // envelope). Fall back to plain tag search for the fields we use.
        for name in ["NewExternalIPAddress", "errorCode", "errorDescription"] {
            let open = format!("<{name}>");
            let close = format!("</{name}>");
            if let Some(start) = xml.find(&open) {
                let rest = &xml[start + open.len()..];
                if let Some(end) = rest.find(&close) {
                    out.insert(name.to_string(), rest[..end].trim().to_string());
                }
            }
        }
    }
    out
}

fn xml_escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// `http://host[:port]/path` — all a router speaks.
#[derive(Debug, Clone, PartialEq, Eq)]
struct HttpUrl {
    host: String,
    port: u16,
    path: String,
}

impl HttpUrl {
    fn parse(url: &str) -> Option<Self> {
        let rest = url.strip_prefix("http://")?;
        let (authority, path) = match rest.find('/') {
            Some(i) => (&rest[..i], &rest[i..]),
            None => (rest, "/"),
        };
        let (host, port) = match authority.rsplit_once(':') {
            Some((host, port)) => (host, port.parse().ok()?),
            None => (authority, 80),
        };
        if host.is_empty() {
            return None;
        }
        Some(Self {
            host: host.to_string(),
            port,
            path: path.to_string(),
        })
    }

    /// Resolve a path from a description against this URL's host.
    fn join(&self, path: &str) -> Self {
        let path = if path.starts_with('/') {
            path.to_string()
        } else {
            format!("/{path}")
        };
        Self {
            host: self.host.clone(),
            port: self.port,
            path,
        }
    }
}

impl std::fmt::Display for HttpUrl {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "http://{}:{}{}", self.host, self.port, self.path)
    }
}

/// Minimal HTTP/1.1 exchange (`Connection: close`), chunked bodies included.
async fn http(
    url: &HttpUrl,
    method: &str,
    headers: &[(&str, &str)],
    body: &str,
) -> Result<(u16, String), String> {
    let exchange = async {
        let mut stream = TcpStream::connect((url.host.as_str(), url.port))
            .await
            .map_err(|e| format!("connect {}:{}: {e}", url.host, url.port))?;
        let mut request = format!(
            "{method} {} HTTP/1.1\r\nHost: {}:{}\r\nConnection: close\r\nUser-Agent: Bardwave UPnP/1.0\r\nContent-Length: {}\r\n",
            url.path,
            url.host,
            url.port,
            body.len()
        );
        for (name, value) in headers {
            request.push_str(&format!("{name}: {value}\r\n"));
        }
        request.push_str("\r\n");
        request.push_str(body);
        stream
            .write_all(request.as_bytes())
            .await
            .map_err(|e| e.to_string())?;
        let mut raw = Vec::new();
        stream
            .read_to_end(&mut raw)
            .await
            .map_err(|e| e.to_string())?;
        parse_http_response(&raw)
    };
    tokio::time::timeout(HTTP_TIMEOUT, exchange)
        .await
        .map_err(|_| format!("{url} did not answer in time"))?
}

fn parse_http_response(raw: &[u8]) -> Result<(u16, String), String> {
    let split = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .ok_or("truncated HTTP response")?;
    let head = String::from_utf8_lossy(&raw[..split]);
    let mut body = raw[split + 4..].to_vec();
    let mut lines = head.split("\r\n");
    let status: u16 = lines
        .next()
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|code| code.parse().ok())
        .ok_or("bad HTTP status line")?;
    let mut chunked = false;
    let mut length = None;
    for line in lines {
        if let Some((name, value)) = line.split_once(':') {
            match name.trim().to_ascii_lowercase().as_str() {
                "transfer-encoding" => chunked = value.to_ascii_lowercase().contains("chunked"),
                "content-length" => length = value.trim().parse::<usize>().ok(),
                _ => {}
            }
        }
    }
    if chunked {
        body = dechunk(&body)?;
    } else if let Some(length) = length {
        body.truncate(length);
    }
    Ok((status, String::from_utf8_lossy(&body).into_owned()))
}

fn dechunk(mut data: &[u8]) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    loop {
        let line_end = data
            .windows(2)
            .position(|w| w == b"\r\n")
            .ok_or("truncated chunked body")?;
        let size_text = String::from_utf8_lossy(&data[..line_end]);
        let size_text = size_text.split(';').next().unwrap_or("").trim();
        let size = usize::from_str_radix(size_text, 16).map_err(|_| "bad chunk size")?;
        data = &data[line_end + 2..];
        if size == 0 {
            return Ok(out);
        }
        if data.len() < size {
            // Some routers close the connection without the final chunk.
            out.extend_from_slice(data);
            return Ok(out);
        }
        out.extend_from_slice(&data[..size]);
        data = data.get(size + 2..).unwrap_or(&[]);
    }
}

/// Whether hosts on the internet can route to this address.
pub fn is_public_ipv4(ip: Ipv4Addr) -> bool {
    let [a, b, c, _] = ip.octets();
    !(ip.is_unspecified()
        || ip.is_private()
        || ip.is_loopback()
        || ip.is_link_local()
        || ip.is_broadcast()
        || ip.is_multicast()
        || ip.is_documentation()
        // 100.64.0.0/10, carrier-grade NAT
        || (a == 100 && (64..=127).contains(&b))
        // 192.0.0.0/24, IETF protocol assignments
        || (a == 192 && b == 0 && c == 0)
        // 198.18.0.0/15, benchmarking
        || (a == 198 && (b & 0xfe) == 18)
        // 0.0.0.0/8 and 240.0.0.0/4
        || a == 0
        || a >= 240)
}

/// Lets inbound connections on `port` through Windows Defender Firewall.
///
/// Needs an elevated process; otherwise Windows asks the user on the first
/// listen, and this only logs why it could not add the rule. The rule is
/// replaced, not duplicated, on every start. Blocking (it runs `netsh`):
/// call it from a blocking thread, never from the async server loop.
#[cfg(windows)]
pub fn allow_through_firewall(port: u16) -> bool {
    use std::os::windows::process::CommandExt;
    use std::process::Command;
    // No console window flashing up from a GUI app.
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    let name = format!("name=Bardwave TCP {port}");
    let _ = Command::new("netsh")
        .args(["advfirewall", "firewall", "delete", "rule", &name])
        .creation_flags(CREATE_NO_WINDOW)
        .output();
    let added = Command::new("netsh")
        .args([
            "advfirewall",
            "firewall",
            "add",
            "rule",
            &name,
            "dir=in",
            "action=allow",
            "protocol=TCP",
            &format!("localport={port}"),
            "enable=yes",
            "profile=any",
        ])
        .creation_flags(CREATE_NO_WINDOW)
        .output();
    match added {
        Ok(out) if out.status.success() => {
            tracing::info!("Windows firewall: inbound TCP {port} allowed");
            true
        }
        Ok(_) => {
            tracing::warn!(
                "Windows firewall rule for TCP {port} not added (needs administrator rights); \
                 allow Bardwave in the Windows prompt, for private AND public networks"
            );
            false
        }
        Err(e) => {
            tracing::warn!("Windows firewall rule for TCP {port} not added: {e}");
            false
        }
    }
}

/// Lets inbound connections on `port` through the host firewall (Windows only).
#[cfg(not(windows))]
pub fn allow_through_firewall(_port: u16) -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn public_addresses_are_public() {
        for ip in ["8.8.8.8", "85.140.1.20", "140.232.64.5", "100.128.0.1"] {
            assert!(is_public_ipv4(ip.parse().unwrap()), "{ip}");
        }
    }

    #[test]
    fn private_and_special_addresses_are_not_public() {
        for ip in [
            "10.0.0.1",
            "172.16.5.4",
            "192.168.1.1",
            "100.64.0.1",
            "100.127.255.254",
            "127.0.0.1",
            "169.254.1.1",
            "0.0.0.0",
            "255.255.255.255",
            "224.0.0.1",
            "192.0.2.1",
            "198.18.0.1",
            "192.0.0.8",
            "240.0.0.1",
        ] {
            assert!(!is_public_ipv4(ip.parse().unwrap()), "{ip}");
        }
    }

    #[tokio::test]
    async fn ipv6_and_loopback_binds_are_not_forwarded() {
        assert!(start("::".parse().unwrap(), 8080).is_none());
        assert!(start("127.0.0.1".parse().unwrap(), 8080).is_none());
        assert!(start("0.0.0.0".parse().unwrap(), 0).is_none());
    }

    #[test]
    fn only_a_public_external_address_gets_a_url() {
        let public: SocketAddrV4 = "85.140.1.20:8080".parse().unwrap();
        let mapped = describe_external(public);
        assert_eq!(mapped.url(), Some("http://85.140.1.20:8080"));

        let cgnat: SocketAddrV4 = "100.70.1.2:8080".parse().unwrap();
        assert_eq!(
            describe_external(cgnat),
            UpnpStatus::DoubleNat { external: cgnat }
        );
        assert_eq!(UpnpStatus::Searching.url(), None);
    }

    #[test]
    fn ssdp_answers_from_gateways_are_recognised() {
        let answer = b"HTTP/1.1 200 OK\r\nCACHE-CONTROL: max-age=120\r\nST: urn:schemas-upnp-org:device:InternetGatewayDevice:1\r\n\
USN: uuid:abc::urn:schemas-upnp-org:device:InternetGatewayDevice:1\r\nLOCATION: http://192.168.0.1:33311/rootDesc.xml\r\n\r\n";
        assert_eq!(
            ssdp_location(answer).as_deref(),
            Some("http://192.168.0.1:33311/rootDesc.xml")
        );
        let tv = b"HTTP/1.1 200 OK\r\nST: urn:dial-multiscreen-org:service:dial:1\r\nLOCATION: http://192.168.0.5:8008/\r\n\r\n";
        assert_eq!(ssdp_location(tv), None);
    }

    #[test]
    fn description_yields_the_wan_ip_service() {
        let xml = r#"<?xml version="1.0"?>
<root xmlns="urn:schemas-upnp-org:device-1-0"><device><deviceList><device><deviceList><device>
<serviceList>
 <service><serviceType>urn:schemas-upnp-org:service:WANPPPConnection:1</serviceType><controlURL>/ctl/PPP</controlURL></service>
 <service><serviceType>urn:schemas-upnp-org:service:WANIPConnection:1</serviceType><controlURL>ctl/IPConn</controlURL></service>
 <service><serviceType>urn:schemas-upnp-org:service:Layer3Forwarding:1</serviceType><controlURL>/ctl/L3F</controlURL></service>
</serviceList></device></deviceList></device></deviceList></device></root>"#;
        let location = HttpUrl::parse("http://192.168.0.1:33311/rootDesc.xml").unwrap();
        let services = wan_services(xml, &location).unwrap();
        assert_eq!(services.len(), 2);
        assert!(services[0].0.contains("WANIPConnection"));
        assert_eq!(
            services[0].1.to_string(),
            "http://192.168.0.1:33311/ctl/IPConn"
        );
        assert_eq!(
            services[1].1.to_string(),
            "http://192.168.0.1:33311/ctl/PPP"
        );
    }

    #[test]
    fn soap_fault_and_chunked_bodies_are_parsed() {
        let parts = [
            "<s:Envelope xmlns:s=\"http://schemas.xmlsoap.org/soap/envelope/\"><s:Body><s:F",
            "ault><detail><UPnPError><errorCode>718</errorCode>",
            "<errorDescription>Conflict</errorDescription></UPnPError></detail></s:Fault></s:Body></s:Envelope>",
        ];
        let mut fault = String::from(
            "HTTP/1.1 500 Internal Server Error\r\nTransfer-Encoding: chunked\r\n\r\n",
        );
        for part in parts {
            fault.push_str(&format!("{:x}\r\n{part}\r\n", part.len()));
        }
        fault.push_str("0\r\n\r\n");
        let (status, body) = parse_http_response(fault.as_bytes()).unwrap();
        assert_eq!(status, 500);
        let fields = xml_leaves(&body);
        assert_eq!(fields.get("errorCode").map(String::as_str), Some("718"));
    }

    #[test]
    fn urls_parse_and_join() {
        let url = HttpUrl::parse("http://192.168.1.1/desc.xml").unwrap();
        assert_eq!(url.port, 80);
        assert_eq!(url.join("ctl").path, "/ctl");
        assert!(HttpUrl::parse("https://x/").is_none());
    }
}
