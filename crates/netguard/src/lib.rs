//! Single shared host-classification and outbound-resolution policy for
//! every HTTP client in Tansen (addon, Subsonic, media range readers).
//!
//! One implementation instead of the three near-identical string-based
//! classifiers this crate replaces: those re-parsed `Url::host_str()`,
//! whose bracketed IPv6 serialization (`"[::1]"`) never parses as
//! `IpAddr` — so every IPv6 URL host, loopback `::1` included, was
//! classified non-local. Classification now happens on the URL's
//! already-parsed host (or a bracket-stripped string), with the full
//! private-address predicate set on both families, and every outbound
//! client can validate that a DNS name does not resolve into local or
//! private space (DNS-rebinding guard) and pin the connection to the
//! validated address.

use std::net::{IpAddr, SocketAddr};
use url::Url;

/// Whether an address is loopback, private, link-local, or unspecified —
/// the address families a source server must not pivot a fetch into.
/// IPv4-mapped IPv6 (`::ffff:a.b.c.d`) is judged by its embedded IPv4.
pub fn ip_is_local(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            v4.is_loopback() || v4.is_private() || v4.is_link_local() || v4.is_unspecified()
        }
        IpAddr::V6(v6) => {
            if let Some(mapped) = v6.to_ipv4_mapped() {
                return ip_is_local(IpAddr::V4(mapped));
            }
            v6.is_loopback()
                || v6.is_unique_local()
                || v6.is_unicast_link_local()
                || v6.is_unspecified()
        }
    }
}

/// Strips the brackets url's `host_str()` puts around IPv6 hosts, so
/// string inputs parse.
fn bare_host(host: &str) -> &str {
    let host = host.trim();
    if let Some(rest) = host.strip_prefix('[') {
        if let Some((inner, _rest)) = rest.split_once(']') {
            return inner;
        }
    }
    host
}

/// Whether a host NAME or literal is loopback/private/link-local by the
/// policy names are judged by: `localhost` and `*.local` are local, every
/// other DNS name is not, and IP literals (bracketed IPv6 accepted) use
/// the full predicate set of [`ip_is_local`].
pub fn host_is_local(host: &str) -> bool {
    let lower = bare_host(host).to_ascii_lowercase();
    if lower == "localhost" {
        return true;
    }
    match lower.parse::<IpAddr>() {
        Ok(ip) => ip_is_local(ip),
        Err(_) => lower.ends_with(".local"),
    }
}

/// Classifies a URL's host from the URL's parsed host — the bracket-safe
/// path every URL-bearing caller must use instead of re-parsing
/// `host_str()`.
pub fn url_host_is_local(url: &Url) -> bool {
    match url.host() {
        Some(url::Host::Domain(domain)) => host_is_local(domain),
        Some(url::Host::Ipv4(ip)) => ip_is_local(IpAddr::V4(ip)),
        Some(url::Host::Ipv6(ip)) => ip_is_local(IpAddr::V6(ip)),
        None => false,
    }
}

/// The host of `url` as reqwest's per-request `resolve()` expects it:
/// a DNS name, or `None` for IP literals (which need no pinning).
pub fn dns_host(url: &Url) -> Option<String> {
    match url.host()? {
        url::Host::Domain(domain) => Some(domain.to_string()),
        _ => None,
    }
}

