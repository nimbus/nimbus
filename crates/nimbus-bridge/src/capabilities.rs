use std::sync::Arc;

use nimbus_core::{
    AtomicWriteBatch, AtomicWriteBatchOutcome, Document, DocumentLocator, Error, PrincipalContext,
    Result, TableName,
};
use nimbus_engine::{Engine, MutationExecutionUnit};
use nimbus_runtime::{HostCallCancellation, NimbusRuntimeError};
use nimbus_tenant::TenantServiceAccessDecision;
use nimbus_workloads::LocalEnforcementBinding;
use serde_json::{Map, Value};

use nimbus_tenant::TenantStorageAccessDecision;

use super::cancellation::{check_host_cancellation, ensure_runtime_host_not_cancelled};

pub trait RuntimeCapabilityHost {
    fn validate_host_call_session(
        &self,
        host_call_session_id: Option<&str>,
    ) -> std::result::Result<(), NimbusRuntimeError>;

    fn mutation_execution_unit(&self) -> Option<&Arc<MutationExecutionUnit>>;

    fn engine(&self) -> &Arc<Engine>;

    fn storage_access(&self) -> &TenantStorageAccessDecision;

    fn principal(&self) -> &PrincipalContext;

    fn record_document_read(&self, locator: &DocumentLocator);
}

pub trait RuntimeServiceCapabilityHost {
    fn service_access(&self, service_name: &str) -> Result<TenantServiceAccessDecision>;
}

pub struct GrantedRuntimeServiceCapabilities<'a> {
    local_enforcement: &'a LocalEnforcementBinding,
}

impl<'a> GrantedRuntimeServiceCapabilities<'a> {
    pub fn from_local_enforcement(local_enforcement: &'a LocalEnforcementBinding) -> Option<Self> {
        if local_enforcement
            .spec()
            .service_projection()
            .services()
            .is_empty()
        {
            return None;
        }
        Some(Self { local_enforcement })
    }
}

impl RuntimeServiceCapabilityHost for GrantedRuntimeServiceCapabilities<'_> {
    fn service_access(&self, service_name: &str) -> Result<TenantServiceAccessDecision> {
        self.local_enforcement.service_access(service_name).cloned()
    }
}

pub fn validate_runtime_capability_access<H>(
    host: &H,
    host_call_session_id: Option<&str>,
    cancellation: &HostCallCancellation,
) -> std::result::Result<(), NimbusRuntimeError>
where
    H: RuntimeCapabilityHost + ?Sized,
{
    host.validate_host_call_session(host_call_session_id)?;
    ensure_runtime_host_not_cancelled(cancellation)
}

pub fn get_document<H>(host: &H, locator: &DocumentLocator) -> Result<Option<Document>>
where
    H: RuntimeCapabilityHost + ?Sized,
{
    host.mutation_execution_unit()
        .map_or_else(
            || {
                host.engine()
                    .get_document_with_principal(
                        host.storage_access().tenant_id(),
                        &locator.table,
                        locator.id.clone(),
                        host.principal(),
                    )
                    .map(Some)
                    .or_else(|error| match error {
                        Error::DocumentNotFound(_) => Ok(None),
                        other => Err(other),
                    })
            },
            |execution_unit| execution_unit.get_document(&locator.table, locator.id.clone()),
        )
        .inspect(|_| host.record_document_read(locator))
}

pub async fn get_document_async<H>(
    host: &H,
    locator: &DocumentLocator,
    cancellation: &HostCallCancellation,
) -> Result<Option<Document>>
where
    H: RuntimeCapabilityHost + ?Sized,
{
    if let Some(execution_unit) = host.mutation_execution_unit() {
        return execution_unit
            .get_document(&locator.table, locator.id.clone())
            .inspect(|_| host.record_document_read(locator));
    }

    let check_cancellation = cancellation.clone();
    host.engine()
        .get_document_async_cancellable_with_principal(
            host.storage_access().tenant_id().clone(),
            locator.table.clone(),
            locator.id.clone(),
            host.principal().clone(),
            cancellation.cancelled(),
            move || check_host_cancellation(&check_cancellation),
        )
        .await
        .map(Some)
        .or_else(|error| match error {
            Error::DocumentNotFound(_) => Ok(None),
            other => Err(other),
        })
        .inspect(|_| host.record_document_read(locator))
}

