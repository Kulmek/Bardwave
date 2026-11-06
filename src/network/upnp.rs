//! UPnP IGD port mapping for SoundShared.
//!
//! Discovers the local IGD (Internet Gateway Device) via SSDP and adds a port
//! mapping so the bound port is reachable from the global IP. If no IGD is
//! found or the mapping fails, the server still works on LAN — UPnP is purely
//! additive and never a hard failure.
//!
//! Built on [`port-control-client`](https://crates.io/crates/port-control-client),
//! which speaks UPnP IGD (v1/v2) over Tokio.

use std::net::IpAddr;
use std::num::NonZeroU16;
use port_control_client::{Config, Method, Protocol};
pub use port_control_client::PortMapping;
use tracing::{debug, info};

/// Result of an attempted UPnP port mapping.
#[derive(Debug, Clone)]
pub struct UpnpResult {
    /// True if a mapping was successfully created.
    pub mapped: bool,
    /// External port on the router that forwards to our bind port.
    pub external_port: u16,
    /// The router's external (global) IP address, if we could read it.
    pub external_ip: Option<IpAddr>,
}

/// Try to open the given port on the local IGD for TCP via UPnP.
///
/// Returns `Ok((UpnpResult, PortMapping))` — keep the `PortMapping` handle alive
/// for as long as the server runs, and call [`remove_port_mapping`] on shutdown.
/// On failure returns `Ok((UpnpResult { mapped: false, .. }, ...))` so callers
/// can continue without port forwarding. Only truly unexpected errors surface as
/// `Err`.
pub async fn add_port_mapping(bind_addr: IpAddr, port: u16) -> Result<(UpnpResult, PortMapping), String> {
    if !bind_addr.is_ipv4() {
        debug!("UPnP skipped: bind address is not IPv4 ({bind_addr})");
        let mapping = PortMapping::start(Config::new(Protocol::Tcp, NonZeroU16::new(1).unwrap())
            .methods(vec![Method::Upnp]));
        return Ok((
            UpnpResult {
                mapped: false,
                external_port: port,
                external_ip: None,
            },
            mapping,
        ));
    }

    info!("Searching for UPnP IGD devices...");

    let config = Config::new(Protocol::Tcp, NonZeroU16::new(port).unwrap())
        .lifetime(std::time::Duration::from_secs(2 * 60 * 60)) // 2 hours
        .retry_interval(std::time::Duration::from_secs(60))
        .description("SoundShared")
        .methods(vec![Method::Upnp]);

    let mapping = PortMapping::start(config);

    // Give the background task a moment to discover the router and grant the mapping.
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;

    let external = mapping.mapping();
    let (mapped, external_port, external_ip) = match external {
        Some(m) => {
            info!("UPnP port mapping granted: external {} -> local {}", m.external, port);
            (true, m.external.port(), Some(IpAddr::V4(*m.external.ip())))
        }
        None => {
            info!("No UPnP mapping granted yet — continuing without port forwarding");
            let err = mapping.last_error();
            if let Some(e) = &err {
                debug!("UPnP last error: {e}");
            }
            (false, port, None)
        }
    };

    Ok((
        UpnpResult {
            mapped,
            external_port,
            external_ip,
        },
        mapping,
    ))
}

/// Remove a previously added port mapping on shutdown.
pub async fn remove_port_mapping(mapping: PortMapping) {
    mapping.stop().await;
    info!("UPnP port mapping released");
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    #[test]
    fn upnp_result_default_fields() {
        let result = UpnpResult {
            mapped: false,
            external_port: 8080,
            external_ip: None,
        };
        assert!(!result.mapped);
        assert_eq!(result.external_port, 8080);
        assert!(result.external_ip.is_none());
    }

    #[test]
    fn non_ipv4_and_ipv4_detected() {
        let v6 = IpAddr::V6(std::net::Ipv6Addr::LOCALHOST);
        assert!(v6.is_ipv6());
        assert!(!v6.is_ipv4());
        let v4 = IpAddr::V4(Ipv4Addr::LOCALHOST);
        assert!(v4.is_ipv4());
    }
}
