use super::OperatorPolicyEvaluation;
use super::formatting::{admission_label, image_policy_summary, join_or_none, quota_summary};

impl OperatorPolicyEvaluation {
    pub fn render_validate_text(&self) -> String {
        let mut output = String::new();
        output.push_str("Policy validation: allowed\n");
        if let Some(name) = &self.policy_name {
            output.push_str(&format!("Policy: {name}\n"));
        }
        output.push_str(&format!("Tenant: {}\n", self.tenant_id));
        output.push_str(&format!("Decisions: {}\n", self.decision_count));
        for decision in &self.decisions {
            output.push_str(&format!(
                "- {} -> {}\n",
                decision.workload_key, decision.decision_id
            ));
        }
        output
    }

    pub fn render_explain_text(&self) -> String {
        let mut output = String::new();
        output.push_str("Policy explanation\n");
        if let Some(name) = &self.policy_name {
            output.push_str(&format!("Policy: {name}\n"));
        }
        output.push_str(&format!("Tenant: {}\n", self.tenant_id));
        for decision in &self.decisions {
            output.push_str(&format!("\n{}\n", decision.workload_key));
            output.push_str(&format!("  decision_id: {}\n", decision.decision_id));
            output.push_str(&format!(
                "  tenant_isolation_mode: {}\n",
                decision.tenant_isolation_mode.as_str()
            ));
            output.push_str(&format!(
                "  runtime_profile: {}\n",
                decision.runtime_profile.label()
            ));
            output.push_str(&format!(
                "  runtime_tier: {}\n",
                decision.runtime_tier.label()
            ));
            output.push_str(&format!(
                "  runtime_admission: {}\n",
                admission_label(&decision.runtime_admission)
            ));
            output.push_str(&format!(
                "  services: {}\n",
                join_or_none(&decision.services)
            ));
            output.push_str(&format!(
                "  network_endpoints: {}\n",
                join_or_none(&decision.network_endpoints)
            ));
            output.push_str(&format!(
                "  sandbox_egress: {}\n",
                join_or_none(&decision.sandbox_egress)
            ));
            output.push_str(&format!(
                "  storage_namespace: {}\n",
                decision.storage_namespace
            ));
            output.push_str(&format!(
                "  named_volumes: {}\n",
                join_or_none(&decision.named_volumes)
            ));
            output.push_str(&format!(
                "  image_policy: {}\n",
                image_policy_summary(&decision.image_policy)
            ));
            output.push_str(&format!(
                "  secret_handle_count: {}\n",
                decision.secret_handle_count
            ));
            output.push_str(&format!(
                "  quotas: {}\n",
                quota_summary(decision.quotas.sandbox_charge)
            ));
            output.push_str(&format!(
                "  audit_redactions: {}\n",
                join_or_none(&decision.audit_redactions)
            ));
            if let Some(external_policy) = &decision.external_policy {
                output.push_str(&format!(
                    "  external_policy: {}\n",
                    external_policy.summary()
                ));
            }
            for trace in &decision.trace {
                output.push_str(&format!("  trace: {trace}\n"));
            }
        }
        output
    }
}
