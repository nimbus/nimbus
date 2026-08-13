use std::net::{Ipv4Addr, SocketAddr, TcpListener as StdTcpListener};
use std::num::NonZeroU16;
use std::time::Duration;

use nimbus_core::TenantId;
use nimbus_kv::{
    CredentialRegistry, KvError, NimbusKvConfig, NimbusKvListenerConfig, NimbusKvMetrics,
    NimbusKvStore, TieringConfig, adopt_listener, bind_listener, run_listener,
};
use nimbus_network::{
    LocalNetworkManager, LocalNetworkManagerError, LocalNetworkStateStore, LocalPortLeaseAuthority,
    NetworkCapabilityRegistry, NetworkResourceId, PortBindFailureKind, PortBindingProvenance,
    PortLeasePhase,
};
use nimbus_process_harness::{
    ContentionOutcome, ProcessRoleSpec, SubprocessCrashCutHarness, TwoProcessContentionHarness,
    run_contention_child, run_crash_cut_child, run_crash_recovery_child,
};

const CHILD_TEST: &str = "nnc3_6_kv_listener_child";
const CHILD_ADDR_ENV: &str = "NIMBUS_NNC3_6_KV_ADDR";
const RECOVERY_CHILD_TEST: &str = "nnc3_8_kv_listener_recovery_child";
const RECOVERY_MODE_ENV: &str = "NIMBUS_NNC3_8_KV_MODE";
const KV_ACTIVE_BOUNDARY: &str = "network.kv-listener.active-process-owner";
const KV_RECOVERY_OBSERVATION: &str = "old-released:replacement-active";

fn credentials() -> CredentialRegistry {
    CredentialRegistry::single_dev(TenantId::new("tenant-a").unwrap(), "secret")
}

fn config(
    root: impl Into<std::path::PathBuf>,
    addr: SocketAddr,
    incarnation: &str,
) -> NimbusKvConfig {
    let root = root.into();
    NimbusKvConfig::new(
        addr,
        credentials(),
        NimbusKvListenerConfig::reconstruct_direct_for_incarnation(&root, incarnation)
            .expect("direct test authority should reconstruct once"),
    )
}

fn selected_loopback_addr() -> SocketAddr {
    let selector =
        StdTcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("port selector should bind");
    selector
        .local_addr()
        .expect("selected address should resolve")
}

#[test]
fn two_standalone_kv_processes_contend_in_one_authority() {
    let root = tempfile::tempdir().expect("state root should exist");
    let addr = selected_loopback_addr();
    let result = TwoProcessContentionHarness::new(Duration::from_secs(10))
        .run(root.path(), [child("alpha", addr), child("beta", addr)])
        .unwrap_or_else(|error| panic!("KV listener contention failed: {error}"));

    let records = LocalPortLeaseAuthority::open(root.path())
        .expect("authority should reopen")
        .list()
        .expect("records should list");
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].phase(), PortLeasePhase::Active);
    assert_eq!(
        records[0].request().owner_id(),
        &owner_id(root.path(), result.winner())
    );
    assert_eq!(
        records[0].reserved_port().map(NonZeroU16::get),
        Some(addr.port())
    );
}

#[test]
fn fresh_process_kv_reconciles_dead_listener_before_exact_rebind() {
    let root = tempfile::tempdir().expect("state root should exist");
    SubprocessCrashCutHarness::new(Duration::from_secs(10))
        .run(
            root.path(),
            KV_ACTIVE_BOUNDARY,
            KV_RECOVERY_OBSERVATION,
            recovery_child("crash-kv-owner", "crash-active"),
            recovery_child("fresh-kv-owner", "recover-active"),
        )
        .unwrap_or_else(|error| panic!("fresh-process KV recovery failed: {error}"));
}

#[tokio::test]
async fn fixed_conflict_reports_both_stable_owner_identities() {
    let root = tempfile::tempdir().expect("state root should exist");
    let addr = selected_loopback_addr();
    let winner_config = config(root.path(), addr, "alpha");
    let contender_config = config(root.path(), addr, "beta");
    let winner_owner = winner_config.listener.listener_id().clone();
    let contender_owner = contender_config.listener.listener_id().clone();
    let winner = bind_listener(&winner_config)
        .await
        .expect("first KV listener should bind");

    let error = match bind_listener(&contender_config).await {
        Ok(_) => panic!("second stable owner must not bind the fixed port"),
        Err(error) => error,
    };
    let diagnostic = error.to_string();
    assert!(
        diagnostic.contains(&format!("{:?}", NetworkResourceId::from(winner_owner))),
        "conflict must identify the durable winner: {diagnostic}"
    );
    assert!(
        diagnostic.contains(&format!("{:?}", NetworkResourceId::from(contender_owner))),
        "conflict must identify the rejected owner: {diagnostic}"
    );
    winner
        .close_and_settle()
        .expect("confirmed winner close should release authority");
    let records = LocalPortLeaseAuthority::open(root.path())
        .expect("authority should reopen")
        .list()
        .expect("records should list");
    assert_eq!(records[0].phase(), PortLeasePhase::Released);
}

