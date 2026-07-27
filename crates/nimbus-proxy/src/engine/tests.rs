use super::*;
use crate::worker::WorkloadPepConfig;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::{
    Barrier,
    atomic::{AtomicBool, Ordering},
    mpsc,
};
use std::thread;
use std::time::{Duration, Instant};

fn test_pep() -> WorkloadPep {
    WorkloadPep::start(
        WorkloadPepConfig::without_active_policy()
            .with_bind_addr(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0)),
    )
    .expect("test PEP should start on an ephemeral port")
}

fn wid(raw: &str) -> WorkloadId {
    WorkloadId::new(raw).expect("test workload id")
}

struct DropProbe(Arc<AtomicBool>);

impl Drop for DropProbe {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }
}

fn wait_until_preparation_is_waiting(preparation: &RegistrationPreparation) {
    let deadline = Instant::now() + Duration::from_secs(1);
    while !preparation
        .wait_started
        .load(std::sync::atomic::Ordering::SeqCst)
    {
        assert!(
            Instant::now() < deadline,
            "same-workload contender did not reach the preparation wait boundary"
        );
        thread::yield_now();
    }
}

fn wait_until_lifecycle_wait_count<A>(lifecycle: &LifecycleCell<A>, expected: usize) {
    let deadline = Instant::now() + Duration::from_secs(1);
    while lifecycle.cleanup_wait_count.load(Ordering::SeqCst) < expected {
        assert!(
            Instant::now() < deadline,
            "same-workload contender did not reach lifecycle wait {expected}"
        );
        thread::yield_now();
    }
}

fn wake_lifecycle_without_progress<A>(lifecycle: &LifecycleCell<A>) {
    let state = lifecycle
        .state
        .lock()
        .expect("lifecycle state should remain healthy");
    lifecycle.changed.notify_all();
    drop(state);
}

#[test]
fn reserve_commit_registers_and_stop_retains_attachment() {
    let engine: EgressEngine<u32> = EgressEngine::new();
    let id = wid("workload-a");

    let slot = engine
        .try_reserve(id.clone())
        .expect("lock healthy")
        .expect("slot free");
    assert_eq!(slot.id(), &id);
    slot.commit(test_pep(), 7)
        .expect("exact preparation should commit");

    assert!(engine.contains(&id).unwrap());
    assert_eq!(engine.len().unwrap(), 1);

    let stop = engine
        .begin_stop_if_attachment(&id, |attachment| *attachment == 7)
        .expect("lock healthy")
        .expect("entry present");
    assert_eq!(stop.with_attachment(|attachment| *attachment).unwrap(), 7);
    stop.shutdown_provider()
        .expect("provider should acknowledge stop");
    engine.complete_stop(&stop).expect("stop should complete");
    assert!(!engine.contains(&id).unwrap());
    assert!(engine.is_empty().unwrap());
}

#[test]
fn second_reserve_for_same_id_is_refused_while_registered() {
    let engine: EgressEngine = EgressEngine::new();
    let id = wid("workload-b");
    engine
        .try_reserve(id.clone())
        .unwrap()
        .expect("first reservation free")
        .commit(test_pep(), ())
        .expect("exact preparation should commit");

    assert!(
        engine.try_reserve(id.clone()).unwrap().is_none(),
        "an occupied id must not hand out a second slot"
    );

    // After deregistration the id is reusable.
    let stop = engine
        .begin_stop_if_attachment(&id, |_| true)
        .unwrap()
        .expect("registered");
    stop.shutdown_provider().expect("provider should stop");
    engine.complete_stop(&stop).expect("stop should complete");
    assert!(engine.try_reserve(id).unwrap().is_some());
}

#[test]
fn dropping_slot_without_commit_releases_reservation() {
    let engine: EgressEngine = EgressEngine::new();
    let id = wid("workload-c");
    let slot = engine.try_reserve(id.clone()).unwrap().expect("free");
    drop(slot);
    assert!(!engine.contains(&id).unwrap());
    assert!(
        engine.try_reserve(id).unwrap().is_some(),
        "an uncommitted reservation must not leak"
    );
}

#[test]
fn failed_registration_commit_retains_cleanup_evidence_until_acknowledged_stop() {
    let engine: EgressEngine<DropProbe> = EgressEngine::new();
    let id = wid("workload-commit-failure");
    let slot = engine
        .try_reserve(id.clone())
        .expect("registry should remain healthy")
        .expect("preparation slot should be free");
    engine
        .lock()
        .expect("registry should remain healthy")
        .remove(&id);
    let dropped = Arc::new(AtomicBool::new(false));

    let failure = slot
        .commit(test_pep(), DropProbe(Arc::clone(&dropped)))
        .expect_err("lost preparation ownership must fail");
    assert!(
        !dropped.load(Ordering::SeqCst),
        "a failed post-activation commit must return caller-owned provider and cleanup evidence"
    );
    let (_, retained) = failure.retain();
    let (stop, conflict) = retained.into_parts();
    assert!(
        conflict.is_none(),
        "the exact failed slot should retain as primary"
    );
    assert!(
        !stop
            .with_attachment(|_| dropped.load(Ordering::SeqCst))
            .expect("retained attachment should remain inspectable")
    );
    assert!(
        engine
            .try_reserve(id.clone())
            .expect("registry should remain inspectable")
            .is_none(),
        "a live failed provider must retain the exact registration fence until shutdown"
    );
    stop.shutdown_provider()
        .expect("caller should explicitly confirm provider shutdown");
    engine
        .complete_stop(&stop)
        .expect("acknowledged failed-provider cleanup should retire");
    assert!(
        dropped.load(Ordering::SeqCst),
        "cleanup evidence may retire only with its acknowledged provider tombstone"
    );
}

