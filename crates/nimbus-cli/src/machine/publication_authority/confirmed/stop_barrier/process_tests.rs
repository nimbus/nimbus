use std::ffi::CString;
use std::fs::{self, OpenOptions};
use std::io::{Read as _, Write as _};
use std::os::unix::ffi::OsStrExt as _;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use nimbus::Engine;
use nimbus_compute::machine_stop_authority::{
    MachineStopAuthorizationError, MachineWorkloadAuthorityStore as _,
    authorize_physical_machine_stop,
};
use nimbus_compute::workload_saga::{
    WorkloadDesireAdmissionError, WorkloadDesireAdmissionFuture, WorkloadDesireAdmissionGuard,
    WorkloadDesireAdmissionRequest, WorkloadSagaCoordinator,
};
use nimbus_core::{TenantId, WorkloadId};
use nimbus_machine::MachineForwarderAuthority;
use nimbus_network::{
    NetworkAttachmentCapabilitySet, NetworkCapabilityRequirements, NetworkControlPlaneLocality,
    NetworkEndpointCapabilitySet, NetworkForwardingCapabilitySet, NetworkIngressCapabilitySet,
    NetworkLifecycleCapabilitySet, NetworkManagementMode, NetworkProviderHandle, NetworkProviderId,
    NetworkResourceGeneration, NetworkSovereigntyRequirements,
};
use nimbus_process_harness::{
    ContentionOutcome, ProcessRoleSpec, SubprocessCrashCutHarness, TwoProcessContentionHarness,
    run_contention_child, run_crash_cut_child, run_crash_recovery_child,
};
use nimbus_server::EngineWorkloadSagaStore;
use nimbus_workloads::{
    CompiledWorkloadNetworkPlan, DesiredWorkloadKind, DesiredWorkloadState, NodeIdentity,
    WorkloadActivationIntent, WorkloadAdmissionEvidence, WorkloadDesiredDigest,
    WorkloadExecutableEncoding, WorkloadExecutableIntent, WorkloadExecutionProviderId,
    WorkloadGeneration, WorkloadNetworkIntent, WorkloadNetworkPlanContent,
    WorkloadNetworkPlanIdentity, WorkloadProvisionSourceDigest, WorkloadProvisionSourceEvidence,
    WorkloadProvisionSourceGeneration, WorkloadProvisionSourceIdentity,
    WorkloadProvisionSourceResourceVersion, WorkloadPublicationIntent,
    WorkloadRestartCandidatePage, WorkloadRestartCandidatePageRequest, WorkloadSagaCommit,
    WorkloadSagaExpected, WorkloadSagaFuture, WorkloadSagaIntent, WorkloadSagaKey,
    WorkloadSagaPage, WorkloadSagaPageRequest, WorkloadSagaRecord, WorkloadSagaStore,
    WorkloadSagaStoreError, WorkloadSagaTenantPage, WorkloadSagaTenantPageRequest,
};

use super::super::{
    ADMISSION_PERMIT_HELD_FIFO, LOCK_CONTENTION_ARMED, LOCK_CONTENTION_FIFO,
    STOP_BARRIER_STAGED_FIFO, STORE_DIRECTORY,
};
use super::*;

const CHILD_TEST: &str = "machine::publication_authority::confirmed::stop_barrier::process_tests::machine_stop_barrier_process_child";
const MODE_ENV: &str = "NIMBUS_NNC65F3_PROCESS_MODE";
const ORDER_ENV: &str = "NIMBUS_NNC65F3_PROCESS_ORDER";
const BOUNDARY_ENV: &str = "NIMBUS_NNC65F3_PROCESS_BOUNDARY";
const ENGINE_ROOT: &str = "engine-workload-authority";

fn authority() -> MachineForwarderAuthority {
    MachineForwarderAuthority::new(
        NetworkProviderHandle::new(
            NetworkProviderId::for_registration_key("machine-provider"),
            "process-machine-provider",
        )
        .expect("process provider identity should validate"),
        NetworkResourceGeneration::new(7),
    )
}

fn execution_provider() -> WorkloadExecutionProviderId {
    WorkloadExecutionProviderId::for_registration_key("forwarded-machine")
}

