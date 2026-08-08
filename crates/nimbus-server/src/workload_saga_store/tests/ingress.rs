use std::fs::OpenOptions;
use std::future::pending;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use nimbus_compute::workload_saga::{
    WorkloadSagaCoordinator, WorkloadSagaDecision, WorkloadSagaIngressDisposition,
};
use nimbus_core::TenantId;
use nimbus_engine::Engine;
use nimbus_testing::{
    ContentionOutcome, ProcessRoleSpec, SubprocessCrashCutHarness, TwoProcessContentionHarness,
    run_contention_child, run_crash_cut_child, run_crash_recovery_child,
};
use nimbus_workloads::{
    WorkloadSagaCommit, WorkloadSagaError, WorkloadSagaExpected, WorkloadSagaFuture,
    WorkloadSagaKey, WorkloadSagaPage, WorkloadSagaPageRequest, WorkloadSagaRecord,
    WorkloadSagaStore, WorkloadSagaStoreError, WorkloadSagaTenantPage,
    WorkloadSagaTenantPageRequest,
};
use sha2::{Digest, Sha256};
use tokio::sync::Notify;

use super::super::EngineWorkloadSagaStore;
use super::initial_record_with_seed;

const CHILD_TEST: &str = "workload_saga_store::tests::ingress::workload_saga_ingress_child";
const MODE_ENV: &str = "NIMBUS_NNC61E1_INGRESS_MODE";
const SAME_CONTENTION: &str = "contend-same";
const DIVERGENT_CONTENTION: &str = "contend-divergent";
const CRASH_BEFORE: &str = "crash-before";
const CRASH_AFTER: &str = "crash-after";
const RECOVER_MISSING: &str = "recover-missing";
const RECOVER_EXACT: &str = "recover-exact";
const BEFORE_BOUNDARY: &str = "workload-saga.ingress-before-durability";
const AFTER_BOUNDARY: &str = "workload-saga.ingress-after-durability";
const MISSING_OBSERVATION: &str = "workload-saga-ingress-missing";
const CONTENTION_LABEL: &str = "ingress-contention";
const CRASH_LABEL: &str = "ingress-crash";
const CONTENDER_READY_FILE: &str = "nnc61e1-ingress-submit.contender-ready";
const CHECKPOINT_TIMEOUT: Duration = Duration::from_secs(20);
const BARRIER_TIMEOUT: Duration = Duration::from_secs(20);

#[test]
fn distinct_process_intent_contention_converges() {
    for mode in [SAME_CONTENTION, DIVERGENT_CONTENTION] {
        let root = tempfile::tempdir().expect("contention root should build");
        prepare_empty_saga_store(root.path());
        let result = TwoProcessContentionHarness::new(CHECKPOINT_TIMEOUT)
            .run(root.path(), [child("alpha", mode), child("beta", mode)])
            .unwrap_or_else(|error| panic!("{mode} ingress contention failed: {error}"));

        assert_ne!(result.winner(), result.contender());
        let expected_seed = match mode {
            SAME_CONTENTION => "same",
            DIVERGENT_CONTENTION => result.winner(),
            _ => unreachable!("the parent uses only known contention modes"),
        };
        let expected = initial_record_with_seed(CONTENTION_LABEL, expected_seed);
        let runtime = runtime().expect("parent contention runtime should build");
        let observed = runtime
            .block_on(async {
                let store = EngineWorkloadSagaStore::new(Arc::new(
                    Engine::new(root.path()).expect("contention Engine should reopen"),
                ));
                store.load(expected.key()).await
            })
            .expect("contention record should load")
            .expect("contention must retain one record");
        assert_eq!(observed, expected);
    }
}

fn prepare_empty_saga_store(root: &Path) {
    let runtime = runtime().expect("schema preparation runtime should build");
    runtime.block_on(async {
        let store = EngineWorkloadSagaStore::new(Arc::new(
            Engine::new(root).expect("schema preparation Engine should open"),
        ));
        store
            .prepare()
            .await
            .expect("empty workload-saga schema should prepare");
    });
}