#[test]
fn dropped_failed_registration_commit_retains_cleanup_evidence_until_acknowledged_stop() {
    let engine: EgressEngine<DropProbe> = EgressEngine::new();
    let id = wid("workload-dropped-commit-failure");
    let slot = engine
        .try_reserve(id.clone())
        .expect("registry should remain healthy")
        .expect("preparation slot should be free");
    engine
        .lock()
        .expect("registry should remain healthy")
        .remove(&id);
    let dropped = Arc::new(AtomicBool::new(false));

    let failure = slot
        .commit(test_pep(), DropProbe(Arc::clone(&dropped)))
        .expect_err("lost preparation ownership must fail");
    drop(failure);

    assert!(
        !dropped.load(Ordering::SeqCst),
        "dropping a commit error must retain its live provider and cleanup evidence"
    );
    assert!(
        engine
            .try_reserve(id.clone())
            .expect("registry should remain inspectable")
            .is_none(),
        "implicit retention must fence replacement until acknowledged provider shutdown"
    );
    let stop = engine
        .begin_stop_if_attachment(&id, |_| true)
        .expect("registry should remain healthy")
        .expect("implicit retention should leave an exact stopping tombstone");
    stop.shutdown_provider()
        .expect("provider should acknowledge shutdown");
    engine
        .complete_stop(&stop)
        .expect("acknowledged implicit retention should retire");
    assert!(
        dropped.load(Ordering::SeqCst),
        "cleanup evidence may retire only with its acknowledged provider tombstone"
    );
    assert!(engine.is_empty().expect("registry should remain healthy"));
}

#[test]
fn stale_failed_commit_retention_never_replaces_a_foreign_preparation() {
    let engine: EgressEngine<u32> = EgressEngine::new();
    let id = wid("workload-stale-failed-commit");
    let stale = engine
        .try_reserve(id.clone())
        .expect("registry should remain healthy")
        .expect("the first preparation should reserve");
    engine
        .lock()
        .expect("registry should remain healthy")
        .remove(&id);
    let failure = stale
        .commit(test_pep(), 7)
        .expect_err("the removed preparation must reject the stale commit");
    let foreign = engine
        .try_reserve(id.clone())
        .expect("registry should remain healthy")
        .expect("the replacement preparation should reserve");
    let foreign_preparation = Arc::clone(&foreign.preparation);

    let (_, retained) = failure.retain();
    let (stale_stop, conflict) = retained.into_parts();

    assert!(
        !foreign_preparation
            .is_resolved()
            .expect("foreign preparation state should inspect"),
        "stale recovery must not resolve a newer preparation marker"
    );
    let owns_foreign_preparation = matches!(
        engine
            .lock()
            .expect("registry should remain healthy")
            .get(&id),
        Some(EngineEntry::Preparing {
            preparation,
            quarantined,
        }) if Arc::ptr_eq(preparation, &foreign_preparation) && quarantined.len() == 1
    );
    assert!(
        owns_foreign_preparation,
        "stale recovery must preserve the newer preparation and exact quarantine evidence"
    );
    assert!(
        conflict.is_some(),
        "stale cleanup evidence must be retained as a visible quarantine conflict"
    );
    stale_stop
        .shutdown_provider()
        .expect("stale provider should acknowledge shutdown");
    engine
        .complete_stop(&stale_stop)
        .expect("stale quarantine cleanup should retire beside the foreign preparation");
    assert!(
        !foreign_preparation
            .is_resolved()
            .expect("foreign preparation state should inspect"),
        "stale cleanup completion must not resolve the foreign preparation"
    );
    let still_owns_foreign_preparation = matches!(
        engine
            .lock()
            .expect("registry should remain healthy")
            .get(&id),
        Some(EngineEntry::Preparing {
            preparation,
            quarantined,
        }) if Arc::ptr_eq(preparation, &foreign_preparation) && quarantined.is_empty()
    );
    assert!(
        still_owns_foreign_preparation,
        "stale cleanup completion must remove only its quarantine evidence"
    );

    foreign
        .commit(test_pep(), 11)
        .expect("the foreign preparation must retain commit authority");
    assert!(
        engine.contains(&id).expect("registry should inspect"),
        "the foreign primary may become ready only after stale quarantine retirement"
    );
    let foreign_stop = engine
        .begin_stop_if_attachment(&id, |attachment| *attachment == 11)
        .expect("foreign lifecycle should inspect")
        .expect("foreign primary should remain");
    foreign_stop
        .shutdown_provider()
        .expect("foreign provider should acknowledge shutdown");
    engine
        .complete_stop(&foreign_stop)
        .expect("foreign primary should retire");
}

#[test]
fn commit_rejects_new_primary_while_quarantine_evidence_remains() {
    let engine: EgressEngine<u32> = EgressEngine::new();
    let id = wid("workload-quarantine-before-commit");
    let stale = engine
        .try_reserve(id.clone())
        .expect("registry should remain healthy")
        .expect("the first preparation should reserve");
    engine
        .lock()
        .expect("registry should remain healthy")
        .remove(&id);
    let stale_failure = stale
        .commit(test_pep(), 7)
        .expect_err("the removed preparation must reject the stale commit");
    let foreign = engine
        .try_reserve(id.clone())
        .expect("registry should remain healthy")
        .expect("the replacement preparation should reserve");
    let (_, retained_stale) = stale_failure.retain();
    let (stale_stop, stale_conflict) = retained_stale.into_parts();
    assert!(
        stale_conflict.is_some(),
        "stale provider evidence must be quarantined beside the foreign preparation"
    );

    let attempted = test_pep();
    let attempted_addr = attempted.local_addr();
    let commit_failure = foreign
        .commit(attempted, 11)
        .expect_err("unresolved quarantine must fence a new running primary");
    assert_eq!(
        commit_failure.provider_local_addr(),
        attempted_addr,
        "commit rejection must return the exact caller-owned provider"
    );
    let (_, retained_foreign) = commit_failure.retain();
    let (foreign_stop, foreign_conflict) = retained_foreign.into_parts();
    assert!(
        foreign_conflict.is_some(),
        "the rejected primary must retain the earlier quarantine conflict"
    );
    assert!(
        !engine.contains(&id).expect("registry should inspect"),
        "neither provider may become request-ready while quarantine remains"
    );

    foreign_stop
        .shutdown_provider()
        .expect("rejected foreign provider should acknowledge shutdown");
    engine
        .complete_stop(&foreign_stop)
        .expect("foreign cleanup should preserve stale quarantine evidence");
    assert!(
        !engine.contains(&id).expect("registry should inspect"),
        "stale quarantine must remain fail-closed after foreign cleanup"
    );
    stale_stop
        .shutdown_provider()
        .expect("stale provider should acknowledge shutdown");
    engine
        .complete_stop(&stale_stop)
        .expect("stale quarantine should retire independently");
    assert!(engine.is_empty().expect("registry should remain healthy"));
}

