use super::*;
use crate::runtime::host_state::RuntimeHostState;

#[derive(Clone)]
pub(in crate::adapters::convex) struct ConvexHostBridge {
    pub(in crate::adapters::convex) service: Arc<neovex_engine::Service>,
    pub(in crate::adapters::convex) registry: Arc<ConvexRegistry>,
    pub(in crate::adapters::convex) tenant_id: TenantId,
    pub(in crate::adapters::convex) state: Arc<RuntimeHostState>,
    pub(in crate::adapters::convex) query_builders: Arc<Mutex<ConvexRuntimeQueryBuilders>>,
}

impl ConvexHostBridge {
    pub(in crate::adapters::convex) fn new(
        service: Arc<neovex_engine::Service>,
        registry: Arc<ConvexRegistry>,
        tenant_id: TenantId,
        server_request_id: Option<String>,
    ) -> Self {
        let max_nested_runtime_invocations = registry
            .runtime_policy()
            .limits()
            .max_nested_runtime_invocations;
        Self {
            service,
            registry,
            tenant_id,
            state: Arc::new(RuntimeHostState::new(
                "convex-runtime-session",
                server_request_id,
                max_nested_runtime_invocations,
            )),
            query_builders: Arc::new(Mutex::new(ConvexRuntimeQueryBuilders::default())),
        }
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
