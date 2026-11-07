//! Router port forwarding, so players outside the LAN can reach the console.
//!
//! On `Start` the server thread asks the local router to forward the listen
//! port to this machine, through UPnP IGD first and PCP / NAT-PMP as fallbacks
//! ([`port-control-client`](https://crates.io/crates/port-control-client)).
//! The request runs in the background and never delays or fails the server:
//! the LAN URL is published at once and the internet URL is added when (and
//! if) the router grants the mapping. The background task renews the lease
//! and keeps retrying while no router answers; `Stop` removes the mapping.
//!
//! A granted mapping is not always reachable from the internet: behind a second
//! NAT (carrier-grade NAT, a provider modem in front of the router) the
//! router's external address is itself private. That case is reported as
//! [`UpnpStatus::DoubleNat`] instead of advertising a URL nobody can open.

pub use port_control_client::PortMapping;
use port_control_client::{Config, Method, Protocol, Status};
use std::net::{IpAddr, Ipv4Addr, SocketAddrV4};
use std::num::NonZeroU16;
use std::time::Duration;

/// Lease asked of the router; the task renews it at half-time.
const LEASE: Duration = Duration::from_secs(2 * 60 * 60);

/// How long to wait before asking again when no router granted a mapping.
const RETRY_INTERVAL: Duration = Duration::from_secs(30);

/// Description shown in the router's port-forwarding table.
const DESCRIPTION: &str = "Bardwave";

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

/// Starts forwarding `port` (TCP) on the router, if the bind address can be
/// forwarded at all.
///
/// Returns `None` for IPv6 and loopback binds: routers only forward IPv4, and
/// a loopback socket is unreachable from anywhere else anyway. Keep the handle
/// for as long as the server runs and call [`release`] on stop; dropping it
/// also releases the mapping, in the background.
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
    let port = NonZeroU16::new(port)?;

    tracing::info!("Asking the router to forward TCP port {port} (UPnP IGD, PCP, NAT-PMP)");
    let config = Config::new(Protocol::Tcp, port)
        .lifetime(LEASE)
        .retry_interval(RETRY_INTERVAL)
        .description(DESCRIPTION)
        .methods([Method::Upnp, Method::Pcp, Method::NatPmp]);
    Some(PortMapping::start(config))
}

/// Removes the mapping from the router; returns within about two seconds.
pub async fn release(mapping: PortMapping) {
    let had_mapping = mapping.mapping().is_some();
    mapping.stop().await;
    if had_mapping {
        match mapping.last_error() {
            Some(e) => tracing::warn!("Could not remove the router port mapping: {e}"),
            None => tracing::info!("Router port mapping removed"),
        }
    }
}

/// What the operator should see for a mapping task's status.
pub fn describe(status: &Status) -> UpnpStatus {
    if let Some(mapping) = status.mapping() {
        let external = mapping.external;
        return if is_public_ipv4(*external.ip()) {
            UpnpStatus::Mapped {
                external,
                url: format!("http://{external}"),
            }
        } else {
            UpnpStatus::DoubleNat { external }
        };
    }
    match status {
        Status::Stopped { .. } => UpnpStatus::Off,
        _ => match status.error() {
            Some(e) => UpnpStatus::Unavailable(e.to_string()),
            None => UpnpStatus::Searching,
        },
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
/// replaced, not duplicated, on every start.
#[cfg(windows)]
pub fn allow_through_firewall(port: u16) {
    use std::process::Command;

    let name = format!("name=Bardwave TCP {port}");
    let _ = Command::new("netsh")
        .args(["advfirewall", "firewall", "delete", "rule", &name])
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
        .output();
    match added {
        Ok(out) if out.status.success() => {
            tracing::info!("Windows firewall: inbound TCP {port} allowed")
        }
        Ok(out) => tracing::warn!(
            "Windows firewall rule for TCP {port} not added (run as administrator to add it): {}",
            String::from_utf8_lossy(&out.stdout).trim()
        ),
        Err(e) => tracing::warn!("Windows firewall rule for TCP {port} not added: {e}"),
    }
}

/// Lets inbound connections on `port` through the host firewall (Windows only).
#[cfg(not(windows))]
pub fn allow_through_firewall(_port: u16) {}

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
    fn describe_without_mapping() {
        assert_eq!(
            describe(&Status::Unmapped { error: None }),
            UpnpStatus::Searching
        );
        assert_eq!(describe(&Status::Stopped { error: None }), UpnpStatus::Off);
    }

    #[test]
    fn only_mapped_status_has_a_url() {
        let external: SocketAddrV4 = "85.140.1.20:8080".parse().unwrap();
        let mapped = UpnpStatus::Mapped {
            external,
            url: format!("http://{external}"),
        };
        assert_eq!(mapped.url(), Some("http://85.140.1.20:8080"));
        assert_eq!(UpnpStatus::DoubleNat { external }.url(), None);
        assert_eq!(UpnpStatus::Searching.url(), None);
    }
}