pub(super) fn desire_request() -> WorkloadDesireAdmissionRequest {
    WorkloadDesireAdmissionRequest::new(
        WorkloadSagaKey::new(
            TenantId::new("tenant-stop-process").expect("tenant should validate"),
            WorkloadId::new("workload-stop-process").expect("workload should validate"),
        ),
        execution_provider(),
        WorkloadGeneration::new(1),
        WorkloadDesiredDigest::sha256(b"process-desire"),
        WorkloadProvisionSourceDigest::sha256(b"process-source"),
    )
}

pub(super) fn running_intent() -> WorkloadSagaIntent {
    let tenant_id = TenantId::new("tenant-stop-process").expect("tenant should validate");
    let activation = WorkloadActivationIntent::ActivateWhenAttached;
    let identity = WorkloadNetworkPlanIdentity::new(
        tenant_id,
        "machine-stop-process-plan",
        NetworkResourceGeneration::new(1),
    )
    .expect("process network identity should validate");
    let requirements = NetworkCapabilityRequirements::new(
        NetworkAttachmentCapabilitySet::new(NetworkManagementMode::NimbusHostManaged, [], []),
        NetworkEndpointCapabilitySet::new([], [], [], [], []),
        NetworkIngressCapabilitySet::new([]),
        NetworkForwardingCapabilitySet::new([]),
        nimbus_network::NetworkLifecycleRequirements::new(
            NetworkLifecycleCapabilitySet::new([]),
            NetworkLifecycleCapabilitySet::new([]),
        ),
        NetworkSovereigntyRequirements::new(NetworkControlPlaneLocality::LocalOnly, [], true),
    );
    let content = WorkloadNetworkPlanContent::new(
        identity,
        requirements,
        None,
        None,
        None,
        [],
        [],
        [],
        activation,
        WorkloadPublicationIntent::Withheld,
    )
    .expect("process network content should validate");
    let executable = WorkloadExecutableIntent::new(
        WorkloadExecutableEncoding::SandboxSpecCanonicalJsonV1,
        r#"{"process":"machine-stop-desire"}"#,
    )
    .expect("process executable should validate");
    let source = WorkloadProvisionSourceEvidence::standalone_sandbox(
        WorkloadProvisionSourceIdentity::standalone_sandbox(
            "machine-stop-process-source",
            "machine-stop-process-sandbox",
        )
        .expect("process source identity should validate"),
        WorkloadProvisionSourceGeneration::new(1),
        WorkloadProvisionSourceResourceVersion::new("machine-stop-process-v1")
            .expect("process source version should validate"),
        executable.content_digest(),
        NetworkProviderId::for_registration_key("machine-stop-process-attachment"),
        execution_provider(),
    )
    .expect("process source evidence should validate");
    WorkloadSagaIntent::new_without_automatic_restart(
        DesiredWorkloadKind::Sandbox,
        DesiredWorkloadState::Running,
        WorkloadGeneration::new(1),
        executable,
        source,
        WorkloadNetworkIntent::new(
            CompiledWorkloadNetworkPlan::from_content(content)
                .expect("process compiled plan should validate"),
        ),
        activation,
        WorkloadPublicationIntent::Withheld,
        WorkloadAdmissionEvidence::new(
            format!("tid_{}", "11".repeat(32))
                .try_into()
                .expect("process decision id should validate"),
            format!("twu_{}", "22".repeat(32))
                .try_into()
                .expect("process workload uid should validate"),
            NodeIdentity::new("machine-stop-process-node")
                .expect("process node identity should validate"),
        ),
    )
    .expect("process running intent should validate")
}

fn engine_store(root: &Path) -> Result<Arc<EngineWorkloadSagaStore>, String> {
    let engine = Arc::new(
        Engine::new(root.join(ENGINE_ROOT))
            .map_err(|error| format!("failed to open process Engine: {error}"))?,
    );
    Ok(Arc::new(EngineWorkloadSagaStore::new(engine)))
}

