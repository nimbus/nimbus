use std::io;
use std::net::TcpListener;
use std::path::Path;

use crate::wire_credentials::{WireCredentials, load_or_generate};

use super::surfaces::WireSurfaces;

pub(super) const MONGODB_CONVENTIONAL_PORT: u16 = 27017;
pub(super) const DYNAMODB_CONVENTIONAL_PORT: u16 = 8000;

/// A resolved wire-listener port under the D4 policy: a *detected* surface
/// prefers its conventional port (stable, recognizable connection strings)
/// and falls back to an ephemeral port when it is busy; an *undetected*
/// surface always binds an ephemeral port, so dev never squats a
/// conventional port the app isn't using — a real `mongod` beside a
/// pure-Convex app sees zero interference.
#[derive(Debug, Clone, Copy)]
pub(super) struct WireListenerPort {
    pub(super) port: u16,
    /// True when the conventional port was busy and an ephemeral port was
    /// chosen instead; the run path reports this, and the Nimbus-owned
    /// `.env.local` key carries the real endpoint either way.
    pub(super) conventional_fallback: bool,
}

/// Resolved wire-listener ports plus the shared persisted credentials
/// (D4/D5). Always resolved — listeners are always available (D6) — while
/// detection only chooses port prominence and what `.env.local` carries.
#[derive(Debug)]
pub(super) struct WirePlan {
    pub(super) mongodb_port: WireListenerPort,
    pub(super) dynamodb_port: WireListenerPort,
    pub(super) credentials: WireCredentials,
}

impl WirePlan {
    /// The Nimbus-owned `.env.local` entries for the *detected* surfaces.
    /// Undetected surfaces stay out of the app's env file entirely: their
    /// listeners are still up (D6), but on ephemeral ports nothing in the
    /// app references.
    pub(super) fn env_local_entries(&self, surfaces: WireSurfaces) -> Vec<(&'static str, String)> {
        let mut entries = Vec::new();
        if surfaces.mongodb {
            entries.push((
                "NIMBUS_MONGODB_URL",
                format!(
                    "mongodb://{}:{}@127.0.0.1:{}/",
                    self.credentials.mongodb_username,
                    self.credentials.mongodb_password,
                    self.mongodb_port.port
                ),
            ));
        }
        if surfaces.dynamodb {
            entries.push((
                "NIMBUS_DYNAMODB_ENDPOINT",
                format!("http://127.0.0.1:{}", self.dynamodb_port.port),
            ));
            entries.push((
                "NIMBUS_DYNAMODB_ACCESS_KEY_ID",
                self.credentials.dynamodb_access_key_id.clone(),
            ));
            entries.push((
                "NIMBUS_DYNAMODB_SECRET_ACCESS_KEY",
                self.credentials.dynamodb_secret_access_key.clone(),
            ));
        }
        entries
    }

    #[cfg(test)]
    pub(super) fn fixture() -> Self {
        Self {
            mongodb_port: WireListenerPort {
                port: MONGODB_CONVENTIONAL_PORT,
                conventional_fallback: false,
            },
            dynamodb_port: WireListenerPort {
                port: DYNAMODB_CONVENTIONAL_PORT,
                conventional_fallback: false,
            },
            credentials: WireCredentials {
                mongodb_username: "nimbus".to_owned(),
                mongodb_password: "0123456789abcdef0123456789abcdef".to_owned(),
                dynamodb_access_key_id: "AKIA0123456789ABCDEF".to_owned(),
                dynamodb_secret_access_key: "0123456789abcdef0123456789abcdef01234567".to_owned(),
            },
        }
    }
}

pub(super) fn resolve_wire_plan(surfaces: WireSurfaces, data_dir: &Path) -> io::Result<WirePlan> {
    let credentials = load_or_generate(data_dir)?;
    Ok(WirePlan {
        mongodb_port: resolve_listener_port(surfaces.mongodb, MONGODB_CONVENTIONAL_PORT)?,
        dynamodb_port: resolve_listener_port(surfaces.dynamodb, DYNAMODB_CONVENTIONAL_PORT)?,
        credentials,
    })
}

/// One stderr notice per detected surface whose conventional port was busy,
/// so the developer learns where the endpoint actually lives; the
/// Nimbus-owned `.env.local` key already carries the real port, so apps
/// reading it keep working without edits.
pub(super) fn port_fallback_notices(plan: &WirePlan, surfaces: WireSurfaces) -> Vec<String> {
    let mut notices = Vec::new();
    if surfaces.mongodb && plan.mongodb_port.conventional_fallback {
        notices.push(format!(
            "MongoDB conventional port {MONGODB_CONVENTIONAL_PORT} is busy; \
             using 127.0.0.1:{} (recorded in .env.local)",
            plan.mongodb_port.port
        ));
    }
    if surfaces.dynamodb && plan.dynamodb_port.conventional_fallback {
        notices.push(format!(
            "DynamoDB conventional port {DYNAMODB_CONVENTIONAL_PORT} is busy; \
             using 127.0.0.1:{} (recorded in .env.local)",
            plan.dynamodb_port.port
        ));
    }
    notices
}

