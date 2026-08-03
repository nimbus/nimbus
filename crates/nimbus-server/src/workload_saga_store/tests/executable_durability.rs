use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use nimbus_compute::workload_executable::{decode_sandbox_spec, encode_sandbox_spec};
use nimbus_core::{
    AtomicWrite, AtomicWriteBatch, Document, DocumentId, DocumentLocator, PrincipalContext,
    SequenceNumber, TenantId, WorkloadId, WriteKey, WritePrecondition, WriteSetMode,
};
use nimbus_engine::Engine;
use nimbus_sandbox::{
    SandboxBackendKind, SandboxLifecycleSpec, SandboxMountSpec, SandboxOwnerSpec,
    SandboxPortBinding, SandboxProcessSpec, SandboxResourceLimits, SandboxRestartPolicy,
    SandboxRootSpec, SandboxSpec,
};
use nimbus_testing::{
    ProcessRoleSpec, SubprocessCrashCutHarness, run_crash_cut_child, run_crash_recovery_child,
};
use nimbus_workloads::{
    DesiredWorkloadKind, DesiredWorkloadState, MAX_WORKLOAD_EXECUTABLE_CONTENT_BYTES, NodeIdentity,
    WorkloadActivationIntent, WorkloadAdmissionEvidence, WorkloadExecutableContentDigest,
    WorkloadExecutableEncoding, WorkloadExecutableIntent, WorkloadGeneration,
    WorkloadNetworkIntent, WorkloadPublicationIntent, WorkloadSagaCommit, WorkloadSagaExpected,
    WorkloadSagaIntent, WorkloadSagaKey, WorkloadSagaPhase, WorkloadSagaRecord, WorkloadSagaStore,
    WorkloadSagaStoreError,
};
use sha2::{Digest, Sha256};

use super::super::EngineWorkloadSagaStore;
use super::super::codec::decode_workload_saga_record;
use super::super::schema::{workload_saga_table, workload_saga_tenant};
use super::durability::assert_all_index_projections;
use super::{compiled_network_plan, document_for, provision_source};

const CHILD_TEST: &str =
    "workload_saga_store::tests::executable_durability::executable_durability_child";
const MODE_ENV: &str = "NIMBUS_NNC63A_DURABILITY_MODE";
const WRITE_MODE: &str = "write";
const RECOVER_MODE: &str = "recover";
const BOUNDARY: &str = "workload-saga.executable-durable";
const TIMEOUT: Duration = Duration::from_secs(20);
const MAX_CHILD_DIAGNOSTIC_BYTES: usize = 4 * 1024;
const PID_PREFIX: &str = "NIMBUS_NNC63A_PROCESS_ID";
const TENANT: &str = "tenant-nnc63a-executable";
const WORKLOAD: &str = "workload-nnc63a-executable";
const SECRET: &str = "NNC63A_SECRET=durable-but-redacted";
const GENERATION: u64 = 9;

#[test]
fn physical_record_retains_one_complete_executable_object() {
    let (record, _) = populated_record();
    let document = document_for(&record);
    let expected = serde_json::to_value(record.active_intent().executable()).unwrap();

    assert_eq!(document.fields.get("executable"), Some(&expected));
    for forbidden in [
        "executableDigest",
        "executableEncoding",
        "executableCacheKey",
    ] {
        assert!(!document.fields.contains_key(forbidden));
    }
}

