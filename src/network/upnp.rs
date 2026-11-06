//! UPnP IGD port mapping for SoundShared.
//!
//! Discovers the local IGD (Internet Gateway Device) via SSDP and adds a port
//! mapping so the bound port is reachable from the global IP. If no IGD is
//! found or the mapping fails, the server still works on LAN — UPnP is purely
//! additive and never a hard failure.
//!
//! Built on [`port-control-client`](https://crates.io/crates/port-control-client),
//! which speaks UPnP IGD (v1/v2) over Tokio.
//!
//! On Windows, a firewall rule is also added so inbound connections on the
//! mapped port are not blocked by Windows Defender Firewall.

use std::net::IpAddr;
use std::num::NonZeroU16;
use port_control_client::{Config, Method, Protocol};
pub use port_control_client::PortMapping;
use tracing::{debug, info, warn};

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

/// How long to wait for the UPnP background task to discover the router and
/// grant the mapping before checking the result.
const UPNP_DISCOVER_WAIT_SECS: u64 = 10;

/// How many times to retry the whole discovery + mapping cycle.
const UPNP_MAX_RETRIES: u32 = 3;

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

    let mut last_error: Option<String> = None;

    for attempt in 1..=UPNP_MAX_RETRIES {
        if attempt > 1 {
            info!("UPnP retry {attempt}/{UPNP_MAX_RETRIES}...");
            tokio::time::sleep(std::time::Duration::from_secs(3)).await;
        }

        let config = Config::new(Protocol::Tcp, NonZeroU16::new(port).unwrap())
            .lifetime(std::time::Duration::from_secs(2 * 60 * 60)) // 2 hours
            .retry_interval(std::time::Duration::from_secs(60))
            .description("SoundShared")
            .methods(vec![Method::Upnp]);

        let mapping = PortMapping::start(config);

        // Give the background task time to discover the router via SSDP and
        // negotiate the port mapping. 2 seconds is usually too short on a busy
        // home network.
        tokio::time::sleep(std::time::Duration::from_secs(UPNP_DISCOVER_WAIT_SECS)).await;

        let external = mapping.mapping();
        match external {
            Some(m) => {
                info!(
                    "UPnP port mapping granted: external {} -> local {}",
                    m.external, port
                );
                // Also ensure Windows Defender Firewall allows inbound on this port.
                #[cfg(windows)]
                if let Err(e) = add_firewall_rule(port) {
                    warn!("Could not add Windows firewall rule for port {port}: {e}");
                }
                return Ok((
                    UpnpResult {
                        mapped: true,
                        external_port: m.external.port(),
                        external_ip: Some(IpAddr::V4(*m.external.ip())),
                    },
                    mapping,
                ));
            }
            None => {
                let err = mapping.last_error();
                if let Some(e) = &err {
                    last_error = Some(e.to_string());
                    debug!("UPnP attempt {attempt} last error: {e}");
                } else {
                    debug!("UPnP attempt {attempt}: no mapping granted yet");
                }
                // Stop this mapping attempt before the next retry.
                mapping.stop().await;
            }
        }
    }

    info!(
        "No UPnP mapping granted after {UPNP_MAX_RETRIES} attempts — continuing without port forwarding{}",
        match &last_error {
            Some(e) => format!(" (last error: {e})"),
            None => String::new(),
        }
    );

    let dummy_mapping = PortMapping::start(Config::new(Protocol::Tcp, NonZeroU16::new(1).unwrap())
        .methods(vec![Method::Upnp]));

    Ok((
        UpnpResult {
            mapped: false,
            external_port: port,
            external_ip: None,
        },
        dummy_mapping,
    ))
}

/// Remove a previously added port mapping on shutdown.
pub async fn remove_port_mapping(mapping: PortMapping) {
    mapping.stop().await;
    info!("UPnP port mapping released");
}

/// Add a Windows Defender Firewall inbound rule for the given TCP port so
/// connections from the internet (after UPnP router forwarding) are not blocked.
#[cfg(windows)]
fn add_firewall_rule(port: u16) -> Result<(), String> {
    let rule_name = format!("SoundShared port {port}");
    let output = std::process::Command::new("netsh")
        .args([
            "advfirewall",
            "firewall",
            "add",
            "rule",
            &format!("name={rule_name}"),
            "dir=in",
            "action=allow",
            &format!("protocol=TCP"),
            &format!("localport={port}"),
            "enable=yes",
            "profile=any",
        ])
        .output()
        .map_err(|e| format!("failed to run netsh: {e}"))?;

    if output.status.success() {
        info!("Windows firewall rule added for TCP port {port}");
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(format!("netsh returned non-zero: {stderr}"))
    }
}

#[cfg(not(windows))]
fn add_firewall_rule(_port: u16) -> Result<(), String> {
    Ok(())
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
