//! A pocket URI parser.
//!
//! The core only ever needs scheme/host/port/path/query out of an absolute URI
//! (HTTP proxy absolute-form requests and the speed-test endpoints). Pulling in
//! the `url` crate for that drags along `idna`'s Unicode tables — hundreds of
//! kilobytes of compiled data for a feature we never use. This module covers
//! exactly what we need, and nothing else.

/// A parsed absolute URI: `scheme://[user[:pass]@]host[:port][/path][?query]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Uri {
    pub scheme: String,
    pub user: Option<String>,
    pub password: Option<String>,
    pub host: String,
    pub port: u16,
    pub path: String,
    pub query: Option<String>,
}

impl Uri {
    /// Path plus `?query`, ready to put on a request line.
    pub fn request_target(&self) -> String {
        match &self.query {
            Some(q) if !q.is_empty() => format!("{}?{}", self.path, q),
            _ => self.path.clone(),
        }
    }

    pub fn is_secure(&self) -> bool {
        matches!(self.scheme.as_str(), "https" | "wss" | "tls" | "ssl")
    }
}

/// Default port for a scheme, when the authority does not spell one out.
pub fn default_port(scheme: &str) -> Option<u16> {
    Some(match scheme {
        "http" | "ws" => 80,
        "https" | "wss" => 443,
        "socks" | "socks5" | "socks5h" | "socks4" => 1080,
        "ssh" => 22,
        "ftp" => 21,
        _ => return None,
    })
}

/// Parse an absolute URI. Returns `None` when there is no scheme or no host.
pub fn parse(input: &str) -> Option<Uri> {
    let raw = input.trim();
    let (scheme, rest) = raw.split_once("://")?;
    if scheme.is_empty()
        || !scheme
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"+-.".contains(&b))
    {
        return None;
    }
    let scheme = scheme.to_ascii_lowercase();

    // authority ends at the first '/', '?' or '#'
    let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let authority = &rest[..end];
    let tail = &rest[end..];

    let (userinfo, hostport) = match authority.rsplit_once('@') {
        Some((u, h)) => (Some(u), h),
        None => (None, authority),
    };
    let (user, password) = match userinfo {
        Some(u) => match u.split_once(':') {
            Some((a, b)) => (Some(a.to_string()), Some(b.to_string())),
            None => (Some(u.to_string()), None),
        },
        None => (None, None),
    };

    // host may be an IPv6 literal in brackets
    let (host, port) = if let Some(close) = hostport.find(']') {
        let host = hostport[..=close]
            .trim_start_matches('[')
            .trim_end_matches(']');
        let port = hostport[close + 1..]
            .strip_prefix(':')
            .and_then(|p| p.parse().ok());
        (host.to_string(), port)
    } else {
        match hostport.rsplit_once(':') {
            Some((h, p)) => (h.to_string(), p.parse().ok()),
            None => (hostport.to_string(), None),
        }
    };
    if host.is_empty() {
        return None;
    }

    // strip the fragment, then split path from query
    let tail = tail.split('#').next().unwrap_or("");
    let (path, query) = match tail.split_once('?') {
        Some((p, q)) => (p, Some(q.to_string())),
        None => (tail, None),
    };

    Some(Uri {
        port: port.or_else(|| default_port(&scheme)).unwrap_or(0),
        scheme,
        user,
        password,
        host,
        path: if path.is_empty() {
            "/".into()
        } else {
            path.into()
        },
        query,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_http_url() {
        let u = parse("http://example.com/index.html").unwrap();
        assert_eq!(u.scheme, "http");
        assert_eq!(u.host, "example.com");
        assert_eq!(u.port, 80);
        assert_eq!(u.path, "/index.html");
        assert_eq!(u.query, None);
        assert!(!u.is_secure());
    }

    #[test]
    fn https_with_port_query_and_fragment() {
        let u = parse("https://speed.example.net:8443/dl?size=25MB&x=1#frag").unwrap();
        assert_eq!(u.port, 8443);
        assert_eq!(u.path, "/dl");
        assert_eq!(u.query.as_deref(), Some("size=25MB&x=1"));
        assert_eq!(u.request_target(), "/dl?size=25MB&x=1");
        assert!(u.is_secure());
    }

    #[test]
    fn default_ports_by_scheme() {
        assert_eq!(parse("https://a.example").unwrap().port, 443);
        assert_eq!(parse("ws://a.example").unwrap().port, 80);
        assert_eq!(parse("socks5://a.example").unwrap().port, 1080);
        assert_eq!(parse("ssh://a.example").unwrap().port, 22);
    }

    #[test]
    fn userinfo_and_empty_path() {
        let u = parse("http://bob:s3cret@proxy.local:3128").unwrap();
        assert_eq!(u.user.as_deref(), Some("bob"));
        assert_eq!(u.password.as_deref(), Some("s3cret"));
        assert_eq!(u.host, "proxy.local");
        assert_eq!(u.port, 3128);
        assert_eq!(u.path, "/");
    }

    #[test]
    fn ipv6_literal() {
        let u = parse("http://[2606:4700::1111]:8080/x").unwrap();
        assert_eq!(u.host, "2606:4700::1111");
        assert_eq!(u.port, 8080);
        assert_eq!(u.path, "/x");
    }

    #[test]
    fn rejects_garbage() {
        assert!(parse("not a url").is_none());
        assert!(parse("http://").is_none());
        assert!(parse("/relative/path").is_none());
    }
}