#[tokio::test]
async fn malformed_executable_does_not_mutate_store() {
    let (record, _) = populated_record();
    let original = document_for(&record);
    let mut cases = Vec::new();

    let mut missing = original.clone();
    missing.fields.remove("executable");
    cases.push(("missing envelope", missing, true));

    let mut unknown = original.clone();
    unknown.fields["executable"]["cacheKey"] = serde_json::json!("forbidden");
    cases.push(("unknown envelope field", unknown, false));

    let mut crossed_digest = original.clone();
    crossed_digest.fields["executable"]["content"] = serde_json::json!("{not-json");
    cases.push(("crossed content digest", crossed_digest, false));

    let oversized = "x".repeat(MAX_WORKLOAD_EXECUTABLE_CONTENT_BYTES + 1);
    let oversized_digest = WorkloadExecutableContentDigest::sha256(oversized.as_bytes());
    let mut oversized_document = original.clone();
    oversized_document.fields["executable"]["content"] = serde_json::json!(oversized);
    oversized_document.fields["executable"]["contentDigest"] =
        serde_json::json!(oversized_digest.to_string());
    cases.push(("oversized content", oversized_document, false));

    let crossed = WorkloadExecutableIntent::new(
        WorkloadExecutableEncoding::SandboxSpecCanonicalJsonV1,
        r#"{"fixture":"crossed-physical-executable"}"#,
    )
    .expect("crossed executable should validate independently");
    let mut crossed_envelope = original.clone();
    crossed_envelope.fields["executable"] = serde_json::to_value(crossed).unwrap();
    cases.push(("crossed desired digest", crossed_envelope, false));

    for (label, malformed, rejected_by_schema) in cases {
        assert!(
            decode_workload_saga_record(&malformed).is_err(),
            "{label} must fail before producing a durable record"
        );

        let root = tempfile::tempdir().expect("fixture root should build");
        let engine = Arc::new(Engine::new(root.path()).expect("fixture Engine should open"));
        let store = EngineWorkloadSagaStore::new(engine.clone());
        assert_eq!(
            store
                .compare_and_swap(WorkloadSagaExpected::Missing, record.clone())
                .await
                .expect("valid record should persist"),
            WorkloadSagaCommit::Applied
        );
        let valid_document = raw_document(&engine, &record).await;
        assert_all_index_projections(&engine, &record).await;
        let journal_before_injection = durable_journal(&engine).await;

        let write_result = overwrite_physical_document(&engine, &valid_document, &malformed);
        if rejected_by_schema {
            assert!(
                write_result.is_err(),
                "{label} must be rejected by the Engine schema"
            );
            assert_eq!(
                raw_document(&engine, &record).await,
                valid_document,
                "failed Engine write for {label} must preserve durable truth"
            );
            assert_eq!(
                durable_journal(&engine).await,
                journal_before_injection,
                "failed Engine write for {label} must not append a journal commit"
            );
            assert_all_index_projections(&engine, &record).await;
            continue;
        }

        write_result.unwrap_or_else(|error| {
            panic!("{label} should cross the physical object schema for store validation: {error}")
        });
        let corrupted_document = raw_document(&engine, &record).await;
        assert_eq!(
            corrupted_document.fields, malformed.fields,
            "{label} should be present as physical corruption before the store operation"
        );
        assert_all_index_projections(&engine, &record).await;
        let journal_before_store = durable_journal(&engine).await;

        assert_eq!(
            store
                .compare_and_swap(
                    WorkloadSagaExpected::Revision(record.revision()),
                    record.clone(),
                )
                .await,
            Err(WorkloadSagaStoreError::Corrupt),
            "the Engine-backed store must reject {label} before staging a replacement"
        );
        assert_eq!(
            raw_document(&engine, &record).await,
            corrupted_document,
            "rejected store operation for {label} must preserve the exact physical document"
        );
        assert_eq!(
            durable_journal(&engine).await,
            journal_before_store,
            "rejected store operation for {label} must not append a journal commit"
        );
        assert_all_index_projections(&engine, &record).await;
    }
}

async fn raw_document(engine: &Arc<Engine>, record: &WorkloadSagaRecord) -> Document {
    engine
        .get_document_async_with_principal(
            workload_saga_tenant().expect("system tenant should validate"),
            workload_saga_table().expect("saga table should validate"),
            DocumentId::from_key(record.saga_id().as_str())
                .expect("fixture saga id should be a document id"),
            PrincipalContext::system(),
        )
        .await
        .expect("physical saga document should remain readable")
}

fn overwrite_physical_document(
    engine: &Arc<Engine>,
    current: &Document,
    replacement: &Document,
) -> nimbus_core::Result<()> {
    let unit = engine.begin_mutation_execution_unit(
        workload_saga_tenant().expect("system tenant should validate"),
        PrincipalContext::system(),
    )?;
    unit.execute_atomic_write_batch(AtomicWriteBatch::new(vec![AtomicWrite::Set {
        key: WriteKey::from(DocumentLocator::new(
            workload_saga_table().expect("saga table should validate"),
            current.id.clone(),
        )),
        document: replacement.fields.clone(),
        typed_fields: Default::default(),
        mode: WriteSetMode::Overwrite,
        precondition: WritePrecondition::update_time(current.update_time),
        transforms: Vec::new(),
    }])?)?;
    Ok(())
}

async fn durable_journal(engine: &Arc<Engine>) -> Vec<nimbus_core::TenantEventRecord> {
    engine
        .read_durable_journal_async(
            workload_saga_tenant().expect("system tenant should validate"),
            SequenceNumber(0),
        )
        .await
        .expect("durable saga journal should remain readable")
}

