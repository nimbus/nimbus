use std::sync::Arc;

use nimbus_core::Result;

use super::{
    HostLifecycleFuture, TenantSystemEvidenceProjection, TenantWorkloadStatus,
    ensure_status_matches_projection,
};

pub trait StatusEvidenceWriter: Send + Sync + 'static {
    fn write_status<'a>(&'a self, write: StatusEvidenceWrite<'a>) -> HostLifecycleFuture<'a, ()>;
}

impl<T> StatusEvidenceWriter for Arc<T>
where
    T: StatusEvidenceWriter + ?Sized,
{
    fn write_status<'a>(&'a self, write: StatusEvidenceWrite<'a>) -> HostLifecycleFuture<'a, ()> {
        (**self).write_status(write)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct StatusEvidenceWrite<'a> {
    projection: &'a TenantSystemEvidenceProjection,
    status: &'a TenantWorkloadStatus,
}

impl<'a> StatusEvidenceWrite<'a> {
    pub fn new(
        projection: &'a TenantSystemEvidenceProjection,
        status: &'a TenantWorkloadStatus,
    ) -> Result<Self> {
        ensure_status_matches_projection(projection, status)?;
        Ok(Self { projection, status })
    }

    pub fn projection(&self) -> &'a TenantSystemEvidenceProjection {
        self.projection
    }

    pub fn status(&self) -> &'a TenantWorkloadStatus {
        self.status
    }
}

#[cfg(test)]
mod tests {
    use nimbus_core::{PrincipalContext, TenantId};
    use nimbus_runtime::{RuntimeLimits, RuntimePolicy};

    use super::*;
    use crate::{LocalEnforcementBinding, TenantWorkloadPhase};
    use nimbus_tenant::{
        RuntimeIsolationTier, TenantIsolationContext, TenantIsolationDecision, TenantIsolationMode,
        TenantIsolationPolicyInput, TenantServiceGrantPolicyDecision, TenantStoragePolicyDecision,
        WorkloadAttributes, WorkloadLocation,
    };

    fn admitted_decision_for_location(
        workload_name: &str,
        invocation_id: &str,
        generation: u64,
        workload_location: WorkloadLocation,
    ) -> TenantIsolationDecision {
        let context = TenantIsolationContext::application(
            TenantId::new("tenant-a").expect("tenant id should parse"),
            PrincipalContext {
                authenticated: true,
                claims: serde_json::Map::from_iter([(
                    "tenant_id".to_string(),
                    serde_json::Value::String("tenant-a".to_string()),
                )]),
                verified_claims: serde_json::Map::new(),
            },
            "node.reconciler",
        )
        .with_deployment_generation(generation)
        .with_workload_location(workload_location);
        let policy = RuntimePolicy::new(RuntimeLimits::application_web_standard());
        let workload = WorkloadAttributes::runtime_function(
            workload_name,
            RuntimeIsolationTier::InProcessUntrusted,
        )
        .with_invocation_id(invocation_id);
        let input = TenantIsolationPolicyInput::new(workload)
            .with_runtime_policy(
                &context,
                &policy,
                RuntimeIsolationTier::InProcessUntrusted,
                TenantIsolationMode::Production,
            )
            .with_services(TenantServiceGrantPolicyDecision::new(["db"]))
            .with_storage(TenantStoragePolicyDecision::namespace("tenant-a"));

        context
            .admit_decision(input)
            .expect("decision should admit matching tenant authority")
    }

    fn admitted_decision(
        workload_name: &str,
        invocation_id: &str,
        generation: u64,
    ) -> TenantIsolationDecision {
        admitted_decision_for_location(
            workload_name,
            invocation_id,
            generation,
            WorkloadLocation::new().with_node_id("node-a"),
        )
    }

    fn binding() -> LocalEnforcementBinding {
        LocalEnforcementBinding::from_decision(&admitted_decision("messages:send", "invoke-1", 7))
            .expect("binding should materialize")
    }

    #[test]
    fn status_evidence_write_rejects_mismatched_generation_before_persistence() {
        let binding = binding();
        let spec = binding.spec();
        let projection = binding.system_evidence_projection();
        let stale_status = crate::NodeStatusAuthorizer
            .authorize(
                spec,
                crate::TenantWorkloadStatusPatch::observed_status(spec).with_observed_generation(
                    crate::WorkloadGeneration::new(spec.generation().as_u64() - 1),
                ),
            )
            .expect_err("stale generation should fail before status write");
        assert!(
            stale_status.to_string().contains("referenced generation"),
            "stale generation error should name generation mismatch: {stale_status}"
        );

        let live_status = crate::NodeStatusAuthorizer
            .authorize(
                spec,
                crate::TenantWorkloadStatusPatch::observed_status(spec)
                    .with_phase(TenantWorkloadPhase::Running),
            )
            .expect("matching status should authorize");
        StatusEvidenceWrite::new(&projection, &live_status)
            .expect("matching projection/status should build a narrow write request");
    }
}
