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
}
