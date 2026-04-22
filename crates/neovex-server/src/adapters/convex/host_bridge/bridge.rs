use super::*;
use crate::execution::host_state::RuntimeHostState;
use crate::service_registry::RuntimeServiceRegistry;

#[derive(Clone)]
pub(in crate::adapters::convex) struct ConvexHostBridgeScope {
    service: Arc<neovex_engine::Service>,
    registry: Arc<ConvexRegistry>,
    tenant_id: TenantId,
    runtime_service_registry: Arc<dyn RuntimeServiceRegistry>,
}

impl ConvexHostBridgeScope {
    pub(in crate::adapters::convex) fn new(
        service: Arc<neovex_engine::Service>,
        registry: Arc<ConvexRegistry>,
        tenant_id: TenantId,
        runtime_service_registry: Arc<dyn RuntimeServiceRegistry>,
    ) -> Self {
        Self {
            service,
            registry,
            tenant_id,
            runtime_service_registry,
        }
    }
}

#[derive(Clone)]
pub(in crate::adapters::convex) struct ConvexHostBridgeInvocation {
    auth: Option<InvocationAuth>,
    services: neovex_runtime::InvocationServices,
    principal: neovex_core::PrincipalContext,
    server_request_id: Option<String>,
    invocation_kind: InvocationKind,
}

impl ConvexHostBridgeInvocation {
    pub(in crate::adapters::convex) fn new(
        auth: Option<InvocationAuth>,
        services: neovex_runtime::InvocationServices,
        principal: neovex_core::PrincipalContext,
        server_request_id: Option<String>,
        invocation_kind: InvocationKind,
    ) -> Self {
        Self {
            auth,
            services,
            principal,
            server_request_id,
            invocation_kind,
        }
    }
}

#[derive(Clone)]
pub(in crate::adapters::convex) struct ConvexHostBridge {
    pub(in crate::adapters::convex) service: Arc<neovex_engine::Service>,
    pub(in crate::adapters::convex) registry: Arc<ConvexRegistry>,
    pub(in crate::adapters::convex) tenant_id: TenantId,
    pub(in crate::adapters::convex) auth: Option<InvocationAuth>,
    pub(in crate::adapters::convex) services: neovex_runtime::InvocationServices,
    pub(in crate::adapters::convex) runtime_service_registry: Arc<dyn RuntimeServiceRegistry>,
    pub(in crate::adapters::convex) principal: neovex_core::PrincipalContext,
    pub(in crate::adapters::convex) execution_unit:
        Option<Arc<neovex_engine::MutationExecutionUnit>>,
    pub(in crate::adapters::convex) state: Arc<RuntimeHostState>,
    pub(in crate::adapters::convex) query_builders: Arc<Mutex<ConvexRuntimeQueryBuilders>>,
}

impl ConvexHostBridge {
    #[cfg(test)]
    pub(in crate::adapters::convex) fn new(
        scope: ConvexHostBridgeScope,
        invocation: ConvexHostBridgeInvocation,
    ) -> Self {
        Self::build(scope, invocation).expect("default convex host bridge should build")
    }

    pub(in crate::adapters::convex) fn build(
        scope: ConvexHostBridgeScope,
        invocation: ConvexHostBridgeInvocation,
    ) -> Result<Self, Error> {
        let max_nested_runtime_invocations = scope
            .registry
            .runtime_policy()
            .limits()
            .max_nested_runtime_invocations;
        let execution_unit = matches!(invocation.invocation_kind, InvocationKind::Mutation)
            .then(|| {
                scope.service.begin_mutation_execution_unit(
                    scope.tenant_id.clone(),
                    invocation.principal.clone(),
                )
            })
            .transpose()?;
        Ok(Self {
            service: scope.service,
            registry: scope.registry,
            tenant_id: scope.tenant_id,
            auth: invocation.auth,
            services: invocation.services,
            runtime_service_registry: scope.runtime_service_registry,
            principal: invocation.principal,
            execution_unit,
            state: Arc::new(RuntimeHostState::new(
                "convex-runtime-session",
                invocation.server_request_id,
                max_nested_runtime_invocations,
            )),
            query_builders: Arc::new(Mutex::new(ConvexRuntimeQueryBuilders::default())),
        })
    }

    pub(in crate::adapters::convex) fn server_request_id(&self) -> Option<&str> {
        self.state.server_request_id()
    }

    pub(in crate::adapters::convex) fn session_id(&self) -> &str {
        self.state.session_id()
    }

    pub(in crate::adapters::convex) fn snapshot_read_set(&self) -> RuntimeReadSet {
        self.state.snapshot_read_set()
    }

    pub(in crate::adapters::convex) fn mutation_execution_unit(
        &self,
    ) -> Option<&Arc<neovex_engine::MutationExecutionUnit>> {
        self.execution_unit.as_ref()
    }

    pub(in crate::adapters::convex) fn commit_mutation_execution_unit(&self) -> Result<(), Error> {
        if let Some(execution_unit) = &self.execution_unit {
            let _ = execution_unit.commit()?;
        }
        Ok(())
    }

    pub(in crate::adapters::convex) fn validate_session(
        &self,
        session_id: Option<&str>,
    ) -> std::result::Result<(), NeovexRuntimeError> {
        self.state.validate_session(&self.tenant_id, session_id)
    }

    pub(in crate::adapters::convex) fn consume_nested_runtime_invocation_budget(
        &self,
    ) -> Result<(), Error> {
        self.state
            .consume_nested_runtime_invocation_budget()
            .map_err(runtime_error_to_core)
    }
}
