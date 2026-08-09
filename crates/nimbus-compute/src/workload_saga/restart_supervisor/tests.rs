use std::collections::VecDeque;
use std::sync::atomic::{AtomicUsize, Ordering};

use tokio::sync::Semaphore;

use super::*;
use crate::workload_saga::test_support;

struct ScriptedCoordinator {
    calls: AtomicUsize,
    entered: Semaphore,
    release: Semaphore,
    outcomes: Mutex<VecDeque<Result<(), String>>>,
}

impl ScriptedCoordinator {
    fn new(outcomes: impl IntoIterator<Item = Result<(), String>>) -> Arc<Self> {
        Arc::new(Self {
            calls: AtomicUsize::new(0),
            entered: Semaphore::new(0),
            release: Semaphore::new(0),
            outcomes: Mutex::new(outcomes.into_iter().collect()),
        })
    }

    async fn wait_for_calls(&self, expected: usize) {
        let expected = u32::try_from(expected).expect("test call count fits in u32");
        let permits = self
            .entered
            .acquire_many(expected)
            .await
            .expect("test coordinator remains open");
        permits.forget();
    }

    fn release(&self, count: usize) {
        self.release.add_permits(count);
    }
}

impl RestartCandidateCoordinator for ScriptedCoordinator {
    fn coordinate(&self, _record: WorkloadSagaRecord) -> RestartCandidateFuture<'_> {
        Box::pin(async move {
            self.calls.fetch_add(1, Ordering::AcqRel);
            self.entered.add_permits(1);
            let permit = self
                .release
                .acquire()
                .await
                .expect("test coordinator remains open");
            permit.forget();
            self.outcomes
                .lock()
                .expect("test outcome queue remains healthy")
                .pop_front()
                .expect("test supplies one outcome per coordination")
        })
    }
}

fn supervisor(coordinator: Arc<ScriptedCoordinator>) -> RetainedRestartSupervisor {
    RetainedRestartSupervisor::new(coordinator)
}

#[tokio::test]
async fn exact_duplicate_record_tracks_one_task_and_joins() {
    let coordinator = ScriptedCoordinator::new([Ok(())]);
    let supervisor = supervisor(Arc::clone(&coordinator));
    let record = test_support::scheduled_restart_record("duplicate", 0);

    assert_eq!(supervisor.track(record.clone()), Ok(RestartTrack::Started));
    coordinator.wait_for_calls(1).await;
    assert_eq!(supervisor.track(record), Ok(RestartTrack::Joined));
    assert_eq!(coordinator.calls.load(Ordering::Acquire), 1);

    coordinator.release(1);
    supervisor
        .wait_until_quiescent()
        .await
        .expect("successful task becomes quiescent");
}

#[tokio::test]
async fn distinct_workload_keys_coordinate_independently() {
    let coordinator = ScriptedCoordinator::new([Ok(()), Ok(())]);
    let supervisor = supervisor(Arc::clone(&coordinator));

    assert_eq!(
        supervisor.track(test_support::scheduled_restart_record("left", 0)),
        Ok(RestartTrack::Started)
    );
    assert_eq!(
        supervisor.track(test_support::scheduled_restart_record("right", 0)),
        Ok(RestartTrack::Started)
    );
    coordinator.wait_for_calls(2).await;
    assert_eq!(coordinator.calls.load(Ordering::Acquire), 2);

    coordinator.release(2);
    supervisor
        .wait_until_quiescent()
        .await
        .expect("both tasks become quiescent");
}

#[tokio::test]
async fn matching_completion_removes_task_and_later_record_can_start() {
    let coordinator = ScriptedCoordinator::new([Ok(()), Ok(())]);
    let supervisor = supervisor(Arc::clone(&coordinator));
    let record = test_support::scheduled_restart_record("later", 0);

    assert_eq!(supervisor.track(record.clone()), Ok(RestartTrack::Started));
    coordinator.wait_for_calls(1).await;
    coordinator.release(1);
    supervisor
        .wait_until_quiescent()
        .await
        .expect("first task becomes quiescent");

    assert_eq!(supervisor.track(record), Ok(RestartTrack::Started));
    coordinator.wait_for_calls(1).await;
    assert_eq!(coordinator.calls.load(Ordering::Acquire), 2);
    coordinator.release(1);
    supervisor
        .wait_until_quiescent()
        .await
        .expect("second task becomes quiescent");
}

#[tokio::test]
async fn dropping_watch_facing_clone_does_not_cancel_retained_work() {
    let coordinator = ScriptedCoordinator::new([Ok(())]);
    let retained_owner = supervisor(Arc::clone(&coordinator));
    let watch_facing = retained_owner.clone();
    let record = test_support::scheduled_restart_record("watch-cancel", 0);

    assert_eq!(
        watch_facing.track(record.clone()),
        Ok(RestartTrack::Started)
    );
    coordinator.wait_for_calls(1).await;
    drop(watch_facing);

    assert_eq!(retained_owner.track(record), Ok(RestartTrack::Joined));
    assert_eq!(coordinator.calls.load(Ordering::Acquire), 1);
    coordinator.release(1);
    retained_owner
        .wait_until_quiescent()
        .await
        .expect("retained owner observes completion");
}

#[tokio::test]
async fn acknowledged_failure_retries_once_and_rejects_stale_completion() {
    let coordinator =
        ScriptedCoordinator::new([Err("durable coordinator unavailable".to_owned()), Ok(())]);
    let supervisor = supervisor(Arc::clone(&coordinator));
    let record = test_support::scheduled_restart_record("failure", 0);
    let key = record.key().clone();

    assert_eq!(supervisor.track(record.clone()), Ok(RestartTrack::Started));
    coordinator.wait_for_calls(1).await;
    coordinator.release(1);
    supervisor
        .wait_until_quiescent()
        .await
        .expect("failed task is no longer active");

    let failure = supervisor
        .failure(&key)
        .expect("failure lookup succeeds")
        .expect("coordinator failure remains observable");
    assert_eq!(failure.message(), "durable coordinator unavailable");
    assert_eq!(
        supervisor.track(record.clone()),
        Ok(RestartTrack::Failed(failure.clone())),
        "watch rescan observes the retained failure without retrying it"
    );
    assert_eq!(coordinator.calls.load(Ordering::Acquire), 1);

    assert!(
        supervisor
            .retire_failure(&failure)
            .expect("exact failure acknowledgement succeeds")
    );
    assert!(
        !supervisor
            .retire_failure(&failure)
            .expect("stale failure acknowledgement is harmless")
    );
    assert_eq!(supervisor.track(record.clone()), Ok(RestartTrack::Started));
    coordinator.wait_for_calls(1).await;
    supervisor.state.complete(key, failure.token, Ok(()));
    assert_eq!(
        supervisor.track(record),
        Ok(RestartTrack::Joined),
        "stale completion token must not remove the active retry"
    );
    coordinator.release(1);
    supervisor
        .wait_until_quiescent()
        .await
        .expect("explicit retry completes");
    assert_eq!(coordinator.calls.load(Ordering::Acquire), 2);
}