#[test]
fn foreign_slot_drop_preserves_quarantined_failed_cleanup() {
    let engine: EgressEngine<u32> = EgressEngine::new();
    let id = wid("workload-foreign-drop-quarantine");
    let stale = engine
        .try_reserve(id.clone())
        .expect("registry should remain healthy")
        .expect("the first preparation should reserve");
    engine
        .lock()
        .expect("registry should remain healthy")
        .remove(&id);
    let failure = stale
        .commit(test_pep(), 7)
        .expect_err("the removed preparation must reject the stale commit");
    let foreign = engine
        .try_reserve(id.clone())
        .expect("registry should remain healthy")
        .expect("the replacement preparation should reserve");
    let foreign_preparation = Arc::clone(&foreign.preparation);
    let (_, retained) = failure.retain();
    let (stale_stop, conflict) = retained.into_parts();
    assert!(
        conflict.is_some(),
        "foreign preparation collision must remain visible"
    );

    drop(foreign);
    assert!(
        foreign_preparation
            .is_resolved()
            .expect("foreign preparation state should inspect"),
        "dropping the exact foreign slot must wake its waiters"
    );
    assert!(
        !engine.contains(&id).expect("registry should inspect"),
        "dropping a foreign slot must preserve fail-closed quarantine evidence"
    );
    assert_eq!(
        stale_stop
            .with_attachment(|attachment| *attachment)
            .expect("stale attachment should remain exact"),
        7
    );
    stale_stop
        .shutdown_provider()
        .expect("stale provider should acknowledge shutdown");
    engine
        .complete_stop(&stale_stop)
        .expect("preserved quarantine should retire independently");
    assert!(engine.is_empty().expect("registry should remain healthy"));
}

#[test]
fn exact_failed_retention_carries_preexisting_quarantine() {
    let engine: EgressEngine<u32> = EgressEngine::new();
    let id = wid("workload-foreign-failure-quarantine");
    let stale = engine
        .try_reserve(id.clone())
        .expect("registry should remain healthy")
        .expect("the first preparation should reserve");
    engine
        .lock()
        .expect("registry should remain healthy")
        .remove(&id);
    let failure = stale
        .commit(test_pep(), 7)
        .expect_err("the removed preparation must reject the stale commit");
    let foreign = engine
        .try_reserve(id.clone())
        .expect("registry should remain healthy")
        .expect("the replacement preparation should reserve");
    let (_, retained_stale) = failure.retain();
    let (stale_stop, stale_conflict) = retained_stale.into_parts();
    assert!(
        stale_conflict.is_some(),
        "stale provider evidence must be quarantined beside the foreign preparation"
    );

    let (_, retained_foreign) = foreign.retain_failed(
        EgressProxyError::OperationFailed {
            message: "injected foreign post-activation failure".to_owned(),
        },
        test_pep(),
        11,
    );
    let (foreign_stop, foreign_conflict) = retained_foreign.into_parts();
    assert!(
        foreign_conflict.is_some(),
        "the exact foreign failure must report and carry earlier quarantine evidence"
    );
    assert!(
        !engine.contains(&id).expect("registry should inspect"),
        "neither failed provider may become request-ready"
    );

    foreign_stop
        .shutdown_provider()
        .expect("foreign provider should acknowledge shutdown");
    engine
        .complete_stop(&foreign_stop)
        .expect("foreign primary cleanup should preserve stale quarantine evidence");
    assert!(
        !engine.contains(&id).expect("registry should inspect"),
        "stale quarantine must remain fail-closed after primary cleanup"
    );
    stale_stop
        .shutdown_provider()
        .expect("stale provider should acknowledge shutdown");
    engine
        .complete_stop(&stale_stop)
        .expect("stale quarantine should retire independently");
    assert!(engine.is_empty().expect("registry should remain healthy"));
}

#[test]
fn failed_registration_retention_repairs_registry_poison_for_stop_retry() {
    let engine: EgressEngine<u32> = EgressEngine::new();
    let id = wid("workload-poison-recovery");
    let slot = engine
        .try_reserve(id.clone())
        .expect("registry should start healthy")
        .expect("preparation slot should be free");
    let poison = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _guard = engine
            .peps
            .lock()
            .expect("registry should start unpoisoned");
        panic!("poison the registry while holding its lock");
    }));
    assert!(poison.is_err(), "the test must poison the registry lock");
    assert!(engine.peps.is_poisoned());

    let (_, retained) = slot.retain_failed(
        EgressProxyError::OperationFailed {
            message: "injected post-activation failure".to_owned(),
        },
        test_pep(),
        7,
    );
    let (stop, conflict) = retained.into_parts();
    assert!(
        conflict.is_none(),
        "vacant failed-registration retention should not synthesize a collision"
    );
    assert!(
        !engine.peps.is_poisoned(),
        "repaired registry invariants must clear the poison flag"
    );
    stop.shutdown_provider()
        .expect("retained provider should acknowledge stop");
    engine
        .complete_stop(&stop)
        .expect("normal retry must remove the retained tombstone");
    assert!(!engine.contains(&id).expect("registry should remain usable"));
}