/// Test-only adapter that closes its Engine handle after the admission CAS and
/// before the coordinator releases the provider permit. The second process can
/// then perform the canonical Engine scan without an unrelated embedded-store
/// handle conflict obscuring the provider-lock ordering under test.
struct CloseEngineAfterCasStore {
    inner: Mutex<Option<Arc<EngineWorkloadSagaStore>>>,
}

impl CloseEngineAfterCasStore {
    fn new(inner: Arc<EngineWorkloadSagaStore>) -> Self {
        Self {
            inner: Mutex::new(Some(inner)),
        }
    }

    fn current(&self) -> Result<Arc<EngineWorkloadSagaStore>, WorkloadSagaStoreError> {
        self.inner
            .lock()
            .map_err(|_| WorkloadSagaStoreError::Corrupt)?
            .clone()
            .ok_or(WorkloadSagaStoreError::Corrupt)
    }
}

impl WorkloadSagaStore for CloseEngineAfterCasStore {
    fn load<'a>(
        &'a self,
        key: &'a WorkloadSagaKey,
    ) -> WorkloadSagaFuture<'a, Option<WorkloadSagaRecord>> {
        Box::pin(async move { self.current()?.load(key).await })
    }

    fn compare_and_swap<'a>(
        &'a self,
        expected: WorkloadSagaExpected,
        next: WorkloadSagaRecord,
    ) -> WorkloadSagaFuture<'a, WorkloadSagaCommit> {
        Box::pin(async move {
            let inner = self.current()?;
            let result = inner.compare_and_swap(expected, next).await;
            drop(inner);
            self.inner
                .lock()
                .map_err(|_| WorkloadSagaStoreError::Corrupt)?
                .take();
            result
        })
    }

    fn list_recoverable<'a>(
        &'a self,
        request: WorkloadSagaPageRequest,
    ) -> WorkloadSagaFuture<'a, WorkloadSagaPage> {
        Box::pin(async move { self.current()?.list_recoverable(request).await })
    }

    fn list_restart_candidates<'a>(
        &'a self,
        request: WorkloadRestartCandidatePageRequest,
    ) -> WorkloadSagaFuture<'a, WorkloadRestartCandidatePage> {
        Box::pin(async move { self.current()?.list_restart_candidates(request).await })
    }

    fn list_for_tenant<'a>(
        &'a self,
        tenant_id: &'a TenantId,
        request: WorkloadSagaTenantPageRequest,
    ) -> WorkloadSagaFuture<'a, WorkloadSagaTenantPage> {
        Box::pin(async move { self.current()?.list_for_tenant(tenant_id, request).await })
    }
}

fn engine_has_desire(root: &Path) -> Result<bool, String> {
    let store = engine_store(root)?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| format!("failed to build Engine inspection runtime: {error}"))?;
    runtime
        .block_on(store.list_machine_workload_authority_from_engine(&execution_provider()))
        .map(|records| !records.is_empty())
        .map_err(|error| format!("failed to inspect Engine desire authority: {error}"))
}