#[test]
fn fresh_process_recovers_exact_executable_without_snapshot_handoff() {
    let root = tempfile::tempdir().expect("fixture root should build");
    let (expected_record, expected_spec) = populated_record();
    let expected = observation_for(&expected_record, &expected_spec);
    let result = SubprocessCrashCutHarness::new(TIMEOUT)
        .run(
            root.path(),
            BOUNDARY,
            &expected,
            child("executable-writer", WRITE_MODE),
            child("executable-recovery", RECOVER_MODE),
        )
        .unwrap_or_else(|error| panic!("executable recovery failed: {error}"));

    assert_eq!(result.boundary(), BOUNDARY);
    assert_eq!(result.observation(), expected);
    assert_eq!(
        result.crash_diagnostic().cleanup(),
        "killed-at-boundary-and-reaped"
    );
    assert_eq!(result.crash_diagnostic().successful(), Some(false));
    assert_eq!(result.recovery_diagnostic().successful(), Some(true));
    assert_eq!(result.recovery_diagnostic().cleanup(), "exited-and-reaped");
    let writer_pid = process_id(result.crash_diagnostic().stderr(), "writer");
    let recovery_pid = process_id(result.recovery_diagnostic().stderr(), "recovery");
    assert_ne!(writer_pid, recovery_pid, "recovery must be a fresh process");

    for diagnostic in [result.crash_diagnostic(), result.recovery_diagnostic()] {
        assert!(diagnostic.stdout().len() <= MAX_CHILD_DIAGNOSTIC_BYTES);
        assert!(diagnostic.stderr().len() <= MAX_CHILD_DIAGNOSTIC_BYTES);
        assert!(!diagnostic.stdout().contains(SECRET));
        assert!(!diagnostic.stderr().contains(SECRET));
        assert!(!diagnostic.stdout().contains("durable-but-redacted"));
        assert!(!diagnostic.stderr().contains("durable-but-redacted"));
    }
}

#[test]
#[ignore = "spawned only by the executable durability parent"]
fn executable_durability_child() {
    let mode = std::env::var(MODE_ENV).expect("executable child mode should be set");
    match mode.as_str() {
        WRITE_MODE => run_crash_cut_child(|context| {
            eprintln!("{PID_PREFIX} writer {}", std::process::id());
            let runtime = runtime()?;
            let engine = Arc::new(
                Engine::new(context.state_root())
                    .map_err(|error| format!("writer Engine open failed: {error}"))?,
            );
            let store = EngineWorkloadSagaStore::new(engine);
            runtime.block_on(persist_executable(&store))?;
            context.reach_boundary(BOUNDARY)
        })
        .unwrap_or_else(|error| panic!("executable writer failed: {error}")),
        RECOVER_MODE => run_crash_recovery_child(|context| {
            eprintln!("{PID_PREFIX} recovery {}", std::process::id());
            let runtime = runtime()?;
            runtime.block_on(recover_executable(context.state_root()))
        })
        .unwrap_or_else(|error| panic!("executable recovery failed: {error}")),
        unknown => panic!("unknown executable child mode {unknown:?}"),
    }
}

async fn persist_executable(store: &EngineWorkloadSagaStore) -> Result<(), String> {
    let (record, _) = populated_record();
    if record.phase() != WorkloadSagaPhase::IntentCommitted
        || !record.phase_detail().references().is_empty()
    {
        return Err("writer fixture is not unreserved IntentCommitted truth".to_owned());
    }
    let commit = store
        .compare_and_swap(WorkloadSagaExpected::Missing, record)
        .await
        .map_err(|error| format!("writer executable persistence failed: {error}"))?;
    if commit != WorkloadSagaCommit::Applied {
        return Err(format!(
            "writer did not newly persist executable truth: {commit:?}"
        ));
    }
    Ok(())
}

async fn recover_executable(root: &Path) -> Result<String, String> {
    let engine = Arc::new(
        Engine::new(root).map_err(|error| format!("recovery Engine open failed: {error}"))?,
    );
    let store = EngineWorkloadSagaStore::new(engine);
    let record = store
        .load(&workload_key())
        .await
        .map_err(|error| format!("recovery load failed: {error}"))?
        .ok_or_else(|| "recovery omitted the fixed workload saga".to_owned())?;
    if record.phase() != WorkloadSagaPhase::IntentCommitted
        || !record.phase_detail().references().is_empty()
    {
        return Err("recovered saga is not unreserved IntentCommitted truth".to_owned());
    }

    let (expected_record, expected_spec) = populated_record();
    assert_exact_executable(&record, &expected_record, &expected_spec)?;
    Ok(observation_for(&record, &expected_spec))
}