#[test]
fn failed_commit_drop_repairs_poisoned_registry_and_releases_exact_preparation() {
    let engine: EgressEngine<u32> = EgressEngine::new();
    let id = wid("workload-resolved-preparation");
    let slot = engine
        .try_reserve(id.clone())
        .expect("registry should start healthy")
        .expect("preparation slot should be free");
    let poison = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _guard = engine
            .peps
            .lock()
            .expect("registry should start unpoisoned");
        panic!("poison the registry before preparation commit");
    }));
    assert!(poison.is_err(), "the test must poison the registry lock");

    let failure = slot
        .commit(test_pep(), 7)
        .expect_err("poisoned commit must return exact cleanup evidence");
    let (_, retained) = failure.retain();
    let (stop, conflict) = retained.into_parts();
    assert!(
        conflict.is_none(),
        "the exact poisoned slot should retain as primary"
    );
    assert_eq!(
        stop.with_attachment(|attachment| *attachment)
            .expect("retained attachment should inspect"),
        7
    );
    stop.shutdown_provider()
        .expect("failed provider should acknowledge shutdown");
    engine
        .complete_stop(&stop)
        .expect("acknowledged failed provider should retire");

    assert!(
        !engine.peps.is_poisoned(),
        "dropping the exact failed slot must repair the global registry lock"
    );
    assert!(
        !engine
            .contains(&id)
            .expect("the repaired registry should remain inspectable"),
        "the resolved preparation marker must not survive exact caller cleanup"
    );
    let retry = engine
        .try_reserve(id.clone())
        .expect("the repaired registry should admit a retry")
        .expect("the exact workload preparation must become vacant");
    retry
        .commit(test_pep(), 11)
        .expect("a fresh exact registration should commit");
    assert!(
        engine
            .contains(&id)
            .expect("the replacement registration should inspect"),
        "successful recovery must restore request-ready registration"
    );
}

#[test]
fn failed_registration_retention_conflict_quarantines_exact_cleanup_evidence() {
    let engine: EgressEngine<u32> = EgressEngine::new();
    let id = wid("workload-retention-conflict");
    let stale = engine
        .try_reserve(id.clone())
        .expect("registry should remain healthy")
        .expect("stale slot should be vacant");
    engine
        .lock()
        .expect("registry should remain healthy")
        .remove(&id);
    engine
        .try_reserve(id.clone())
        .expect("registry should remain healthy")
        .expect("primary slot should be vacant")
        .commit(test_pep(), 7)
        .expect("primary lifecycle should commit");

    let (_, retained) = stale.retain_failed(
        EgressProxyError::OperationFailed {
            message: "injected stale post-activation failure".to_owned(),
        },
        test_pep(),
        11,
    );
    let (quarantine, conflict) = retained.into_parts();
    assert!(
        conflict
            .expect("the occupied primary must produce a visible conflict")
            .to_string()
            .contains("quarantine tombstone"),
        "the retention diagnostic should name the exact safe disposition"
    );
    assert!(
        !engine
            .contains(&id)
            .expect("conflicted registry should inspect"),
        "readiness must fail closed while quarantine evidence exists"
    );
    assert!(
        engine
            .with_pep(&id, |_| ())
            .expect("conflicted registry should inspect")
            .is_none(),
        "request-facing access must not select either conflicting provider"
    );
    let registration_inspected = std::cell::Cell::new(false);
    let decision = engine.reserve_or_inspect(id.clone(), |_| {
        registration_inspected.set(true);
    });
    assert!(
        matches!(decision, Err(EgressProxyError::OperationFailed { .. })),
        "ordinary registration must fail closed while conflicting quarantine evidence exists"
    );
    assert!(
        !registration_inspected.get(),
        "registration idempotency must not select an arbitrary primary or quarantine attachment"
    );
    assert_eq!(
        quarantine
            .with_attachment(|attachment| *attachment)
            .expect("quarantine attachment should remain exact"),
        11
    );
    drop(quarantine);

    let retry = engine
        .begin_stop_if_attachment(&id, |attachment| *attachment == 11)
        .expect("quarantine retry should remain addressable")
        .expect("quarantine tombstone should remain retained");
    retry
        .shutdown_provider()
        .expect("quarantined provider should acknowledge shutdown");
    engine
        .complete_stop(&retry)
        .expect("exact quarantine tombstone should retire");
    assert!(
        engine
            .contains(&id)
            .expect("primary lifecycle should re-inspect"),
        "retiring quarantine evidence must preserve the original primary lifecycle"
    );

    let primary = engine
        .begin_stop_if_attachment(&id, |attachment| *attachment == 7)
        .expect("primary stop should inspect")
        .expect("primary lifecycle should remain");
    primary
        .shutdown_provider()
        .expect("primary provider should acknowledge shutdown");
    engine
        .complete_stop(&primary)
        .expect("primary lifecycle should retire");
}

#[test]
fn registration_preparation_does_not_block_unrelated_workload_lifecycle() {
    let engine: Arc<EgressEngine> = Arc::new(EgressEngine::new());
    let slot = engine
        .try_reserve(wid("workload-preparing"))
        .expect("registry should remain healthy")
        .expect("preparation slot should be free");
    let probe_engine = Arc::clone(&engine);
    let unrelated_id = wid("workload-unrelated");
    let (completed_tx, completed_rx) = mpsc::channel();
    let started = Arc::new(Barrier::new(2));
    let worker_started = Arc::clone(&started);
    let worker = thread::spawn(move || {
        worker_started.wait();
        completed_tx
            .send(probe_engine.contains(&unrelated_id))
            .expect("unrelated result should send");
    });

    started.wait();
    let unrelated_result = completed_rx.recv_timeout(Duration::from_secs(1));
    drop(slot);
    worker.join().expect("unrelated probe should join");

    assert!(
        !unrelated_result
            .expect("unrelated lifecycle reads must not wait for another workload's preparation")
            .expect("registry should remain healthy"),
        "an unrelated workload must remain absent"
    );
}

