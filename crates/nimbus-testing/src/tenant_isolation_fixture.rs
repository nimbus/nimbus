//! Shared control-plane scenario builders for node/system/workloads tests.
//!
//! `nimbus-node`, `nimbus-system`, and `nimbus-workloads` each re-rolled the
//! same "admit an application-authority `TenantIsolationDecision` and
//! materialize a `LocalEnforcementBinding`" scenario inline, differing only in
//! surface string, generation, workload name/invocation id, and node/tenant
//! ids. `AdmittedDecisionScenario` centralizes the shape so call sites state
//! only what varies.

use nimbus_core::{PrincipalContext, TenantId};
use nimbus_runtime::{RuntimeLimits, RuntimePolicy};
use nimbus_tenant::{
    RuntimeIsolationTier, TenantIsolationContext, TenantIsolationDecision, TenantIsolationMode,
    TenantIsolationPolicyInput, TenantServiceGrantPolicyDecision, TenantStoragePolicyDecision,
    WorkloadAttributes, WorkloadLocation,
};
use nimbus_workloads::LocalEnforcementBinding;

/// Builds a `PrincipalContext` carrying a single `tenant_id` claim: the shape
/// node/system/workloads control-plane tests use to stand in for an
/// authenticated application principal.
pub fn principal_with_tenant_claim(tenant_id: &str) -> PrincipalContext {
    PrincipalContext {
        authenticated: true,
        claims: serde_json::Map::from_iter([(
            "tenant_id".to_string(),
            serde_json::Value::String(tenant_id.to_string()),
        )]),
        verified_claims: serde_json::Map::new(),
    }
}

/// A minimal, admitted application-authority `TenantIsolationDecision`
/// scenario: an `InProcessUntrusted` runtime-function workload with a `db`
/// service grant and a tenant-namespaced storage grant.
pub struct AdmittedDecisionScenario {
    tenant_id: String,
    surface: &'static str,
    node_id: String,
    workload_name: String,
    invocation_id: String,
    generation: u64,
    services: Vec<String>,
    storage_namespace: String,
}

impl Default for AdmittedDecisionScenario {
    fn default() -> Self {
        Self {
            tenant_id: "tenant-a".to_string(),
            surface: "convex.runtime",
            node_id: "node-a".to_string(),
            workload_name: "messages:send".to_string(),
            invocation_id: "invoke-1".to_string(),
            generation: 7,
            services: vec!["db".to_string()],
            storage_namespace: "tenant-a".to_string(),
        }
    }
}

impl AdmittedDecisionScenario {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_tenant_id(mut self, tenant_id: impl Into<String>) -> Self {
        self.tenant_id = tenant_id.into();
        self
    }

    /// `surface` is `&'static str` because `TenantIsolationContext::application`
    /// requires a `'static` surface identifier.
    pub fn with_surface(mut self, surface: &'static str) -> Self {
        self.surface = surface;
        self
    }

    pub fn with_node_id(mut self, node_id: impl Into<String>) -> Self {
        self.node_id = node_id.into();
        self
    }

    pub fn with_workload_name(mut self, workload_name: impl Into<String>) -> Self {
        self.workload_name = workload_name.into();
        self
    }

    pub fn with_invocation_id(mut self, invocation_id: impl Into<String>) -> Self {
        self.invocation_id = invocation_id.into();
        self
    }

    pub fn with_generation(mut self, generation: u64) -> Self {
        self.generation = generation;
        self
    }

    pub fn with_services<I, S>(mut self, services: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.services = services.into_iter().map(Into::into).collect();
        self
    }

    pub fn with_storage_namespace(mut self, storage_namespace: impl Into<String>) -> Self {
        self.storage_namespace = storage_namespace.into();
        self
    }

    /// Builds the application-authority `TenantIsolationContext` the scenario
    /// admits against. Exposed separately from `admit` for tests that need to
    /// reuse the same context after the decision is admitted (for example, to
    /// contrast it against an operator-authority context).
    pub fn context(&self) -> TenantIsolationContext {
        let tenant_id = TenantId::new(&self.tenant_id).expect("scenario tenant id should parse");
        TenantIsolationContext::application(
            tenant_id,
            principal_with_tenant_claim(&self.tenant_id),
            self.surface,
        )
        .with_deployment_generation(self.generation)
        .with_workload_location(WorkloadLocation::new().with_node_id(self.node_id.clone()))
    }

    /// Admits the scenario, panicking with an explanatory message if the
    /// scenario's own inputs fail to admit (a scenario bug, not the behavior
    /// under test).
    pub fn admit(&self) -> TenantIsolationDecision {
        let context = self.context();
        let policy = RuntimePolicy::new(RuntimeLimits::application_web_standard());
        let workload = WorkloadAttributes::runtime_function(
            self.workload_name.clone(),
            RuntimeIsolationTier::InProcessUntrusted,
        )
        .with_invocation_id(self.invocation_id.clone());
        let input = TenantIsolationPolicyInput::new(workload)
            .with_runtime_policy(
                &context,
                &policy,
                RuntimeIsolationTier::InProcessUntrusted,
                TenantIsolationMode::Production,
            )
            .with_services(TenantServiceGrantPolicyDecision::new(self.services.clone()))
            .with_storage(TenantStoragePolicyDecision::namespace(
                self.storage_namespace.clone(),
            ));

        context
            .admit_decision(input)
            .expect("scenario decision should admit matching tenant authority")
    }

    /// Admits the scenario and materializes the resulting
    /// `LocalEnforcementBinding`.
    pub fn binding(&self) -> LocalEnforcementBinding {
        LocalEnforcementBinding::from_decision(&self.admit())
            .expect("binding should materialize from admitted decision")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_scenario_admits_and_materializes_expected_binding() {
        let scenario = AdmittedDecisionScenario::new();
        let decision = scenario.admit();

        assert_eq!(decision.tenant_id().as_str(), "tenant-a");
        assert_eq!(decision.surface(), "convex.runtime");

        let binding = scenario.binding();
        let spec = binding.spec();
        assert_eq!(spec.generation().as_u64(), 7);
        assert_eq!(
            spec.assigned_node_id()
                .expect("node assignment should be present")
                .as_str(),
            "node-a"
        );
        assert_eq!(binding.storage_access().namespace_name(), "tenant-a");
        assert_eq!(
            binding
                .service_access("db")
                .expect("db service should be admitted")
                .service_name(),
            "db"
        );
    }

    #[test]
    fn overrides_flow_through_to_the_admitted_decision_and_binding() {
        let scenario = AdmittedDecisionScenario::new()
            .with_tenant_id("tenant-b")
            .with_surface("node.reconciler")
            .with_node_id("node-z")
            .with_workload_name("service:run")
            .with_invocation_id("invoke-override")
            .with_generation(42)
            .with_services(["db", "cache"])
            .with_storage_namespace("tenant-b-storage");
        let decision = scenario.admit();

        assert_eq!(decision.tenant_id().as_str(), "tenant-b");
        assert_eq!(decision.surface(), "node.reconciler");

        let binding = scenario.binding();
        let spec = binding.spec();
        assert_eq!(spec.generation().as_u64(), 42);
        assert_eq!(
            spec.assigned_node_id()
                .expect("node assignment should be present")
                .as_str(),
            "node-z"
        );
        assert_eq!(
            binding.storage_access().namespace_name(),
            "tenant-b-storage"
        );
        assert_eq!(
            binding
                .service_access("cache")
                .expect("cache service should be admitted")
                .service_name(),
            "cache"
        );
    }
}
