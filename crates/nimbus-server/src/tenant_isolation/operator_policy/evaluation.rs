use nimbus_core::{Result, TenantId};
use nimbus_runtime::RuntimePolicy;
use nimbus_sandbox::SandboxBackendKind;
use serde::Serialize;

use crate::tenant_isolation::{
    RuntimeIsolationTier, TenantAuditRedactionPolicy, TenantIsolationContext,
    TenantIsolationDecision, TenantIsolationMode, TenantQuotaPolicyDecision,
    TenantRuntimePolicyAdmission, TenantSecretPolicyDecision, TenantServiceGrantPolicyDecision,
    TenantStoragePolicyDecision, TenantVolumePolicyDecision,
};

use super::external::evaluate_external_policy_backend;
use super::formatting::{admission_label, normalized_strings};
use super::{
    OperatorExternalPolicyEngine, OperatorExternalPolicyEvidence, OperatorExternalPolicyRequest,
    OperatorPolicyDocument, OperatorPolicyImageSummary, OperatorPolicyQuotaSummary,
    OperatorPolicyWorkload, OperatorRuntimeProfile,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OperatorPolicyEvaluation {
    pub policy_name: Option<String>,
    pub tenant_id: String,
    pub decision_count: usize,
    pub decisions: Vec<OperatorPolicyDecisionEvaluation>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OperatorPolicyDecisionEvaluation {
    pub workload_key: String,
    pub decision_id: String,
    pub tenant_id: String,
    pub runtime_tier: RuntimeIsolationTier,
    pub runtime_profile: OperatorRuntimeProfile,
    pub tenant_isolation_mode: TenantIsolationMode,
    pub runtime_admission: TenantRuntimePolicyAdmission,
    pub sandbox_backend: Option<SandboxBackendKind>,
    pub sandbox_id: Option<String>,
    pub services: Vec<String>,
    pub network_endpoints: Vec<String>,
    pub sandbox_egress: Vec<String>,
    pub storage_namespace: String,
    pub named_volumes: Vec<String>,
    pub image_policy: OperatorPolicyImageSummary,
    pub image_reference: Option<String>,
    #[serde(skip_serializing)]
    pub(super) secret_handles: Vec<String>,
    pub secret_handle_count: usize,
    pub quotas: OperatorPolicyQuotaSummary,
    pub audit_redactions: Vec<String>,
    pub external_policy: Option<OperatorExternalPolicyEvidence>,
    pub trace: Vec<String>,
    #[serde(skip_serializing)]
    pub decision: TenantIsolationDecision,
}

pub(super) struct OperatorPolicyTraceInput<'a> {
    pub(super) mode: TenantIsolationMode,
    pub(super) storage_namespace: &'a str,
    pub(super) services: &'a [String],
    pub(super) network_endpoint_summaries: &'a [String],
    pub(super) sandbox_egress_summaries: &'a [String],
    pub(super) named_volumes: &'a [String],
    pub(super) secret_handle_count: usize,
}

impl OperatorPolicyDocument {
    pub fn evaluate(&self) -> Result<OperatorPolicyEvaluation> {
        self.evaluate_with_external_policy(None)
    }

    pub fn evaluate_with_external_policy(
        &self,
        external_backend: Option<&OperatorExternalPolicyEngine>,
    ) -> Result<OperatorPolicyEvaluation> {
        self.validate_shape()?;
        let tenant_id = TenantId::new(self.tenant.clone())?;
        let mut decisions = Vec::with_capacity(self.workloads.len());
        for workload in &self.workloads {
            decisions.push(self.evaluate_workload(&tenant_id, workload, external_backend)?);
        }
        Ok(OperatorPolicyEvaluation {
            policy_name: self.metadata.name.clone(),
            tenant_id: tenant_id.as_str().to_string(),
            decision_count: decisions.len(),
            decisions,
        })
    }

    fn evaluate_workload(
        &self,
        tenant_id: &TenantId,
        workload: &OperatorPolicyWorkload,
        external_backend: Option<&OperatorExternalPolicyEngine>,
    ) -> Result<OperatorPolicyDecisionEvaluation> {
        let context = TenantIsolationContext::operator(tenant_id.clone(), "operator.policy");
        let mode = workload
            .runtime
            .tenant_isolation_mode
            .unwrap_or(self.defaults.tenant_isolation_mode);
        let services = workload.services.normalized_services();
        let mut runtime_limits = workload.runtime.profile.runtime_limits();
        runtime_limits.grants.service = services.clone();
        let runtime_policy = RuntimePolicy::new(runtime_limits);

        let identity = workload.identity()?;
        let storage_namespace = workload
            .storage
            .namespace
            .as_deref()
            .unwrap_or(self.defaults.storage_namespace.as_str());
        let storage_namespace = storage_namespace_for_policy(storage_namespace, tenant_id);
        let named_volumes = normalized_strings(&workload.volumes.named);
        let secret_handles = normalized_strings(&workload.secrets.handles);
        let audit_redactions = workload
            .audit
            .redacted_fields
            .clone()
            .unwrap_or_else(|| self.defaults.audit_redactions.clone());
        let audit_redactions = normalized_strings(&audit_redactions);
        let image_policy = workload.image.summary();
        let image_reference = image_policy.reference.clone();
        let endpoint_summaries = workload.network.endpoint_summaries();
        let sandbox_egress = workload.network.egress_summaries();
        let quotas_summary = workload.quotas.summary();
        let trace = workload.trace(OperatorPolicyTraceInput {
            mode,
            storage_namespace: storage_namespace.as_str(),
            services: &services,
            network_endpoint_summaries: &endpoint_summaries,
            sandbox_egress_summaries: &sandbox_egress,
            named_volumes: &named_volumes,
            secret_handle_count: secret_handles.len(),
        });

        let mut quotas = TenantQuotaPolicyDecision::default()
            .with_runtime_budget(runtime_policy.tenant_budget());
        if let Some(charge) = workload.quotas.sandbox_charge {
            quotas = quotas.with_sandbox_charge(charge);
        }

        let decision = context.admit_decision(
            crate::tenant_isolation::TenantIsolationPolicyInput::new(identity)
                .with_runtime_policy(&context, &runtime_policy, workload.runtime.tier, mode)
                .with_services(TenantServiceGrantPolicyDecision::new(services.clone()))
                .with_network(workload.network.to_decision()?)
                .with_storage(TenantStoragePolicyDecision::namespace(
                    storage_namespace.clone(),
                ))
                .with_volumes(TenantVolumePolicyDecision::new(named_volumes.clone()))
                .with_image(workload.image.to_decision())
                .with_secrets(TenantSecretPolicyDecision::handles(secret_handles.clone()))
                .with_quotas(quotas)
                .with_audit_redactions(TenantAuditRedactionPolicy {
                    redacted_fields: audit_redactions,
                }),
        )?;

        let mut evaluation = OperatorPolicyDecisionEvaluation {
            workload_key: workload.key(),
            decision_id: decision.id().as_str().to_string(),
            tenant_id: decision.tenant_id().as_str().to_string(),
            runtime_tier: decision.runtime().tier(),
            runtime_profile: workload.runtime.profile,
            tenant_isolation_mode: mode,
            runtime_admission: decision.runtime().admission().clone(),
            sandbox_backend: workload.sandbox.backend,
            sandbox_id: workload.sandbox.sandbox_id.clone(),
            services,
            network_endpoints: endpoint_summaries,
            sandbox_egress,
            storage_namespace,
            named_volumes,
            image_policy,
            image_reference,
            secret_handle_count: secret_handles.len(),
            secret_handles,
            quotas: quotas_summary,
            audit_redactions: decision.audit_redactions().redacted_fields().to_vec(),
            external_policy: None,
            trace,
            decision,
        };
        if let Some(backend) = external_backend {
            let external_policy = evaluate_external_policy_backend(
                backend,
                evaluation.external_policy_request(self.metadata.name.clone()),
            )?;
            evaluation
                .trace
                .push(format!("external policy: {}", external_policy.summary()));
            evaluation.external_policy = Some(external_policy);
        }
        Ok(evaluation)
    }
}

impl OperatorPolicyDecisionEvaluation {
    fn external_policy_request(
        &self,
        policy_name: Option<String>,
    ) -> OperatorExternalPolicyRequest {
        OperatorExternalPolicyRequest {
            policy_name,
            tenant_id: self.tenant_id.clone(),
            workload_key: self.workload_key.clone(),
            decision_id: self.decision_id.clone(),
            workload_kind: self.decision.workload().kind().label().to_owned(),
            workload_name: self.decision.workload().name().to_owned(),
            runtime_tier: self.runtime_tier.label().to_owned(),
            tenant_isolation_mode: self.tenant_isolation_mode.as_str().to_owned(),
            runtime_admission: admission_label(&self.runtime_admission),
            sandbox_backend: self.sandbox_backend.map(|backend| format!("{backend:?}")),
            sandbox_id: self.sandbox_id.clone(),
            services: self.services.clone(),
            network_endpoints: self.network_endpoints.clone(),
            sandbox_egress: self.sandbox_egress.clone(),
            storage_namespace: self.storage_namespace.clone(),
            named_volumes: self.named_volumes.clone(),
            image_reference: self.image_reference.clone(),
            secret_handle_count: self.secret_handle_count,
            audit_redactions: self.audit_redactions.clone(),
            policy_bundle_hash: None,
            input_digest: String::new(),
            timeout_millis: 0,
        }
    }
}

fn storage_namespace_for_policy(namespace: &str, tenant_id: &TenantId) -> String {
    if namespace == "tenant" {
        tenant_id.as_str().to_string()
    } else {
        namespace.to_string()
    }
}