#[test]
fn same_workload_registration_waits_then_rechecks_committed_preparation() {
    let engine: Arc<EgressEngine> = Arc::new(EgressEngine::new());
    let id = wid("workload-same-registration");
    let slot = engine
        .try_reserve(id.clone())
        .expect("registry should remain healthy")
        .expect("preparation slot should be free");
    let preparation = Arc::clone(&slot.preparation);
    let contender_engine = Arc::clone(&engine);
    let contender_id = id.clone();
    let (completed_tx, completed_rx) = mpsc::channel();
    let started = Arc::new(Barrier::new(2));
    let worker_started = Arc::clone(&started);
    let worker = thread::spawn(move || {
        worker_started.wait();
        completed_tx
            .send(
                contender_engine
                    .try_reserve(contender_id)
                    .map(|candidate| candidate.is_none()),
            )
            .expect("contender result should send");
    });

    started.wait();
    wait_until_preparation_is_waiting(&preparation);
    assert!(
        matches!(completed_rx.try_recv(), Err(mpsc::TryRecvError::Empty)),
        "the contender must remain blocked after entering the exact preparation wait"
    );
    slot.commit(test_pep(), ())
        .expect("exact preparation should commit");
    let contender_observed_existing = completed_rx
        .recv_timeout(Duration::from_secs(1))
        .expect("same-workload contender should wake after commit");
    worker.join().expect("same-workload contender should join");

    assert!(
        contender_observed_existing.expect("registry should remain healthy"),
        "the contender must re-check and observe the committed registration"
    );
}

#[test]
fn dropping_preparation_wakes_same_workload_for_a_new_exact_slot() {
    let engine: Arc<EgressEngine> = Arc::new(EgressEngine::new());
    let id = wid("workload-same-drop");
    let slot = engine
        .try_reserve(id.clone())
        .expect("registry should remain healthy")
        .expect("preparation slot should be free");
    let preparation = Arc::clone(&slot.preparation);
    let contender_engine = Arc::clone(&engine);
    let (completed_tx, completed_rx) = mpsc::channel();
    let started = Arc::new(Barrier::new(2));
    let worker_started = Arc::clone(&started);
    let worker = thread::spawn(move || {
        worker_started.wait();
        completed_tx
            .send(
                contender_engine
                    .try_reserve(id)
                    .map(|candidate| candidate.is_some()),
            )
            .expect("contender result should send");
    });

    started.wait();
    wait_until_preparation_is_waiting(&preparation);
    assert!(
        matches!(completed_rx.try_recv(), Err(mpsc::TryRecvError::Empty)),
        "the contender must remain blocked after entering the exact preparation wait"
    );
    drop(slot);
    let contender_acquired = completed_rx
        .recv_timeout(Duration::from_secs(1))
        .expect("same-workload contender should wake after withdrawal");
    worker.join().expect("same-workload contender should join");

    assert!(
        contender_acquired.expect("registry should remain healthy"),
        "dropping the prior preparation must make the exact id reusable"
    );
}

#[test]
fn with_pep_exposes_lifecycle_reads_without_escaping_handles() {
    let engine: EgressEngine = EgressEngine::new();
    let id = wid("workload-d");
    engine
        .try_reserve(id.clone())
        .unwrap()
        .expect("free")
        .commit(test_pep(), ())
        .expect("exact preparation should commit");

    let addr = engine
        .with_pep(&id, |pep| pep.local_addr())
        .unwrap()
        .expect("registered");
    assert_eq!(addr.ip(), IpAddr::V4(Ipv4Addr::LOCALHOST));
    assert_ne!(addr.port(), 0);

    assert!(
        engine
            .with_pep(&wid("workload-absent"), |pep| pep.local_addr())
            .unwrap()
            .is_none(),
        "absent id reads as None, not an error"
    );
}

#[test]
fn conditional_stop_preserves_a_divergent_attachment() {
    let engine: EgressEngine<u32> = EgressEngine::new();
    let id = wid("workload-conditional-stop");
    engine
        .try_reserve(id.clone())
        .unwrap()
        .expect("free")
        .commit(test_pep(), 7)
        .expect("exact preparation should commit");

    assert!(
        engine
            .begin_stop_if_attachment(&id, |attachment| *attachment == 8)
            .expect("lock healthy")
            .is_none(),
        "a stale attachment must not remove the current provider"
    );
    assert!(engine.contains(&id).expect("registry should inspect"));

    let stop = engine
        .begin_stop_if_attachment(&id, |attachment| *attachment == 7)
        .expect("lock healthy")
        .expect("the exact attachment may remove the provider");
    assert_eq!(stop.with_attachment(|attachment| *attachment).unwrap(), 7);
    stop.shutdown_provider().expect("provider should stop");
    engine.complete_stop(&stop).expect("stop should complete");
    assert!(!engine.contains(&id).expect("registry should inspect"));
}

#[test]
fn with_attachment_exposes_only_the_registered_lifecycle_evidence() {
    let engine: EgressEngine<String> = EgressEngine::new();
    let id = wid("workload-attachment");
    engine
        .try_reserve(id.clone())
        .unwrap()
        .expect("free")
        .commit(test_pep(), "lease-generation-7".to_owned())
        .expect("exact preparation should commit");

    let evidence = engine
        .with_attachment(&id, |attachment| attachment.clone())
        .expect("lock healthy")
        .expect("registered");
    assert_eq!(evidence, "lease-generation-7");
    assert!(
        engine
            .with_attachment(&wid("workload-absent"), String::clone)
            .expect("lock healthy")
            .is_none(),
        "absent lifecycle evidence must not be synthesized"
    );
}

