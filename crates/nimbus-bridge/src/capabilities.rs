use std::sync::Arc;

use nimbus_core::{
    AtomicWriteBatch, AtomicWriteBatchOutcome, Document, DocumentLocator, Error, PrincipalContext,
    Result, TableName,
};
use nimbus_engine::{Engine, MutationExecutionUnit};
use nimbus_runtime::{HostCallCancellation, InvocationKind, NimbusRuntimeError};
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

    fn invocation_kind(&self) -> InvocationKind;

    fn engine(&self) -> &Arc<Engine>;

    fn storage_access(&self) -> &TenantStorageAccessDecision;

    fn principal(&self) -> &PrincipalContext;

    fn record_document_read(&self, locator: &DocumentLocator);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostWritePosture {
    Allowed,
    Rejected,
}

/// Bridge-owned direct-host-write policy for every runtime invocation kind.
pub fn host_write_posture(kind: &InvocationKind) -> HostWritePosture {
    match kind {
        InvocationKind::Mutation => HostWritePosture::Allowed,
        InvocationKind::Query
        | InvocationKind::PaginatedQuery
        | InvocationKind::Action
        | InvocationKind::CloudflareWorkerFetch => HostWritePosture::Rejected,
    }
}

/// Convex-parity scheduling policy shared by every runtime capability host.
pub fn ensure_scheduling_allowed<H>(host: &H) -> Result<()>
where
    H: RuntimeCapabilityHost + ?Sized,
{
    match host.invocation_kind() {
        InvocationKind::Mutation | InvocationKind::Action => Ok(()),
        InvocationKind::Query
        | InvocationKind::PaginatedQuery
        | InvocationKind::CloudflareWorkerFetch => Err(Error::PermissionDenied(
            "query invocations cannot schedule functions; scheduling requires a mutation or action"
                .to_string(),
        )),
    }
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
    ensure_direct_host_writes_allowed(host)?;
    if let Some(execution_unit) = host.mutation_execution_unit() {
        execution_unit.stage_atomic_write_batch(batch)
    } else {
        // PPSC3 parity exclusion: this fallback is reserved for an allowed
        // mutation invocation whose transaction is independent of its caller
        // (notably an action's nested `runMutation`). Queries and actions are
        // rejected above, so `None` can no longer bypass OCC via a raw host
        // write. Nested mutations remain serialized through the engine path.
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
    ensure_direct_host_writes_allowed(host)?;
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
    ensure_direct_host_writes_allowed(host)?;
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
    ensure_direct_host_writes_allowed(host)?;
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
    ensure_direct_host_writes_allowed(host)?;
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
    ensure_direct_host_writes_allowed(host)?;
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
    ensure_direct_host_writes_allowed(host)?;
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
    ensure_direct_host_writes_allowed(host)?;
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

fn ensure_direct_host_writes_allowed<H>(host: &H) -> Result<()>
where
    H: RuntimeCapabilityHost + ?Sized,
{
    if host_write_posture(&host.invocation_kind()) == HostWritePosture::Allowed {
        return Ok(());
    }
    Err(Error::PermissionDenied(
        "query and action invocations cannot perform direct host writes; use a mutation or action ctx.runMutation"
            .to_string(),
    ))
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

    fn non_mutation_host(
        engine: Arc<Engine>,
        tenant_id: &TenantId,
        invocation_kind: InvocationKind,
    ) -> RuntimeHostContext {
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
                invocation_kind,
                "bridge_capability_test",
            ),
        )
        .expect("runtime host context should build")
    }

    #[test]
    fn query_and_action_host_writes_are_rejected() {
        let harness = EngineHarness::new();
        let engine = harness.engine();
        let tenant_id = TenantId::new("tenant-a").expect("tenant id should parse");
        engine
            .create_tenant(tenant_id.clone())
            .expect("tenant should create");
        let table = TableName::new("messages").expect("table should parse");
        let document_id = DocumentId::from_key("message-1").expect("document id should parse");
        for invocation_kind in [InvocationKind::Query, InvocationKind::Action] {
            let host = non_mutation_host(engine.clone(), &tenant_id, invocation_kind.clone());
            assert!(
                RuntimeCapabilityHost::mutation_execution_unit(&host).is_none(),
                "non-mutation host should have no execution unit"
            );
            let batch = AtomicWriteBatch::new(vec![AtomicWrite::Set {
                key: WriteKey::from(DocumentLocator::new(table.clone(), document_id.clone())),
                document: serde_json::Map::from_iter([(
                    "body".to_string(),
                    json!("must not commit"),
                )]),
                mode: WriteSetMode::Overwrite,
                precondition: WritePrecondition::default(),
                transforms: Vec::new(),
            }])
            .expect("batch should build");

            let error = run_ready(execute_atomic_write_batch_async(
                &host,
                batch,
                &HostCallCancellation::default(),
            ))
            .expect_err("query and action host writes must be rejected");
            assert!(matches!(error, Error::PermissionDenied(_)), "{error}");
        }

        assert!(
            engine
                .get_document(&tenant_id, &table, document_id)
                .is_err_and(|error| matches!(error, Error::DocumentNotFound(_))),
            "rejected host writes must leave no document"
        );
    }

    #[test]
    fn host_write_posture_is_mutation_only() {
        let cases = [
            (InvocationKind::Query, HostWritePosture::Rejected),
            (InvocationKind::PaginatedQuery, HostWritePosture::Rejected),
            (InvocationKind::Mutation, HostWritePosture::Allowed),
            (InvocationKind::Action, HostWritePosture::Rejected),
            (
                InvocationKind::CloudflareWorkerFetch,
                HostWritePosture::Rejected,
            ),
        ];

        for (kind, expected) in cases {
            assert_eq!(host_write_posture(&kind), expected, "{kind:?}");
        }
    }

    #[test]
    fn scheduling_posture_allows_mutation_and_action_rejects_queries() {
        let harness = EngineHarness::new();
        let engine = harness.engine();
        let tenant_id = TenantId::new("tenant-scheduling").expect("tenant id should parse");
        engine
            .create_tenant(tenant_id.clone())
            .expect("tenant should create");
        let cases = [
            (InvocationKind::Query, false),
            (InvocationKind::PaginatedQuery, false),
            (InvocationKind::Mutation, true),
            (InvocationKind::Action, true),
            (InvocationKind::CloudflareWorkerFetch, false),
        ];

        for (kind, expected_allowed) in cases {
            let host = non_mutation_host(engine.clone(), &tenant_id, kind.clone());
            let result = ensure_scheduling_allowed(&host);
            if expected_allowed {
                assert!(result.is_ok(), "{kind:?}: {result:?}");
            } else {
                assert!(
                    matches!(result, Err(Error::PermissionDenied(_))),
                    "{kind:?}: {result:?}"
                );
            }
        }
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
        let host = non_mutation_host(engine, &tenant_id, InvocationKind::Query);
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
