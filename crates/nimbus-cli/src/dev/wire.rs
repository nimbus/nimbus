use std::io;
use std::net::{Ipv4Addr, SocketAddr, TcpListener};
use std::path::Path;

use nimbus_network::LocalNetworkAuthority;
use nimbus_server::PreboundServerListeners;

use crate::start::adapters::{
    DYNAMODB_CONVENTIONAL_PORT, MONGODB_CONVENTIONAL_PORT, S3_CONVENTIONAL_PORT,
};
use crate::wire_credentials::{WireCredentials, load_or_generate};

use super::surfaces::WireSurfaces;

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

/// The D3 hint shown when only the ambiguous aws-sdk v2 import shape is
/// present: v2 alone never promotes the DynamoDB endpoint, but it earns
/// this pointer in both the dev banner and the redetect notices.
pub(super) const AWS_SDK_V2_HINT: &str = "aws-sdk v2 detected; @aws-sdk/client-dynamodb (v3) \
     enables automatic DynamoDB endpoint + credentials in .env.local";

/// Everything the presentation layers need to describe one wire surface.
/// Adding a wire surface means adding one entry to
/// [`WirePlan::surface_presentations`]; the `.env.local` entries, the
/// port-fallback notices, the dev banner, and the redetect notices all
/// render from this list instead of hand-listing surfaces.
pub(super) struct SurfacePresentation {
    /// Display name in banners and notices ("MongoDB", "DynamoDB", "S3").
    pub(super) display_name: &'static str,
    /// What detection saw, for redetect notices ("mongodb dependency").
    pub(super) dependency_label: &'static str,
    /// Which `.env.local` keys the surface advertises, for notices.
    pub(super) env_keys_label: &'static str,
    /// True when the app's dependency set references this surface.
    pub(super) detected: bool,
    /// The resolved listener port (the listener is always serving — D6).
    pub(super) port: WireListenerPort,
    /// The conventional port the surface prefers, for fallback notices.
    pub(super) conventional_port: u16,
    /// Credential-free endpoint shown in the banner beside the env key.
    pub(super) endpoint: String,
    /// The headline Nimbus-owned env key, named in the banner.
    pub(super) primary_env_key: &'static str,
    /// Nimbus-owned `.env.local` entries advertising this surface.
    pub(super) env_entries: Vec<(&'static str, String)>,
    /// Copy-paste client snippet referencing env keys — never values.
    pub(super) client_snippet: &'static str,
}

/// Resolved wire-listener ports plus the shared persisted credentials
/// (D4/D5). Always resolved — listeners are always available (D6) — while
/// detection only chooses port prominence and what `.env.local` carries.
#[derive(Debug)]
pub(super) struct WirePlan {
    pub(super) mongodb_port: WireListenerPort,
    pub(super) dynamodb_port: WireListenerPort,
    pub(super) s3_port: WireListenerPort,
    pub(super) credentials: WireCredentials,
}

#[derive(Debug)]
pub(super) struct PreparedWirePlan {
    pub(super) plan: WirePlan,
    pub(super) listeners: PreboundServerListeners,
}

impl WirePlan {
    /// One presentation per wire surface, in canonical order. Callers
    /// filter on `detected` themselves; the aws-sdk v2 hint is not a
    /// surface and stays with [`AWS_SDK_V2_HINT`].
    pub(super) fn surface_presentations(&self, surfaces: WireSurfaces) -> Vec<SurfacePresentation> {
        vec![
            SurfacePresentation {
                display_name: "MongoDB",
                dependency_label: "mongodb dependency",
                env_keys_label: "NIMBUS_MONGODB_URL",
                detected: surfaces.mongodb,
                port: self.mongodb_port,
                conventional_port: MONGODB_CONVENTIONAL_PORT,
                endpoint: format!("mongodb://127.0.0.1:{}/", self.mongodb_port.port),
                primary_env_key: "NIMBUS_MONGODB_URL",
                env_entries: vec![(
                    "NIMBUS_MONGODB_URL",
                    format!(
                        "mongodb://{}:{}@127.0.0.1:{}/",
                        self.credentials.mongodb_username,
                        self.credentials.mongodb_password,
                        self.mongodb_port.port
                    ),
                )],
                client_snippet: "new MongoClient(process.env.NIMBUS_MONGODB_URL)",
            },
            SurfacePresentation {
                display_name: "DynamoDB",
                dependency_label: "DynamoDB SDK dependency",
                env_keys_label: "NIMBUS_DYNAMODB_ENDPOINT and access keys",
                detected: surfaces.dynamodb,
                port: self.dynamodb_port,
                conventional_port: DYNAMODB_CONVENTIONAL_PORT,
                endpoint: format!("http://127.0.0.1:{}", self.dynamodb_port.port),
                primary_env_key: "NIMBUS_DYNAMODB_ENDPOINT",
                env_entries: vec![
                    (
                        "NIMBUS_DYNAMODB_ENDPOINT",
                        format!("http://127.0.0.1:{}", self.dynamodb_port.port),
                    ),
                    (
                        "NIMBUS_DYNAMODB_ACCESS_KEY_ID",
                        self.credentials.dynamodb_access_key_id.clone(),
                    ),
                    (
                        "NIMBUS_DYNAMODB_SECRET_ACCESS_KEY",
                        self.credentials.dynamodb_secret_access_key.clone(),
                    ),
                ],
                client_snippet: "new DynamoDBClient({ endpoint: \
                     process.env.NIMBUS_DYNAMODB_ENDPOINT, credentials: { accessKeyId: \
                     process.env.NIMBUS_DYNAMODB_ACCESS_KEY_ID, secretAccessKey: \
                     process.env.NIMBUS_DYNAMODB_SECRET_ACCESS_KEY } })",
            },
            SurfacePresentation {
                display_name: "S3",
                dependency_label: "S3 SDK dependency",
                env_keys_label: "NIMBUS_S3_ENDPOINT and access keys",
                detected: surfaces.s3,
                port: self.s3_port,
                conventional_port: S3_CONVENTIONAL_PORT,
                endpoint: format!("http://127.0.0.1:{}", self.s3_port.port),
                primary_env_key: "NIMBUS_S3_ENDPOINT",
                env_entries: vec![
                    (
                        "NIMBUS_S3_ENDPOINT",
                        format!("http://127.0.0.1:{}", self.s3_port.port),
                    ),
                    ("NIMBUS_S3_REGION", "us-east-1".to_string()),
                    (
                        "NIMBUS_S3_ACCESS_KEY_ID",
                        self.credentials.s3_access_key_id.clone(),
                    ),
                    (
                        "NIMBUS_S3_SECRET_ACCESS_KEY",
                        self.credentials.s3_secret_access_key.clone(),
                    ),
                ],
                client_snippet: "new S3Client({ endpoint: process.env.NIMBUS_S3_ENDPOINT, \
                     region: process.env.NIMBUS_S3_REGION, forcePathStyle: true, credentials: { \
                     accessKeyId: process.env.NIMBUS_S3_ACCESS_KEY_ID, secretAccessKey: \
                     process.env.NIMBUS_S3_SECRET_ACCESS_KEY } })",
            },
        ]
    }

