use std::path::Path;

use nimbus_workloads::WorkloadOwnerEvidenceDigest;

use super::*;

fn state_bytes(root: &Path) -> Vec<u8> {
    fs::read(root.join("systemd-teardown-state.json")).expect("systemd teardown state should read")
}

fn stop_inspection(primary: &Fixture, transition_tag: &str) -> HostTeardownInspectClaim {
    let inspection = inspection_fixture(primary, transition_tag);
    HostTeardownInspectClaim::new(input(&inspection, WorkloadTeardownCommandMode::Inspect))
        .expect("stop inspection claim should validate")
}

#[tokio::test]
async fn systemd_stop_inspect_is_read_only_and_byte_stable() {
    let client = TeardownFakeSystemdClient::new();
    let (state, backend) = durable_backend(client.clone());
    let primary = fixture(WorkloadTeardownStep::StopExecution);
    activate(&backend, &primary).await;
    client.fail_before_next_stop();
    let execute =
        HostTeardownExecuteClaim::new(input(&primary, WorkloadTeardownCommandMode::Execute))
            .expect("pre-call stop claim should validate");
    assert_eq!(
        backend.execute_stop(execute).await,
        HostTeardownExecuteObservation::Ambiguous
    );
    let before = state_bytes(state.path());
    assert!(matches!(
        backend.inspect_stop(stop_inspection(&primary, "9a")).await,
        HostTeardownInspectObservation::NotCompleted(_)
    ));
    assert_eq!(state_bytes(state.path()), before);

    let client = TeardownFakeSystemdClient::new();
    let (state, backend) = durable_backend(client.clone());
    let primary = fixture(WorkloadTeardownStep::StopExecution);
    activate(&backend, &primary).await;
    client.accept_next_stop_without_terminal_result();
    let execute =
        HostTeardownExecuteClaim::new(input(&primary, WorkloadTeardownCommandMode::Execute))
            .expect("accepted-job stop claim should validate");
    assert_eq!(
        backend.execute_stop(execute).await,
        HostTeardownExecuteObservation::Ambiguous
    );
    let before = state_bytes(state.path());
    assert!(matches!(
        backend.inspect_stop(stop_inspection(&primary, "9b")).await,
        HostTeardownInspectObservation::InProgress(_)
    ));
    assert_eq!(state_bytes(state.path()), before);

    let client = TeardownFakeSystemdClient::new();
    let (state, backend) = durable_backend(client.clone());
    let primary = fixture(WorkloadTeardownStep::StopExecution);
    activate(&backend, &primary).await;
    client.unknown_next_stop_submission();
    let execute =
        HostTeardownExecuteClaim::new(input(&primary, WorkloadTeardownCommandMode::Execute))
            .expect("unknown-submission stop claim should validate");
    assert_eq!(
        backend.execute_stop(execute).await,
        HostTeardownExecuteObservation::Ambiguous
    );
    client.clear_unit();
    let before = state_bytes(state.path());
    assert!(matches!(
        backend.inspect_stop(stop_inspection(&primary, "9c")).await,
        HostTeardownInspectObservation::Satisfied(_)
    ));
    assert_eq!(state_bytes(state.path()), before);

    let client = TeardownFakeSystemdClient::new();
    let (state, backend) = durable_backend(client.clone());
    let primary = fixture(WorkloadTeardownStep::StopExecution);
    activate(&backend, &primary).await;
    let execute =
        HostTeardownExecuteClaim::new(input(&primary, WorkloadTeardownCommandMode::Execute))
            .expect("terminal stop claim should validate");
    assert!(matches!(
        backend.execute_stop(execute).await,
        HostTeardownExecuteObservation::Succeeded(_)
    ));
    let before = state_bytes(state.path());
    assert!(matches!(
        backend.inspect_stop(stop_inspection(&primary, "9d")).await,
        HostTeardownInspectObservation::Satisfied(_)
    ));
    assert_eq!(state_bytes(state.path()), before);
}