#[test]
fn crash_before_and_after_durability_reopens_exact_decision() {
    let before_root = tempfile::tempdir().expect("pre-durability root should build");
    let before = SubprocessCrashCutHarness::new(CHECKPOINT_TIMEOUT)
        .run(
            before_root.path(),
            BEFORE_BOUNDARY,
            MISSING_OBSERVATION,
            child("before-writer", CRASH_BEFORE),
            child("before-recovery", RECOVER_MISSING),
        )
        .unwrap_or_else(|error| panic!("pre-durability crash cut failed: {error}"));
    assert_eq!(before.boundary(), BEFORE_BOUNDARY);
    assert_eq!(before.observation(), MISSING_OBSERVATION);
    assert_eq!(
        before.crash_diagnostic().cleanup(),
        "killed-at-boundary-and-reaped"
    );
    assert_eq!(before.recovery_diagnostic().successful(), Some(true));

    let expected = initial_record_with_seed(CRASH_LABEL, "exact");
    let expected_observation = decision_observation(&expected);
    let after_root = tempfile::tempdir().expect("post-durability root should build");
    let after = SubprocessCrashCutHarness::new(CHECKPOINT_TIMEOUT)
        .run(
            after_root.path(),
            AFTER_BOUNDARY,
            &expected_observation,
            child("after-writer", CRASH_AFTER),
            child("after-recovery", RECOVER_EXACT),
        )
        .unwrap_or_else(|error| panic!("post-durability crash cut failed: {error}"));
    assert_eq!(after.boundary(), AFTER_BOUNDARY);
    assert_eq!(after.observation(), expected_observation);
    assert_eq!(
        after.crash_diagnostic().cleanup(),
        "killed-at-boundary-and-reaped"
    );
    assert_eq!(after.recovery_diagnostic().successful(), Some(true));
}

#[test]
#[ignore = "spawned only by workload-saga ingress subprocess parents"]
fn workload_saga_ingress_child() {
    let mode = std::env::var(MODE_ENV).expect("ingress child mode should be set");
    match mode.as_str() {
        SAME_CONTENTION | DIVERGENT_CONTENTION => run_contention_child(|context| {
            let runtime = runtime()?;
            runtime.block_on(run_contention(
                context.state_root(),
                context.role(),
                mode.as_str(),
            ))
        })
        .unwrap_or_else(|error| panic!("ingress contention child failed: {error}")),
        CRASH_BEFORE => run_crash_cut_child(run_pre_durability_crash)
            .unwrap_or_else(|error| panic!("pre-durability child failed: {error}")),
        CRASH_AFTER => run_crash_cut_child(|context| {
            let runtime = runtime()?;
            let coordinator = coordinator(context.state_root())?;
            let expected = initial_record_with_seed(CRASH_LABEL, "exact");
            let confirmed = runtime
                .block_on(
                    coordinator
                        .submit_intent(expected.key().clone(), expected.active_intent().clone()),
                )
                .map_err(|error| format!("post-durability submit failed: {error}"))?;
            if confirmed.record() != &expected
                || confirmed.decision()
                    != &WorkloadSagaDecision::for_record(&expected)
                        .map_err(|error| format!("expected decision failed: {error}"))?
            {
                return Err("post-durability submit changed exact record or decision".to_owned());
            }
            context.reach_boundary(AFTER_BOUNDARY)
        })
        .unwrap_or_else(|error| panic!("post-durability child failed: {error}")),
        RECOVER_MISSING => run_crash_recovery_child(|context| {
            let runtime = runtime()?;
            let coordinator = coordinator(context.state_root())?;
            let key = initial_record_with_seed(CRASH_LABEL, "exact").key().clone();
            match runtime.block_on(coordinator.load(&key)) {
                Ok(None) => Ok(MISSING_OBSERVATION.to_owned()),
                Ok(Some(record)) => Err(format!(
                    "pre-durability recovery unexpectedly found revision {}",
                    record.revision()
                )),
                Err(error) => Err(format!("pre-durability recovery failed: {error}")),
            }
        })
        .unwrap_or_else(|error| panic!("pre-durability recovery failed: {error}")),
        RECOVER_EXACT => run_crash_recovery_child(|context| {
            let runtime = runtime()?;
            let coordinator = coordinator(context.state_root())?;
            let expected = initial_record_with_seed(CRASH_LABEL, "exact");
            let confirmed = runtime
                .block_on(
                    coordinator
                        .submit_intent(expected.key().clone(), expected.active_intent().clone()),
                )
                .map_err(|error| format!("fresh ingress replay failed: {error}"))?;
            if confirmed.record() != &expected
                || confirmed.disposition() != WorkloadSagaIngressDisposition::ConfirmedReplay
                || confirmed.decision()
                    != &WorkloadSagaDecision::for_record(&expected)
                        .map_err(|error| format!("recovered decision failed: {error}"))?
            {
                return Err("fresh ingress replay changed exact durable truth".to_owned());
            }
            Ok(decision_observation(&expected))
        })
        .unwrap_or_else(|error| panic!("post-durability recovery failed: {error}")),
        unknown => panic!("unknown ingress child mode {unknown:?}"),
    }
}