#[test]
fn begin_stop_absent_id_is_none_not_error() {
    let engine: EgressEngine = EgressEngine::new();
    assert!(
        engine
            .begin_stop_if_attachment(&wid("never-registered"), |_| true)
            .unwrap()
            .is_none()
    );
}

#[test]
fn stopping_tombstone_denies_readiness_and_replacement_until_acknowledged_completion() {
    let engine: EgressEngine<String> = EgressEngine::new();
    let id = wid("workload-stopping");
    engine
        .try_reserve(id.clone())
        .unwrap()
        .expect("free")
        .commit(test_pep(), "lease-generation-7".to_owned())
        .expect("exact preparation should commit");
    let stop = engine
        .begin_stop_if_attachment(&id, |attachment| attachment == "lease-generation-7")
        .unwrap()
        .expect("exact attachment should begin stop");

    assert!(!engine.contains(&id).expect("registry should inspect"));
    assert!(
        engine
            .with_pep(&id, |pep| pep.local_addr())
            .expect("registry should inspect")
            .is_none(),
        "a stopping entry must never publish lifecycle readiness"
    );
    assert!(
        engine.try_reserve(id.clone()).unwrap().is_none(),
        "a stopping entry must retain the occupied registration fence"
    );
    assert!(
        engine
            .begin_stop_if_attachment(&id, |attachment| attachment == "other-generation")
            .unwrap()
            .is_none(),
        "a divergent attachment must not borrow cleanup evidence"
    );
    assert!(
        engine.complete_stop(&stop).is_err(),
        "completion before provider acknowledgement must fail"
    );

    stop.shutdown_provider()
        .expect("provider should acknowledge stop");
    engine.complete_stop(&stop).expect("stop should complete");
    assert!(engine.try_reserve(id).unwrap().is_some());
}

#[test]
fn stopping_attachment_callback_can_reenter_registry_without_lock_inversion() {
    let engine = Arc::new(EgressEngine::<u32>::new());
    let id = wid("workload-stopping-lock-order");
    engine
        .try_reserve(id.clone())
        .unwrap()
        .expect("slot should be free")
        .commit(test_pep(), 7)
        .expect("exact preparation should commit");
    let stop = engine
        .begin_stop_if_attachment(&id, |attachment| *attachment == 7)
        .expect("registry should remain healthy")
        .expect("exact running entry should stop");
    let worker_engine = Arc::clone(&engine);
    let worker_id = id.clone();
    let (completed_tx, completed_rx) = std::sync::mpsc::channel();

    let worker = std::thread::spawn(move || {
        let running = stop
            .with_attachment_mut(|attachment| {
                *attachment += 1;
                worker_engine.contains(&worker_id)
            })
            .expect("tombstone should remain healthy")
            .expect("registry re-entry must not deadlock");
        assert!(!running, "a stopping entry must remain non-ready");
        stop.shutdown_provider().expect("provider should stop");
        worker_engine
            .complete_stop(&stop)
            .expect("acknowledged stop should complete");
        completed_tx.send(()).expect("completion should report");
    });

    completed_rx
        .recv_timeout(std::time::Duration::from_secs(2))
        .expect("registry re-entry must complete without lock inversion");
    worker.join().expect("lock-order worker should join");
    assert!(!engine.contains(&id).expect("registry should inspect"));
}

#[test]
fn running_stop_matcher_reenters_registry_after_node_map_lock_is_released() {
    let engine = Arc::new(EgressEngine::<u32>::new());
    let id = wid("workload-running-lock-order");
    engine
        .try_reserve(id.clone())
        .unwrap()
        .expect("slot should be free")
        .commit(test_pep(), 7)
        .expect("exact preparation should commit");
    let worker_engine = Arc::clone(&engine);
    let worker_id = id.clone();
    let (completed_tx, completed_rx) = mpsc::channel();

    let worker = thread::spawn(move || {
        let stop = worker_engine
            .begin_stop_if_attachment(&worker_id, |attachment| {
                assert_eq!(*attachment, 7);
                worker_engine
                    .contains(&worker_id)
                    .expect("matcher must be able to re-enter the node registry")
            })
            .expect("registry should remain healthy")
            .expect("exact running attachment should stop");
        stop.shutdown_provider().expect("provider should stop");
        worker_engine
            .complete_stop(&stop)
            .expect("acknowledged stop should complete");
        completed_tx.send(()).expect("completion should report");
    });

    completed_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("running matcher must not execute under the node-global map lock");
    worker.join().expect("lock-order worker should join");
    assert!(!engine.contains(&id).expect("registry should inspect"));
}

#[test]
fn dropped_stop_handle_transfers_one_cleanup_executor_to_waiting_retry() {
    let engine = Arc::new(EgressEngine::<u32>::new());
    let id = wid("workload-exclusive-cleanup");
    engine
        .try_reserve(id.clone())
        .unwrap()
        .expect("slot should be free")
        .commit(test_pep(), 7)
        .expect("exact preparation should commit");
    let first = engine
        .begin_stop_if_attachment(&id, |attachment| *attachment == 7)
        .expect("registry should remain healthy")
        .expect("first cleanup executor should be granted");
    let lifecycle = engine
        .lifecycle(&id)
        .expect("registry should remain healthy")
        .expect("lifecycle cell should remain occupied");
    let retry_engine = Arc::clone(&engine);
    let retry_id = id.clone();
    let (retry_tx, retry_rx) = mpsc::channel();
    let retry = thread::spawn(move || {
        let stop = retry_engine
            .begin_stop_if_attachment(&retry_id, |attachment| *attachment == 7)
            .expect("registry should remain healthy")
            .expect("retry should inherit exact cleanup evidence");
        retry_tx.send(stop).expect("retry handle should report");
    });

    let deadline = Instant::now() + Duration::from_secs(2);
    while !lifecycle.cleanup_wait_started.load(Ordering::SeqCst) {
        assert!(
            Instant::now() < deadline,
            "retry did not reach the per-workload cleanup wait"
        );
        thread::yield_now();
    }
    assert!(
        matches!(retry_rx.try_recv(), Err(mpsc::TryRecvError::Empty)),
        "two cleanup executors must never run concurrently"
    );

    drop(first);
    let second = retry_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("dropping the first executor must wake the exact retry");
    retry.join().expect("retry thread should join");
    assert_eq!(
        second
            .with_attachment(|attachment| *attachment)
            .expect("retry owns the attachment"),
        7
    );
    second.shutdown_provider().expect("provider should stop");
    engine
        .complete_stop(&second)
        .expect("retry should complete the exact lifecycle");
    assert!(!engine.contains(&id).expect("registry should inspect"));
}