#[tokio::test]
async fn prebound_kv_listener_adopts_exact_external_provenance() {
    let root = tempfile::tempdir().expect("state root should exist");
    let raw = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .await
        .expect("pre-bound listener should bind");
    let addr = raw.local_addr().expect("pre-bound address should resolve");
    let config = config(root.path(), addr, "prebound");
    let expected_owner = NetworkResourceId::from(config.listener.listener_id().clone());
    let listener = adopt_listener(raw, &config).expect("pre-bound listener should adopt");

    let records = LocalPortLeaseAuthority::open(root.path())
        .expect("authority should reopen")
        .list()
        .expect("records should list");
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].phase(), PortLeasePhase::Active);
    assert_eq!(records[0].request().owner_id(), &expected_owner);
    assert_eq!(
        records[0]
            .binding()
            .expect("adopted listener should retain binding evidence")
            .provenance(),
        PortBindingProvenance::ExternallyOwned
    );
    assert_eq!(listener.local_addr().unwrap(), addr);
    listener
        .close_and_settle()
        .expect("local descriptor close should withdraw the external adoption");
    let records = LocalPortLeaseAuthority::open(root.path())
        .expect("authority should reopen")
        .list()
        .expect("records should list");
    assert_eq!(records[0].phase(), PortLeasePhase::Withdrawing);
}

#[tokio::test]
async fn provider_assigned_bind_activates_exact_kernel_port_and_releases_on_confirmed_close() {
    let root = tempfile::tempdir().expect("state root should exist");
    let requested = SocketAddr::from((Ipv4Addr::LOCALHOST, 0));
    let config = config(root.path(), requested, "provider-assigned");
    let listener = bind_listener(&config)
        .await
        .expect("provider-assigned KV listener should bind");
    let actual = listener
        .local_addr()
        .expect("bound listener should report an address");
    assert_ne!(actual.port(), 0);

    let records = LocalPortLeaseAuthority::open(root.path())
        .expect("authority should reopen")
        .list()
        .expect("records should list");
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].phase(), PortLeasePhase::Active);
    assert_eq!(
        records[0]
            .binding()
            .expect("Active lease should retain provider evidence")
            .actual_port()
            .get(),
        actual.port()
    );
    assert_eq!(
        records[0]
            .binding()
            .expect("Active lease should retain provider evidence")
            .provenance(),
        PortBindingProvenance::ProviderAssigned
    );

    listener
        .close_and_settle()
        .expect("confirmed close should settle the exact lease");
    let records = LocalPortLeaseAuthority::open(root.path())
        .expect("authority should reopen")
        .list()
        .expect("records should list");
    assert_eq!(records[0].phase(), PortLeasePhase::Released);
}

#[tokio::test]
async fn dead_process_bound_listener_drop_reconciles_before_reuse() {
    let root = tempfile::tempdir().expect("state root should exist");
    let requested = SocketAddr::from((Ipv4Addr::LOCALHOST, 0));
    let owner_config = config(root.path(), requested, "ambiguous-owner");
    let listener = bind_listener(&owner_config)
        .await
        .expect("provider-assigned KV listener should bind");
    let actual = listener
        .local_addr()
        .expect("bound listener should report an address");
    drop(listener);

    let records = LocalPortLeaseAuthority::open(root.path())
        .expect("authority should reopen")
        .list()
        .expect("records should list");
    assert_eq!(records.len(), 1);
    assert_eq!(
        records[0].phase(),
        PortLeasePhase::Active,
        "ordinary Drop must retain authority until explicit lifetime reconciliation"
    );
    assert!(
        records[0].active_lifetime().is_some(),
        "the retained record must carry exact process-lifetime evidence"
    );

    let kernel_probe = tokio::net::TcpListener::bind(actual)
        .await
        .expect("dropping the wrapper should close Nimbus's local descriptor");
    drop(kernel_probe);
    let contender_config = config(root.path(), actual, "ambiguous-contender");
    let contender = bind_listener(&contender_config)
        .await
        .expect("fresh preparation should reconcile the dead process-bound owner");
    let records = LocalPortLeaseAuthority::open(root.path())
        .expect("authority should reopen")
        .list()
        .expect("records should list");
    assert_eq!(records.len(), 2);
    assert_eq!(
        records
            .iter()
            .find(|record| {
                record.request().owner_id() == &owner_id(root.path(), "ambiguous-owner")
            })
            .expect("dead owner record should remain auditable")
            .phase(),
        PortLeasePhase::Released
    );
    assert_eq!(
        records
            .iter()
            .find(|record| {
                record.request().owner_id() == &owner_id(root.path(), "ambiguous-contender")
            })
            .expect("replacement owner record should exist")
            .phase(),
        PortLeasePhase::Active
    );
    contender
        .close_and_settle()
        .expect("replacement should close cleanly");
}

