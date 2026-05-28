use std::sync::Arc;

use nimbus_core::{PrincipalContext, Result, TenantId, TriggerWriteOrigin};
use nimbus_engine::{MutationExecutionUnit, Service};
use nimbus_runtime::{InvocationKind, RuntimePolicy};

pub mod abi;
pub mod admission;
pub mod cancellation;
pub mod capabilities;
pub mod host_calls;
pub mod read_tracking;
pub mod responses;
pub mod state;

use nimbus_node::LocalEnforcementBinding;
use nimbus_tenant::{TenantIsolationDecision, TenantStorageAccessDecision};

use self::state::RuntimeHostState;

pub struct RuntimeHostBootstrap {
    pub principal: PrincipalContext,
    pub execution_unit: Option<Arc<MutationExecutionUnit>>,
    pub state: Arc<RuntimeHostState>,
}

pub struct RuntimeHostBootstrapRequest<'a> {
    pub service: &'a Arc<Service>,
    pub tenant_id: &'a TenantId,
    pub principal: PrincipalContext,
    pub server_request_id: Option<String>,
    pub invocation_kind: InvocationKind,
    pub trigger_write_origin: Option<TriggerWriteOrigin>,
    pub max_nested_runtime_invocations: usize,
    pub session_prefix: &'a str,
}

pub fn build_runtime_host_bootstrap(
    request: RuntimeHostBootstrapRequest<'_>,
) -> Result<RuntimeHostBootstrap> {
    let RuntimeHostBootstrapRequest {
        service,
        tenant_id,
        principal,
        server_request_id,
        invocation_kind,
        trigger_write_origin,
        max_nested_runtime_invocations,
        session_prefix,
    } = request;
    let execution_unit = matches!(invocation_kind, InvocationKind::Mutation)
        .then(|| service.begin_mutation_execution_unit(tenant_id.clone(), principal.clone()))
        .transpose()?;
    if let (Some(execution_unit), Some(trigger_write_origin)) =
        (execution_unit.as_ref(), trigger_write_origin.as_ref())
    {
        execution_unit.set_trigger_write_origin(trigger_write_origin.clone())?;
    }
    Ok(RuntimeHostBootstrap {
        principal,
        execution_unit,
        state: Arc::new(RuntimeHostState::new(
            session_prefix,
            server_request_id,
            max_nested_runtime_invocations,
        )),
    })
}

pub fn commit_runtime_mutation_execution_unit(
    execution_unit: Option<&Arc<MutationExecutionUnit>>,
) -> Result<()> {
    if let Some(execution_unit) = execution_unit {
        let _ = execution_unit.commit()?;
    }
    Ok(())
}

#[derive(Clone)]
pub struct RuntimeHostScope {
    service: Arc<Service>,
    runtime_policy: Arc<RuntimePolicy>,
    decision: TenantIsolationDecision,
}

impl RuntimeHostScope {
    pub fn new(
        service: Arc<Service>,
        runtime_policy: Arc<RuntimePolicy>,
        decision: TenantIsolationDecision,
    ) -> Self {
        Self {
            service,
            runtime_policy,
            decision,
        }
    }

    pub fn runtime_policy(&self) -> &Arc<RuntimePolicy> {
        &self.runtime_policy
    }
}

#[derive(Clone)]
pub struct RuntimeHostInvocation {
    principal: nimbus_core::PrincipalContext,
    server_request_id: Option<String>,
    invocation_kind: InvocationKind,
    trigger_write_origin: Option<TriggerWriteOrigin>,
}

impl RuntimeHostInvocation {
    pub fn new(
        principal: nimbus_core::PrincipalContext,
        server_request_id: Option<String>,
        invocation_kind: InvocationKind,
    ) -> Self {
        Self {
            principal,
            server_request_id,
            invocation_kind,
            trigger_write_origin: None,
        }
    }

    pub fn with_trigger_write_origin(mut self, origin: TriggerWriteOrigin) -> Self {
        self.trigger_write_origin = Some(origin);
        self
    }
}

#[derive(Clone)]
pub struct RuntimeHostContext {
    service: Arc<Service>,
    tenant_id: TenantId,
    storage_access: TenantStorageAccessDecision,
    principal: nimbus_core::PrincipalContext,
    execution_unit: Option<Arc<nimbus_engine::MutationExecutionUnit>>,
    state: Arc<RuntimeHostState>,
}

impl RuntimeHostContext {
    pub fn build(
        scope: RuntimeHostScope,
        invocation: RuntimeHostInvocation,
        session_prefix: &str,
    ) -> Result<Self> {
        let binding = LocalEnforcementBinding::from_decision(&scope.decision)?;
        let bootstrap = build_runtime_host_bootstrap(RuntimeHostBootstrapRequest {
            service: &scope.service,
            tenant_id: scope.decision.tenant_id(),
            principal: invocation.principal,
            server_request_id: invocation.server_request_id,
            invocation_kind: invocation.invocation_kind,
            trigger_write_origin: invocation.trigger_write_origin,
            max_nested_runtime_invocations: scope
                .runtime_policy
                .limits()
                .max_nested_runtime_invocations,
            session_prefix,
        })?;
        Ok(Self {
            service: scope.service,
            tenant_id: scope.decision.tenant_id().clone(),
            storage_access: binding.storage_access().clone(),
            principal: bootstrap.principal,
            execution_unit: bootstrap.execution_unit,
            state: bootstrap.state,
        })
    }

    pub fn commit_mutation_execution_unit(&self) -> Result<()> {
        commit_runtime_mutation_execution_unit(self.execution_unit.as_ref())
    }

    pub fn server_request_id(&self) -> Option<&str> {
        self.state.server_request_id()
    }

    pub fn session_id(&self) -> &str {
        self.state.session_id()
    }

    pub fn tenant_id(&self) -> &TenantId {
        &self.tenant_id
    }

    pub fn storage_access(&self) -> &TenantStorageAccessDecision {
        &self.storage_access
    }

    pub fn validate_session(
        &self,
        session_id: Option<&str>,
    ) -> std::result::Result<(), nimbus_runtime::NimbusRuntimeError> {
        self.state.validate_session(&self.tenant_id, session_id)
    }
}

impl capabilities::RuntimeCapabilityHost for RuntimeHostContext {
    fn validate_session(
        &self,
        session_id: Option<&str>,
    ) -> std::result::Result<(), nimbus_runtime::NimbusRuntimeError> {
        RuntimeHostContext::validate_session(self, session_id)
    }

    fn mutation_execution_unit(&self) -> Option<&Arc<nimbus_engine::MutationExecutionUnit>> {
        self.execution_unit.as_ref()
    }

    fn service(&self) -> &Arc<Service> {
        &self.service
    }

    fn storage_access(&self) -> &TenantStorageAccessDecision {
        RuntimeHostContext::storage_access(self)
    }

    fn principal(&self) -> &nimbus_core::PrincipalContext {
        &self.principal
    }

    fn record_document_read(&self, locator: &nimbus_core::DocumentLocator) {
        let table_id = self
            .service
            .table_id(&self.tenant_id, &locator.table)
            .ok()
            .flatten();
        self.state
            .record_document_read(&locator.table, table_id.as_ref(), &locator.id);
    }
}
