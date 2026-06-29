use nimbus_core::{Error, Result};
use nimbus_egress::{EgressPolicy, EgressProtocol, EgressRule};
use serde::{Deserialize, Serialize};

use super::{egress_protocol_label, normalized_strings};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OperatorSandboxEgressPolicy {
    #[serde(default)]
    pub allow: Vec<OperatorSandboxEgressRulePolicy>,
}

impl OperatorSandboxEgressPolicy {
    pub(super) fn to_sandbox_policy(&self) -> EgressPolicy {
        EgressPolicy::new(self.normalized_rules().into_iter().map(|rule| {
            let mut egress_rule = EgressRule::new(
                rule.name.clone(),
                rule.protocol,
                rule.host.clone(),
                rule.port,
            )
            .with_methods(normalized_strings(&rule.methods))
            .with_path_prefixes(normalized_strings(&rule.path_prefixes));
            if rule.allow_internal_ips {
                egress_rule = egress_rule.allow_internal_ips(true);
            }
            egress_rule
        }))
    }

    pub(super) fn summaries(&self) -> Vec<String> {
        self.normalized_rules()
            .into_iter()
            .map(OperatorSandboxEgressRulePolicy::summary)
            .collect()
    }

    fn normalized_rules(&self) -> Vec<&OperatorSandboxEgressRulePolicy> {
        let mut rules: Vec<_> = self.allow.iter().collect();
        rules.sort_by(|left, right| left.name.cmp(&right.name));
        rules
    }

    pub(super) fn validate(&self, workload_key: &str) -> Result<()> {
        self.to_sandbox_policy().validate().map_err(|message| {
            Error::InvalidInput(format!(
                "operator policy invalid: workload `{workload_key}` network.egress {message}"
            ))
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OperatorSandboxEgressRulePolicy {
    pub name: String,
    pub protocol: EgressProtocol,
    pub host: String,
    pub port: u16,
    #[serde(default)]
    pub methods: Vec<String>,
    #[serde(default)]
    pub path_prefixes: Vec<String>,
    #[serde(default)]
    pub allow_internal_ips: bool,
}

impl OperatorSandboxEgressRulePolicy {
    fn summary(&self) -> String {
        let mut parts = vec![format!(
            "{} {} {}:{}",
            self.name,
            egress_protocol_label(self.protocol),
            self.host,
            self.port
        )];
        if !self.methods.is_empty() {
            parts.push(format!(
                "methods={}",
                normalized_strings(&self.methods).join(",")
            ));
        }
        if !self.path_prefixes.is_empty() {
            parts.push(format!(
                "paths={}",
                normalized_strings(&self.path_prefixes).join(",")
            ));
        }
        if self.allow_internal_ips {
            parts.push("allow_internal_ips=true".to_string());
        }
        parts.join(" ")
    }
}