fn confirmed_journal_root(root: &Path) -> PathBuf {
    root.join("networks").join(STORE_DIRECTORY)
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

pub(super) fn create_fifo(root: &Path, name: &str) {
    let path = root.join(name);
    let path = CString::new(path.as_os_str().as_bytes()).expect("FIFO path must not contain NUL");
    // SAFETY: `path` is a live NUL-terminated path and the mode is valid.
    let result = unsafe { libc::mkfifo(path.as_ptr(), 0o600) };
    assert_eq!(result, 0, "contention FIFO should be created");
}

fn signal_fifo(root: &Path, name: &str) -> Result<(), String> {
    let mut fifo = OpenOptions::new()
        .write(true)
        .open(root.join(name))
        .map_err(|error| format!("failed to open {name} FIFO for write: {error}"))?;
    fifo.write_all(b"1")
        .map_err(|error| format!("failed to signal {name} FIFO: {error}"))?;
    fifo.flush()
        .map_err(|error| format!("failed to flush {name} FIFO: {error}"))
}

pub(super) fn await_fifo(root: &Path, name: &str) -> Result<(), String> {
    let mut fifo = OpenOptions::new()
        .read(true)
        .open(root.join(name))
        .map_err(|error| format!("failed to open {name} FIFO for read: {error}"))?;
    let mut byte = [0_u8; 1];
    fifo.read_exact(&mut byte)
        .map_err(|error| format!("failed to receive {name} signal: {error}"))?;
    if byte != [b'1'] {
        return Err(format!("{name} FIFO carried an invalid semantic token"));
    }
    Ok(())
}

struct ContentionCoordinatedAdmissionGuard {
    inner: ConfirmedMachineDesireAdmissionGuard,
    journal_root: PathBuf,
}

impl WorkloadDesireAdmissionGuard for ContentionCoordinatedAdmissionGuard {
    fn acquire<'a>(
        &'a self,
        request: &'a WorkloadDesireAdmissionRequest,
    ) -> WorkloadDesireAdmissionFuture<'a> {
        let inner = self.inner.clone();
        let request = request.clone();
        let journal_root = self.journal_root.clone();
        Box::pin(async move {
            let permit = inner.acquire(&request).await?;
            let armed = journal_root.join(LOCK_CONTENTION_ARMED);
            let synchronization = (|| {
                fs::write(&armed, b"armed").map_err(|error| {
                    format!(
                        "failed to arm admission lock contention {}: {error}",
                        armed.display()
                    )
                })?;
                signal_fifo(&journal_root, ADMISSION_PERMIT_HELD_FIFO)?;
                await_fifo(&journal_root, LOCK_CONTENTION_FIFO)?;
                fs::remove_file(&armed).map_err(|error| {
                    format!(
                        "failed to disarm admission lock contention {}: {error}",
                        armed.display()
                    )
                })
            })();
            if synchronization.is_err() {
                let _ = fs::remove_file(&armed);
                return Err(WorkloadDesireAdmissionError::Unavailable);
            }
            Ok(permit)
        })
    }
}

fn commit_desire_under_guard(
    root: &Path,
    coordinate_contention: bool,
) -> Result<ContentionOutcome, String> {
    let journal = ConfirmedMachinePublicationJournal::open(root)
        .map_err(|error| format!("failed to open admission journal: {error}"))?;
    let journal_root = journal.root.clone();
    let inner = ConfirmedMachineDesireAdmissionGuard::new(
        journal,
        "default",
        authority(),
        execution_provider(),
    )
    .map_err(|error| format!("failed to construct admission guard: {error}"))?;
    let guard: Arc<dyn WorkloadDesireAdmissionGuard> = if coordinate_contention {
        Arc::new(ContentionCoordinatedAdmissionGuard {
            inner,
            journal_root,
        })
    } else {
        Arc::new(inner)
    };
    let saga_store: Arc<dyn WorkloadSagaStore> =
        Arc::new(CloseEngineAfterCasStore::new(engine_store(root)?));
    let coordinator = WorkloadSagaCoordinator::with_desire_admission_guard(saga_store, guard);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| format!("failed to build admission runtime: {error}"))?;
    match runtime
        .block_on(coordinator.submit_intent(desire_request().key().clone(), running_intent()))
    {
        Ok(_) => Ok(ContentionOutcome::Won),
        Err(nimbus_compute::workload_saga::WorkloadSagaIngressError::Admission(
            WorkloadDesireAdmissionError::Fenced,
        )) => Ok(ContentionOutcome::Lost),
        Err(error) => Err(format!(
            "Engine desire CAS failed with unexpected outcome: {error}"
        )),
    }
}

fn attempt_desire_admission_after_stop(root: &Path) -> Result<ContentionOutcome, String> {
    let journal = ConfirmedMachinePublicationJournal::existing_for_contention_test(root);
    let guard = ConfirmedMachineDesireAdmissionGuard::new(
        journal,
        "default",
        authority(),
        execution_provider(),
    )
    .map_err(|error| format!("failed to construct contending admission guard: {error}"))?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| format!("failed to build contending admission runtime: {error}"))?;
    match runtime.block_on(guard.acquire(&desire_request())) {
        Err(WorkloadDesireAdmissionError::Fenced) => Ok(ContentionOutcome::Lost),
        Ok(_) => Err("stop-first admission crossed the durable barrier".to_owned()),
        Err(error) => Err(format!(
            "stop-first admission failed with unexpected outcome: {error}"
        )),
    }
}