pub fn execute_atomic_write_batch<H>(
    host: &H,
    batch: AtomicWriteBatch,
) -> Result<AtomicWriteBatchOutcome>
where
    H: RuntimeCapabilityHost + ?Sized,
{
    if let Some(execution_unit) = host.mutation_execution_unit() {
        execution_unit.stage_atomic_write_batch(batch)
    } else {
        host.engine()
            .begin_mutation_execution_unit(
                host.storage_access().tenant_id().clone(),
                host.principal().clone(),
            )?
            .execute_atomic_write_batch(batch)
    }
}

pub async fn execute_atomic_write_batch_async<H>(
    host: &H,
    batch: AtomicWriteBatch,
    cancellation: &HostCallCancellation,
) -> Result<AtomicWriteBatchOutcome>
where
    H: RuntimeCapabilityHost + ?Sized,
{
    if let Some(execution_unit) = host.mutation_execution_unit() {
        return execution_unit.stage_atomic_write_batch(batch);
    }
    check_host_cancellation(cancellation)?;
    host.engine()
        .begin_mutation_execution_unit(
            host.storage_access().tenant_id().clone(),
            host.principal().clone(),
        )?
        .execute_atomic_write_batch(batch)
}

pub fn insert_document<H>(
    host: &H,
    table: TableName,
    fields: Map<String, Value>,
) -> Result<nimbus_core::DocumentId>
where
    H: RuntimeCapabilityHost + ?Sized,
{
    if let Some(execution_unit) = host.mutation_execution_unit() {
        execution_unit.insert_document(table, fields)
    } else {
        host.engine().insert_document_with(
            host.storage_access().tenant_id(),
            table,
            None,
            fields,
            nimbus_engine::MutationActor::with_principal(host.principal()),
        )
    }
}

pub async fn insert_document_async<H>(
    host: &H,
    table: TableName,
    fields: Map<String, Value>,
    cancellation: &HostCallCancellation,
) -> Result<nimbus_core::DocumentId>
where
    H: RuntimeCapabilityHost + ?Sized,
{
    if let Some(execution_unit) = host.mutation_execution_unit() {
        return execution_unit.insert_document(table, fields);
    }

    let check_cancellation = cancellation.clone();
    let cancel_wait = {
        let cancellation = cancellation.clone();
        async move {
            cancellation.cancelled().await;
        }
    };
    host.engine()
        .insert_document_async_with(
            host.storage_access().tenant_id().clone(),
            table,
            None,
            fields,
            nimbus_engine::AsyncMutationContext::with_principal(
                host.principal().clone(),
                cancel_wait,
                move || check_host_cancellation(&check_cancellation),
            ),
        )
        .await
}

pub fn update_document<H>(
    host: &H,
    table: TableName,
    document_id: nimbus_core::DocumentId,
    patch: Map<String, Value>,
) -> Result<nimbus_core::DocumentId>
where
    H: RuntimeCapabilityHost + ?Sized,
{
    if let Some(execution_unit) = host.mutation_execution_unit() {
        execution_unit.update_document(table, document_id, patch)
    } else {
        host.engine().update_document_with(
            host.storage_access().tenant_id(),
            table,
            document_id,
            patch,
            nimbus_engine::MutationActor::with_principal(host.principal()),
        )
    }
}

