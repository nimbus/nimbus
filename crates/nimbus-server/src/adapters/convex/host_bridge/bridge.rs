use super::*;
use nimbus_bridge::capabilities::{GrantedRuntimeServiceCapabilities, RuntimeCapabilityHost};
use nimbus_bridge::state::RuntimeHostState;
use nimbus_bridge::{
    RuntimeHostBootstrapRequest, build_runtime_host_bootstrap,
    commit_runtime_mutation_execution_unit,
};
use nimbus_services::RuntimeServiceRegistry;
use nimbus_tenant::{TenantIsolationDecision, TenantStorageAccessDecision};
use nimbus_workloads::LocalEnforcementBinding;

use super::egress_gateway::EgressGatewayEnforcementReadiness;

#[derive(Clone)]
pub(crate) struct ConvexHostBridgeScope {
    engine: Arc<nimbus_engine::Engine>,
    registry: Arc<ConvexRegistry>,
    decision: TenantIsolationDecision,
    runtime_service_registry: Arc<dyn RuntimeServiceRegistry>,
    egress_readiness: Option<EgressGatewayEnforcementReadiness>,
}

impl ConvexHostBridgeScope {
    pub(crate) fn new(
        engine: Arc<nimbus_engine::Engine>,
        registry: Arc<ConvexRegistry>,
        decision: TenantIsolationDecision,
        runtime_service_registry: Arc<dyn RuntimeServiceRegistry>,
    ) -> Self {
        Self {
            engine,
            registry,
            decision,
            runtime_service_registry,
            egress_readiness: None,
        }
    }

    #[cfg(test)]
    pub(in crate::adapters::convex) fn with_egress_readiness(
        mut self,
        egress_readiness: EgressGatewayEnforcementReadiness,
    ) -> Self {
        self.egress_readiness = Some(egress_readiness);
        self
    }
}

#[derive(Clone)]
pub(crate) struct ConvexHostBridgeInvocation {
    auth: Option<InvocationAuth>,
    services: nimbus_runtime::InvocationServices,
    principal: nimbus_core::PrincipalContext,
    server_request_id: Option<String>,
    invocation_kind: InvocationKind,
    function_name: String,
    trigger_write_origin: Option<nimbus_core::TriggerWriteOrigin>,
}

impl ConvexHostBridgeInvocation {
    pub(crate) fn new(
        auth: Option<InvocationAuth>,
        services: nimbus_runtime::InvocationServices,
        principal: nimbus_core::PrincipalContext,
        server_request_id: Option<String>,
        invocation_kind: InvocationKind,
        function_name: impl Into<String>,
    ) -> Self {
        Self {
            auth,
            services,
            principal,
            server_request_id,
            invocation_kind,
            function_name: function_name.into(),
            trigger_write_origin: None,
        }
    }
}

