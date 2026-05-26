use std::collections::{BTreeMap, BTreeSet};

use nimbus_core::Result;
use nimbus_sandbox::SandboxBackendKind;
use serde::Serialize;

use super::formatting::{
    admission_label, bool_label, optional_backend_label, provenance_summary, quota_summary,
    signature_summary,
};
use super::{OperatorPolicyDecisionEvaluation, OperatorPolicyDocument, OperatorPolicyImageSummary};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OperatorPolicyDiff {
    pub added_workloads: Vec<OperatorPolicyDecisionEvaluation>,
    pub removed_workloads: Vec<OperatorPolicyDecisionEvaluation>,
    pub changed_workloads: Vec<OperatorPolicyDiffSummary>,
}

impl OperatorPolicyDiff {
    pub fn between(from: &OperatorPolicyDocument, to: &OperatorPolicyDocument) -> Result<Self> {
        let from = from.evaluate()?;
        let to = to.evaluate()?;
        let from_by_key: BTreeMap<_, _> = from
            .decisions
            .into_iter()
            .map(|decision| (decision.workload_key.clone(), decision))
            .collect();
        let to_by_key: BTreeMap<_, _> = to
            .decisions
            .into_iter()
            .map(|decision| (decision.workload_key.clone(), decision))
            .collect();

        let mut added_workloads = Vec::new();
        let mut removed_workloads = Vec::new();
        let mut changed_workloads = Vec::new();

        for (key, next) in &to_by_key {
            match from_by_key.get(key) {
                None => added_workloads.push(next.clone()),
                Some(previous) => {
                    if let Some(summary) = OperatorPolicyDiffSummary::between(previous, next) {
                        changed_workloads.push(summary);
                    }
                }
            }
        }
        for (key, previous) in &from_by_key {
            if !to_by_key.contains_key(key) {
                removed_workloads.push(previous.clone());
            }
        }

        Ok(Self {
            added_workloads,
            removed_workloads,
            changed_workloads,
        })
    }

    pub fn render_text(&self) -> String {
        let mut output = String::from("Policy diff\n");
        output.push_str(&format!("Lifecycle: {}\n", self.lifecycle().label()));
        if self.added_workloads.is_empty()
            && self.removed_workloads.is_empty()
            && self.changed_workloads.is_empty()
        {
            output.push_str("No authority changes.\n");
            return output;
        }
        for decision in &self.added_workloads {
            output.push_str(&format!("+ {}\n", decision.workload_key));
        }
        for decision in &self.removed_workloads {
            output.push_str(&format!("- {}\n", decision.workload_key));
        }
        for summary in &self.changed_workloads {
            output.push_str(&format!(
                "~ {} (lifecycle: {})\n",
                summary.workload_key,
                summary.lifecycle.label()
            ));
            for change in &summary.changes {
                output.push_str(&format!("  {change}\n"));
            }
        }
        output
    }

