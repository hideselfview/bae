use if_addrs::get_if_addrs;
use std::net::{IpAddr, SocketAddr};

/// Upgrade HTTP URLs to HTTPS for App Transport Security compliance
pub fn upgrade_to_https(url: &str) -> String {
    if url.starts_with("http://") {
        url.replace("http://", "https://")
    } else {
        url.to_string()
    }
}

/// Get network interfaces with error handling
fn get_interfaces() -> Result<Vec<if_addrs::Interface>, String> {
    get_if_addrs().map_err(|e| format!("Failed to enumerate network interfaces: {}", e))
}

/// Check if an IP address is bound to any interface
fn is_ip_bound(ip: IpAddr, interfaces: &[if_addrs::Interface]) -> bool {
    interfaces.iter().any(|iface| iface.addr.ip() == ip)
}

/// Collect available IP addresses for error messages (excluding loopback and unspecified)
fn collect_available_ips(interfaces: &[if_addrs::Interface]) -> Vec<String> {
    interfaces
        .iter()
        .filter_map(|iface| {
            let ip = iface.addr.ip();
            if !ip.is_loopback() && !ip.is_unspecified() {
                Some(ip.to_string())
            } else {
                None
            }
        })
        .collect()
}

/// Validate that an IP address is bound to an interface
fn validate_ip_address(ip: IpAddr, allow_unspecified: bool) -> Result<(), String> {
    if ip.is_unspecified() {
        if allow_unspecified {
            // 0.0.0.0 or :: means bind to all interfaces - always valid
            return Ok(());
        } else {
            return Err("IP address cannot be unspecified (0.0.0.0 or ::) without a port. Use format IP:port (e.g., 0.0.0.0:6881)".to_string());
        }
    }

    let interfaces = get_interfaces()?;

    if !is_ip_bound(ip, &interfaces) {
        let available_ips = collect_available_ips(&interfaces);
        return Err(format!(
            "IP address '{}' not bound to any interface. Available IPs: {}",
            ip,
            if available_ips.is_empty() {
                "none".to_string()
            } else {
                available_ips.join(", ")
            }
        ));
    }

    Ok(())
}

/// Validate network interface configuration
/// Accepts:
/// - Interface name (e.g., "eth0", "tun0")
/// - IP:port format (e.g., "0.0.0.0:6881", "192.168.1.1:6881")
pub fn validate_network_interface(interface: &str) -> Result<(), String> {
    // Check if it's IP:port format
    if let Ok(socket_addr) = interface.parse::<SocketAddr>() {
        let ip = socket_addr.ip();
        let port = socket_addr.port();

        // Validate port is not 0 (port 0 means "any port" which libtorrent doesn't support)
        if port == 0 {
            return Err(
                "Port 0 is not supported. Please specify a valid port number (1-65535)".to_string(),
            );
        }

        // Validate IP (allow unspecified for IP:port format)
        validate_ip_address(ip, true)?;
        return Ok(());
    }

    // Try parsing as just an IP address (no port)
    if let Ok(ip) = interface.parse::<IpAddr>() {
        // Validate IP (don't allow unspecified without port)
        validate_ip_address(ip, false)?;
        return Ok(());
    }

    // Otherwise, treat as interface name
    let interfaces = get_interfaces()?;

    if interfaces.iter().any(|iface| iface.name == interface) {
        Ok(())
    } else {
        let available: Vec<String> = interfaces.iter().map(|iface| iface.name.clone()).collect();
        Err(format!(
            "Network interface '{}' not found. Available interfaces: {}",
            interface,
            available.join(", ")
        ))
    }
}
