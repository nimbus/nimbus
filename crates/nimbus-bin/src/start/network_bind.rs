use std::error::Error;
use std::fmt;
use std::net::IpAddr;

use nimbus_server::LocalAdminTokenRecord;
use time::{Duration, OffsetDateTime};

/// Maximum age of a `rotated_at` timestamp before the non-loopback bind
/// tripwire refuses to expose the local admin token publicly. Operators
/// must run `nimbus auth rotate-admin` within this window to keep a
/// `--allow-network` bind active.
pub(super) const ADMIN_TOKEN_FRESHNESS_WINDOW: Duration = Duration::days(30);

/// Pre-bind gate for `nimbus start` host selection. Loopback hosts always
/// pass. Non-loopback hosts require the explicit `--allow-network` opt-in
/// AND a freshly rotated admin token.
pub(super) fn enforce_loopback_or_allow_network(
    host: &str,
    allow_network: bool,
    admin_token: &LocalAdminTokenRecord,
    now: OffsetDateTime,
) -> Result<(), NetworkBindError> {
    if host_is_loopback(host) {
        return Ok(());
    }
    if !allow_network {
        return Err(NetworkBindError::NonLoopbackRequiresOptIn {
            host: host.to_string(),
        });
    }
    if !admin_token.rotation_is_fresh(now, ADMIN_TOKEN_FRESHNESS_WINDOW) {
        return Err(NetworkBindError::StaleAdminTokenRotation {
            host: host.to_string(),
        });
    }
    Ok(())
}

fn host_is_loopback(host: &str) -> bool {
    if host.eq_ignore_ascii_case("localhost") {
        return true;
    }
    match host.parse::<IpAddr>() {
        Ok(ip) => ip.is_loopback(),
        Err(_) => false,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum NetworkBindError {
    NonLoopbackRequiresOptIn { host: String },
    StaleAdminTokenRotation { host: String },
}

impl fmt::Display for NetworkBindError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NetworkBindError::NonLoopbackRequiresOptIn { host } => write!(
                f,
                "refusing to bind on non-loopback host `{host}` without --allow-network.\n\
                 \n\
                 The local admin token grants full control of this server. Binding it on a\n\
                 public interface is opt-in. Re-run with `--allow-network` to acknowledge\n\
                 the exposure, or bind on a loopback address (127.0.0.1, ::1, or localhost)."
            ),
            NetworkBindError::StaleAdminTokenRotation { host } => write!(
                f,
                "refusing to bind on non-loopback host `{host}` with a stale local admin token.\n\
                 \n\
                 The local admin token has not been rotated within the last 30 days (or has\n\
                 never been rotated since first boot). Rotate it before exposing the server\n\
                 publicly:\n\
                 \n\
                     nimbus auth rotate-admin\n\
                 \n\
                 Then re-run `nimbus start --host {host} --allow-network`."
            ),
        }
    }
}

impl Error for NetworkBindError {}

#[cfg(test)]
mod tests {
    use super::*;

    fn admin_token(rotated_at: Option<&str>) -> LocalAdminTokenRecord {
        LocalAdminTokenRecord {
            version: 1,
            token: "nimbus_at_test".to_string(),
            generation: 1,
            issued_at: "2026-01-01T00:00:00Z".to_string(),
            scope: "local-admin".to_string(),
            rotated_at: rotated_at.map(|stamp| stamp.to_string()),
        }
    }

    fn fixed_now() -> OffsetDateTime {
        OffsetDateTime::parse(
            "2026-05-20T00:00:00Z",
            &time::format_description::well_known::Rfc3339,
        )
        .expect("fixed test timestamp should parse")
    }

    #[test]
    fn loopback_host_passes_without_allow_network() {
        let token = admin_token(None);
        let now = fixed_now();
        for host in ["127.0.0.1", "::1", "localhost", "LOCALHOST"] {
            enforce_loopback_or_allow_network(host, false, &token, now)
                .unwrap_or_else(|error| panic!("loopback host {host} should pass: {error}"));
        }
    }

    #[test]
    fn non_loopback_without_allow_network_is_refused_with_hint() {
        let token = admin_token(Some("2026-05-19T00:00:00Z"));
        let now = fixed_now();
        let error = enforce_loopback_or_allow_network("0.0.0.0", false, &token, now)
            .expect_err("non-loopback bind without --allow-network must be refused");
        match &error {
            NetworkBindError::NonLoopbackRequiresOptIn { host } => assert_eq!(host, "0.0.0.0"),
            other => panic!("expected NonLoopbackRequiresOptIn, got {other:?}"),
        }
        let message = error.to_string();
        assert!(
            message.contains("--allow-network"),
            "refusal must mention the opt-in flag, got: {message}"
        );
        assert!(
            message.contains("loopback"),
            "refusal must mention the loopback alternative, got: {message}"
        );
    }

    #[test]
    fn non_loopback_with_allow_network_and_never_rotated_token_triggers_tripwire() {
        let token = admin_token(None);
        let now = fixed_now();
        let error = enforce_loopback_or_allow_network("0.0.0.0", true, &token, now)
            .expect_err("never-rotated admin token must trip the rotation gate");
        match &error {
            NetworkBindError::StaleAdminTokenRotation { host } => assert_eq!(host, "0.0.0.0"),
            other => panic!("expected StaleAdminTokenRotation, got {other:?}"),
        }
        let message = error.to_string();
        assert!(
            message.contains("nimbus auth rotate-admin"),
            "tripwire must point at the rotate-admin command, got: {message}"
        );
    }

    #[test]
    fn non_loopback_with_allow_network_and_stale_rotation_triggers_tripwire() {
        let token = admin_token(Some("2026-01-01T00:00:00Z"));
        let now = fixed_now();
        let error = enforce_loopback_or_allow_network("203.0.113.5", true, &token, now)
            .expect_err("stale rotation must trip the gate");
        assert!(matches!(
            error,
            NetworkBindError::StaleAdminTokenRotation { .. }
        ));
        assert!(error.to_string().contains("nimbus auth rotate-admin"));
    }

    #[test]
    fn non_loopback_with_allow_network_and_fresh_rotation_passes() {
        let token = admin_token(Some("2026-05-15T00:00:00Z"));
        let now = fixed_now();
        enforce_loopback_or_allow_network("0.0.0.0", true, &token, now)
            .expect("fresh rotation under --allow-network should bind");
    }

    #[test]
    fn unparseable_rotation_timestamp_is_treated_as_stale() {
        let token = admin_token(Some("not-an-rfc3339-string"));
        let now = fixed_now();
        let error = enforce_loopback_or_allow_network("0.0.0.0", true, &token, now)
            .expect_err("unparseable rotated_at must be treated as stale");
        assert!(matches!(
            error,
            NetworkBindError::StaleAdminTokenRotation { .. }
        ));
    }
}