#[test]
fn cleanup_wait_timeout_is_one_operation_budget_across_spurious_wakeups() {
    let engine = Arc::new(EgressEngine::<u32>::new());
    let id = wid("workload-cleanup-deadline");
    engine
        .try_reserve(id.clone())
        .unwrap()
        .expect("slot should be free")
        .commit(test_pep(), 7)
        .expect("exact preparation should commit");
    let first = engine
        .begin_stop_if_attachment(&id, |attachment| *attachment == 7)
        .expect("registry should remain healthy")
        .expect("first cleanup executor should be granted");
    let lifecycle = engine
        .lifecycle(&id)
        .expect("registry should remain healthy")
        .expect("lifecycle cell should remain occupied");
    let (budget, forced_expired) = LifecycleWaitBudget::controlled();
    let retry_engine = Arc::clone(&engine);
    let retry_id = id.clone();
    let (retry_tx, retry_rx) = mpsc::channel();
    let retry = thread::spawn(move || {
        let result = retry_engine.begin_stop_if_attachment_until(
            &retry_id,
            |attachment| *attachment == 7,
            &budget,
        );
        retry_tx.send(result).expect("retry result should report");
    });

    for expected_wait in 1..=3 {
        wait_until_lifecycle_wait_count(&lifecycle, expected_wait);
        assert!(
            matches!(retry_rx.try_recv(), Err(mpsc::TryRecvError::Empty)),
            "a spurious wake must not grant a concurrent cleanup executor"
        );
        wake_lifecycle_without_progress(&lifecycle);
    }
    wait_until_lifecycle_wait_count(&lifecycle, 4);
    forced_expired.store(true, Ordering::SeqCst);
    wake_lifecycle_without_progress(&lifecycle);

    let error = match retry_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("expired operation budget should return")
    {
        Err(error) => error,
        Ok(_) => panic!("expired operation budget must not grant a cleanup executor"),
    };
    assert!(
        error
            .to_string()
            .contains("timed out waiting for same-workload egress cleanup executor"),
        "timeout should identify the fenced cleanup wait: {error}"
    );
    retry.join().expect("retry thread should join");

    {
        let state = lifecycle.lock().expect("lifecycle should remain healthy");
        assert!(matches!(
            &*state,
            LifecycleState::Stopping {
                attachment: 7,
                provider_stopped: false,
                active_executor: Some(executor),
                ..
            } if *executor == first.executor
        ));
    }
    assert!(
        !engine
            .contains(&id)
            .expect("stopping lifecycle should remain inspectable"),
        "timed-out cleanup must remain fail-closed"
    );
    assert!(
        engine
            .try_reserve(id.clone())
            .expect("registry should remain healthy")
            .is_none(),
        "timed-out cleanup must retain the occupied registration fence"
    );

    drop(first);
    let second = engine
        .begin_stop_if_attachment(&id, |attachment| *attachment == 7)
        .expect("fresh retry should remain healthy")
        .expect("fresh retry should inherit exact cleanup evidence");
    assert_eq!(
        second
            .with_attachment(|attachment| *attachment)
            .expect("fresh retry owns the attachment"),
        7
    );
    second.shutdown_provider().expect("provider should stop");
    engine
        .complete_stop(&second)
        .expect("fresh retry should complete the exact lifecycle");
    assert!(!engine.contains(&id).expect("registry should inspect"));
}

#[test]
fn matching_retiring_cleanup_is_fenced_as_existing_work() {
    let engine = EgressEngine::<u32>::new();
    let id = wid("workload-cleanup-retiring");
    engine
        .try_reserve(id.clone())
        .unwrap()
        .expect("slot should be free")
        .commit(test_pep(), 7)
        .expect("exact preparation should commit");
    let stop = engine
        .begin_stop_if_attachment(&id, |attachment| *attachment == 7)
        .expect("registry should remain healthy")
        .expect("cleanup executor should be granted");
    let lifecycle = engine
        .lifecycle(&id)
        .expect("registry should remain healthy")
        .expect("lifecycle cell should remain occupied");
    {
        let mut state = lifecycle.lock().expect("lifecycle should remain healthy");
        let LifecycleState::Stopping {
            pep,
            attachment,
            provider_stopped,
            active_executor,
            next_executor,
        } = std::mem::replace(&mut *state, LifecycleState::Retired)
        else {
            panic!("cleanup should own a stopping lifecycle");
        };
        assert_eq!(active_executor, Some(stop.executor));
        *state = LifecycleState::Retiring {
            pep,
            attachment,
            provider_stopped,
            executor: stop.executor,
            next_executor,
        };
        lifecycle.set_phase(LifecyclePhase::Retiring);
    }

    let (budget, forced_expired) = LifecycleWaitBudget::controlled();
    forced_expired.store(true, Ordering::SeqCst);
    let result = engine.begin_stop_if_attachment_until(&id, |attachment| *attachment == 7, &budget);
    let error = match result {
        Err(error) => error,
        Ok(_) => panic!("matching retiring evidence must not be reported as absent"),
    };
    assert!(
        error
            .to_string()
            .contains("timed out waiting for same-workload egress cleanup executor"),
        "matching retiring cleanup must enter the existing-work wait: {error}"
    );
    {
        let state = lifecycle.lock().expect("lifecycle should remain healthy");
        assert!(matches!(
            &*state,
            LifecycleState::Retiring {
                attachment: 7,
                executor,
                ..
            } if *executor == stop.executor
        ));
    }

    lifecycle.restore_retiring(stop.executor);
    stop.shutdown_provider().expect("provider should stop");
    engine
        .complete_stop(&stop)
        .expect("restored cleanup executor should converge");
    assert!(!engine.contains(&id).expect("registry should inspect"));
}