fn assert_exact_executable(
    recovered: &WorkloadSagaRecord,
    expected: &WorkloadSagaRecord,
    expected_spec: &SandboxSpec,
) -> Result<(), String> {
    let recovered_carrier = recovered.active_intent().executable();
    let expected_carrier = expected.active_intent().executable();
    let decoded = decode_sandbox_spec(recovered_carrier)
        .map_err(|error| format!("recovered executable decode failed: {error}"))?;
    if recovered_carrier != expected_carrier
        || recovered_carrier.canonical_content().as_bytes()
            != expected_carrier.canonical_content().as_bytes()
        || recovered_carrier.content_digest() != expected_carrier.content_digest()
        || recovered.active_intent().desired_digest() != expected.active_intent().desired_digest()
        || decoded != *expected_spec
        || serde_json::to_vec(&decoded).map_err(|error| error.to_string())?
            != serde_json::to_vec(expected_spec).map_err(|error| error.to_string())?
    {
        return Err("recovered executable value, bytes, or digest changed".to_owned());
    }
    Ok(())
}

fn populated_record() -> (WorkloadSagaRecord, SandboxSpec) {
    let tenant_id = tenant_id();
    let spec = complete_spec();
    let executable = encode_sandbox_spec(&spec).expect("fixed sandbox spec should encode");
    let source = provision_source(
        &executable,
        WORKLOAD,
        GENERATION,
        nimbus_network::NetworkProviderId::for_registration_key("fixture-attachment"),
    );
    let intent = WorkloadSagaIntent::new(
        DesiredWorkloadKind::Sandbox,
        DesiredWorkloadState::Running,
        WorkloadGeneration::new(GENERATION),
        executable,
        source,
        WorkloadNetworkIntent::new(compiled_network_plan(
            &tenant_id,
            WORKLOAD,
            GENERATION,
            WorkloadActivationIntent::ActivateWhenAttached,
            WorkloadPublicationIntent::Withheld,
        )),
        WorkloadActivationIntent::ActivateWhenAttached,
        WorkloadPublicationIntent::Withheld,
        WorkloadAdmissionEvidence::new(
            format!("tid_{}", "3".repeat(64))
                .try_into()
                .expect("fixed decision id should validate"),
            format!("twu_{}", "4".repeat(64))
                .try_into()
                .expect("fixed workload uid should validate"),
            NodeIdentity::new("node-nnc63a").expect("fixed node should validate"),
        ),
    )
    .expect("fixed workload intent should validate");
    (
        WorkloadSagaRecord::new(workload_key(), intent).expect("fixed record should validate"),
        spec,
    )
}

fn complete_spec() -> SandboxSpec {
    SandboxSpec::new(
        tenant_id(),
        SandboxOwnerSpec::service("nnc63a-service"),
        SandboxBackendKind::Container,
        SandboxRootSpec::rootfs("/fixtures/nnc63a-rootfs"),
        SandboxProcessSpec::new(["/bin/serve", "--port", "8080"])
            .with_env([SECRET, "MODE=durable"])
            .with_cwd("/workspace")
            .with_user("1000:1000"),
    )
    .with_resource_limits(
        SandboxResourceLimits::default()
            .with_cpu_count(2)
            .with_memory_limit_bytes(256 * 1024 * 1024)
            .with_disk_limit_bytes(2 * 1024 * 1024 * 1024)
            .with_log_limit_bytes(8 * 1024 * 1024),
    )
    .with_lifecycle(
        SandboxLifecycleSpec::default()
            .with_restart_policy(SandboxRestartPolicy::OnFailure { max_restarts: 2 })
            .with_stop_timeout(Duration::from_millis(1_500)),
    )
    .with_port_binding(SandboxPortBinding::tcp("http", 32_809, 8_080))
    .with_mount(SandboxMountSpec::tenant_volume("durable-state", "/data"))
}

fn observation_for(record: &WorkloadSagaRecord, spec: &SandboxSpec) -> String {
    let carrier = record.active_intent().executable();
    let spec_digest = Sha256::digest(
        serde_json::to_vec(spec).expect("validated sandbox specification always serializes"),
    );
    format!(
        "executable-v1:content-{}:desired-{}:spec-{spec_digest:x}",
        carrier.content_digest(),
        record.active_intent().desired_digest(),
    )
}

fn tenant_id() -> TenantId {
    TenantId::new(TENANT).expect("fixed tenant should validate")
}

fn workload_key() -> WorkloadSagaKey {
    WorkloadSagaKey::new(
        tenant_id(),
        WorkloadId::new(WORKLOAD).expect("fixed workload should validate"),
    )
}

fn runtime() -> Result<tokio::runtime::Runtime, String> {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .map_err(|error| format!("executable runtime failed: {error}"))
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

fn process_id(stderr: &str, role: &str) -> u32 {
    stderr
        .lines()
        .find_map(|line| {
            line.strip_prefix(&format!("{PID_PREFIX} {role} "))
                .and_then(|value| value.parse().ok())
        })
        .unwrap_or_else(|| panic!("missing {role} child process id in stderr:\n{stderr}"))
}