#[derive(Clone)]
pub(crate) struct ConvexHostBridge {
    engine: Arc<nimbus_engine::Engine>,
    registry: Arc<ConvexRegistry>,
    tenant_id: TenantId,
    decision: TenantIsolationDecision,
    local_enforcement: LocalEnforcementBinding,
    storage_access: TenantStorageAccessDecision,
    auth: Option<InvocationAuth>,
    services: nimbus_runtime::InvocationServices,
    runtime_service_registry: Arc<dyn RuntimeServiceRegistry>,
    egress_readiness: EgressGatewayEnforcementReadiness,
    principal: nimbus_core::PrincipalContext,
    execution_unit: Option<Arc<nimbus_engine::MutationExecutionUnit>>,
    invocation_kind: InvocationKind,
    state: Arc<RuntimeHostState>,
    query_builders: Arc<Mutex<ConvexRuntimeQueryBuilders>>,
    function_name: String,
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
        let local_enforcement = LocalEnforcementBinding::from_decision(&scope.decision)?;
        let egress_readiness = scope.egress_readiness.clone().unwrap_or_else(|| {
            EgressGatewayEnforcementReadiness::ready_for_decision(&scope.decision)
        });
        let invocation_kind = invocation.invocation_kind.clone();
        let bootstrap = build_runtime_host_bootstrap(RuntimeHostBootstrapRequest {
            engine: &scope.engine,
            tenant_id: scope.decision.tenant_id(),
            principal: invocation.principal,
            server_request_id: invocation.server_request_id,
            invocation_kind: invocation.invocation_kind,
            function_name: &invocation.function_name,
            trigger_write_origin: invocation.trigger_write_origin,
            max_nested_runtime_invocations: scope
                .registry
                .runtime_policy()
                .limits()
                .max_nested_runtime_invocations,
        })?;
        Ok(Self {
            engine: scope.engine,
            registry: scope.registry,
            tenant_id: scope.decision.tenant_id().clone(),
            storage_access: local_enforcement.storage_access().clone(),
            decision: scope.decision,
            local_enforcement,
            auth: invocation.auth,
            services: invocation.services,
            runtime_service_registry: scope.runtime_service_registry,
            egress_readiness,
            principal: bootstrap.principal,
            execution_unit: bootstrap.execution_unit,
            invocation_kind,
            state: bootstrap.state,
            query_builders: Arc::new(Mutex::new(ConvexRuntimeQueryBuilders::default())),
            function_name: invocation.function_name,
        })
    }

    pub(crate) fn server_request_id(&self) -> Option<&str> {
        self.state.server_request_id()
    }

    pub(crate) fn host_call_session_id(&self) -> &str {
        self.state.host_call_session_id()
    }

    pub(crate) fn snapshot_read_set(&self) -> RuntimeReadSet {
        self.state.snapshot_read_set()
    }

    pub(crate) fn mutation_execution_unit(
        &self,
    ) -> Option<&Arc<nimbus_engine::MutationExecutionUnit>> {
        self.execution_unit.as_ref()
    }

    pub(crate) fn engine(&self) -> &Arc<nimbus_engine::Engine> {
        &self.engine
    }

    pub(crate) fn tenant_id(&self) -> &TenantId {
        &self.tenant_id
    }

    pub(in crate::adapters::convex) fn decision(&self) -> &TenantIsolationDecision {
        &self.decision
    }

    pub(in crate::adapters::convex) fn egress_readiness(
        &self,
    ) -> &EgressGatewayEnforcementReadiness {
        &self.egress_readiness
    }

    pub(crate) fn storage_access(&self) -> &TenantStorageAccessDecision {
        &self.storage_access
    }

    pub(crate) fn principal(&self) -> &nimbus_core::PrincipalContext {
        &self.principal
    }

    pub(crate) fn registry(&self) -> &Arc<ConvexRegistry> {
        &self.registry
    }

    /// The name of the function this bridge was built to serve (HG4). Used to
    /// resolve THIS invocation's own runtime lane, so the callee-lane oracle
    /// can answer a same-vs-cross-lane boolean without ever handing the guest
    /// the underlying lane bucket.
    pub(in crate::adapters::convex) fn current_function_name(&self) -> &str {
        &self.function_name
    }

    /// A clone of this bridge retargeted to serve as the host object for a new
    /// nested `ctx.run*` invocation of `function_name` (HG4). Nested host
    /// dispatch reuses the calling bridge's session/bootstrap state rather
    /// than building a fresh `ConvexHostBridge` per hop (see
    /// `nested_runtime/dispatch.rs`), so without this retarget
    /// `current_function_name()` would stay frozen at the top-level
    /// entrypoint's name across every hop of a nested dispatch chain, and the
    /// callee-lane oracle would compare the callee against the WRONG "current"
    /// lane past the first hop.
    pub(in crate::adapters::convex) fn retargeted_for_nested_invocation(
        &self,
        invocation_kind: InvocationKind,
        function_name: impl Into<String>,
    ) -> Self {
        let mut nested = self.clone();
        nested.invocation_kind = invocation_kind;
        nested.function_name = function_name.into();
        nested
    }

    pub(in crate::adapters::convex) fn invocation_kind(&self) -> &InvocationKind {
        &self.invocation_kind
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

    pub(in crate::adapters::convex) fn service_capabilities(
        &self,
    ) -> Option<GrantedRuntimeServiceCapabilities<'_>> {
        GrantedRuntimeServiceCapabilities::from_local_enforcement(&self.local_enforcement)
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

    pub(crate) fn validate_host_call_session(
        &self,
        host_call_session_id: Option<&str>,
    ) -> std::result::Result<(), NimbusRuntimeError> {
        self.state
            .validate_host_call_session(&self.tenant_id, host_call_session_id)
    }

    pub(crate) fn consume_nested_runtime_invocation_budget(&self) -> Result<(), Error> {
        self.state
            .consume_nested_runtime_invocation_budget()
            .map_err(runtime_error_to_core)
    }
}

impl RuntimeCapabilityHost for ConvexHostBridge {
    fn validate_host_call_session(
        &self,
        host_call_session_id: Option<&str>,
    ) -> std::result::Result<(), NimbusRuntimeError> {
        ConvexHostBridge::validate_host_call_session(self, host_call_session_id)
    }

    fn mutation_execution_unit(&self) -> Option<&Arc<nimbus_engine::MutationExecutionUnit>> {
        ConvexHostBridge::mutation_execution_unit(self)
    }

    fn direct_host_writes_allowed(&self) -> bool {
        matches!(self.invocation_kind, InvocationKind::Mutation)
    }

    fn engine(&self) -> &Arc<nimbus_engine::Engine> {
        ConvexHostBridge::engine(self)
    }

    fn storage_access(&self) -> &TenantStorageAccessDecision {
        ConvexHostBridge::storage_access(self)
    }

    fn principal(&self) -> &nimbus_core::PrincipalContext {
        ConvexHostBridge::principal(self)
    }

    fn record_document_read(&self, locator: &nimbus_core::DocumentLocator) {
        ConvexHostBridge::record_document_read(self, &locator.table, &locator.id);
    }
}
