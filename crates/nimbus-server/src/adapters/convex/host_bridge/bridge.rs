use super::*;
use crate::execution::host_state::RuntimeHostState;
use crate::runtime_host::capabilities::RuntimeCapabilityHost;
use crate::runtime_host::{
    RuntimeHostBootstrapRequest, build_runtime_host_bootstrap,
    commit_runtime_mutation_execution_unit,
};
use crate::service_registry::RuntimeServiceRegistry;

#[derive(Clone)]
pub(crate) struct ConvexHostBridgeScope {
    service: Arc<nimbus_engine::Service>,
    registry: Arc<ConvexRegistry>,
    tenant_id: TenantId,
    runtime_service_registry: Arc<dyn RuntimeServiceRegistry>,
}

impl ConvexHostBridgeScope {
    pub(crate) fn new(
        service: Arc<nimbus_engine::Service>,
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
pub(crate) struct ConvexHostBridgeInvocation {
    auth: Option<InvocationAuth>,
    services: nimbus_runtime::InvocationServices,
    principal: nimbus_core::PrincipalContext,
    server_request_id: Option<String>,
    invocation_kind: InvocationKind,
    trigger_write_origin: Option<nimbus_core::TriggerWriteOrigin>,
}

impl ConvexHostBridgeInvocation {
    pub(crate) fn new(
        auth: Option<InvocationAuth>,
        services: nimbus_runtime::InvocationServices,
        principal: nimbus_core::PrincipalContext,
        server_request_id: Option<String>,
        invocation_kind: InvocationKind,
    ) -> Self {
        Self {
            auth,
            services,
            principal,
            server_request_id,
            invocation_kind,
            trigger_write_origin: None,
        }
    }
}

#[derive(Clone)]
pub(crate) struct ConvexHostBridge {
    service: Arc<nimbus_engine::Service>,
    registry: Arc<ConvexRegistry>,
    tenant_id: TenantId,
    auth: Option<InvocationAuth>,
    services: nimbus_runtime::InvocationServices,
    runtime_service_registry: Arc<dyn RuntimeServiceRegistry>,
    principal: nimbus_core::PrincipalContext,
    execution_unit: Option<Arc<nimbus_engine::MutationExecutionUnit>>,
    state: Arc<RuntimeHostState>,
    query_builders: Arc<Mutex<ConvexRuntimeQueryBuilders>>,
}

impl ConvexHostBridge {
    #[cfg(test)]
    pub(crate) fn new(
        scope: ConvexHostBridgeScope,
        invocation: ConvexHostBridgeInvocation,
    ) -> Self {
        Self::build(scope, invocation).expect("default convex host bridge should build")
    }

    pub(crate) fn build(
        scope: ConvexHostBridgeScope,
        invocation: ConvexHostBridgeInvocation,
    ) -> Result<Self, Error> {
        let bootstrap = build_runtime_host_bootstrap(RuntimeHostBootstrapRequest {
            service: &scope.service,
            tenant_id: &scope.tenant_id,
            principal: invocation.principal,
            server_request_id: invocation.server_request_id,
            invocation_kind: invocation.invocation_kind,
            trigger_write_origin: invocation.trigger_write_origin,
            max_nested_runtime_invocations: scope
                .registry
                .runtime_policy()
                .limits()
                .max_nested_runtime_invocations,
            session_prefix: "convex-runtime-session",
        })?;
        Ok(Self {
            service: scope.service,
            registry: scope.registry,
            tenant_id: scope.tenant_id,
            auth: invocation.auth,
            services: invocation.services,
            runtime_service_registry: scope.runtime_service_registry,
            principal: bootstrap.principal,
            execution_unit: bootstrap.execution_unit,
            state: bootstrap.state,
            query_builders: Arc::new(Mutex::new(ConvexRuntimeQueryBuilders::default())),
        })
    }

    pub(crate) fn server_request_id(&self) -> Option<&str> {
        self.state.server_request_id()
    }

    pub(crate) fn session_id(&self) -> &str {
        self.state.session_id()
    }

    pub(crate) fn snapshot_read_set(&self) -> RuntimeReadSet {
        self.state.snapshot_read_set()
    }

    pub(crate) fn mutation_execution_unit(
        &self,
    ) -> Option<&Arc<nimbus_engine::MutationExecutionUnit>> {
        self.execution_unit.as_ref()
    }

    pub(crate) fn service(&self) -> &Arc<nimbus_engine::Service> {
        &self.service
    }

    pub(crate) fn tenant_id(&self) -> &TenantId {
        &self.tenant_id
    }

    pub(crate) fn principal(&self) -> &nimbus_core::PrincipalContext {
        &self.principal
    }

    pub(crate) fn registry(&self) -> &Arc<ConvexRegistry> {
        &self.registry
    }

    pub(crate) fn auth(&self) -> Option<&InvocationAuth> {
        self.auth.as_ref()
    }

    pub(crate) fn services(&self) -> &nimbus_runtime::InvocationServices {
        &self.services
    }

    pub(crate) fn runtime_service_registry(&self) -> &Arc<dyn RuntimeServiceRegistry> {
        &self.runtime_service_registry
    }

    pub(crate) fn host_state(&self) -> &Arc<RuntimeHostState> {
        &self.state
    }

    pub(in crate::adapters::convex) fn query_builders(
        &self,
    ) -> &Arc<Mutex<ConvexRuntimeQueryBuilders>> {
        &self.query_builders
    }

    pub(crate) fn commit_mutation_execution_unit(&self) -> Result<(), Error> {
        commit_runtime_mutation_execution_unit(self.execution_unit.as_ref())
    }

    pub(crate) fn validate_session(
        &self,
        session_id: Option<&str>,
    ) -> std::result::Result<(), NimbusRuntimeError> {
        self.state.validate_session(&self.tenant_id, session_id)
    }

    pub(crate) fn consume_nested_runtime_invocation_budget(&self) -> Result<(), Error> {
        self.state
            .consume_nested_runtime_invocation_budget()
            .map_err(runtime_error_to_core)
    }
}

impl RuntimeCapabilityHost for ConvexHostBridge {
    fn validate_session(
        &self,
        session_id: Option<&str>,
    ) -> std::result::Result<(), NimbusRuntimeError> {
        ConvexHostBridge::validate_session(self, session_id)
    }

    fn mutation_execution_unit(&self) -> Option<&Arc<nimbus_engine::MutationExecutionUnit>> {
        ConvexHostBridge::mutation_execution_unit(self)
    }

    fn service(&self) -> &Arc<nimbus_engine::Service> {
        ConvexHostBridge::service(self)
    }

    fn tenant_id(&self) -> &TenantId {
        ConvexHostBridge::tenant_id(self)
    }

    fn principal(&self) -> &nimbus_core::PrincipalContext {
        ConvexHostBridge::principal(self)
    }

    fn record_document_read(&self, locator: &nimbus_core::DocumentLocator) {
        ConvexHostBridge::record_document_read(self, &locator.table, &locator.id);
    }
}
