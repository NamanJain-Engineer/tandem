//! Where plugin traffic may go (ARCHITECTURE §11.5, F-PL-03, F-PL-04):
//! pinned hosts (scheme, host, port and path prefix must all match) and the
//! private, loopback and link-local ranges no redirect may reach.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use url::Url;

/// A place a module's credentials may be sent to: scheme, host, port and
/// path prefix must all match (F-PL-04).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PinnedHost {
    pub scheme: String,
    pub host: String,
    pub port: u16,
    pub path_prefix: String,
}

impl PinnedHost {
    pub fn new(scheme: &str, host: &str, port: u16, path_prefix: &str) -> Self {
        Self {
            scheme: scheme.to_string(),
            host: host.to_ascii_lowercase(),
            port,
            path_prefix: path_prefix.to_string(),
        }
    }

    /// `https://api.example.com/v1/` → scheme, host, the effective port and
    /// the path as the prefix.
    pub fn from_url(url: &str) -> Option<Self> {
        let u = Url::parse(url).ok()?;
        Some(Self::new(
            u.scheme(),
            u.host_str()?,
            u.port_or_known_default()?,
            u.path(),
        ))
    }

    /// The exact match F-PL-04 asks for. A prefix ending in `/` matches
    /// itself and below; one without matches itself or `prefix/…` (so
    /// `/mcp` never matches `/mcpx`).
    pub fn matches(&self, url: &Url) -> bool {
        let (Some(host), Some(port)) = (url.host_str(), url.port_or_known_default()) else {
            return false;
        };
        if url.scheme() != self.scheme
            || !host.eq_ignore_ascii_case(&self.host)
            || port != self.port
        {
            return false;
        }
        let path = url.path();
        if self.path_prefix.ends_with('/') || self.path_prefix.is_empty() {
            path.starts_with(&self.path_prefix)
        } else {
            path == self.path_prefix
                || path
                    .strip_prefix(&self.path_prefix)
                    .is_some_and(|rest| rest.starts_with('/'))
        }
    }

    /// Same scheme, host and port (path ignored): auth hosts serve their
    /// metadata, registration and token endpoints anywhere on the origin.
    pub fn same_origin(&self, url: &Url) -> bool {
        url.scheme() == self.scheme
            && url
                .host_str()
                .is_some_and(|h| h.eq_ignore_ascii_case(&self.host))
            && url.port_or_known_default() == Some(self.port)
    }
}

pub fn any_matches(pins: &[PinnedHost], url: &Url) -> bool {
    pins.iter().any(|p| p.matches(url))
}

fn private_v4(ip: Ipv4Addr) -> bool {
    ip.is_private()
        || ip.is_loopback()
        || ip.is_link_local()
        || ip.is_unspecified()
        || ip.is_broadcast()
        // 100.64/10 (carrier-grade NAT) reaches inside networks too.
        || (ip.octets()[0] == 100 && (ip.octets()[1] & 0xc0) == 64)
}

fn private_v6(ip: Ipv6Addr) -> bool {
    if let Some(v4) = ip.to_ipv4_mapped() {
        return private_v4(v4);
    }
    let first = ip.segments()[0];
    ip.is_loopback()
        || ip.is_unspecified()
        || (first & 0xfe00) == 0xfc00 // fc00::/7 unique local
        || (first & 0xffc0) == 0xfe80 // fe80::/10 link local
}

/// `10/8, 172.16/12, 192.168/16, 127/8, 169.254/16, ::1, fc00::/7,
/// fe80::/10` and friends.
pub fn is_private_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => private_v4(v4),
        IpAddr::V6(v6) => private_v6(v6),
    }
}

/// Whether a URL names a private address: a literal IP in a private range,
/// `localhost`, or a name that resolves to one.
pub async fn is_private_url(url: &Url) -> bool {
    match url.host() {
        Some(url::Host::Ipv4(ip)) => private_v4(ip),
        Some(url::Host::Ipv6(ip)) => private_v6(ip),
        Some(url::Host::Domain(name)) => {
            let name = name.to_ascii_lowercase();
            if name == "localhost" || name.ends_with(".localhost") {
                return true;
            }
            let port = url.port_or_known_default().unwrap_or(443);
            let target = format!("{name}:{port}");
            match tokio::net::lookup_host(target).await {
                Ok(addrs) => addrs.into_iter().any(|a| is_private_ip(a.ip())),
                // Unresolvable: nothing to reach, and nothing private.
                Err(_) => false,
            }
        }
        None => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn u(s: &str) -> Url {
        Url::parse(s).expect("url")
    }

    #[test]
    fn f_pl_04_pins_match_scheme_host_port_and_prefix_exactly() {
        let pin = PinnedHost::new("https", "api.example.com", 443, "/v1/");
        assert!(pin.matches(&u("https://api.example.com/v1/issues")));
        assert!(pin.matches(&u("https://API.example.com:443/v1/")));
        assert!(!pin.matches(&u("http://api.example.com/v1/issues")));
        assert!(!pin.matches(&u("https://api.example.com:8443/v1/issues")));
        assert!(!pin.matches(&u("https://api.example.com/v2/issues")));
        assert!(!pin.matches(&u("https://evil.example.com/v1/issues")));
        assert!(!pin.matches(&u("https://api.example.com.evil.net/v1/")));
        let exact = PinnedHost::new("https", "mcp.example.com", 443, "/mcp");
        assert!(exact.matches(&u("https://mcp.example.com/mcp")));
        assert!(exact.matches(&u("https://mcp.example.com/mcp/sse")));
        assert!(!exact.matches(&u("https://mcp.example.com/mcpx")));
    }

    #[tokio::test]
    async fn f_pl_03_2_private_and_link_local_addresses_are_private() {
        for s in [
            "http://169.254.169.254/latest/meta-data",
            "http://10.0.0.1/",
            "http://172.16.5.4/",
            "http://192.168.1.1/",
            "http://127.0.0.1:8080/",
            "http://[::1]/",
            "http://[fd00::1]/",
            "http://[fe80::1]/",
            "http://localhost/",
            "http://[::ffff:169.254.169.254]/",
        ] {
            assert!(is_private_url(&u(s)).await, "{s}");
        }
        assert!(!is_private_url(&u("https://8.8.8.8/")).await);
    }
}