#[tokio::test]
async fn external_kernel_collision_records_durable_no_effect_failure() {
    let root = tempfile::tempdir().expect("state root should exist");
    let external = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .await
        .expect("external listener should bind");
    let occupied = external
        .local_addr()
        .expect("external listener should report an address");
    let config = config(root.path(), occupied, "collision");

    let error = match bind_listener(&config).await {
        Ok(_) => panic!("external kernel owner must reject the KV bind"),
        Err(error) => error,
    };
    match error {
        KvError::Io(error) => assert_eq!(error.kind(), std::io::ErrorKind::AddrInUse),
        other => panic!("expected the original kernel collision, got {other:?}"),
    }

    let records = LocalPortLeaseAuthority::open(root.path())
        .expect("authority should reopen")
        .list()
        .expect("records should list");
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].phase(), PortLeasePhase::Failed);
    assert_eq!(
        records[0]
            .failure()
            .expect("failed bind should retain its no-effect receipt")
            .kind(),
        PortBindFailureKind::AddrInUse
    );
    assert_eq!(
        records[0]
            .failure()
            .expect("failed bind should retain its attempted endpoint")
            .attempt()
            .port(),
        occupied.port()
    );
}

#[tokio::test]
async fn prebound_address_mismatch_closes_socket_without_creating_authority() {
    let root = tempfile::tempdir().expect("state root should exist");
    let raw = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .await
        .expect("pre-bound listener should bind");
    let actual = raw.local_addr().expect("listener should report an address");
    let configured = selected_loopback_addr();
    assert_ne!(actual, configured);
    let config = config(root.path(), configured, "mismatch");

    let error = match adopt_listener(raw, &config) {
        Ok(_) => panic!("mismatched pre-bound listener must fail"),
        Err(error) => error,
    };
    match error {
        KvError::Io(error) => assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput),
        other => panic!("expected an invalid-input mismatch, got {other:?}"),
    }

    let records = LocalPortLeaseAuthority::open(root.path())
        .expect("authority should open")
        .list()
        .expect("records should list");
    assert!(records.is_empty());
    tokio::net::TcpListener::bind(actual)
        .await
        .expect("failed adoption must close the supplied descriptor");
}

#[tokio::test]
async fn synchronous_server_setup_failure_releases_direct_listener_authority() {
    let root = tempfile::tempdir().expect("state root should exist");
    let requested = SocketAddr::from((Ipv4Addr::LOCALHOST, 0));
    let config = config(root.path(), requested, "setup-failure")
        .with_store(
            NimbusKvStore::no_disk(TieringConfig::no_disk())
                .expect("prebuilt test store should open"),
        )
        .with_metrics(NimbusKvMetrics::default());

    let error = run_listener(config)
        .await
        .expect_err("conflicting store configuration must fail after binding");
    assert!(
        error
            .to_string()
            .contains("with_metrics cannot be combined with a prebuilt NimbusKvStore")
    );
    let records = LocalPortLeaseAuthority::open(root.path())
        .expect("authority should reopen")
        .list()
        .expect("records should list");
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].phase(), PortLeasePhase::Released);
}