#[test]
fn reserve_or_inspect_timeout_is_one_operation_budget_across_retiring_wakeups() {
    let engine = Arc::new(EgressEngine::<u32>::new());
    let id = wid("workload-registration-retirement-deadline");
    engine
        .try_reserve(id.clone())
        .unwrap()
        .expect("slot should be free")
        .commit(test_pep(), 7)
        .expect("exact preparation should commit");
    let stop = engine
        .begin_stop_if_attachment(&id, |attachment| *attachment == 7)
        .expect("registry should remain healthy")
        .expect("cleanup executor should be granted");
    let lifecycle = engine
        .lifecycle(&id)
        .expect("registry should remain healthy")
        .expect("lifecycle cell should remain occupied");
    {
        let mut state = lifecycle.lock().expect("lifecycle should remain healthy");
        let LifecycleState::Stopping {
            pep,
            attachment,
            provider_stopped,
            active_executor,
            next_executor,
        } = std::mem::replace(&mut *state, LifecycleState::Retired)
        else {
            panic!("cleanup should own a stopping lifecycle");
        };
        assert_eq!(active_executor, Some(stop.executor));
        *state = LifecycleState::Retiring {
            pep,
            attachment,
            provider_stopped,
            executor: stop.executor,
            next_executor,
        };
        lifecycle.set_phase(LifecyclePhase::Retiring);
    }

    let (budget, forced_expired) = LifecycleWaitBudget::controlled();
    let inspected = Arc::new(AtomicBool::new(false));
    let retry_engine = Arc::clone(&engine);
    let retry_id = id.clone();
    let retry_inspected = Arc::clone(&inspected);
    let (retry_tx, retry_rx) = mpsc::channel();
    let retry = thread::spawn(move || {
        let result = retry_engine
            .reserve_or_inspect_until(
                retry_id,
                |_| {
                    retry_inspected.store(true, Ordering::SeqCst);
                },
                &budget,
            )
            .map(|decision| match decision {
                RegistrationDecision::Reserved(slot) => drop(slot),
                RegistrationDecision::Occupied { .. } => {}
            });
        retry_tx.send(result).expect("retry result should report");
    });

    for expected_wait in 1..=3 {
        wait_until_lifecycle_wait_count(&lifecycle, expected_wait);
        assert!(
            matches!(retry_rx.try_recv(), Err(mpsc::TryRecvError::Empty)),
            "a spurious wake must not inspect or reserve a retiring lifecycle"
        );
        wake_lifecycle_without_progress(&lifecycle);
    }
    wait_until_lifecycle_wait_count(&lifecycle, 4);
    forced_expired.store(true, Ordering::SeqCst);
    wake_lifecycle_without_progress(&lifecycle);

    let error = retry_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("expired operation budget should return")
        .expect_err("expired operation budget must not produce a registration decision");
    assert!(
        error
            .to_string()
            .contains("timed out waiting for same-workload egress registration retirement"),
        "timeout should identify the fenced retirement wait: {error}"
    );
    retry.join().expect("retry thread should join");
    assert!(
        !inspected.load(Ordering::SeqCst),
        "timed-out retirement must not expose attachment evidence"
    );
    {
        let state = lifecycle.lock().expect("lifecycle should remain healthy");
        assert!(matches!(
            &*state,
            LifecycleState::Retiring {
                attachment: 7,
                executor,
                ..
            } if *executor == stop.executor
        ));
    }

    lifecycle.restore_retiring(stop.executor);
    stop.shutdown_provider().expect("provider should stop");
    engine
        .complete_stop(&stop)
        .expect("restored cleanup executor should converge");
    assert!(!engine.contains(&id).expect("registry should inspect"));
}

#[test]
fn reserve_or_inspect_reports_running_and_stopping_without_vacancy_window() {
    let engine: EgressEngine<u32> = EgressEngine::new();
    let id = wid("workload-registration-decision");
    engine
        .try_reserve(id.clone())
        .unwrap()
        .expect("slot should be free")
        .commit(test_pep(), 7)
        .expect("exact preparation should commit");

    assert!(matches!(
        engine
            .reserve_or_inspect(id.clone(), |attachment| *attachment)
            .expect("decision should remain healthy"),
        RegistrationDecision::Occupied {
            phase: RegisteredLifecyclePhase::Running,
            evidence: 7
        }
    ));

    let stop = engine
        .begin_stop_if_attachment(&id, |attachment| *attachment == 7)
        .expect("registry should remain healthy")
        .expect("exact cleanup should begin");
    assert!(matches!(
        engine
            .reserve_or_inspect(id.clone(), |attachment| *attachment)
            .expect("decision should remain healthy"),
        RegistrationDecision::Occupied {
            phase: RegisteredLifecyclePhase::Stopping,
            evidence: 7
        }
    ));
    stop.shutdown_provider().expect("provider should stop");
    engine.complete_stop(&stop).expect("stop should complete");
    assert!(matches!(
        engine
            .reserve_or_inspect(id, |attachment| *attachment)
            .expect("vacant decision should remain healthy"),
        RegistrationDecision::Reserved(_)
    ));
}
