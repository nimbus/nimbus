use std::error::Error;
use std::fmt;
use std::net::IpAddr;

use nimbus_server::LocalAdminTokenRecord;
use time::format_description::well_known::Rfc3339;
use time::{Duration, OffsetDateTime};

/// Age of a `rotated_at` timestamp past which a non-loopback bind logs a
/// rotation-hygiene warning naming `nimbus auth rotate-admin`. Advisory
/// only: a long-running public server must always be able to restart.
/// The hard requirement is that an explicit rotation happened at least
/// once — the auto-minted first-boot token is never exposed publicly.
pub(super) const ADMIN_TOKEN_ROTATION_WARNING_WINDOW: Duration = Duration::days(30);

/// Stage 1: refuse non-loopback hosts unless the operator passed
/// `--allow-network`. Loopback hosts always pass. Called before any
/// expensive startup work (codegen, registry loads) so a typo'd `--host`
/// fails fast.
pub(super) fn ensure_host_opt_in(host: &str, allow_network: bool) -> Result<(), NetworkBindError> {
    if host_is_loopback(host) {
        return Ok(());
    }
    if !allow_network {
        return Err(NetworkBindError::NonLoopbackRequiresOptIn {
            host: host.to_string(),
        });
    }
    Ok(())
}

/// Refuse to enable the Firebase **dev-mode token-verification bypass** on any
/// non-loopback host.
///
/// The bypass fabricates a *verified* Firebase project from an unsigned,
/// caller-controlled emulator token (see
/// `nimbus_auth::firebase_emulator_verification_bypass_principal_from_bearer`).
/// On a network-reachable listener that is a verified-claim forgery primitive, so
/// it is allowed only on loopback — the firebase analog of "an unbound credential
/// may bind loopback-only" (`guard_bind_address`). This makes the forgery
/// structurally unreachable on a public bind by construction, not by convention.
/// Called at boot once the adapter configs are resolved. Loopback always passes;
/// when the bypass is disabled there is nothing to guard.
pub(super) fn ensure_firebase_bypass_loopback_only(
    host: &str,
    firebase_bypass_enabled: bool,
) -> Result<(), NetworkBindError> {
    if !firebase_bypass_enabled || host_is_loopback(host) {
        return Ok(());
    }
    Err(NetworkBindError::FirebaseBypassRequiresLoopback {
        host: host.to_string(),
    })
}

/// Stage 2: refuse non-loopback hosts whose admin token has never been
/// explicitly rotated (`rotated_at` absent or unparseable — the
/// auto-minted first-boot token). A token rotated longer ago than
/// [`ADMIN_TOKEN_ROTATION_WARNING_WINDOW`] binds successfully and returns
/// a [`StaleRotationWarning`] for the caller to log; restarts of a
/// long-running public server must not be refused on age alone. Loopback
/// hosts always pass. Called after the admin token is loaded from disk.
pub(super) fn ensure_admin_token_rotated_for_public_bind(
    host: &str,
    admin_token: &LocalAdminTokenRecord,
    now: OffsetDateTime,
) -> Result<Option<StaleRotationWarning>, NetworkBindError> {
    if host_is_loopback(host) {
        return Ok(None);
    }
    let Some(rotated_at) = admin_token.rotated_at.as_deref() else {
        return Err(NetworkBindError::NeverRotatedAdminToken {
            host: host.to_string(),
        });
    };
    let Ok(rotated) = OffsetDateTime::parse(rotated_at, &Rfc3339) else {
        // Fail closed: an unreadable rotation stamp proves nothing, so it
        // gets the same treatment as no rotation at all.
        return Err(NetworkBindError::NeverRotatedAdminToken {
            host: host.to_string(),
        });
    };
    let age = now - rotated;
    if age >= ADMIN_TOKEN_ROTATION_WARNING_WINDOW {
        return Ok(Some(StaleRotationWarning {
            age_days: age.whole_days(),
        }));
    }
    Ok(None)
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

/// Advisory rotation-hygiene notice returned when a public bind proceeds
/// on a token whose last explicit rotation is older than the warning
/// window. The caller logs it; it never blocks startup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct StaleRotationWarning {
    pub(super) age_days: i64,
}