fn claim_stop_and_classify(root: &Path) -> Result<ContentionOutcome, String> {
    let journal = ConfirmedMachinePublicationJournal::open(root)
        .map_err(|error| format!("failed to open stop journal: {error}"))?;
    let barriers = ConfirmedMachineStopBarrierAuthority::new(journal);
    let store = engine_store(root)?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| format!("failed to build stop authorization runtime: {error}"))?;
    match runtime.block_on(authorize_physical_machine_stop(
        &barriers,
        store.as_ref(),
        "default",
        &authority(),
        &execution_provider(),
    )) {
        Ok(_) => Ok(ContentionOutcome::Won),
        Err(MachineStopAuthorizationError::ActiveWorkloadTeardownRequired) => {
            Ok(ContentionOutcome::Lost)
        }
        Err(error) => Err(format!(
            "physical stop classification failed with unexpected outcome: {error}"
        )),
    }
}

#[test]
fn machine_stop_and_concurrent_admission_linearize_at_one_fence() {
    for (order, expected_winner) in [("admission-first", "admission"), ("stop-first", "stop")] {
        let root = tempfile::tempdir().expect("process root should exist");
        let journal = ConfirmedMachinePublicationJournal::open(root.path())
            .expect("contention journal should initialize");
        create_fifo(&journal.root, LOCK_CONTENTION_FIFO);
        create_fifo(
            &journal.root,
            if order == "admission-first" {
                ADMISSION_PERMIT_HELD_FIFO
            } else {
                STOP_BARRIER_STAGED_FIFO
            },
        );
        drop(journal);
        let result = TwoProcessContentionHarness::new(Duration::from_secs(10))
            .run(
                root.path(),
                [
                    child("admission", "contend").env(ORDER_ENV, order),
                    child("stop", "contend").env(ORDER_ENV, order),
                ],
            )
            .unwrap_or_else(|error| panic!("{order} process contention failed: {error}"));
        assert_eq!(result.winner(), expected_winner);
        assert_ne!(result.winner(), result.contender());
        assert_eq!(
            engine_has_desire(root.path()).expect("Engine desire authority should inspect"),
            order == "admission-first"
        );

        let journal = ConfirmedMachinePublicationJournal::open(root.path())
            .expect("contention journal should reopen");
        let state = journal
            .with_body(|body| {
                Ok(body
                    .stop_barriers
                    .last()
                    .expect("contention must persist one barrier")
                    .state)
            })
            .expect("durable barrier should inspect");
        assert_eq!(
            state,
            if order == "admission-first" {
                DurableMachineStopBarrierState::ClearedEffectFree
            } else {
                DurableMachineStopBarrierState::EffectFreeFenced
            }
        );
    }
}

#[test]
fn machine_stop_reopen_rediscovers_active_durable_authority() {
    for (boundary, expected) in [
        ("machine-stop.effect-free-fenced", "effect-free-fenced"),
        ("machine-stop.stop-may-exist", "stop-may-exist-fenced"),
    ] {
        let root = tempfile::tempdir().expect("crash root should exist");
        let result = SubprocessCrashCutHarness::new(Duration::from_secs(10))
            .run(
                root.path(),
                boundary,
                expected,
                child("barrier-crash", "crash").env(BOUNDARY_ENV, boundary),
                child("fresh-recovery", "recover").env(BOUNDARY_ENV, boundary),
            )
            .unwrap_or_else(|error| panic!("crash case {boundary} failed: {error}"));
        assert_eq!(result.boundary(), boundary);
        assert_eq!(result.observation(), expected);
        assert_eq!(
            result.crash_diagnostic().cleanup(),
            "killed-at-boundary-and-reaped"
        );
        assert_eq!(result.crash_diagnostic().successful(), Some(false));
        assert_eq!(result.recovery_diagnostic().successful(), Some(true));
    }
}