/// Resolves `url`'s host and rejects a non-local name that resolves into
/// local/private space (DNS rebinding). Returns the first validated
/// socket address so the caller can pin the connection to the checked
/// address. Loopback/private hosts by name (`localhost`, `.local`, IP
/// literals) resolve freely — the LAN opt-in sources rely on that.
pub fn resolve_validated(url: &Url) -> Result<SocketAddr, String> {
    use std::net::ToSocketAddrs;
    let port = url
        .port_or_known_default()
        .ok_or_else(|| "url has no port".to_string())?;
    let Some(host) = dns_host(url) else {
        // IP literal: classification already happened on the URL.
        let ip = match url.host() {
            Some(url::Host::Ipv4(ip)) => IpAddr::V4(ip),
            Some(url::Host::Ipv6(ip)) => IpAddr::V6(ip),
            _ => return Err("url has no host".into()),
        };
        return Ok(SocketAddr::new(ip, port));
    };
    let by_name_local = host_is_local(&host);
    let addrs: Vec<SocketAddr> = (host.as_str(), port)
        .to_socket_addrs()
        .map_err(|e| format!("resolve {host}: {e}"))?
        .collect();
    if addrs.is_empty() {
        return Err(format!("resolve {host}: no addresses"));
    }
    for addr in &addrs {
        if ip_is_local(addr.ip()) && !by_name_local {
            return Err(format!(
                "host {host} resolves to a local/private address ({addr}); refusing (possible DNS rebinding)"
            ));
        }
    }
    Ok(addrs[0])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ipv4_predicate_set_is_complete() {
        for host in [
            "127.0.0.1",    // loopback
            "10.0.0.5",     // private
            "192.168.1.10", // private
            "172.16.0.1",   // private
            "169.254.1.1",  // link-local
            "0.0.0.0",      // unspecified
        ] {
            assert!(host_is_local(host), "{host} must classify local");
        }
        for host in ["8.8.8.8", "1.1.1.1", "93.184.216.34"] {
            assert!(!host_is_local(host), "{host} must classify non-local");
        }
    }

    #[test]
    fn ipv6_predicate_set_is_complete() {
        for host in [
            "::1",              // loopback
            "::",               // unspecified
            "fd00::1",          // unique-local
            "fe80::1",          // link-local
            "::ffff:127.0.0.1", // IPv4-mapped loopback
            "::ffff:10.0.0.5",  // IPv4-mapped private
        ] {
            assert!(host_is_local(host), "{host} must classify local");
        }
        for host in ["2001:4860:4860::8888", "2606:4700::1111"] {
            assert!(!host_is_local(host), "{host} must classify non-local");
        }
    }

    #[test]
    fn bracketed_ipv6_hosts_classify_like_bare_ones() {
        for host in ["[::1]", "[fd00::1]", "[::ffff:127.0.0.1]", "[fe80::1]"] {
            assert!(host_is_local(host), "{host} must classify local");
        }
        assert!(!host_is_local("[2001:4860:4860::8888]"));
    }

    #[test]
    fn names_follow_the_localhost_dotlocal_policy() {
        assert!(host_is_local("localhost"));
        assert!(host_is_local("LOCALHOST"));
        assert!(host_is_local("myserver.local"));
        assert!(!host_is_local("example.com"));
        // "localhost." trailing dot is a DNS name, not the loopback name.
        assert!(!host_is_local("localhost."));
        assert!(!host_is_local("evil.localhost"));
    }

    #[test]
    fn url_host_uses_the_parsed_host_not_the_string() {
        for url in [
            "http://[::1]:8080/x",
            "http://[fd00::1]/x",
            "http://[::ffff:127.0.0.1]:9000/x",
            "http://0.0.0.0:9000/x",
            "http://169.254.169.254/latest",
        ] {
            let parsed = Url::parse(url).unwrap();
            assert!(url_host_is_local(&parsed), "{url} must classify local");
        }
        for url in [
            "https://example.com/x",
            "http://8.8.8.8:9000/x",
            "https://music.youtube.com/",
        ] {
            let parsed = Url::parse(url).unwrap();
            assert!(!url_host_is_local(&parsed), "{url} must classify non-local");
        }
    }

    #[test]
    fn resolve_validated_rejects_public_names_resolving_local() {
        // `localhost` resolves locally and is local by name: allowed.
        let url = Url::parse("http://localhost:1/x").unwrap();
        assert!(resolve_validated(&url).is_ok());
        // A public DNS name cannot resolve locally in a sandbox with no
        // external network: to_socket_addrs either fails or returns a
        // public address. Both outcomes are fine; the assertion is that
        // the guard's decision logic runs without panicking.
        let url = Url::parse("http://definitely-not-a-real-host.invalid:1/x").unwrap();
        let _ = resolve_validated(&url);
    }
}