impl fmt::Display for StaleRotationWarning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "local admin token was last rotated {} days ago; rotate it with \
             `nimbus auth rotate-admin`",
            self.age_days
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum NetworkBindError {
    NonLoopbackRequiresOptIn { host: String },
    NeverRotatedAdminToken { host: String },
    FirebaseBypassRequiresLoopback { host: String },
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
            NetworkBindError::NeverRotatedAdminToken { host } => write!(
                f,
                "refusing to bind on non-loopback host `{host}` with a never-rotated local\n\
                 admin token.\n\
                 \n\
                 The auto-minted first-boot token must be explicitly rotated once before the\n\
                 server is exposed on a public interface:\n\
                 \n\
                     nimbus auth rotate-admin\n\
                 \n\
                 Then re-run `nimbus start --host {host} --allow-network`."
            ),
            NetworkBindError::FirebaseBypassRequiresLoopback { host } => write!(
                f,
                "refusing to enable the Firebase emulator token-verification bypass on\n\
                 non-loopback host `{host}`.\n\
                 \n\
                 The bypass accepts unsigned emulator tokens and fabricates a verified\n\
                 Firebase project from their (unverified) claims — a credential-forgery\n\
                 primitive that must never be reachable over the network. It is allowed\n\
                 only on a loopback address (127.0.0.1, ::1, or localhost) for local dev.\n\
                 \n\
                 Bind on loopback for emulator dev, or configure real project bindings\n\
                 (NIMBUS_FIREBASE_PROJECTS) and drop the bypass for a public bind."
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

    // Stage 1 — `ensure_host_opt_in` (cheap pre-codegen check).

    #[test]
    fn ensure_host_opt_in_passes_loopback_without_allow_network() {
        for host in ["127.0.0.1", "::1", "localhost", "LOCALHOST"] {
            ensure_host_opt_in(host, false)
                .unwrap_or_else(|error| panic!("loopback host {host} should pass: {error}"));
        }
    }

    #[test]
    fn ensure_host_opt_in_refuses_non_loopback_without_flag_with_hint() {
        let error = ensure_host_opt_in("0.0.0.0", false)
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
    fn ensure_host_opt_in_passes_non_loopback_when_flag_set() {
        ensure_host_opt_in("0.0.0.0", true)
            .expect("--allow-network should let non-loopback through stage 1");
        ensure_host_opt_in("203.0.113.5", true)
            .expect("--allow-network should let non-loopback through stage 1");
    }

    // Firebase token-verification-bypass guard (#24).

    #[test]
    fn firebase_bypass_guard_passes_when_bypass_disabled_on_any_host() {
        // No bypass -> nothing to guard, even on a public host (and even with
        // --allow-network already granted for the admin surface).
        for host in ["127.0.0.1", "0.0.0.0", "203.0.113.5"] {
            ensure_firebase_bypass_loopback_only(host, false)
                .unwrap_or_else(|error| panic!("disabled bypass should pass on {host}: {error}"));
        }
    }

    #[test]
    fn firebase_bypass_guard_passes_on_loopback_when_enabled() {
        for host in ["127.0.0.1", "::1", "localhost", "LOCALHOST"] {
            ensure_firebase_bypass_loopback_only(host, true).unwrap_or_else(|error| {
                panic!("enabled bypass should pass on loopback {host}: {error}")
            });
        }
    }

    #[test]
    fn firebase_bypass_guard_refuses_enabled_bypass_on_non_loopback() {
        // The forgery primitive must be unreachable on a public bind even when
        // --allow-network was granted — the guard is independent of that flag.
        for host in ["0.0.0.0", "203.0.113.5", "::"] {
            let error = ensure_firebase_bypass_loopback_only(host, true).expect_err(
                "the verification bypass must be refused on a non-loopback bind",
            );
            match &error {
                NetworkBindError::FirebaseBypassRequiresLoopback { host: h } => {
                    assert_eq!(h, host)
                }
                other => panic!("expected FirebaseBypassRequiresLoopback, got {other:?}"),
            }
            let message = error.to_string();
            assert!(
                message.contains("loopback"),
                "refusal must point to the loopback requirement, got: {message}"
            );
            assert!(
                message.contains("NIMBUS_FIREBASE_PROJECTS"),
                "refusal must point to the real-bindings alternative, got: {message}"
            );
        }
    }

    // Stage 2 — `ensure_admin_token_rotated_for_public_bind`
    // (post-token-load check).

    #[test]
    fn public_bind_passes_loopback_regardless_of_rotation() {
        let token = admin_token(None);
        let now = fixed_now();
        for host in ["127.0.0.1", "::1", "localhost"] {
            let outcome = ensure_admin_token_rotated_for_public_bind(host, &token, now)
                .unwrap_or_else(|error| panic!("loopback host {host} should pass: {error}"));
            assert_eq!(outcome, None, "loopback binds never warn");
        }
    }

    #[test]
    fn public_bind_requires_explicit_rotation() {
        let token = admin_token(None);
        let now = fixed_now();
        let error = ensure_admin_token_rotated_for_public_bind("0.0.0.0", &token, now)
            .expect_err("never-rotated admin token must refuse a public bind");
        match &error {
            NetworkBindError::NeverRotatedAdminToken { host } => assert_eq!(host, "0.0.0.0"),
            other => panic!("expected NeverRotatedAdminToken, got {other:?}"),
        }
        let message = error.to_string();
        assert!(
            message.contains("nimbus auth rotate-admin"),
            "tripwire must point at the rotate-admin command, got: {message}"
        );
    }

    #[test]
    fn public_bind_restart_does_not_retrip_freshness() {
        // Rotated 139 days before `now` — far past the warning window. A
        // restart of a long-running public server must bind successfully
        // and surface only an advisory warning.
        let token = admin_token(Some("2026-01-01T00:00:00Z"));
        let now = fixed_now();
        let warning = ensure_admin_token_rotated_for_public_bind("203.0.113.5", &token, now)
            .expect("an old-but-explicit rotation must not refuse the bind")
            .expect("an old rotation should produce an advisory warning");
        assert_eq!(warning.age_days, 139);
        let message = warning.to_string();
        assert!(
            message.contains("nimbus auth rotate-admin"),
            "warning must point at the rotate-admin command, got: {message}"
        );
        assert!(
            message.contains("139 days"),
            "warning must state the rotation age, got: {message}"
        );
    }

    #[test]
    fn public_bind_passes_fresh_rotation_without_warning() {
        let token = admin_token(Some("2026-05-15T00:00:00Z"));
        let now = fixed_now();
        let outcome = ensure_admin_token_rotated_for_public_bind("0.0.0.0", &token, now)
            .expect("fresh rotation should pass stage 2");
        assert_eq!(outcome, None, "fresh rotations must not warn");
    }

    #[test]
    fn public_bind_treats_unparseable_rotated_at_as_never_rotated() {
        let token = admin_token(Some("not-an-rfc3339-string"));
        let now = fixed_now();
        let error = ensure_admin_token_rotated_for_public_bind("0.0.0.0", &token, now)
            .expect_err("unparseable rotated_at must fail closed");
        assert!(matches!(
            error,
            NetworkBindError::NeverRotatedAdminToken { .. }
        ));
    }
}
