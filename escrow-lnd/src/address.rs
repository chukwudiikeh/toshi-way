//! Checking a node address a seller gives us before the server connects to it.
//!
//! The address is attacker-controlled input that makes our server open a network connection, so
//! left unchecked it could be pointed at the server's own loopback, a cloud metadata service, or a
//! machine on the internal network. Unless the operator allows private addresses (needed for a
//! local regtest network), the address must be `https://` and every IP it resolves to must be public.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

/// Parsed `https://host:port`.
#[derive(Debug, PartialEq, Eq)]
pub struct NodeAddress {
    pub host: String,
    pub port: u16,
}

pub fn parse(address: &str) -> Result<NodeAddress, String> {
    let rest = address
        .trim()
        .strip_prefix("https://")
        .ok_or("the node address must start with https://")?;
    if rest.is_empty() || rest.contains(['/', '?', '#', '@', ' ']) {
        return Err("use just https://host:port, with no path or credentials".into());
    }
    let (host, port) = if let Some(v6) = rest.strip_prefix('[') {
        let (host, after) = v6.split_once(']').ok_or("malformed IPv6 address")?;
        (host.to_string(), after.strip_prefix(':').ok_or("the node address needs a port, for example :10009")?)
    } else {
        let (host, port) = rest.rsplit_once(':').ok_or("the node address needs a port, for example :10009")?;
        (host.to_string(), port)
    };
    let port: u16 = port.parse().map_err(|_| "the port must be a number")?;
    if host.is_empty() || port == 0 {
        return Err("the node address needs a host and a port".into());
    }
    Ok(NodeAddress { host, port })
}

/// Loopback, private, link-local, unique-local, multicast, unspecified, documentation and
/// carrier-grade NAT ranges: anything that is not an ordinary public address.
pub fn is_public(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => is_public_v4(v4),
        IpAddr::V6(v6) => match v6.to_ipv4_mapped() {
            Some(v4) => is_public_v4(v4),
            None => is_public_v6(v6),
        },
    }
}

fn is_public_v4(ip: Ipv4Addr) -> bool {
    let o = ip.octets();
    !(ip.is_loopback()
        || ip.is_private()
        || ip.is_link_local()
        || ip.is_broadcast()
        || ip.is_unspecified()
        || ip.is_multicast()
        || o[0] == 0
        || (o[0] == 100 && (64..=127).contains(&o[1])) // carrier-grade NAT
        || (o[0] == 192 && o[1] == 0 && o[2] == 0) // IETF protocol assignments
        || (o[0] == 192 && o[1] == 0 && o[2] == 2) // documentation
        || (o[0] == 198 && (o[1] == 18 || o[1] == 19)) // benchmarking
        || (o[0] == 198 && o[1] == 51 && o[2] == 100)
        || (o[0] == 203 && o[1] == 0 && o[2] == 113)
        || o[0] >= 240) // reserved
}

fn is_public_v6(ip: Ipv6Addr) -> bool {
    let s = ip.segments();
    !(ip.is_loopback()
        || ip.is_unspecified()
        || ip.is_multicast()
        || (s[0] & 0xfe00) == 0xfc00 // unique local
        || (s[0] & 0xffc0) == 0xfe80 // link local
        || (s[0] == 0x2001 && s[1] == 0x0db8)) // documentation
}

/// Resolve the host and refuse unless every address is public (or private ones are allowed).
/// This cannot close the gap between checking and connecting (a DNS answer can change), so the
/// connection itself is also pinned to the certificate the seller supplied.
pub async fn check(address: &str, allow_private: bool) -> Result<NodeAddress, String> {
    let parsed = parse(address)?;
    if allow_private {
        return Ok(parsed);
    }
    let addrs: Vec<_> = tokio::net::lookup_host((parsed.host.as_str(), parsed.port))
        .await
        .map_err(|_| "could not resolve that host".to_string())?
        .collect();
    if addrs.is_empty() {
        return Err("could not resolve that host".into());
    }
    if addrs.iter().any(|a| !is_public(a.ip())) {
        return Err("that address points at a private or local network; use your node's public address".into());
    }
    Ok(parsed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_host_and_port() {
        assert_eq!(parse("https://node.example.com:10009").unwrap(), NodeAddress { host: "node.example.com".into(), port: 10009 });
        assert_eq!(parse("https://[2001:4860::1]:10009").unwrap().host, "2001:4860::1");
    }

    #[test]
    fn rejects_what_is_not_a_bare_https_host_and_port() {
        for bad in [
            "http://node.example.com:10009",
            "node.example.com:10009",
            "https://node.example.com",
            "https://node.example.com:0",
            "https://node.example.com:99999",
            "https://node.example.com:10009/path",
            "https://user:pw@node.example.com:10009",
            "https://:10009",
            "https://",
            "https://node.example.com:10009?x=1",
        ] {
            assert!(parse(bad).is_err(), "{bad} should be refused");
        }
    }

    #[test]
    fn only_ordinary_public_addresses_pass() {
        for ok in ["8.8.8.8", "1.1.1.1", "93.184.216.34", "2606:4700:4700::1111"] {
            assert!(is_public(ok.parse().unwrap()), "{ok}");
        }
        for bad in [
            "127.0.0.1", "10.0.0.5", "172.16.3.4", "192.168.1.1", "169.254.169.254", "0.0.0.0", "100.64.0.1",
            "224.0.0.1", "255.255.255.255", "198.18.0.1", "203.0.113.9", "240.0.0.1", "::1", "::", "fe80::1",
            "fc00::1", "fd12:3456::1", "ff02::1", "2001:db8::1", "::ffff:127.0.0.1", "::ffff:10.0.0.1",
        ] {
            assert!(!is_public(bad.parse().unwrap()), "{bad} must not be public");
        }
    }

    #[tokio::test]
    async fn private_addresses_are_refused_unless_allowed() {
        for bad in ["https://127.0.0.1:10009", "https://169.254.169.254:80", "https://10.1.2.3:10009", "https://[::1]:10009"] {
            assert!(check(bad, false).await.is_err(), "{bad}");
            assert!(check(bad, true).await.is_ok(), "{bad} with private allowed");
        }
        assert!(check("https://8.8.8.8:10009", false).await.is_ok());
    }
}