fn run_pre_durability_crash(context: &nimbus_testing::CrashCutChildContext) -> Result<(), String> {
    let runtime = runtime()?;
    let engine = Arc::new(
        Engine::new(context.state_root())
            .map_err(|error| format!("pre-durability Engine open failed: {error}"))?,
    );
    let inner = Arc::new(EngineWorkloadSagaStore::new(engine));
    let store = Arc::new(PreCommitCrashStore::new(inner));
    let coordinator = WorkloadSagaCoordinator::new(store.clone());
    let expected = initial_record_with_seed(CRASH_LABEL, "exact");
    let submission = runtime.spawn(async move {
        coordinator
            .submit_intent(expected.key().clone(), expected.active_intent().clone())
            .await
    });

    runtime.block_on(async {
        tokio::time::timeout(CHECKPOINT_TIMEOUT, store.cas_started.notified())
            .await
            .map_err(|_| "submission did not enter the pre-commit CAS cut".to_owned())
    })?;
    if submission.is_finished() {
        return Err("submission returned instead of remaining parked before commit".to_owned());
    }
    context.reach_boundary(BEFORE_BOUNDARY)
}

async fn run_contention(root: &Path, role: &str, mode: &str) -> Result<ContentionOutcome, String> {
    let seed = match mode {
        SAME_CONTENTION => "same",
        DIVERGENT_CONTENTION => role,
        _ => return Err(format!("unknown contention mode {mode:?}")),
    };
    let expected = initial_record_with_seed(CONTENTION_LABEL, seed);
    let submission_lock = SubmissionLock::acquire(root).await?;
    let coordinator = coordinator(root)?;
    let result = coordinator
        .submit_intent(expected.key().clone(), expected.active_intent().clone())
        .await;
    drop(coordinator);
    let outcome = match result {
        Ok(confirmed) if confirmed.record() != &expected => {
            Err("contention confirmed different durable content".to_owned())
        }
        Ok(confirmed)
            if !submission_lock.contended()
                && confirmed.disposition() == WorkloadSagaIngressDisposition::Applied =>
        {
            Ok(ContentionOutcome::Won)
        }
        Ok(confirmed)
            if submission_lock.contended()
                && mode == SAME_CONTENTION
                && confirmed.disposition() == WorkloadSagaIngressDisposition::ConfirmedReplay =>
        {
            Ok(ContentionOutcome::Lost)
        }
        Ok(confirmed) => Err(format!(
            "unexpected {mode} contention disposition {:?} after contended={}",
            confirmed.disposition(),
            submission_lock.contended(),
        )),
        Err(WorkloadSagaStoreError::InvalidTransition(
            WorkloadSagaError::EqualGenerationConflict(_),
        )) if submission_lock.contended() && mode == DIVERGENT_CONTENTION => {
            Ok(ContentionOutcome::Lost)
        }
        Err(error) => Err(format!("unexpected {mode} contention error: {error}")),
    };
    drop(submission_lock);
    outcome
}

fn coordinator(root: &Path) -> Result<WorkloadSagaCoordinator, String> {
    let engine = Arc::new(
        Engine::new(root).map_err(|error| format!("ingress child Engine open failed: {error}"))?,
    );
    let store: Arc<dyn WorkloadSagaStore> = Arc::new(EngineWorkloadSagaStore::new(engine));
    Ok(WorkloadSagaCoordinator::new(store))
}

struct SubmissionLock {
    path: PathBuf,
    contended: bool,
}