    /// The Nimbus-owned `.env.local` entries for the *detected* surfaces.
    /// Undetected surfaces stay out of the app's env file entirely: their
    /// listeners are still up (D6), but on ephemeral ports nothing in the
    /// app references.
    pub(super) fn env_local_entries(&self, surfaces: WireSurfaces) -> Vec<(&'static str, String)> {
        self.surface_presentations(surfaces)
            .into_iter()
            .filter(|surface| surface.detected)
            .flat_map(|surface| surface.env_entries)
            .collect()
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
            s3_port: WireListenerPort {
                port: S3_CONVENTIONAL_PORT,
                conventional_fallback: false,
            },
            credentials: WireCredentials {
                mongodb_username: "nimbus".to_owned(),
                mongodb_password: "0123456789abcdef0123456789abcdef".to_owned(),
                dynamodb_access_key_id: "AKIA0123456789ABCDEF".to_owned(),
                dynamodb_secret_access_key: "0123456789abcdef0123456789abcdef01234567".to_owned(),
                s3_access_key_id: "AKIAFEDCBA9876543210".to_owned(),
                s3_secret_access_key: "76543210fedcba9876543210fedcba9876543210".to_owned(),
            },
        }
    }
}

pub(super) fn resolve_wire_plan(
    surfaces: WireSurfaces,
    data_dir: &Path,
    network_authority: LocalNetworkAuthority,
) -> io::Result<PreparedWirePlan> {
    resolve_wire_plan_with_listeners(
        surfaces,
        data_dir,
        PreboundServerListeners::new(network_authority),
    )
}

#[cfg(test)]
pub(super) fn reconstruct_direct_wire_plan_for_test(
    surfaces: WireSurfaces,
    data_dir: &Path,
) -> io::Result<PreparedWirePlan> {
    resolve_wire_plan_with_listeners(
        surfaces,
        data_dir,
        PreboundServerListeners::reconstruct_direct(data_dir)?,
    )
}

