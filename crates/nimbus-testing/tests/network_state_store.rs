use std::convert::Infallible;
use std::time::Duration;

use nimbus_core::TenantId;
use nimbus_network::test_support::{
    NetworkStateDurabilityEvent, transaction_with_durability_observer,
};
use nimbus_network::{LocalNetworkStateStore, NetworkStatePartition, NetworkStateTransactionError};
use nimbus_testing::{
    ContentionOutcome, ProcessRoleSpec, SubprocessCrashCutHarness, TwoProcessContentionHarness,
    run_contention_child, run_crash_cut_child, run_crash_recovery_child,
};
use serde::{Deserialize, Serialize};

const CHILD_TEST: &str = "network_state_store_child";
const MODE_ENV: &str = "NIMBUS_NETWORK_STATE_STORE_TEST_MODE";
const EVENT_ENV: &str = "NIMBUS_NETWORK_STATE_STORE_EVENT";
const BOUNDARY_ENV: &str = "NIMBUS_NETWORK_STATE_STORE_BOUNDARY";

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
struct OwnerState {
    owner: Option<String>,
}

fn partition() -> NetworkStatePartition {
    NetworkStatePartition::TenantIpam(
        TenantId::new("crash-harness").expect("fixture tenant should parse"),
    )
}

#[test]
fn exact_subprocess_crash_cuts_recover_old_or_new_complete_state() {
    let cases = [
        (
            NetworkStateDurabilityEvent::StateFileSynced,
            "network.store.state-file-synced",
            "owner-old",
        ),
        (
            NetworkStateDurabilityEvent::StateReplaced,
            "network.store.state-replaced",
            "owner-new",
        ),
        (
            NetworkStateDurabilityEvent::ParentDirectorySynced,
            "network.store.parent-directory-synced",
            "owner-new",
        ),
    ];

    for (event, boundary, expected_recovery) in cases {
        let root = tempfile::tempdir().expect("state root should exist");
        seed_owner(root.path(), "old");
        let result = SubprocessCrashCutHarness::new(Duration::from_secs(5))
            .run(
                root.path(),
                boundary,
                expected_recovery,
                child("crash-writer", "crash")
                    .env(EVENT_ENV, event_label(event))
                    .env(BOUNDARY_ENV, boundary),
                child("fresh-recovery", "recover"),
            )
            .unwrap_or_else(|error| panic!("crash case {boundary} failed: {error}"));

        assert_eq!(result.boundary(), boundary);
        assert_eq!(result.observation(), expected_recovery);
        assert_eq!(
            result.crash_diagnostic().cleanup(),
            "killed-at-boundary-and-reaped"
        );
        assert_eq!(result.recovery_diagnostic().successful(), Some(true));
    }
}

#[test]
fn two_real_processes_share_one_network_authority_and_choose_one_owner() {
    let root = tempfile::tempdir().expect("state root should exist");
    let result = TwoProcessContentionHarness::new(Duration::from_secs(5))
        .run(
            root.path(),
            [child("alpha", "contend"), child("beta", "contend")],
        )
        .unwrap_or_else(|error| panic!("network authority contention failed: {error}"));

    assert_ne!(result.winner(), result.contender());
    let store = LocalNetworkStateStore::open(root.path()).expect("authority should reopen");
    let state: OwnerState = store
        .read(&partition())
        .expect("authority should read")
        .expect("owner record should exist");
    assert_eq!(state.owner.as_deref(), Some(result.winner()));
}

#[test]
#[ignore = "spawned only by network state-store subprocess parent tests"]
fn network_state_store_child() {
    let mode = std::env::var(MODE_ENV).expect("child test mode should be set");
    match mode.as_str() {
        "crash" => run_crash_cut_child(|context| {
            let target = parse_event(
                &std::env::var(EVENT_ENV).map_err(|error| format!("missing event: {error}"))?,
            )?;
            let boundary = std::env::var(BOUNDARY_ENV)
                .map_err(|error| format!("missing boundary: {error}"))?;
            let store = LocalNetworkStateStore::open(context.state_root())
                .map_err(|error| format!("failed to open crash authority: {error}"))?;
            transaction_with_durability_observer(
                &store,
                &partition(),
                |event| {
                    if event == target {
                        context
                            .reach_boundary(&boundary)
                            .unwrap_or_else(|error| panic!("failed to report boundary: {error}"));
                    }
                },
                |state: &mut OwnerState| {
                    state.owner = Some("new".to_owned());
                    Ok::<_, Infallible>(())
                },
            )
            .map_err(|error| format!("crash transaction failed: {error}"))?;
            Err(format!(
                "transaction completed without reaching requested event {target:?}"
            ))
        })
        .unwrap_or_else(|error| panic!("crash child failed: {error}")),
        "recover" => run_crash_recovery_child(|context| {
            let store = LocalNetworkStateStore::open(context.state_root())
                .map_err(|error| format!("failed to reopen authority: {error}"))?;
            let state: OwnerState = store
                .read(&partition())
                .map_err(|error| format!("failed to read recovered authority: {error}"))?
                .ok_or_else(|| "recovered authority omitted owner partition".to_owned())?;
            state
                .owner
                .map(|owner| format!("owner-{owner}"))
                .ok_or_else(|| "recovered authority omitted owner".to_owned())
        })
        .unwrap_or_else(|error| panic!("recovery child failed: {error}")),
        "contend" => run_contention_child(|context| {
            let store = LocalNetworkStateStore::open(context.state_root())
                .map_err(|error| format!("failed to open contention authority: {error}"))?;
            store
                .transaction(&partition(), |state: &mut OwnerState| {
                    if state.owner.is_none() {
                        state.owner = Some(context.role().to_owned());
                        Ok(ContentionOutcome::Won)
                    } else {
                        Ok(ContentionOutcome::Lost)
                    }
                })
                .map_err(|error: NetworkStateTransactionError<Infallible>| {
                    format!("contention transaction failed: {error}")
                })
        })
        .unwrap_or_else(|error| panic!("contention child failed: {error}")),
        other => panic!("unknown network state-store child mode {other:?}"),
    }
}

fn seed_owner(root: &std::path::Path, owner: &str) {
    let store = LocalNetworkStateStore::open(root).expect("seed authority should open");
    store
        .transaction(&partition(), |state: &mut OwnerState| {
            state.owner = Some(owner.to_owned());
            Ok::<_, Infallible>(())
        })
        .expect("seed authority should commit");
}

fn child(role: &str, mode: &str) -> ProcessRoleSpec {
    ProcessRoleSpec::new(
        role,
        std::env::current_exe().expect("current test executable should resolve"),
    )
    .arg("--exact")
    .arg(CHILD_TEST)
    .arg("--ignored")
    .arg("--nocapture")
    .env(MODE_ENV, mode)
}

fn event_label(event: NetworkStateDurabilityEvent) -> &'static str {
    match event {
        NetworkStateDurabilityEvent::StateFileSynced => "state-file-synced",
        NetworkStateDurabilityEvent::StateReplaced => "state-replaced",
        NetworkStateDurabilityEvent::ParentDirectorySynced => "parent-directory-synced",
    }
}

fn parse_event(value: &str) -> Result<NetworkStateDurabilityEvent, String> {
    match value {
        "state-file-synced" => Ok(NetworkStateDurabilityEvent::StateFileSynced),
        "state-replaced" => Ok(NetworkStateDurabilityEvent::StateReplaced),
        "parent-directory-synced" => Ok(NetworkStateDurabilityEvent::ParentDirectorySynced),
        other => Err(format!("unknown durability event {other:?}")),
    }
}
