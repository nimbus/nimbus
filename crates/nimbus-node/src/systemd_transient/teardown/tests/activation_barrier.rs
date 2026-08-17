use std::process::Command;
use std::thread;
use std::time::{Duration, Instant};

use super::*;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn systemd_drain_closes_activation_admission_before_receipt() {
    let client = TeardownFakeSystemdClient::new();
    let state = tempdir().expect("temporary systemd teardown state root should open");
    let backend = Arc::new(
        SystemdTransientUnitBackend::new_with_teardown_state_root(client.clone(), state.path())
            .expect("durable systemd backend should open"),
    );
    let fixture = fixture(WorkloadTeardownStep::DrainExecution);
    client.pause_next_start();
    let activation_backend = Arc::clone(&backend);
    let activation_fixture = fixture.clone();
    let activation = tokio::spawn(async move {
        activation_backend
            .activate_exact(
                activation_fixture.execution,
                activation_fixture.activation_claim,
                request(),
            )
            .await
    });
    client.wait_until_start_entered().await;

    let drain_claim =
        HostTeardownExecuteClaim::new(input(&fixture, WorkloadTeardownCommandMode::Execute))
            .expect("drain claim should validate");
    let drain_backend = Arc::clone(&backend);
    let mut drain = tokio::spawn(async move { drain_backend.execute_drain(drain_claim).await });
    assert!(
        tokio::time::timeout(Duration::from_millis(50), &mut drain)
            .await
            .is_err(),
        "drain must wait while an admitted activation can still publish"
    );

    client.release_paused_start();
    activation
        .await
        .expect("activation task should join")
        .expect("admitted activation should settle");
    assert!(matches!(
        drain.await.expect("drain task should join"),
        HostTeardownExecuteObservation::Succeeded(_)
    ));
    client.clear_unit();
    let error = backend
        .activate_exact(fixture.execution, fixture.activation_claim, request())
        .await
        .expect_err("closed drain barrier must reject a later activation");
    assert!(error.to_string().contains("admission is closed"));
    assert_eq!(client.start_effect_count(), 1);
}

#[tokio::test]
async fn systemd_unknown_activation_submission_keeps_drain_ambiguous_after_reopen() {
    let client = TeardownFakeSystemdClient::new();
    let (state, backend) = durable_backend(client.clone());
    let fixture = fixture(WorkloadTeardownStep::DrainExecution);
    client.unknown_next_start_submission();
    backend
        .activate_exact(
            fixture.execution.clone(),
            fixture.activation_claim.clone(),
            request(),
        )
        .await
        .expect_err("unknown start submission should not claim activation success");
    let execute =
        HostTeardownExecuteClaim::new(input(&fixture, WorkloadTeardownCommandMode::Execute))
            .expect("drain claim should validate");
    assert_eq!(
        backend.execute_drain(execute).await,
        HostTeardownExecuteObservation::Ambiguous
    );

    drop(backend);
    let reopened =
        SystemdTransientUnitBackend::new_with_teardown_state_root(client.clone(), state.path())
            .expect("reopened backend should load unresolved admission");
    let inspection_fixture = inspection_fixture(&fixture, "0e");
    let inspect = HostTeardownInspectClaim::new(input(
        &inspection_fixture,
        WorkloadTeardownCommandMode::Inspect,
    ))
    .expect("drain inspection should validate");
    assert!(matches!(
        reopened.inspect_drain(inspect).await,
        HostTeardownInspectObservation::InProgress(_)
    ));
    assert_eq!(client.start_effect_count(), 1);
}

#[tokio::test]
async fn systemd_drain_inspection_requires_a_durable_closed_barrier() {
    let client = TeardownFakeSystemdClient::new();
    let (_state, backend) = durable_backend(client);
    let fixture = fixture(WorkloadTeardownStep::DrainExecution);
    let inspection_fixture = inspection_fixture(&fixture, "0f");
    let inspect = HostTeardownInspectClaim::new(input(
        &inspection_fixture,
        WorkloadTeardownCommandMode::Inspect,
    ))
    .expect("drain inspection should validate");

    assert!(matches!(
        backend.inspect_drain(inspect).await,
        HostTeardownInspectObservation::NotCompleted(_)
    ));
}

#[test]
fn systemd_drain_barrier_process_contention_has_one_closer() {
    let state = tempdir().expect("temporary systemd teardown state root should open");
    SystemdTeardownStore::open(state.path()).expect("parent store should open");
    let release = state.path().join("release-drain-children");
    let executable = std::env::current_exe().expect("test executable should resolve");
    let mut children = Vec::new();
    for slot in ["left", "right"] {
        let child = Command::new(&executable)
            .arg("--exact")
            .arg(
                "systemd_transient::teardown_fail_before_tests::activation_barrier::systemd_drain_barrier_process_contention_child",
            )
            .arg("--nocapture")
            .env("NIMBUS_SYSTEMD_DRAIN_CHILD_ROOT", state.path())
            .env("NIMBUS_SYSTEMD_DRAIN_CHILD_SLOT", slot)
            .spawn()
            .expect("drain contention child should spawn");
        children.push((slot, child));
    }
    let deadline = Instant::now() + Duration::from_secs(5);
    while children
        .iter()
        .any(|(slot, _)| !state.path().join(format!("drain-ready-{slot}")).exists())
        && Instant::now() < deadline
    {
        thread::sleep(Duration::from_millis(10));
    }
    assert!(
        children
            .iter()
            .all(|(slot, _)| state.path().join(format!("drain-ready-{slot}")).exists()),
        "both child processes must reach the release barrier"
    );
    fs::write(&release, b"release").expect("child release signal should write");
    for (slot, mut child) in children {
        let status = child.wait().expect("drain contention child should reap");
        assert!(status.success(), "{slot} drain child failed: {status}");
        assert_eq!(
            fs::read(state.path().join(format!("drain-result-{slot}")))
                .expect("child result should read"),
            b"succeeded"
        );
    }
    let store = SystemdTeardownStore::open(state.path()).expect("final store should reopen");
    let state = store.lock_state().expect("final barrier state should lock");
    assert_eq!(
        state.state().closed_drain_count(),
        1,
        "two processes must converge on one closed execution barrier"
    );
}