fn resolve_wire_plan_with_listeners(
    surfaces: WireSurfaces,
    data_dir: &Path,
    mut listeners: PreboundServerListeners,
) -> io::Result<PreparedWirePlan> {
    let credentials = load_or_generate(data_dir)?;
    let result = (|| {
        let mongodb_port = prepare_wire_listener(
            &mut listeners,
            "mongodb",
            surfaces.mongodb,
            MONGODB_CONVENTIONAL_PORT,
        )?;
        let dynamodb_port = prepare_wire_listener(
            &mut listeners,
            "dynamodb",
            surfaces.dynamodb,
            DYNAMODB_CONVENTIONAL_PORT,
        )?;
        let s3_port =
            prepare_wire_listener(&mut listeners, "s3", surfaces.s3, S3_CONVENTIONAL_PORT)?;
        Ok(WirePlan {
            mongodb_port,
            dynamodb_port,
            s3_port,
            credentials,
        })
    })();
    match result {
        Ok(plan) => Ok(PreparedWirePlan { plan, listeners }),
        Err(primary) => match listeners.close_and_settle() {
            Ok(()) => Err(primary),
            Err(cleanup_error) => Err(io::Error::new(
                primary.kind(),
                format!(
                    "{primary}; failed to settle earlier pre-bound dev listeners: {cleanup_error}"
                ),
            )),
        },
    }
}

/// One stderr notice per detected surface whose conventional port was busy,
/// so the developer learns where the endpoint actually lives; the
/// Nimbus-owned `.env.local` key already carries the real port, so apps
/// reading it keep working without edits.
pub(super) fn port_fallback_notices(plan: &WirePlan, surfaces: WireSurfaces) -> Vec<String> {
    plan.surface_presentations(surfaces)
        .into_iter()
        .filter(|surface| surface.detected && surface.port.conventional_fallback)
        .map(|surface| {
            format!(
                "{} conventional port {} is busy; using 127.0.0.1:{} \
                 (recorded in .env.local)",
                surface.display_name, surface.conventional_port, surface.port.port
            )
        })
        .collect()
}

fn prepare_wire_listener(
    listeners: &mut PreboundServerListeners,
    adapter_name: &str,
    detected: bool,
    conventional: u16,
) -> io::Result<WireListenerPort> {
    if detected {
        let requested = SocketAddr::new(Ipv4Addr::LOCALHOST.into(), conventional);
        match bind_and_retain_wire_listener(
            listeners,
            adapter_name,
            &format!("dev-{adapter_name}-conventional"),
            requested,
        ) {
            Ok(port) => {
                return Ok(WireListenerPort {
                    port,
                    conventional_fallback: false,
                });
            }
            Err(error) if error.kind() == io::ErrorKind::AddrInUse => {}
            Err(error) => return Err(error),
        }
    }
    let port = bind_and_retain_wire_listener(
        listeners,
        adapter_name,
        &format!("dev-{adapter_name}-provider-assigned"),
        SocketAddr::new(Ipv4Addr::LOCALHOST.into(), 0),
    )?;
    Ok(WireListenerPort {
        port,
        conventional_fallback: detected,
    })
}

fn bind_and_retain_wire_listener(
    listeners: &mut PreboundServerListeners,
    adapter_name: &str,
    listener_name: &str,
    requested_addr: SocketAddr,
) -> io::Result<u16> {
    let prepared = listeners.prepare(listener_name, requested_addr)?;
    let listener = match TcpListener::bind(requested_addr) {
        Ok(listener) => listener,
        Err(error) => {
            return match prepared.record_bind_failure(error) {
                Ok(receipt) => Err(receipt.into_error()),
                Err(authority_error) => Err(authority_error),
            };
        }
    };
    let prebound = prepared.adopt_std(listener)?;
    let actual_port = match prebound.local_addr() {
        Ok(addr) => addr.port(),
        Err(primary) => {
            return match prebound.close_and_settle() {
                Ok(()) => Err(primary),
                Err(cleanup_error) => Err(io::Error::new(
                    primary.kind(),
                    format!(
                        "{primary}; failed to settle the pre-bound {adapter_name} listener after \
                         its assigned address could not be observed: {cleanup_error}"
                    ),
                )),
            };
        }
    };
    listeners.insert(adapter_name, prebound)?;
    Ok(actual_port)
}

#[cfg(test)]
fn test_free_port() -> io::Result<u16> {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?;
    Ok(listener.local_addr()?.port())
}