    pub fn lifecycle(&self) -> OperatorPolicyLifecycle {
        if !self.added_workloads.is_empty() || !self.removed_workloads.is_empty() {
            return OperatorPolicyLifecycle::RecreateRequired;
        }
        self.changed_workloads
            .iter()
            .map(|summary| summary.lifecycle)
            .fold(
                OperatorPolicyLifecycle::DynamicReload,
                OperatorPolicyLifecycle::max,
            )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OperatorPolicyDiffSummary {
    pub workload_key: String,
    pub lifecycle: OperatorPolicyLifecycle,
    pub changes: Vec<String>,
}

impl OperatorPolicyDiffSummary {
    fn between(
        previous: &OperatorPolicyDecisionEvaluation,
        next: &OperatorPolicyDecisionEvaluation,
    ) -> Option<Self> {
        let mut changes = Vec::new();
        let mut lifecycle = OperatorPolicyLifecycle::DynamicReload;
        if previous.tenant_id != next.tenant_id {
            changes.push(format!(
                "tenant changed: {} -> {}",
                previous.tenant_id, next.tenant_id
            ));
            lifecycle = lifecycle.max(OperatorPolicyLifecycle::RecreateRequired);
        }
        if previous.tenant_isolation_mode != next.tenant_isolation_mode {
            changes.push(format!(
                "tenant isolation mode changed: {} -> {}",
                previous.tenant_isolation_mode.as_str(),
                next.tenant_isolation_mode.as_str()
            ));
            lifecycle = lifecycle.max(OperatorPolicyLifecycle::RecreateRequired);
        }
        if previous.runtime_profile != next.runtime_profile {
            changes.push(format!(
                "runtime profile changed: {} -> {}",
                previous.runtime_profile.label(),
                next.runtime_profile.label()
            ));
            lifecycle = lifecycle.max(OperatorPolicyLifecycle::RecreateRequired);
        }
        if previous.runtime_tier != next.runtime_tier {
            changes.push(format!(
                "runtime tier changed: {} -> {}",
                previous.runtime_tier.label(),
                next.runtime_tier.label()
            ));
            lifecycle = lifecycle.max(OperatorPolicyLifecycle::RecreateRequired);
        }
        if record_vec_delta(&mut changes, "services", &previous.services, &next.services) {
            lifecycle = lifecycle.max(OperatorPolicyLifecycle::RecreateRequired);
        }
        if record_vec_delta(
            &mut changes,
            "network endpoints",
            &previous.network_endpoints,
            &next.network_endpoints,
        ) {
            lifecycle = lifecycle.max(OperatorPolicyLifecycle::RecreateRequired);
        }
        if record_vec_delta(
            &mut changes,
            "sandbox egress",
            &previous.sandbox_egress,
            &next.sandbox_egress,
        ) {
            lifecycle = lifecycle.max(sandbox_egress_reload_lifecycle(next.sandbox_backend));
        }
        if previous.sandbox_backend != next.sandbox_backend {
            changes.push(format!(
                "sandbox backend changed: {} -> {}",
                optional_backend_label(previous.sandbox_backend),
                optional_backend_label(next.sandbox_backend)
            ));
            lifecycle = lifecycle.max(OperatorPolicyLifecycle::RecreateRequired);
        }
        if previous.sandbox_id != next.sandbox_id {
            changes.push(format!(
                "sandbox id changed: {} -> {}",
                previous.sandbox_id.as_deref().unwrap_or("none"),
                next.sandbox_id.as_deref().unwrap_or("none")
            ));
            lifecycle = lifecycle.max(OperatorPolicyLifecycle::RecreateRequired);
        }
        if previous.storage_namespace != next.storage_namespace {
            changes.push(format!(
                "storage namespace changed: {} -> {}",
                previous.storage_namespace, next.storage_namespace
            ));
            lifecycle = lifecycle.max(OperatorPolicyLifecycle::RecreateRequired);
        }
        if record_vec_delta(
            &mut changes,
            "volumes",
            &previous.named_volumes,
            &next.named_volumes,
        ) {
            lifecycle = lifecycle.max(OperatorPolicyLifecycle::RecreateRequired);
        }
        if record_image_policy_delta(&mut changes, &previous.image_policy, &next.image_policy) {
            lifecycle = lifecycle.max(OperatorPolicyLifecycle::RecreateRequired);
        }
        if previous.secret_handles != next.secret_handles {
            changes.push(format!(
                "secret handles changed: count {} -> {}",
                previous.secret_handle_count, next.secret_handle_count
            ));
            lifecycle = lifecycle.max(OperatorPolicyLifecycle::RecreateRequired);
        }
        if previous.quotas != next.quotas {
            changes.push(format!(
                "quotas changed: {} -> {}",
                quota_summary(previous.quotas.sandbox_charge),
                quota_summary(next.quotas.sandbox_charge)
            ));
            lifecycle = lifecycle.max(OperatorPolicyLifecycle::RecreateRequired);
        }
        record_vec_delta(
            &mut changes,
            "audit redactions",
            &previous.audit_redactions,
            &next.audit_redactions,
        );
        if admission_label(&previous.runtime_admission) != admission_label(&next.runtime_admission)
        {
            changes.push(format!(
                "runtime admission changed: {} -> {}",
                admission_label(&previous.runtime_admission),
                admission_label(&next.runtime_admission)
            ));
            lifecycle = lifecycle.max(OperatorPolicyLifecycle::RecreateRequired);
        }
        if previous.decision_id != next.decision_id && changes.is_empty() {
            changes.push(format!(
                "decision authority fingerprint changed: {} -> {}",
                previous.decision_id, next.decision_id
            ));
            lifecycle = lifecycle.max(OperatorPolicyLifecycle::RecreateRequired);
        }
        (!changes.is_empty()).then(|| Self {
            workload_key: next.workload_key.clone(),
            lifecycle,
            changes,
        })
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OperatorPolicyLifecycle {
    #[default]
    DynamicReload,
    RecreateRequired,
}

impl OperatorPolicyLifecycle {
    pub fn label(self) -> &'static str {
        match self {
            Self::DynamicReload => "dynamic_reload",
            Self::RecreateRequired => "recreate_required",
        }
    }

    fn max(self, other: Self) -> Self {
        if matches!(self, Self::RecreateRequired) || matches!(other, Self::RecreateRequired) {
            Self::RecreateRequired
        } else {
            Self::DynamicReload
        }
    }
}

fn record_vec_delta(
    changes: &mut Vec<String>,
    label: &str,
    previous: &[String],
    next: &[String],
) -> bool {
    let previous: BTreeSet<_> = previous.iter().cloned().collect();
    let next: BTreeSet<_> = next.iter().cloned().collect();
    let added: Vec<_> = next.difference(&previous).cloned().collect();
    let removed: Vec<_> = previous.difference(&next).cloned().collect();
    let changed = !added.is_empty() || !removed.is_empty();
    if !added.is_empty() {
        changes.push(format!("{label} added: {}", added.join(", ")));
    }
    if !removed.is_empty() {
        changes.push(format!("{label} removed: {}", removed.join(", ")));
    }
    changed
}

fn sandbox_egress_reload_lifecycle(backend: Option<SandboxBackendKind>) -> OperatorPolicyLifecycle {
    match backend {
        Some(SandboxBackendKind::Container) => OperatorPolicyLifecycle::DynamicReload,
        Some(SandboxBackendKind::Krun) | None => OperatorPolicyLifecycle::RecreateRequired,
    }
}

fn record_image_policy_delta(
    changes: &mut Vec<String>,
    previous: &OperatorPolicyImageSummary,
    next: &OperatorPolicyImageSummary,
) -> bool {
    let original_len = changes.len();
    if previous.reference != next.reference {
        changes.push(format!(
            "image reference changed: {} -> {}",
            previous.reference.as_deref().unwrap_or("none"),
            next.reference.as_deref().unwrap_or("none")
        ));
    }
    if previous.digest_required != next.digest_required {
        changes.push(format!(
            "image digest required changed: {} -> {}",
            bool_label(previous.digest_required),
            bool_label(next.digest_required)
        ));
    }
    record_vec_delta(
        changes,
        "image allowed registries",
        &previous.allowed_registries,
        &next.allowed_registries,
    );
    if previous.signature != next.signature {
        changes.push(format!(
            "image signature policy changed: {} -> {}",
            signature_summary(previous.signature.as_ref()),
            signature_summary(next.signature.as_ref())
        ));
    }
    if previous.provenance != next.provenance {
        changes.push(format!(
            "image provenance policy changed: {} -> {}",
            provenance_summary(previous.provenance.as_ref()),
            provenance_summary(next.provenance.as_ref())
        ));
    }
    if previous.sbom_required != next.sbom_required {
        changes.push(format!(
            "image SBOM requirement changed: {} -> {}",
            bool_label(previous.sbom_required),
            bool_label(next.sbom_required)
        ));
    }
    if previous.allow_local_build != next.allow_local_build {
        changes.push(format!(
            "image local-build permission changed: {} -> {}",
            bool_label(previous.allow_local_build),
            bool_label(next.allow_local_build)
        ));
    }
    changes.len() != original_len
}