#[tokio::test]
async fn systemd_completed_children_adopt_exact_adjacent_retry_without_second_effect() {
    let drain_client = TeardownFakeSystemdClient::new();
    let (_state, drain_backend) = durable_backend(drain_client.clone());
    let drain = fixture(WorkloadTeardownStep::DrainExecution);
    activate(&drain_backend, &drain).await;
    let initial_drain =
        HostTeardownExecuteClaim::new(input(&drain, WorkloadTeardownCommandMode::Execute))
            .expect("initial drain claim should validate");
    let initial_drain_result = drain_backend.execute_drain(initial_drain).await;
    assert!(matches!(
        &initial_drain_result,
        HostTeardownExecuteObservation::Succeeded(_)
    ));

    let drain_inspection = inspection_fixture(&drain, "9e");
    let drain_retry = retry_fixture_after_not_completed(
        &drain,
        &drain_inspection,
        WorkloadOwnerEvidenceDigest::sha256("joined composite drain absence"),
        "9f",
    );
    let second_inspection = inspection_fixture(&drain_retry, "8d");
    let skipped_retry = retry_fixture_after_not_completed(
        &drain_retry,
        &second_inspection,
        WorkloadOwnerEvidenceDigest::sha256("second joined composite drain absence"),
        "8e",
    );
    let skipped =
        HostTeardownExecuteClaim::new(input(&skipped_retry, WorkloadTeardownCommandMode::Execute))
            .expect("skipped drain retry claim should validate in isolation");
    assert!(matches!(
        drain_backend.execute_drain(skipped).await,
        HostTeardownExecuteObservation::DefiniteFailure(_)
    ));

    let adjacent =
        HostTeardownExecuteClaim::new(input(&drain_retry, WorkloadTeardownCommandMode::Execute))
            .expect("adjacent drain retry claim should validate");
    assert_eq!(
        drain_backend.execute_drain(adjacent).await,
        initial_drain_result
    );
    let mut crossed_adjacent = drain_retry.clone();
    crossed_adjacent.confirmed_transition_id = format!("wst_{}", "7e".repeat(32))
        .parse()
        .expect("crossed adjacent transition should validate");
    let crossed_adjacent = HostTeardownExecuteClaim::new(input(
        &crossed_adjacent,
        WorkloadTeardownCommandMode::Execute,
    ))
    .expect("crossed adjacent drain retry should validate in isolation");
    assert!(matches!(
        drain_backend.execute_drain(crossed_adjacent).await,
        HostTeardownExecuteObservation::DefiniteFailure(_)
    ));
    let next_adjacent =
        HostTeardownExecuteClaim::new(input(&skipped_retry, WorkloadTeardownCommandMode::Execute))
            .expect("next adjacent drain retry claim should validate");
    assert_eq!(
        drain_backend.execute_drain(next_adjacent).await,
        initial_drain_result
    );
    let crossed = fixture_with_source_tag(WorkloadTeardownStep::DrainExecution, "adjacent-crossed");
    let crossed =
        HostTeardownExecuteClaim::new(input(&crossed, WorkloadTeardownCommandMode::Execute))
            .expect("crossed drain claim should validate in isolation");
    assert!(matches!(
        drain_backend.execute_drain(crossed).await,
        HostTeardownExecuteObservation::DefiniteFailure(_)
    ));
    assert_eq!(drain_client.start_effect_count(), 1);
    assert_eq!(drain_client.stop_effect_count(), 0);

    let stop_client = TeardownFakeSystemdClient::new();
    let (_state, stop_backend) = durable_backend(stop_client.clone());
    let stop = fixture(WorkloadTeardownStep::StopExecution);
    activate(&stop_backend, &stop).await;
    let initial_stop =
        HostTeardownExecuteClaim::new(input(&stop, WorkloadTeardownCommandMode::Execute))
            .expect("initial stop claim should validate");
    let initial_stop_result = stop_backend.execute_stop(initial_stop).await;
    assert!(matches!(
        &initial_stop_result,
        HostTeardownExecuteObservation::Succeeded(_)
    ));
    let stop_inspection = inspection_fixture(&stop, "8f");
    let stop_retry = retry_fixture_after_not_completed(
        &stop,
        &stop_inspection,
        WorkloadOwnerEvidenceDigest::sha256("joined composite stop absence"),
        "7d",
    );
    let adjacent =
        HostTeardownExecuteClaim::new(input(&stop_retry, WorkloadTeardownCommandMode::Execute))
            .expect("adjacent stop retry claim should validate");
    assert_eq!(
        stop_backend.execute_stop(adjacent).await,
        initial_stop_result
    );
    let mut crossed_adjacent = stop_retry.clone();
    crossed_adjacent.confirmed_transition_id = format!("wst_{}", "7f".repeat(32))
        .parse()
        .expect("crossed adjacent stop transition should validate");
    let crossed_adjacent = HostTeardownExecuteClaim::new(input(
        &crossed_adjacent,
        WorkloadTeardownCommandMode::Execute,
    ))
    .expect("crossed adjacent stop retry should validate in isolation");
    assert!(matches!(
        stop_backend.execute_stop(crossed_adjacent).await,
        HostTeardownExecuteObservation::DefiniteFailure(_)
    ));
    assert_eq!(stop_client.stop_effect_count(), 1);
}