fn resolve_listener_port(detected: bool, conventional: u16) -> io::Result<WireListenerPort> {
    if detected {
        return match TcpListener::bind(("127.0.0.1", conventional)) {
            Ok(probe) => {
                drop(probe);
                Ok(WireListenerPort {
                    port: conventional,
                    conventional_fallback: false,
                })
            }
            Err(_) => Ok(WireListenerPort {
                port: ephemeral_port()?,
                conventional_fallback: true,
            }),
        };
    }
    Ok(WireListenerPort {
        port: ephemeral_port()?,
        conventional_fallback: false,
    })
}

fn ephemeral_port() -> io::Result<u16> {
    let listener = TcpListener::bind(("127.0.0.1", 0))?;
    Ok(listener.local_addr()?.port())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn surfaces(mongodb: bool, dynamodb: bool) -> WireSurfaces {
        WireSurfaces {
            mongodb,
            dynamodb,
            aws_sdk_v2_hint: false,
        }
    }

    #[test]
    fn detected_surface_prefers_conventional_port() {
        // Probe a port the OS just confirmed free, then resolve against it
        // as the "conventional" port: a detected surface must take it.
        let free = ephemeral_port().expect("probe a free port");
        let resolved = resolve_listener_port(true, free).expect("resolve");
        assert_eq!(resolved.port, free);
        assert!(!resolved.conventional_fallback);
    }

    #[test]
    fn port_conflict_fallback_updates_nimbus_owned_key() {
        let blocker = TcpListener::bind(("127.0.0.1", 0)).expect("blocker");
        let blocked_port = blocker.local_addr().expect("blocker addr").port();

        let resolved = resolve_listener_port(true, blocked_port).expect("resolve");
        assert!(resolved.conventional_fallback);
        assert_ne!(resolved.port, blocked_port);

        let mut plan = WirePlan::fixture();
        plan.mongodb_port = resolved;
        let entries = plan.env_local_entries(surfaces(true, false));
        let (key, url) = &entries[0];
        assert_eq!(*key, "NIMBUS_MONGODB_URL");
        assert!(
            url.contains(&format!(":{}/", resolved.port)),
            "env key must carry the fallback port: {url}"
        );
        assert!(
            !url.contains(&format!(":{blocked_port}/")),
            "env key must not carry the busy conventional port: {url}"
        );

        let notices = port_fallback_notices(&plan, surfaces(true, false));
        assert_eq!(notices.len(), 1);
        assert!(notices[0].contains(&resolved.port.to_string()));
    }

    #[test]
    fn undetected_surfaces_take_ephemeral_ports() {
        // Hold the would-be conventional port busy: an undetected surface
        // must neither take it nor report a fallback, because it never
        // probes the conventional port at all.
        let blocker = TcpListener::bind(("127.0.0.1", 0)).expect("blocker");
        let conventional = blocker.local_addr().expect("blocker addr").port();

        let resolved = resolve_listener_port(false, conventional).expect("resolve");
        assert_ne!(resolved.port, conventional);
        assert!(!resolved.conventional_fallback);
    }

    #[test]
    fn env_entries_cover_only_detected_surfaces() {
        let plan = WirePlan::fixture();

        assert!(plan.env_local_entries(surfaces(false, false)).is_empty());

        let mongo_only = plan.env_local_entries(surfaces(true, false));
        assert_eq!(
            mongo_only.iter().map(|(key, _)| *key).collect::<Vec<_>>(),
            vec!["NIMBUS_MONGODB_URL"]
        );

        let both = plan.env_local_entries(surfaces(true, true));
        let keys: Vec<_> = both.iter().map(|(key, _)| *key).collect();
        assert_eq!(
            keys,
            vec![
                "NIMBUS_MONGODB_URL",
                "NIMBUS_DYNAMODB_ENDPOINT",
                "NIMBUS_DYNAMODB_ACCESS_KEY_ID",
                "NIMBUS_DYNAMODB_SECRET_ACCESS_KEY",
            ]
        );
        assert!(keys.iter().all(|key| key.starts_with("NIMBUS_")));
    }

    #[test]
    fn fallback_notices_cover_only_detected_fallbacks() {
        let mut plan = WirePlan::fixture();
        plan.mongodb_port.conventional_fallback = true;
        plan.dynamodb_port.conventional_fallback = true;

        // An undetected surface's fallback flag can't be set in practice
        // (undetected → ephemeral, never fallback), but the notice filter
        // must still scope to detected surfaces only.
        let notices = port_fallback_notices(&plan, surfaces(true, false));
        assert_eq!(notices.len(), 1);
        assert!(notices[0].starts_with("MongoDB"));

        assert!(port_fallback_notices(&plan, surfaces(false, false)).is_empty());
    }
}
