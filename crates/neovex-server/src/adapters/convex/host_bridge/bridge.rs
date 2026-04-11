use super::*;
use crate::execution::host_state::RuntimeHostState;

#[derive(Clone)]
pub(in crate::adapters::convex) struct ConvexHostBridge {
    pub(in crate::adapters::convex) service: Arc<neovex_engine::Service>,
    pub(in crate::adapters::convex) registry: Arc<ConvexRegistry>,
    pub(in crate::adapters::convex) tenant_id: TenantId,
    pub(in crate::adapters::convex) auth: Option<InvocationAuth>,
    pub(in crate::adapters::convex) principal: neovex_core::PrincipalContext,
    pub(in crate::adapters::convex) execution_unit:
        Option<Arc<neovex_engine::MutationExecutionUnit>>,
    pub(in crate::adapters::convex) state: Arc<RuntimeHostState>,
    pub(in crate::adapters::convex) query_builders: Arc<Mutex<ConvexRuntimeQueryBuilders>>,
}

impl ConvexHostBridge {
    #[cfg(test)]
    pub(in crate::adapters::convex) fn new(
        service: Arc<neovex_engine::Service>,
        registry: Arc<ConvexRegistry>,
        tenant_id: TenantId,
        auth: Option<InvocationAuth>,
        principal: neovex_core::PrincipalContext,
        server_request_id: Option<String>,
    ) -> Self {
        Self::new_with_invocation_kind(
            service,
            registry,
            tenant_id,
            auth,
            principal,
            server_request_id,
            InvocationKind::Query,
        )
        .expect("default convex host bridge should build")
    }

    pub(in crate::adapters::convex) fn new_with_invocation_kind(
        service: Arc<neovex_engine::Service>,
        registry: Arc<ConvexRegistry>,
        tenant_id: TenantId,
        auth: Option<InvocationAuth>,
        principal: neovex_core::PrincipalContext,
        server_request_id: Option<String>,
        invocation_kind: InvocationKind,
    ) -> Result<Self, Error> {
        let max_nested_runtime_invocations = registry
            .runtime_policy()
            .limits()
            .max_nested_runtime_invocations;
        let execution_unit = matches!(invocation_kind, InvocationKind::Mutation)
            .then(|| service.begin_mutation_execution_unit(tenant_id.clone(), principal.clone()))
            .transpose()?;
        Ok(Self {
            service,
            registry,
            tenant_id,
            auth,
            principal,
            execution_unit,
            state: Arc::new(RuntimeHostState::new(
                "convex-runtime-session",
                server_request_id,
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
