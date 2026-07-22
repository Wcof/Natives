//! SSRF guards for web_fetch: scheme, DNS, private/link-local/metadata.

use crate::ToolError;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

/// Validate URL scheme and host before connecting. On each redirect, re-run.
pub fn validate_fetch_url(url: &str) -> Result<(), ToolError> {
    let parsed = reqwest::Url::parse(url).map_err(|e| ToolError {
        code: "invalid_url".into(),
        message: e.to_string(),
        retryable: false,
    })?;
    let scheme = parsed.scheme().to_ascii_lowercase();
    if scheme != "http" && scheme != "https" {
        return Err(ToolError {
            code: "ssrf".into(),
            message: format!("scheme `{scheme}` not allowed"),
            retryable: false,
        });
    }
    let host = parsed.host_str().ok_or_else(|| ToolError {
        code: "ssrf".into(),
        message: "URL missing host".into(),
        retryable: false,
    })?;
    let host_l = host.to_ascii_lowercase();
    if host_l == "localhost"
        || host_l.ends_with(".localhost")
        || host_l == "metadata.google.internal"
        || host_l.ends_with(".local")
    {
        return Err(ToolError {
            code: "ssrf".into(),
            message: "blocked host".into(),
            retryable: false,
        });
    }
    // Literal IP in host
    if let Ok(ip) = host.parse::<IpAddr>() {
        if is_blocked_ip(ip) {
            return Err(ToolError {
                code: "ssrf".into(),
                message: format!("blocked address {ip}"),
                retryable: false,
            });
        }
        return Ok(());
    }
    // DNS resolve and reject private answers
    match std::net::ToSocketAddrs::to_socket_addrs(&(host, 80)) {
        Ok(addrs) => {
            let mut any = false;
            for addr in addrs {
                any = true;
                if is_blocked_ip(addr.ip()) {
                    return Err(ToolError {
                        code: "ssrf".into(),
                        message: format!("DNS resolved to blocked address {}", addr.ip()),
                        retryable: false,
                    });
                }
            }
            if !any {
                return Err(ToolError {
                    code: "ssrf".into(),
                    message: "DNS returned no addresses".into(),
                    retryable: true,
                });
            }
        }
        Err(e) => {
            return Err(ToolError {
                code: "ssrf".into(),
                message: format!("DNS resolve failed: {e}"),
                retryable: true,
            });
        }
    }
    Ok(())
}

pub fn is_blocked_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => is_blocked_v4(v4),
        IpAddr::V6(v6) => is_blocked_v6(v6),
    }
}

fn is_blocked_v4(ip: Ipv4Addr) -> bool {
    ip.is_loopback()
        || ip.is_private()
        || ip.is_link_local()
        || ip.is_broadcast()
        || ip.is_unspecified()
        || ip.octets()[0] == 0
        // CGNAT / shared address space
        || (ip.octets()[0] == 100 && (ip.octets()[1] & 0b1100_0000) == 0b0100_0000)
        // metadata 169.254.169.254 already link-local
        || ip.octets() == [169, 254, 169, 254]
}

fn is_blocked_v6(ip: Ipv6Addr) -> bool {
    if ip.is_loopback() || ip.is_unspecified() {
        return true;
    }
    // Unique local fc00::/7
    let segments = ip.segments();
    if (segments[0] & 0xfe00) == 0xfc00 {
        return true;
    }
    // Link-local fe80::/10
    if (segments[0] & 0xffc0) == 0xfe80 {
        return true;
    }
    // IPv4-mapped
    if let Some(v4) = ip.to_ipv4_mapped() {
        return is_blocked_v4(v4);
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocks_loopback_literal() {
        assert!(validate_fetch_url("http://127.0.0.1/").is_err());
        assert!(validate_fetch_url("http://localhost/x").is_err());
    }

    #[test]
    fn blocks_metadata() {
        assert!(validate_fetch_url("http://169.254.169.254/latest").is_err());
    }

    #[test]
    fn blocks_non_http() {
        assert!(validate_fetch_url("file:///etc/passwd").is_err());
        assert!(validate_fetch_url("ftp://example.com/").is_err());
    }

    #[test]
    fn allows_public_ip_literal() {
        // 1.1.1.1 is public; DNS not needed for literal
        assert!(validate_fetch_url("https://1.1.1.1/").is_ok());
    }
}