#[cfg(unix)]
#[tokio::test]
async fn manager_derived_listener_pins_alias_and_retains_process_claim_until_final_drop() {
    use std::fs;
    use std::os::unix::fs::symlink;

    let parent = tempfile::tempdir().expect("fixture root should exist");
    let source_root = parent.path().join("source");
    let foreign_root = parent.path().join("foreign");
    let alias = parent.path().join("network-current");
    fs::create_dir(&source_root).expect("source root should exist");
    fs::create_dir(&foreign_root).expect("foreign root should exist");
    let canonical_source = fs::canonicalize(&source_root).expect("source root should canonicalize");
    let canonical_foreign =
        fs::canonicalize(&foreign_root).expect("foreign root should canonicalize");
    symlink(&source_root, &alias).expect("source alias should exist");

    let bootstrap =
        LocalNetworkManager::bootstrap(&alias).expect("manager should claim the source alias");
    let authority = bootstrap.authority();
    let manager = bootstrap.freeze(
        NetworkCapabilityRegistry::new(Vec::new()).expect("empty registry should be valid"),
    );
    assert_eq!(manager.capability_registry().selections().len(), 0);

    let config = NimbusKvConfig::new(
        SocketAddr::from((Ipv4Addr::LOCALHOST, 0)),
        credentials(),
        NimbusKvListenerConfig::from_network_authority_for_incarnation(
            authority,
            "manager-retained",
        ),
    );
    fs::remove_file(&alias).expect("source alias should remove");
    symlink(&foreign_root, &alias).expect("foreign alias should replace source alias");

    let listener = bind_listener(&config)
        .await
        .expect("manager-derived listener should bind against retained source authority");
    let actual = listener
        .local_addr()
        .expect("manager-derived listener should report an address");

    drop(config);
    drop(manager);

    let records = LocalPortLeaseAuthority::open(&source_root)
        .expect("source authority should reopen")
        .list()
        .expect("source records should list");
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].phase(), PortLeasePhase::Active);
    assert_eq!(
        records[0]
            .binding()
            .expect("active listener should retain binding evidence")
            .actual_port()
            .get(),
        actual.port()
    );
    assert!(
        !LocalNetworkStateStore::authority_path_for(&foreign_root).exists(),
        "retargeting the alias must not create or mutate foreign authority"
    );

    let error = LocalNetworkManager::bootstrap(&foreign_root)
        .expect_err("the active listener must retain the original process claim");
    match error {
        LocalNetworkManagerError::DuplicateProcessComposition {
            active_authority_path,
            attempted_authority_path,
        } => {
            assert_eq!(
                active_authority_path,
                LocalNetworkStateStore::authority_path_for(&canonical_source)
            );
            assert_eq!(
                attempted_authority_path,
                LocalNetworkStateStore::authority_path_for(&canonical_foreign)
            );
        }
        other => panic!("expected typed duplicate composition, got {other:?}"),
    }
    assert!(
        !LocalNetworkStateStore::authority_path_for(&foreign_root).exists(),
        "rejected divergent bootstrap must remain non-mutating"
    );

    listener
        .close_and_settle()
        .expect("confirmed listener close should settle the source lease");
    let settled = LocalPortLeaseAuthority::open(&source_root)
        .expect("source authority should remain pinned through settlement")
        .list()
        .expect("settled source records should list");
    assert_eq!(settled.len(), 1);
    assert_eq!(settled[0].phase(), PortLeasePhase::Released);
    assert!(
        !LocalNetworkStateStore::authority_path_for(&foreign_root).exists(),
        "prepare, activation, and settlement must not follow a retargeted alias"
    );
    let reopened = LocalNetworkManager::bootstrap(&foreign_root)
        .expect("final listener drop should release the process claim")
        .freeze(
            NetworkCapabilityRegistry::new(Vec::new()).expect("empty registry should be valid"),
        );
    assert_eq!(reopened.capability_registry().selections().len(), 0);
}

#[test]
#[ignore = "spawned only by the NNC3.6 two-process contention parent"]
fn nnc3_6_kv_listener_child() {
    run_contention_child(|context| {
        let addr = std::env::var(CHILD_ADDR_ENV)
            .map_err(|error| format!("missing child address: {error}"))?
            .parse::<SocketAddr>()
            .map_err(|error| format!("invalid child address: {error}"))?;
        let config = config(context.state_root(), addr, context.role());
        let expected_owner = NetworkResourceId::from(config.listener.listener_id().clone());
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|error| format!("child runtime failed: {error}"))?;
        match runtime.block_on(bind_listener(&config)) {
            Ok(listener) => {
                let records = LocalPortLeaseAuthority::open(context.state_root())
                    .map_err(|error| format!("authority reopen failed: {error}"))?
                    .list()
                    .map_err(|error| format!("authority list failed: {error}"))?;
                if !records.iter().any(|record| {
                    record.phase() == PortLeasePhase::Active
                        && record.request().owner_id() == &expected_owner
                }) {
                    return Err(format!(
                        "KV bind for {:?} completed without an Active durable lease",
                        expected_owner
                    ));
                }
                std::mem::forget(listener);
                Ok(ContentionOutcome::Won)
            }
            Err(KvError::Network(nimbus_network::PortLeaseError::PortConflict { .. })) => {
                Ok(ContentionOutcome::Lost)
            }
            Err(error) => Err(format!("unexpected KV listener failure: {error}")),
        }
    })
    .unwrap_or_else(|error| panic!("KV listener child failed: {error}"));
}