impl SubmissionLock {
    async fn acquire(root: &Path) -> Result<Self, String> {
        let path = root.join("nnc61e1-ingress-submit.lock");
        let contender_ready = root.join(CONTENDER_READY_FILE);
        let deadline = Instant::now() + BARRIER_TIMEOUT;
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(_) => {
                let lock = Self {
                    path,
                    contended: false,
                };
                lock.wait_for_contender(&contender_ready, deadline).await?;
                Ok(lock)
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                let contender = ContenderAcknowledgement::announce(contender_ready)?;
                Self::acquire_after_contending(path, contender, deadline).await
            }
            Err(error) => Err(format!(
                "failed to acquire process submission lock {}: {error}",
                path.display()
            )),
        }
    }

    async fn wait_for_contender(&self, ready: &Path, deadline: Instant) -> Result<(), String> {
        while !ready.exists() {
            if Instant::now() >= deadline {
                return Err(format!(
                    "timed out waiting for process contender acknowledgement {}",
                    ready.display()
                ));
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        Ok(())
    }

    async fn acquire_after_contending(
        path: PathBuf,
        contender: ContenderAcknowledgement,
        deadline: Instant,
    ) -> Result<Self, String> {
        loop {
            match OpenOptions::new().write(true).create_new(true).open(&path) {
                Ok(_) => {
                    drop(contender);
                    return Ok(Self {
                        path,
                        contended: true,
                    });
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                    if Instant::now() >= deadline {
                        return Err(format!(
                            "timed out acquiring process submission lock {}",
                            path.display()
                        ));
                    }
                }
                Err(error) => {
                    return Err(format!(
                        "failed to acquire process submission lock {}: {error}",
                        path.display()
                    ));
                }
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }

    fn contended(&self) -> bool {
        self.contended
    }
}

struct ContenderAcknowledgement {
    path: PathBuf,
}

impl ContenderAcknowledgement {
    fn announce(path: PathBuf) -> Result<Self, String> {
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(|error| {
                format!(
                    "failed to create process contender acknowledgement {}: {error}",
                    path.display()
                )
            })?;
        Ok(Self { path })
    }
}

impl Drop for ContenderAcknowledgement {
    fn drop(&mut self) {
        if let Err(error) = std::fs::remove_file(&self.path)
            && error.kind() != std::io::ErrorKind::NotFound
        {
            eprintln!(
                "failed to remove process contender acknowledgement {}: {error}",
                self.path.display()
            );
        }
    }
}

struct PreCommitCrashStore {
    inner: Arc<EngineWorkloadSagaStore>,
    cas_started: Notify,
}

impl PreCommitCrashStore {
    fn new(inner: Arc<EngineWorkloadSagaStore>) -> Self {
        Self {
            inner,
            cas_started: Notify::new(),
        }
    }
}

impl WorkloadSagaStore for PreCommitCrashStore {
    fn load<'a>(
        &'a self,
        key: &'a WorkloadSagaKey,
    ) -> WorkloadSagaFuture<'a, Option<WorkloadSagaRecord>> {
        self.inner.load(key)
    }

    fn compare_and_swap<'a>(
        &'a self,
        _expected: WorkloadSagaExpected,
        _next: WorkloadSagaRecord,
    ) -> WorkloadSagaFuture<'a, WorkloadSagaCommit> {
        Box::pin(async move {
            self.cas_started.notify_one();
            pending().await
        })
    }

    fn list_recoverable<'a>(
        &'a self,
        request: WorkloadSagaPageRequest,
    ) -> WorkloadSagaFuture<'a, WorkloadSagaPage> {
        self.inner.list_recoverable(request)
    }

    fn list_restart_candidates<'a>(
        &'a self,
        request: nimbus_workloads::WorkloadRestartCandidatePageRequest,
    ) -> nimbus_workloads::WorkloadSagaFuture<'a, nimbus_workloads::WorkloadRestartCandidatePage>
    {
        self.inner.list_restart_candidates(request)
    }

    fn list_for_tenant<'a>(
        &'a self,
        tenant_id: &'a TenantId,
        request: WorkloadSagaTenantPageRequest,
    ) -> WorkloadSagaFuture<'a, WorkloadSagaTenantPage> {
        self.inner.list_for_tenant(tenant_id, request)
    }
}

impl Drop for SubmissionLock {
    fn drop(&mut self) {
        if let Err(error) = std::fs::remove_file(&self.path) {
            eprintln!(
                "failed to release process submission lock {}: {error}",
                self.path.display()
            );
        }
    }
}

fn decision_observation(record: &WorkloadSagaRecord) -> String {
    let decision = WorkloadSagaDecision::for_record(record)
        .expect("fixed ingress record should have a deterministic decision");
    let digest = Sha256::digest(
        format!(
            "{}|{}|{}|{}|{:?}|{:?}",
            record.key().tenant_id(),
            record.key().workload_id(),
            record.saga_id(),
            record.revision(),
            record.last_transition(),
            decision,
        )
        .as_bytes(),
    );
    format!("workload-saga-ingress-{digest:x}")
}

fn runtime() -> Result<tokio::runtime::Runtime, String> {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .map_err(|error| format!("ingress child runtime failed: {error}"))
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