#[test]
fn systemd_drain_barrier_process_contention_child() {
    let (Ok(root), Ok(slot)) = (
        std::env::var("NIMBUS_SYSTEMD_DRAIN_CHILD_ROOT"),
        std::env::var("NIMBUS_SYSTEMD_DRAIN_CHILD_SLOT"),
    ) else {
        return;
    };
    let root = PathBuf::from(root);
    let backend = SystemdTransientUnitBackend::new_with_teardown_state_root(
        TeardownFakeSystemdClient::new(),
        &root,
    )
    .expect("child backend should open");
    fs::write(root.join(format!("drain-ready-{slot}")), b"ready")
        .expect("child readiness should write");
    let deadline = Instant::now() + Duration::from_secs(5);
    while !root.join("release-drain-children").exists() && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(10));
    }
    assert!(
        root.join("release-drain-children").exists(),
        "child release signal must arrive"
    );
    let fixture = fixture(WorkloadTeardownStep::DrainExecution);
    let claim =
        HostTeardownExecuteClaim::new(input(&fixture, WorkloadTeardownCommandMode::Execute))
            .expect("child drain claim should validate");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("child runtime should build");
    let result = runtime.block_on(backend.execute_drain(claim));
    assert!(matches!(
        result,
        HostTeardownExecuteObservation::Succeeded(_)
    ));
    fs::write(root.join(format!("drain-result-{slot}")), b"succeeded")
        .expect("child result should write");
}

#[tokio::test]
async fn systemd_closed_drain_barrier_blocks_exact_activation_after_reopen() {
    let client = TeardownFakeSystemdClient::new();
    let (state, backend) = durable_backend(client.clone());
    let fixture = fixture(WorkloadTeardownStep::DrainExecution);
    activate_without_drain(&backend, &fixture).await;
    let claim =
        HostTeardownExecuteClaim::new(input(&fixture, WorkloadTeardownCommandMode::Execute))
            .expect("drain claim should validate");
    assert!(matches!(
        backend.execute_drain(claim).await,
        HostTeardownExecuteObservation::Succeeded(_)
    ));
    client.clear_unit();

    let first_error = backend
        .activate_exact(
            fixture.execution.clone(),
            fixture.activation_claim.clone(),
            request(),
        )
        .await
        .expect_err("closed drain barrier must reject exact activation");
    assert!(first_error.to_string().contains("admission is closed"));

    drop(backend);
    let reopened =
        SystemdTransientUnitBackend::new_with_teardown_state_root(client.clone(), state.path())
            .expect("reopened backend should load the closed barrier");
    let reopen_error = reopened
        .activate_exact(
            fixture.execution.clone(),
            fixture.activation_claim.clone(),
            request(),
        )
        .await
        .expect_err("reopened drain barrier must remain closed");
    assert!(reopen_error.to_string().contains("admission is closed"));
    assert_eq!(
        client.start_effect_count(),
        1,
        "neither rejected activation may reach StartTransientUnit"
    );
}

#[tokio::test]
async fn systemd_stop_requires_matching_closed_drain_barrier() {
    let client = TeardownFakeSystemdClient::new();
    let (_state, backend) = durable_backend(client.clone());
    let fixture = fixture(WorkloadTeardownStep::StopExecution);
    activate_without_drain(&backend, &fixture).await;
    let stop = HostTeardownExecuteClaim::new(input(&fixture, WorkloadTeardownCommandMode::Execute))
        .expect("stop claim should validate");

    assert!(matches!(
        backend.execute_stop(stop.clone()).await,
        HostTeardownExecuteObservation::DefiniteFailure(_)
    ));
    assert_eq!(client.stop_effect_count(), 0);

    let drain = prior_drain_fixture(&fixture);
    let drain_claim =
        HostTeardownExecuteClaim::new(input(&drain, WorkloadTeardownCommandMode::Execute))
            .expect("prior drain claim should validate");
    assert!(matches!(
        backend.execute_drain(drain_claim).await,
        HostTeardownExecuteObservation::Succeeded(_)
    ));
    assert!(matches!(
        backend.execute_stop(stop).await,
        HostTeardownExecuteObservation::Succeeded(_)
    ));
    assert_eq!(client.stop_effect_count(), 1);
}