#[tokio::test]
async fn systemd_corrupt_or_unreadable_child_store_is_ambiguous_with_zero_effect() {
    for unreadable in [false, true] {
        let client = TeardownFakeSystemdClient::new();
        let (state, backend) = durable_backend(client.clone());
        let primary = fixture(WorkloadTeardownStep::StopExecution);
        activate(&backend, &primary).await;
        let state_file = state.path().join("systemd-teardown-state.json");
        if unreadable {
            fs::remove_file(&state_file).expect("readable state should remove");
            fs::create_dir(&state_file).expect("unreadable state directory should create");
        } else {
            fs::write(&state_file, b"{corrupt-systemd-child-state")
                .expect("corrupt state should write");
        }
        let execute =
            HostTeardownExecuteClaim::new(input(&primary, WorkloadTeardownCommandMode::Execute))
                .expect("stop claim should validate");
        assert_eq!(
            backend.execute_stop(execute).await,
            HostTeardownExecuteObservation::Ambiguous
        );
        assert_eq!(client.stop_effect_count(), 0);
        assert_eq!(
            backend
                .inspect_stop(stop_inspection(
                    &primary,
                    if unreadable { "7f" } else { "6f" }
                ))
                .await,
            HostTeardownInspectObservation::Ambiguous
        );
        assert_eq!(client.stop_effect_count(), 0);
    }

    let client = TeardownFakeSystemdClient::new();
    let (state, backend) = durable_backend(client.clone());
    let drain = fixture(WorkloadTeardownStep::DrainExecution);
    activate(&backend, &drain).await;
    fs::write(
        state.path().join("systemd-teardown-state.json"),
        b"{corrupt-systemd-drain-state",
    )
    .expect("corrupt drain state should write");
    let execute =
        HostTeardownExecuteClaim::new(input(&drain, WorkloadTeardownCommandMode::Execute))
            .expect("drain claim should validate");
    assert_eq!(
        backend.execute_drain(execute).await,
        HostTeardownExecuteObservation::Ambiguous
    );
    let inspection = inspection_fixture(&drain, "6e");
    let inspection =
        HostTeardownInspectClaim::new(input(&inspection, WorkloadTeardownCommandMode::Inspect))
            .expect("drain inspection claim should validate");
    assert_eq!(
        backend.inspect_drain(inspection).await,
        HostTeardownInspectObservation::Ambiguous
    );
    assert_eq!(client.start_effect_count(), 1);
    assert_eq!(client.stop_effect_count(), 0);
}