pub async fn update_document_async<H>(
    host: &H,
    table: TableName,
    document_id: nimbus_core::DocumentId,
    patch: Map<String, Value>,
    cancellation: &HostCallCancellation,
) -> Result<nimbus_core::DocumentId>
where
    H: RuntimeCapabilityHost + ?Sized,
{
    if let Some(execution_unit) = host.mutation_execution_unit() {
        return execution_unit.update_document(table, document_id, patch);
    }

    let check_cancellation = cancellation.clone();
    let cancel_wait = {
        let cancellation = cancellation.clone();
        async move {
            cancellation.cancelled().await;
        }
    };
    host.engine()
        .update_document_async_with(
            host.storage_access().tenant_id().clone(),
            table,
            document_id,
            patch,
            nimbus_engine::AsyncMutationContext::with_principal(
                host.principal().clone(),
                cancel_wait,
                move || check_host_cancellation(&check_cancellation),
            ),
        )
        .await
}

pub fn delete_document<H>(
    host: &H,
    table: TableName,
    document_id: nimbus_core::DocumentId,
) -> Result<()>
where
    H: RuntimeCapabilityHost + ?Sized,
{
    if let Some(execution_unit) = host.mutation_execution_unit() {
        execution_unit.delete_document(table, document_id)
    } else {
        host.engine().delete_document_with(
            host.storage_access().tenant_id(),
            table,
            document_id,
            nimbus_engine::MutationActor::with_principal(host.principal()),
        )
    }
}

pub async fn delete_document_async<H>(
    host: &H,
    table: TableName,
    document_id: nimbus_core::DocumentId,
    cancellation: &HostCallCancellation,
) -> Result<()>
where
    H: RuntimeCapabilityHost + ?Sized,
{
    if let Some(execution_unit) = host.mutation_execution_unit() {
        return execution_unit.delete_document(table, document_id);
    }

    let check_cancellation = cancellation.clone();
    let cancel_wait = {
        let cancellation = cancellation.clone();
        async move {
            cancellation.cancelled().await;
        }
    };
    host.engine()
        .delete_document_async_with(
            host.storage_access().tenant_id().clone(),
            table,
            document_id,
            nimbus_engine::AsyncMutationContext::with_principal(
                host.principal().clone(),
                cancel_wait,
                move || check_host_cancellation(&check_cancellation),
            ),
        )
        .await
}

#[cfg(test)]
mod tests {
    use std::future::Future;
    use std::path::PathBuf;
    use std::pin::Pin;
    use std::sync::Arc;
    use std::task::{Context, Poll, Waker};
    use std::time::{SystemTime, UNIX_EPOCH};

    use nimbus_core::{
        AtomicWrite, DocumentId, DocumentLocator, PrincipalContext, TableName, TenantId, WriteKey,
        WritePrecondition, WriteSetMode,
    };
    use nimbus_runtime::{InvocationKind, RuntimeLimits, RuntimePolicy};
    use nimbus_tenant::{
        RuntimeIsolationTier, TenantIsolationContext, TenantIsolationMode,
        admit_runtime_invocation_decision,
    };
    use serde_json::json;

    use super::*;
    use crate::{RuntimeHostContext, RuntimeHostInvocation, RuntimeHostScope};

    fn run_ready<F: Future>(future: F) -> F::Output {
        let waker = Waker::noop();
        let mut context = Context::from_waker(waker);
        let mut future = Box::pin(future);
        match Pin::as_mut(&mut future).poll(&mut context) {
            Poll::Ready(output) => output,
            Poll::Pending => panic!("bridge capability future should complete without parking"),
        }
    }

    struct EngineHarness {
        engine: Option<Arc<Engine>>,
        data_dir: PathBuf,
    }

    impl EngineHarness {
        fn new() -> Self {
            let unique = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system clock should be after Unix epoch")
                .as_nanos();
            let data_dir = std::env::temp_dir().join(format!(
                "nimbus-bridge-capabilities-{}-{unique}",
                std::process::id()
            ));
            std::fs::create_dir_all(&data_dir).expect("test engine directory should create");
            let engine = Arc::new(Engine::new(&data_dir).expect("engine should create"));
            Self {
                engine: Some(engine),
                data_dir,
            }
        }

        fn engine(&self) -> Arc<Engine> {
            self.engine
                .as_ref()
                .expect("test engine should be live")
                .clone()
        }
    }

