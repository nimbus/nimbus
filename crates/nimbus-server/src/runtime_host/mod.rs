use std::sync::Arc;

use nimbus_core::{PrincipalContext, Result, TenantId, TriggerWriteOrigin};
use nimbus_engine::{MutationExecutionUnit, Service};
use nimbus_runtime::{InvocationKind, RuntimePolicy};

pub(crate) mod abi;
pub(crate) mod capabilities;
pub(crate) mod responses;

use crate::execution::host_state::RuntimeHostState;

pub(crate) struct RuntimeHostBootstrap {
    pub(crate) principal: PrincipalContext,
    pub(crate) execution_unit: Option<Arc<MutationExecutionUnit>>,
    pub(crate) state: Arc<RuntimeHostState>,
}

pub(crate) struct RuntimeHostBootstrapRequest<'a> {
    pub(crate) service: &'a Arc<Service>,
    pub(crate) tenant_id: &'a TenantId,
    pub(crate) principal: PrincipalContext,
    pub(crate) server_request_id: Option<String>,
    pub(crate) invocation_kind: InvocationKind,
    pub(crate) trigger_write_origin: Option<TriggerWriteOrigin>,
    pub(crate) max_nested_runtime_invocations: usize,
    pub(crate) session_prefix: &'a str,
}

pub(crate) fn build_runtime_host_bootstrap(
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

pub(crate) fn commit_runtime_mutation_execution_unit(
    execution_unit: Option<&Arc<MutationExecutionUnit>>,
) -> Result<()> {
    if let Some(execution_unit) = execution_unit {
        let _ = execution_unit.commit()?;
    }
    Ok(())
}

#[derive(Clone)]
pub(crate) struct RuntimeHostScope {
    service: Arc<Service>,
    runtime_policy: Arc<RuntimePolicy>,
    tenant_id: TenantId,
}

impl RuntimeHostScope {
    pub(crate) fn new(
        service: Arc<Service>,
        runtime_policy: Arc<RuntimePolicy>,
        tenant_id: TenantId,
    ) -> Self {
        Self {
            service,
            runtime_policy,
            tenant_id,
        }
    }

    pub(crate) fn runtime_policy(&self) -> &Arc<RuntimePolicy> {
        &self.runtime_policy
    }
}

#[derive(Clone)]
pub(crate) struct RuntimeHostInvocation {
    principal: nimbus_core::PrincipalContext,
    server_request_id: Option<String>,
    invocation_kind: InvocationKind,
    trigger_write_origin: Option<TriggerWriteOrigin>,
}

impl RuntimeHostInvocation {
    pub(crate) fn new(
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

    pub(crate) fn with_trigger_write_origin(mut self, origin: TriggerWriteOrigin) -> Self {
        self.trigger_write_origin = Some(origin);
        self
    }
}

#[derive(Clone)]
pub(crate) struct RuntimeHostContext {
    service: Arc<Service>,
    tenant_id: TenantId,
    principal: nimbus_core::PrincipalContext,
    execution_unit: Option<Arc<nimbus_engine::MutationExecutionUnit>>,
    state: Arc<RuntimeHostState>,
}

impl RuntimeHostContext {
    pub(crate) fn build(
        scope: RuntimeHostScope,
        invocation: RuntimeHostInvocation,
        session_prefix: &str,
    ) -> Result<Self> {
        let bootstrap = build_runtime_host_bootstrap(RuntimeHostBootstrapRequest {
            service: &scope.service,
            tenant_id: &scope.tenant_id,
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
            tenant_id: scope.tenant_id,
            principal: bootstrap.principal,
            execution_unit: bootstrap.execution_unit,
            state: bootstrap.state,
        })
    }

    pub(crate) fn commit_mutation_execution_unit(&self) -> Result<()> {
        commit_runtime_mutation_execution_unit(self.execution_unit.as_ref())
    }

    pub(crate) fn server_request_id(&self) -> Option<&str> {
        self.state.server_request_id()
    }

    pub(crate) fn session_id(&self) -> &str {
        self.state.session_id()
    }

    pub(crate) fn tenant_id(&self) -> &TenantId {
        &self.tenant_id
    }

    pub(crate) fn validate_session(
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

    fn tenant_id(&self) -> &TenantId {
        &self.tenant_id
    }

    fn principal(&self) -> &nimbus_core::PrincipalContext {
        &self.principal
    }

    fn record_document_read(&self, locator: &nimbus_core::DocumentLocator) {
        self.state.record_document_read(&locator.table, &locator.id);
    }
}
