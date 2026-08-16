//! NDB5 — live integration tests for `ZbusSystemdClient` against a real
//! per-user systemd instance (`systemctl --user`).
//!
//! Gated on `target_os = "linux"` and the explicit
//! `systemd-dbus-integration-tests` feature, so it never runs on non-Linux
//! hosts or in a default build. **No silent skips:** when the gate is on, an
//! unreachable session bus is a test *failure*, not a skip — a misconfigured
//! systemd-user setup must surface as red, never as a vacuous pass. The value
//! is in observing real state transitions, not in the binding merely not
//! panicking.
#![cfg(all(target_os = "linux", feature = "systemd-dbus-integration-tests"))]

use std::time::Duration;
use std::{collections::hash_map::DefaultHasher, hash::Hash, hash::Hasher};

use nimbus_node::{
    BusKind, SystemdDbusClient, SystemdInspectUnitRequest, SystemdStartTransientUnitRequest,
    SystemdStopUnitRequest, WorkloadExecutionId, ZbusSystemdClient,
};

/// Unique per-invocation workload id so concurrent / repeated runs never
/// collide on a unit name.
fn unique_execution_id(tag: &str) -> WorkloadExecutionId {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock should be after the unix epoch")
        .as_nanos();
    let mut hasher = DefaultHasher::new();
    tag.hash(&mut hasher);
    std::process::id().hash(&mut hasher);
    nanos.hash(&mut hasher);
    WorkloadExecutionId::try_from(format!("wex_{:064x}", hasher.finish()))
        .expect("derived live-test execution id should be valid")
}

/// Connect to the session bus. A failure here is a hard test failure: the CI
/// lane is responsible for providing a working `systemctl --user`.
async fn session_client() -> ZbusSystemdClient {
    let client = ZbusSystemdClient::new(BusKind::Session).await.expect(
        "session systemd bus must be reachable; a broken `systemd --user` \
         setup is a test FAILURE, not a skip",
    );
    let caps = client.capabilities();
    assert!(
        caps.dbus_available() && caps.transient_units(),
        "session systemd must expose transient units (capabilities: {caps:?})"
    );
    client
}

#[tokio::test]
async fn start_inspect_stop_roundtrip_against_session_systemd() {
    let client = session_client().await;
    let workload = unique_execution_id("roundtrip");

    // Start a long-lived sleep so the unit is reliably observable as running.
    let start = SystemdStartTransientUnitRequest::for_integration_execution(
        workload.clone(),
        "/usr/bin/sleep",
        vec!["30".to_string()],
    )
    .expect("start request should build");
    let response = client
        .start_transient_unit(start)
        .await
        .expect("StartTransientUnit should complete with JobRemoved=done");
    assert!(
        response
            .job_path()
            .starts_with("/org/freedesktop/systemd1/job/"),
        "unexpected job path: {}",
        response.job_path()
    );

    // Inspect: the unit should be active/running with a main PID.
    let status = client
        .inspect_unit(
            SystemdInspectUnitRequest::for_execution(workload.clone())
                .expect("inspect request should build"),
        )
        .await
        .expect("inspect should succeed");
    assert_eq!(
        status.active_state(),
        "active",
        "started sleep should be active, got {}/{}",
        status.active_state(),
        status.sub_state()
    );
    assert!(
        status.main_pid().is_some(),
        "a running service should report a main PID"
    );

    // Stop, correlated with its JobRemoved completion.
    let stop = client
        .stop_unit(
            SystemdStopUnitRequest::for_execution(workload.clone())
                .expect("stop request should build"),
        )
        .await
        .expect("StopUnit should complete with JobRemoved");
    assert_eq!(stop.status().active_state(), "inactive");

    // After stop the transient unit is inactive/dead (and usually GC'd, which
    // `inspect_unit` reports as inactive/dead via the NoSuchUnit path).
    let after = client
        .inspect_unit(
            SystemdInspectUnitRequest::for_execution(workload)
                .expect("inspect request should build"),
        )
        .await
        .expect("inspect after stop should succeed");
    assert_eq!(
        after.active_state(),
        "inactive",
        "stopped unit should be inactive, got {}",
        after.active_state()
    );
}

#[tokio::test]
async fn failed_unit_is_observable_via_inspect() {
    let client = session_client().await;
    let workload = unique_execution_id("failexec");

    // `/usr/bin/false` exits non-zero immediately; the start job completes and
    // the unit then enters `failed`, which inspect must surface.
    let start = SystemdStartTransientUnitRequest::for_integration_execution(
        workload.clone(),
        "/usr/bin/false",
        Vec::new(),
    )
    .expect("start request should build");
    client
        .start_transient_unit(start)
        .await
        .expect("StartTransientUnit should complete");

    // The unit may be briefly activating/active before `false` exits; poll
    // until it settles into a terminal state.
    let mut final_state = None;
    let mut last_observation = "no systemd observation completed".to_owned();
    for _ in 0..50 {
        match client
            .inspect_unit(
                SystemdInspectUnitRequest::for_execution(workload.clone())
                    .expect("inspect request should build"),
            )
            .await
        {
            Ok(status) => {
                let state = status.active_state().to_owned();
                last_observation = format!("stable state {state}/{}", status.sub_state());
                if state == "failed" || state == "inactive" {
                    final_state = Some(state);
                    break;
                }
            }
            Err(error) => {
                // A fast process can exit between the two lifecycle snapshots.
                // The client rejects that mixed observation. This bounded poll
                // is the caller-owned retry required by that contract.
                last_observation = format!("transient inspection error: {error}");
            }
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert_eq!(
        final_state.as_deref(),
        Some("failed"),
        "a unit whose ExecStart exits non-zero should reach `failed`; last observation: \
         {last_observation}"
    );

    // Best-effort cleanup; unique names mean a lingering failed unit never
    // collides with another run.
    let _ = client
        .stop_unit(
            SystemdStopUnitRequest::for_execution(workload).expect("stop request should build"),
        )
        .await;
}