    impl Drop for EngineHarness {
        fn drop(&mut self) {
            self.engine.take();
            let _ = std::fs::remove_dir_all(&self.data_dir);
        }
    }

    fn query_host(engine: Arc<Engine>, tenant_id: &TenantId) -> RuntimeHostContext {
        let policy = Arc::new(RuntimePolicy::new(RuntimeLimits::application_web_standard()));
        let isolation = TenantIsolationContext::application(
            tenant_id.clone(),
            PrincipalContext::anonymous(),
            "bridge_capability_test",
        );
        let decision = admit_runtime_invocation_decision(
            &isolation,
            "bridge_capability_test",
            None,
            policy.as_ref(),
            RuntimeIsolationTier::InProcessUntrusted,
            TenantIsolationMode::LocalDevelopment,
            std::iter::empty::<String>(),
        )
        .expect("tenant isolation decision should build");
        RuntimeHostContext::build(
            RuntimeHostScope::new(engine, policy, decision),
            RuntimeHostInvocation::new(
                PrincipalContext::anonymous(),
                None,
                InvocationKind::Query,
                "bridge_capability_test",
            ),
        )
        .expect("runtime host context should build")
    }

    #[test]
    fn async_atomic_write_batch_without_bootstrap_unit_commits_through_engine_path() {
        let harness = EngineHarness::new();
        let engine = harness.engine();
        let tenant_id = TenantId::new("tenant-a").expect("tenant id should parse");
        engine
            .create_tenant(tenant_id.clone())
            .expect("tenant should create");
        let host = query_host(engine.clone(), &tenant_id);
        assert!(
            RuntimeCapabilityHost::mutation_execution_unit(&host).is_none(),
            "query host should exercise the no-execution-unit fallback"
        );

        let table = TableName::new("messages").expect("table should parse");
        let document_id = DocumentId::from_key("message-1").expect("document id should parse");
        let batch = AtomicWriteBatch::new(vec![AtomicWrite::Set {
            key: WriteKey::from(DocumentLocator::new(table.clone(), document_id.clone())),
            document: serde_json::Map::from_iter([(
                "body".to_string(),
                json!("written by async fallback"),
            )]),
            mode: WriteSetMode::Overwrite,
            precondition: WritePrecondition::default(),
            transforms: Vec::new(),
        }])
        .expect("batch should build");

        let outcome = run_ready(execute_atomic_write_batch_async(
            &host,
            batch,
            &HostCallCancellation::default(),
        ))
        .expect("async fallback should commit through the engine path");

        assert_eq!(outcome.write_results.len(), 1);
        assert!(
            outcome.commit.is_some(),
            "fallback should execute, not merely stage the batch"
        );
        let document = engine
            .get_document(&tenant_id, &table, document_id)
            .expect("committed document should be visible");
        assert_eq!(
            document.fields.get("body"),
            Some(&json!("written by async fallback"))
        );
    }

    #[test]
    fn get_document_records_absent_document_reads() {
        let harness = EngineHarness::new();
        let engine = harness.engine();
        let tenant_id = TenantId::new("tenant-a").expect("tenant id should parse");
        engine
            .create_tenant(tenant_id.clone())
            .expect("tenant should create");
        let table = TableName::new("messages").expect("table should parse");
        engine
            .insert_document(
                &tenant_id,
                table.clone(),
                serde_json::Map::from_iter([("body".to_string(), json!("existing"))]),
            )
            .expect("fixture document should insert");
        let host = query_host(engine, &tenant_id);
        let missing_id = DocumentId::from_key("missing-message").expect("document id should parse");
        let locator = DocumentLocator::new(table.clone(), missing_id.clone());

        let document = get_document(&host, &locator).expect("missing read should succeed");

        assert!(document.is_none());
        let dependencies = host.snapshot_read_set_for_test().dependency_set();
        assert!(
            dependencies.documents.iter().any(|dependency| {
                dependency.table == table && dependency.document_id == missing_id
            }),
            "absent point reads should still be dependency-tracked: {dependencies:?}"
        );
    }
}