#[cfg(test)]
fn close_test_listeners(listeners: PreboundServerListeners) {
    listeners
        .close_and_settle()
        .expect("test listeners should close and settle");
}

#[cfg(test)]
fn prepare_test_wire_listener(
    data_dir: &Path,
    detected: bool,
    conventional: u16,
) -> io::Result<(WireListenerPort, PreboundServerListeners)> {
    let mut listeners = PreboundServerListeners::reconstruct_direct(data_dir)?;
    let port = prepare_wire_listener(&mut listeners, "mongodb", detected, conventional)?;
    Ok((port, listeners))
}

#[cfg(test)]
fn assert_port_is_still_held(port: u16) {
    let competing_bind = TcpListener::bind((Ipv4Addr::LOCALHOST, port));
    assert!(
        matches!(
            competing_bind,
            Err(ref error) if error.kind() == io::ErrorKind::AddrInUse
        ),
        "the pre-bound provider socket {port} must remain held"
    );
}

#[cfg(test)]
fn provider_assigned_fixture(
    data_dir: &Path,
) -> io::Result<(WireListenerPort, PreboundServerListeners)> {
    let (resolved, listeners) =
        prepare_test_wire_listener(data_dir, false, MONGODB_CONVENTIONAL_PORT)?;
    Ok((
        WireListenerPort {
            port: resolved.port,
            conventional_fallback: false,
        },
        listeners,
    ))
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;

    fn surfaces(mongodb: bool, dynamodb: bool, s3: bool) -> WireSurfaces {
        WireSurfaces {
            mongodb,
            dynamodb,
            s3,
            aws_sdk_v2_hint: false,
        }
    }

    #[test]
    #[serial_test::serial]
    fn manager_derived_dev_bundle_and_main_share_one_primitive_authority() {
        let root = tempfile::tempdir().expect("fixture root");
        let node_root = root.path().join("node");
        let resolved_root =
            nimbus_operator::LocalNodeNetworkRoot::resolve_for_current_platform(Some(&node_root))
                .expect("node root should resolve");
        let prepared_network =
            crate::network_composition::PreparedLocalNetworkComposition::prepare(
                crate::network_composition::StagedLocalNetworkComposition::claim(&resolved_root)
                    .expect("node manager should claim"),
                None,
                &root.path().join("control"),
                nimbus_tenant::TenantIsolationMode::LocalDevelopment,
                nimbus_server::nimbus_owned_workload_ingress_registration(),
            )
            .expect("protocol-only dev composition should freeze");
        let authority = prepared_network.authority();
        let prepared = resolve_wire_plan(
            surfaces(true, true, true),
            &root.path().join("dev"),
            authority.clone(),
        )
        .expect("all dev sibling listeners should prepare");
        for port in [
            prepared.plan.mongodb_port.port,
            prepared.plan.dynamodb_port.port,
            prepared.plan.s3_port.port,
        ] {
            assert_port_is_still_held(port);
        }

        let engine = Arc::new(
            nimbus::Engine::new(root.path().join("engine"))
                .expect("fixture engine should initialize"),
        );
        let options = prepared_network
            .prepare_server_workload_profile()
            .expect("protocol-only dev profile should prepare from the frozen source")
            .complete(engine)
            .expect("protocol-only dev profile should complete with the caller engine")
            .with_prebound_listener_authority(&prepared.listeners)
            .expect("main and every sibling must share manager provenance and primitive authority");
        let requested_main = "127.0.0.1:0"
            .parse()
            .expect("provider-assigned main address should parse");
        let main = options
            .prepare_main_listener(requested_main)
            .expect("main listener should reserve under the same authority");
        let raw = TcpListener::bind(requested_main).expect("main listener should bind");
        let main = main
            .adopt_std(raw)
            .expect("main listener should activate under the same authority");

        let records = authority
            .port_leases()
            .list()
            .expect("shared primitive authority should list");
        let active_records = records
            .iter()
            .filter(|record| record.phase() == nimbus_network::PortLeasePhase::Active)
            .collect::<Vec<_>>();
        assert_eq!(
            active_records.len(),
            4,
            "concurrent external occupancy may leave durable failed-attempt evidence, \
             but MongoDB, DynamoDB, S3, and main must be the four active leases"
        );
        assert!(
            active_records
                .iter()
                .all(|record| record.binding().is_some()),
            "every active dev/main lease must retain binding evidence"
        );

        main.close_and_settle()
            .expect("main listener should settle");
        prepared
            .listeners
            .close_and_settle()
            .expect("dev siblings should settle");
    }

    #[test]
    fn detected_surface_prefers_conventional_port() {
        let temp = tempfile::tempdir().expect("temp dir");
        let free = test_free_port().expect("select a free test port");
        let (resolved, listeners) =
            prepare_test_wire_listener(temp.path(), true, free).expect("prepare listener");
        assert_eq!(resolved.port, free);
        assert!(!resolved.conventional_fallback);
        assert_port_is_still_held(resolved.port);
        close_test_listeners(listeners);
    }

    #[test]
    fn port_conflict_fallback_updates_nimbus_owned_key() {
        let temp = tempfile::tempdir().expect("temp dir");
        let blocker = TcpListener::bind(("127.0.0.1", 0)).expect("blocker");
        let blocked_port = blocker.local_addr().expect("blocker addr").port();

        let (resolved, listeners) =
            prepare_test_wire_listener(temp.path(), true, blocked_port).expect("prepare fallback");
        assert!(resolved.conventional_fallback);
        assert_ne!(resolved.port, blocked_port);
        assert_port_is_still_held(resolved.port);

        let mut plan = WirePlan::fixture();
        plan.mongodb_port = resolved;
        let entries = plan.env_local_entries(surfaces(true, false, false));
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

        let notices = port_fallback_notices(&plan, surfaces(true, false, false));
        assert_eq!(notices.len(), 1);
        assert!(notices[0].contains(&resolved.port.to_string()));
        close_test_listeners(listeners);
    }

    #[test]
    fn undetected_surfaces_take_ephemeral_ports() {
        let temp = tempfile::tempdir().expect("temp dir");
        // Hold the would-be conventional port busy: an undetected surface
        // must neither take it nor report a fallback, because it never
        // probes the conventional port at all.
        let blocker = TcpListener::bind(("127.0.0.1", 0)).expect("blocker");
        let conventional = blocker.local_addr().expect("blocker addr").port();

        let (resolved, listeners) =
            prepare_test_wire_listener(temp.path(), false, conventional).expect("prepare listener");
        assert_ne!(resolved.port, conventional);
        assert!(!resolved.conventional_fallback);
        assert_port_is_still_held(resolved.port);
        close_test_listeners(listeners);
    }

    #[test]
    fn provider_assigned_wire_port_stays_held_until_server_adoption() {
        let temp = tempfile::tempdir().expect("temp dir");
        let (resolved, listeners) =
            provider_assigned_fixture(temp.path()).expect("prepare provider-assigned listener");
        assert_port_is_still_held(resolved.port);
        close_test_listeners(listeners);
    }

    #[test]
    fn env_entries_cover_only_detected_surfaces() {
        let plan = WirePlan::fixture();

        assert!(
            plan.env_local_entries(surfaces(false, false, false))
                .is_empty()
        );

        let mongo_only = plan.env_local_entries(surfaces(true, false, false));
        assert_eq!(
            mongo_only.iter().map(|(key, _)| *key).collect::<Vec<_>>(),
            vec!["NIMBUS_MONGODB_URL"]
        );

        let both = plan.env_local_entries(surfaces(true, true, true));
        let keys: Vec<_> = both.iter().map(|(key, _)| *key).collect();
        assert_eq!(
            keys,
            vec![
                "NIMBUS_MONGODB_URL",
                "NIMBUS_DYNAMODB_ENDPOINT",
                "NIMBUS_DYNAMODB_ACCESS_KEY_ID",
                "NIMBUS_DYNAMODB_SECRET_ACCESS_KEY",
                "NIMBUS_S3_ENDPOINT",
                "NIMBUS_S3_REGION",
                "NIMBUS_S3_ACCESS_KEY_ID",
                "NIMBUS_S3_SECRET_ACCESS_KEY",
            ]
        );
        assert!(keys.iter().all(|key| key.starts_with("NIMBUS_")));
    }

    #[test]
    fn fallback_notices_cover_only_detected_fallbacks() {
        let mut plan = WirePlan::fixture();
        plan.mongodb_port.conventional_fallback = true;
        plan.dynamodb_port.conventional_fallback = true;
        plan.s3_port.conventional_fallback = true;

        // An undetected surface's fallback flag can't be set in practice
        // (undetected → ephemeral, never fallback), but the notice filter
        // must still scope to detected surfaces only.
        let notices = port_fallback_notices(&plan, surfaces(true, false, false));
        assert_eq!(notices.len(), 1);
        assert!(notices[0].starts_with("MongoDB"));

        assert!(port_fallback_notices(&plan, surfaces(false, false, false)).is_empty());
    }
}