#[test]
#[ignore = "spawned only by the NNC3.8 KV crash/recovery parent"]
fn nnc3_8_kv_listener_recovery_child() {
    let mode = std::env::var(RECOVERY_MODE_ENV).expect("recovery mode should be set");
    match mode.as_str() {
        "crash-active" => run_crash_cut_child(|context| {
            let requested = SocketAddr::from((Ipv4Addr::LOCALHOST, 0));
            let config = config(context.state_root(), requested, "crash-kv-incarnation");
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|error| format!("crash child runtime failed: {error}"))?;
            let listener = runtime
                .block_on(bind_listener(&config))
                .map_err(|error| format!("crash child KV bind failed: {error}"))?;
            let records = LocalPortLeaseAuthority::open(context.state_root())
                .map_err(|error| format!("crash authority reopen failed: {error}"))?
                .list()
                .map_err(|error| format!("crash authority list failed: {error}"))?;
            if records.len() != 1
                || records[0].phase() != PortLeasePhase::Active
                || records[0].active_lifetime().is_none()
            {
                return Err(format!(
                    "crash owner must hold one Active lifetime-fenced lease, got {records:?}"
                ));
            }
            context.reach_boundary(KV_ACTIVE_BOUNDARY)?;
            drop(listener);
            Err("crash child resumed after the parent kill boundary".to_owned())
        })
        .unwrap_or_else(|error| panic!("KV crash child failed: {error}")),
        "recover-active" => run_crash_recovery_child(|context| {
            let authority = LocalPortLeaseAuthority::open(context.state_root())
                .map_err(|error| format!("recovery authority reopen failed: {error}"))?;
            let old = authority
                .list()
                .map_err(|error| format!("recovery authority list failed: {error}"))?
                .into_iter()
                .find(|record| record.phase() == PortLeasePhase::Active)
                .ok_or_else(|| "recovery child found no Active crash-owned lease".to_owned())?;
            let port = old
                .reserved_port()
                .ok_or_else(|| "crash-owned KV lease omitted its concrete port".to_owned())?;
            let requested = SocketAddr::from((Ipv4Addr::LOCALHOST, port.get()));
            let config = config(context.state_root(), requested, "fresh-kv-incarnation");
            let replacement_owner = NetworkResourceId::from(config.listener.listener_id().clone());
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|error| format!("recovery child runtime failed: {error}"))?;
            let replacement = runtime
                .block_on(bind_listener(&config))
                .map_err(|error| format!("fresh-process exact KV rebind failed: {error}"))?;
            let records = authority
                .list()
                .map_err(|error| format!("recovery authority relist failed: {error}"))?;
            if records
                .iter()
                .find(|record| record.request().lease_id() == old.request().lease_id())
                .is_none_or(|record| record.phase() != PortLeasePhase::Released)
            {
                return Err("old crash-owned lease did not reach Released".to_owned());
            }
            if !records.iter().any(|record| {
                record.phase() == PortLeasePhase::Active
                    && record.request().owner_id() == &replacement_owner
                    && record.reserved_port() == Some(port)
            }) {
                return Err("replacement KV listener did not own the exact Active port".to_owned());
            }
            std::mem::forget(replacement);
            Ok(KV_RECOVERY_OBSERVATION.to_owned())
        })
        .unwrap_or_else(|error| panic!("KV recovery child failed: {error}")),
        other => panic!("unknown KV recovery mode {other:?}"),
    }
}

fn child(role: &str, addr: SocketAddr) -> ProcessRoleSpec {
    ProcessRoleSpec::new(
        role,
        std::env::current_exe().expect("current test executable should resolve"),
    )
    .arg("--exact")
    .arg(CHILD_TEST)
    .arg("--ignored")
    .arg("--nocapture")
    .env(CHILD_ADDR_ENV, addr.to_string())
}

fn recovery_child(role: &str, mode: &str) -> ProcessRoleSpec {
    ProcessRoleSpec::new(
        role,
        std::env::current_exe().expect("current test executable should resolve"),
    )
    .arg("--exact")
    .arg(RECOVERY_CHILD_TEST)
    .arg("--ignored")
    .arg("--nocapture")
    .env(RECOVERY_MODE_ENV, mode)
}

fn owner_id(root: &std::path::Path, role: &str) -> NetworkResourceId {
    config(root, "127.0.0.1:1".parse().unwrap(), role)
        .listener
        .listener_id()
        .clone()
        .into()
}
