use std::io;
use std::net::SocketAddr;

/// Refuse a listener bind address unless it is loopback-only.
///
/// This is a pure address-shape guard. It performs no socket I/O, so low-level
/// listener owners can call it before binding and still keep `nimbus-core` free
/// of host operations.
pub fn refuse_non_loopback_bind(bind_addr: SocketAddr) -> io::Result<()> {
    if bind_addr.ip().is_loopback() {
        return Ok(());
    }

    Err(io::Error::new(
        io::ErrorKind::InvalidInput,
        format!(
            "refusing non-loopback bind address {bind_addr}; use 127.0.0.1 or ::1 for local dev listeners"
        ),
    ))
}

/// True when `host` is a syntactically valid DNS hostname.
///
/// Each dot-separated label must be 1..=63 ASCII alphanumerics or `-`, must not
/// start or end with `-`, the whole name must be <= 253 chars, and the name must
/// have no leading or trailing dot. This is a pure *shape* check: it never
/// resolves the name and makes no judgement about whether the name points at an
/// internal/non-global address — callers (the egress PDP, the operator-policy
/// validator) layer their own SSRF / bind-target rules on top. It is the single
/// canonical hostname validator shared by `nimbus-egress` and `nimbus-tenant`
/// so the two can no longer drift apart. (egress audit M2.)
pub fn is_valid_dns_hostname(host: &str) -> bool {
    if host.len() > 253 || host.starts_with('.') || host.ends_with('.') {
        return false;
    }
    host.split('.').all(|label| {
        !label.is_empty()
            && label.len() <= 63
            && !label.starts_with('-')
            && !label.ends_with('-')
            && label
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
    })
}

#[cfg(test)]
mod tests {
    use std::io::ErrorKind;

    use super::*;

    #[test]
    fn refuse_non_loopback_bind_allows_ipv4_loopback() {
        let addr = "127.0.0.1:6380".parse().expect("addr parses");
        refuse_non_loopback_bind(addr).expect("loopback should be allowed");
    }

    #[test]
    fn refuse_non_loopback_bind_allows_ipv6_loopback() {
        let addr = "[::1]:6380".parse().expect("addr parses");
        refuse_non_loopback_bind(addr).expect("loopback should be allowed");
    }

    #[test]
    fn refuse_non_loopback_bind_rejects_wildcard() {
        let addr = "0.0.0.0:6380".parse().expect("addr parses");
        let error = refuse_non_loopback_bind(addr).expect_err("wildcard should be rejected");
        assert_eq!(error.kind(), ErrorKind::InvalidInput);
    }

    #[test]
    fn is_valid_dns_hostname_accepts_concrete_names() {
        for host in [
            "api.stripe.com",
            "a",
            "a-b.example.com",
            "xn--bcher-kva.example",
            "host123.sub-domain.example-1.com",
        ] {
            assert!(
                is_valid_dns_hostname(host),
                "{host:?} should be a valid DNS hostname"
            );
        }
    }

    #[test]
    fn is_valid_dns_hostname_rejects_malformed_names() {
        // Each case must FAIL the validator; if the corresponding guard inside
        // `is_valid_dns_hostname` is deleted, the matching case starts passing
        // and this test fails, so none of these assertions is vacuous.
        for host in [
            "",                 // empty
            ".example.com",     // leading dot
            "example.com.",     // trailing dot
            "exam ple.com",     // whitespace is not alphanumeric/-
            "bad_host",         // underscore is not allowed
            "-bad.example.com", // label starts with -
            "bad-.example.com", // label ends with -
            "a..b",             // empty interior label
        ] {
            assert!(
                !is_valid_dns_hostname(host),
                "{host:?} must be rejected as a DNS hostname"
            );
        }
        // A 64-char label (one over the per-label limit) must be rejected.
        let over_label = format!("{}.com", "a".repeat(64));
        assert!(
            !is_valid_dns_hostname(&over_label),
            "a 64-char label must be rejected"
        );
        // A 63-char label is exactly at the limit and is accepted.
        let at_label = format!("{}.com", "a".repeat(63));
        assert!(
            is_valid_dns_hostname(&at_label),
            "a 63-char label is within the limit"
        );
        // A name longer than 253 chars must be rejected.
        let over_total = vec!["a"; 200].join(".") + ".example.com";
        assert!(
            over_total.len() > 253,
            "fixture must exceed the 253-char limit"
        );
        assert!(
            !is_valid_dns_hostname(&over_total),
            "a name longer than 253 chars must be rejected"
        );
    }
}
