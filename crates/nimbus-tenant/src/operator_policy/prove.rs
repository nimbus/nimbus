use std::collections::{BTreeMap, BTreeSet};

use nimbus_core::{Error, Result};
use nimbus_egress::EgressProtocol;
use serde::{Deserialize, Serialize};

use super::{
    OperatorNetworkEndpointPolicy, OperatorPolicyDocument, OperatorPolicyWorkload,
    RuntimeIsolationTier, WorkloadKind,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OperatorPolicyAcceptedRisk {
    pub advisory_id: String,
    pub approved_by: String,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OperatorPolicyProofReport {
    pub tenant_id: String,
    pub checked_workloads: usize,
    pub advisory_count: usize,
    pub accepted_count: usize,
    pub unaccepted_count: usize,
    pub advisories: Vec<OperatorPolicyAdvisory>,
}

impl OperatorPolicyProofReport {
    pub fn render_text(&self) -> String {
        let mut output = String::new();
        output.push_str("Policy prove\n");
        output.push_str(&format!("Tenant: {}\n", self.tenant_id));
        output.push_str(&format!("Checked workloads: {}\n", self.checked_workloads));
        output.push_str(&format!(
            "Advisories: {} (unaccepted: {}, accepted: {})\n",
            self.advisory_count, self.unaccepted_count, self.accepted_count
        ));
        if self.advisories.is_empty() {
            output.push_str("No advisories.\n");
            return output;
        }
        for advisory in &self.advisories {
            let status = if advisory.accepted_risk.is_some() {
                "accepted"
            } else {
                "unaccepted"
            };
            output.push_str(&format!(
                "- {} [{}] {} ({status})\n",
                advisory.id,
                advisory.severity.label(),
                advisory.kind.label()
            ));
            output.push_str(&format!("  workload: {}\n", advisory.workload_key));
            output.push_str(&format!("  message: {}\n", advisory.message));
            if let Some(accepted) = &advisory.accepted_risk {
                output.push_str(&format!(
                    "  accepted_by: {} ({})\n",
                    accepted.approved_by, accepted.reason
                ));
            }
        }
        output
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OperatorPolicyAdvisory {
    pub id: String,
    pub kind: OperatorPolicyAdvisoryKind,
    pub severity: OperatorPolicyAdvisorySeverity,
    pub workload_key: String,
    pub message: String,
    pub accepted_risk: Option<OperatorPolicyAcceptedRisk>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OperatorPolicyAdvisoryKind {
    BroadEgress,
    WriteBypass,
    SecretExposure,
    CrossTenantRegression,
}

impl OperatorPolicyAdvisoryKind {
    fn label(self) -> &'static str {
        match self {
            Self::BroadEgress => "broad_egress",
            Self::WriteBypass => "write_bypass",
            Self::SecretExposure => "secret_exposure",
            Self::CrossTenantRegression => "cross_tenant_regression",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OperatorPolicyAdvisorySeverity {
    Medium,
    High,
}

impl OperatorPolicyAdvisorySeverity {
    fn label(self) -> &'static str {
        match self {
            Self::Medium => "medium",
            Self::High => "high",
        }
    }
}

impl OperatorPolicyDocument {
    pub fn prove(&self) -> Result<OperatorPolicyProofReport> {
        let evaluation = self.evaluate()?;
        let accepted = accepted_risk_map(&self.accepted_risks);
        let mut advisories = Vec::new();
        for workload in &self.workloads {
            prove_broad_egress(workload, &accepted, &mut advisories);
            prove_write_bypass(workload, &accepted, &mut advisories);
            prove_secret_exposure(workload, &accepted, &mut advisories);
            prove_cross_tenant_regressions(&self.tenant, workload, &accepted, &mut advisories);
        }
        advisories.sort_by(|left, right| {
            left.kind
                .cmp(&right.kind)
                .then_with(|| left.workload_key.cmp(&right.workload_key))
                .then_with(|| left.id.cmp(&right.id))
        });
        let accepted_count = advisories
            .iter()
            .filter(|advisory| advisory.accepted_risk.is_some())
            .count();
        let advisory_count = advisories.len();
        Ok(OperatorPolicyProofReport {
            tenant_id: evaluation.tenant_id,
            checked_workloads: evaluation.decision_count,
            advisory_count,
            accepted_count,
            unaccepted_count: advisory_count.saturating_sub(accepted_count),
            advisories,
        })
    }
}

pub(super) fn validate_accepted_risks(risks: &[OperatorPolicyAcceptedRisk]) -> Result<()> {
    let mut seen = BTreeSet::new();
    for risk in risks {
        if risk.advisory_id.trim().is_empty() || risk.advisory_id.contains(char::is_whitespace) {
            return Err(Error::InvalidInput(
                "operator policy invalid: accepted_risks advisory_id must be non-empty and contain no whitespace"
                    .to_string(),
            ));
        }
        if risk.approved_by.trim().is_empty() {
            return Err(Error::InvalidInput(format!(
                "operator policy invalid: accepted_risk `{}` requires approved_by",
                risk.advisory_id
            )));
        }
        if risk.reason.trim().is_empty() {
            return Err(Error::InvalidInput(format!(
                "operator policy invalid: accepted_risk `{}` requires a reason",
                risk.advisory_id
            )));
        }
        if !seen.insert(&risk.advisory_id) {
            return Err(Error::InvalidInput(format!(
                "operator policy invalid: accepted_risk `{}` is duplicated",
                risk.advisory_id
            )));
        }
    }
    Ok(())
}

fn accepted_risk_map(
    accepted_risks: &[OperatorPolicyAcceptedRisk],
) -> BTreeMap<String, OperatorPolicyAcceptedRisk> {
    accepted_risks
        .iter()
        .cloned()
        .map(|risk| (risk.advisory_id.clone(), risk))
        .collect()
}

fn prove_broad_egress(
    workload: &OperatorPolicyWorkload,
    accepted: &BTreeMap<String, OperatorPolicyAcceptedRisk>,
    advisories: &mut Vec<OperatorPolicyAdvisory>,
) {
    let workload_key = workload.key();
    for rule in &workload.network.egress.allow {
        let mut reasons = Vec::new();
        if matches!(
            rule.protocol,
            EgressProtocol::Http | EgressProtocol::Https | EgressProtocol::Ws | EgressProtocol::Wss
        ) {
            if rule.methods.is_empty() {
                reasons.push("all HTTP methods");
            }
            if rule.path_prefixes.is_empty() {
                reasons.push("all paths");
            }
        }
        if rule.allow_internal_ips {
            reasons.push("internal IPs allowed");
        }
        if reasons.is_empty() {
            continue;
        }
        let id = advisory_id(
            OperatorPolicyAdvisoryKind::BroadEgress,
            &workload_key,
            &rule.name,
        );
        push_advisory(
            advisories,
            accepted,
            id,
            OperatorPolicyAdvisoryKind::BroadEgress,
            OperatorPolicyAdvisorySeverity::Medium,
            workload_key.clone(),
            format!(
                "egress rule `{}` permits {}; narrow by method/path or remove internal IP allowance",
                rule.name,
                reasons.join(", ")
            ),
        );
    }
}

fn prove_write_bypass(
    workload: &OperatorPolicyWorkload,
    accepted: &BTreeMap<String, OperatorPolicyAcceptedRisk>,
    advisories: &mut Vec<OperatorPolicyAdvisory>,
) {
    if !matches!(workload.kind, WorkloadKind::RuntimeFunction) {
        return;
    }
    let workload_key = workload.key();
    for endpoint in &workload.network.endpoints {
        if !is_write_capable_endpoint(endpoint) {
            continue;
        }
        let endpoint_key = format!("{}/{}", endpoint.service, endpoint.name);
        let id = advisory_id(
            OperatorPolicyAdvisoryKind::WriteBypass,
            &workload_key,
            &endpoint_key,
        );
        push_advisory(
            advisories,
            accepted,
            id,
            OperatorPolicyAdvisoryKind::WriteBypass,
            OperatorPolicyAdvisorySeverity::High,
            workload_key.clone(),
            format!(
                "runtime workload publishes direct database endpoint `{endpoint_key}`; prefer HostBridge-mediated storage or explicit Nimbus SDK service control"
            ),
        );
    }
}

fn prove_secret_exposure(
    workload: &OperatorPolicyWorkload,
    accepted: &BTreeMap<String, OperatorPolicyAcceptedRisk>,
    advisories: &mut Vec<OperatorPolicyAdvisory>,
) {
    if workload.secrets.handles.is_empty()
        || !matches!(
            workload.runtime.tier,
            RuntimeIsolationTier::InProcessUntrusted
        )
    {
        return;
    }
    let workload_key = workload.key();
    let id = advisory_id(
        OperatorPolicyAdvisoryKind::SecretExposure,
        &workload_key,
        "in_process_untrusted",
    );
    push_advisory(
        advisories,
        accepted,
        id,
        OperatorPolicyAdvisoryKind::SecretExposure,
        OperatorPolicyAdvisorySeverity::High,
        workload_key,
        "in-process untrusted workload receives secret handles; prefer scoped HostBridge/service identity projection".to_string(),
    );
}

fn prove_cross_tenant_regressions(
    tenant: &str,
    workload: &OperatorPolicyWorkload,
    accepted: &BTreeMap<String, OperatorPolicyAcceptedRisk>,
    advisories: &mut Vec<OperatorPolicyAdvisory>,
) {
    let workload_key = workload.key();
    for handle in &workload.secrets.handles {
        let Some(prefix) = handle.split('/').next() else {
            continue;
        };
        if !prefix.starts_with("tenant-") || prefix == tenant {
            continue;
        }
        let id = advisory_id(
            OperatorPolicyAdvisoryKind::CrossTenantRegression,
            &workload_key,
            prefix,
        );
        push_advisory(
            advisories,
            accepted,
            id,
            OperatorPolicyAdvisoryKind::CrossTenantRegression,
            OperatorPolicyAdvisorySeverity::High,
            workload_key.clone(),
            format!("secret handle namespace `{prefix}` does not match policy tenant `{tenant}`"),
        );
    }
}

fn is_write_capable_endpoint(endpoint: &OperatorNetworkEndpointPolicy) -> bool {
    let name = format!(
        "{} {} {}",
        endpoint.service.to_ascii_lowercase(),
        endpoint.name.to_ascii_lowercase(),
        endpoint.host_port
    );
    [
        "postgres", "mysql", "mariadb", "mongo", "redis", "database", "db",
    ]
    .iter()
    .any(|needle| name.contains(needle))
        || matches!(endpoint.guest_port, Some(5432 | 3306 | 27017 | 6379))
        || matches!(endpoint.host_port, 5432 | 3306 | 27017 | 6379)
}

fn push_advisory(
    advisories: &mut Vec<OperatorPolicyAdvisory>,
    accepted: &BTreeMap<String, OperatorPolicyAcceptedRisk>,
    id: String,
    kind: OperatorPolicyAdvisoryKind,
    severity: OperatorPolicyAdvisorySeverity,
    workload_key: String,
    message: String,
) {
    let accepted_risk = accepted.get(&id).cloned();
    advisories.push(OperatorPolicyAdvisory {
        id,
        kind,
        severity,
        workload_key,
        message,
        accepted_risk,
    });
}

fn advisory_id(kind: OperatorPolicyAdvisoryKind, workload_key: &str, subject: &str) -> String {
    format!("{}:{workload_key}:{subject}", kind.label())
}