#[test]
#[ignore = "spawned only by NNC6.5f3 subprocess parent tests"]
fn machine_stop_barrier_process_child() {
    let mode = std::env::var(MODE_ENV).expect("process mode should be set");
    match mode.as_str() {
        "contend" => run_contention_child(|context| {
            let order = std::env::var(ORDER_ENV)
                .map_err(|error| format!("missing process order: {error}"))?;
            match (context.role(), order.as_str()) {
                ("admission", "admission-first") => {
                    let outcome = commit_desire_under_guard(context.state_root(), true)?;
                    if outcome != ContentionOutcome::Won {
                        return Err("admission-first desire did not win".to_owned());
                    }
                    Ok(outcome)
                }
                ("stop", "admission-first") => {
                    await_fifo(
                        &confirmed_journal_root(context.state_root()),
                        ADMISSION_PERMIT_HELD_FIFO,
                    )?;
                    claim_stop_and_classify(context.state_root())
                }
                ("stop", "stop-first") => claim_stop_and_classify(context.state_root()),
                ("admission", "stop-first") => {
                    await_fifo(
                        &confirmed_journal_root(context.state_root()),
                        STOP_BARRIER_STAGED_FIFO,
                    )?;
                    attempt_desire_admission_after_stop(context.state_root())
                }
                (role, order) => Err(format!("unknown contention role/order {role:?}/{order:?}")),
            }
        })
        .unwrap_or_else(|error| panic!("contention child failed: {error}")),
        "crash" => run_crash_cut_child(|context| {
            let boundary = std::env::var(BOUNDARY_ENV)
                .map_err(|error| format!("missing crash boundary: {error}"))?;
            let journal = ConfirmedMachinePublicationJournal::open(context.state_root())
                .map_err(|error| format!("failed to open crash journal: {error}"))?;
            let claimed = journal
                .claim_machine_stop_barrier("default", &authority())
                .map_err(|error| format!("failed to persist crash barrier: {error}"))?;
            if boundary == "machine-stop.stop-may-exist" {
                journal
                    .begin_physical_machine_stop(claimed.barrier())
                    .map_err(|error| format!("failed to persist stop-may-exist: {error}"))?;
            }
            context.reach_boundary(&boundary)
        })
        .unwrap_or_else(|error| panic!("crash child failed: {error}")),
        "recover" => run_crash_recovery_child(|context| {
            let boundary = std::env::var(BOUNDARY_ENV)
                .map_err(|error| format!("missing recovery boundary: {error}"))?;
            let journal = ConfirmedMachinePublicationJournal::open(context.state_root())
                .map_err(|error| format!("failed to reopen recovery journal: {error}"))?;
            let before = fs::read(&journal.state_path)
                .map_err(|error| format!("failed to read recovered journal: {error}"))?;
            let claimed = journal
                .claim_machine_stop_barrier("default", &authority())
                .map_err(|error| format!("failed to replay recovered barrier: {error}"))?;
            let expected_state = if boundary == "machine-stop.effect-free-fenced" {
                DurableMachineStopBarrierState::EffectFreeFenced
            } else {
                DurableMachineStopBarrierState::StopMayExist
            };
            if claimed.state() != expected_state {
                return Err(format!(
                    "recovered barrier state {:?} did not match {expected_state:?}",
                    claimed.state()
                ));
            }
            let guard = ConfirmedMachineDesireAdmissionGuard::new(
                journal.clone(),
                "default",
                authority(),
                execution_provider(),
            )
            .map_err(|error| format!("failed to construct recovery guard: {error}"))?;
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|error| format!("failed to build recovery runtime: {error}"))?;
            if !matches!(
                runtime.block_on(guard.acquire(&desire_request())),
                Err(WorkloadDesireAdmissionError::Fenced)
            ) {
                return Err("recovered stop barrier did not fence desire admission".to_owned());
            }
            let after = fs::read(&journal.state_path)
                .map_err(|error| format!("failed to reread recovered journal: {error}"))?;
            if before != after {
                return Err("recovery inspection changed the durable journal".to_owned());
            }
            Ok(if boundary == "machine-stop.effect-free-fenced" {
                "effect-free-fenced".to_owned()
            } else {
                "stop-may-exist-fenced".to_owned()
            })
        })
        .unwrap_or_else(|error| panic!("recovery child failed: {error}")),
        other => panic!("unknown process mode {other:?}"),
    }
}
