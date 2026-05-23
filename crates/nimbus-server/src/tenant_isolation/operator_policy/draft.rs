use nimbus_core::{Error, Result};
use nimbus_sandbox::{PublishedEndpointProtocol, SandboxEgressPolicy, SandboxEgressRule};
use serde::{Deserialize, Serialize};

use super::{OperatorPolicyDocument, OperatorPolicyWorkload, OperatorSandboxEgressRulePolicy};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OperatorDeniedEgressEvent {
    pub tenant_id: String,
    pub workload_kind: String,
    pub workload_name: String,
    pub protocol: PublishedEndpointProtocol,
    pub host: String,
    pub port: u16,
    pub method: Option<String>,
    pub path: Option<String>,
    pub reason: String,
}

impl OperatorDeniedEgressEvent {
    pub fn workload_key(&self) -> String {
        format!("{}/{}", self.workload_kind, self.workload_name)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OperatorPolicyDraftKind {
    SandboxEgressAllow,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OperatorPolicyDraftStatus {
    ReviewRequired,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OperatorPolicyDraft {
    pub kind: OperatorPolicyDraftKind,
    pub status: OperatorPolicyDraftStatus,
    pub requires_explicit_approval: bool,
    pub auto_apply: bool,
    pub tenant_id: String,
    pub workload_key: String,
    pub reason: String,
    pub suggested_egress_rule: OperatorSandboxEgressRulePolicy,
    pub review_notes: Vec<String>,
}

impl OperatorPolicyDraft {
    pub fn apply_to(
        &self,
        document: &OperatorPolicyDocument,
        approval: Option<&OperatorPolicyDraftApproval>,
    ) -> Result<OperatorPolicyDocument> {
        approval.ok_or_else(|| {
            Error::InvalidInput(
                "operator policy draft requires explicit approval before apply".to_string(),
            )
        })?;
        if self.tenant_id != document.tenant {
            return Err(Error::InvalidInput(format!(
                "operator policy draft tenant `{}` does not match policy tenant `{}`",
                self.tenant_id, document.tenant
            )));
        }
        let mut updated = document.clone();
        let workload = updated
            .workloads
            .iter_mut()
            .find(|workload| workload.key() == self.workload_key)
            .ok_or_else(|| {
                Error::InvalidInput(format!(
                    "operator policy draft workload `{}` is not present in policy",
                    self.workload_key
                ))
            })?;
        if workload
            .network
            .egress
            .allow
            .iter()
            .any(|rule| rule.name == self.suggested_egress_rule.name)
        {
            return Err(Error::InvalidInput(format!(
                "operator policy draft rule `{}` already exists on workload `{}`",
                self.suggested_egress_rule.name, self.workload_key
            )));
        }
        workload
            .network
            .egress
            .allow
            .push(self.suggested_egress_rule.clone());
        updated.validate()?;
        Ok(updated)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OperatorPolicyDraftApproval {
    pub approved_by: String,
    pub reason: String,
}

impl OperatorPolicyDraftApproval {
    pub fn new(approved_by: impl Into<String>, reason: impl Into<String>) -> Result<Self> {
        let approval = Self {
            approved_by: approved_by.into(),
            reason: reason.into(),
        };
        if approval.approved_by.trim().is_empty() {
            return Err(Error::InvalidInput(
                "operator policy draft approval requires approved_by".to_string(),
            ));
        }
        if approval.reason.trim().is_empty() {
            return Err(Error::InvalidInput(
                "operator policy draft approval requires a reason".to_string(),
            ));
        }
        Ok(approval)
    }
}

impl OperatorPolicyDocument {
    pub fn draft_from_denied_egress(
        &self,
        event: OperatorDeniedEgressEvent,
    ) -> Result<OperatorPolicyDraft> {
        if event.tenant_id != self.tenant {
            return Err(Error::InvalidInput(format!(
                "denied egress event tenant `{}` does not match policy tenant `{}`",
                event.tenant_id, self.tenant
            )));
        }
        let workload_key = event.workload_key();
        let workload = self
            .workloads
            .iter()
            .find(|workload| workload.key() == workload_key)
            .ok_or_else(|| {
                Error::InvalidInput(format!(
                    "denied egress event workload `{workload_key}` is not present in policy"
                ))
            })?;
        let suggested_egress_rule = draft_rule_from_denied_event(&event, workload)?;
        validate_draft_rule(&suggested_egress_rule, &workload_key)?;
        Ok(OperatorPolicyDraft {
            kind: OperatorPolicyDraftKind::SandboxEgressAllow,
            status: OperatorPolicyDraftStatus::ReviewRequired,
            requires_explicit_approval: true,
            auto_apply: false,
            tenant_id: event.tenant_id,
            workload_key,
            reason: event.reason,
            suggested_egress_rule,
            review_notes: vec![
                "Review the denied request and upstream service owner before approving."
                    .to_string(),
                "Drafts are never applied automatically; approval must be explicit.".to_string(),
            ],
        })
    }
}

fn draft_rule_from_denied_event(
    event: &OperatorDeniedEgressEvent,
    workload: &OperatorPolicyWorkload,
) -> Result<OperatorSandboxEgressRulePolicy> {
    let host = event.host.trim().to_ascii_lowercase();
    if host.is_empty() {
        return Err(Error::InvalidInput(
            "denied egress event host cannot be empty".to_string(),
        ));
    }
    let mut methods = Vec::new();
    let mut path_prefixes = Vec::new();
    if matches!(
        event.protocol,
        PublishedEndpointProtocol::Http | PublishedEndpointProtocol::Https
    ) {
        if let Some(method) = event.method.as_deref().map(str::trim)
            && !method.is_empty()
        {
            methods.push(method.to_ascii_uppercase());
        }
        if let Some(path_prefix) = sanitized_path_prefix(event.path.as_deref()) {
            path_prefixes.push(path_prefix);
        }
    }
    Ok(OperatorSandboxEgressRulePolicy {
        name: unique_rule_name(workload, &host, event.protocol, event.port),
        protocol: event.protocol,
        host,
        port: event.port,
        methods,
        path_prefixes,
        allow_internal_ips: false,
    })
}

fn sanitized_path_prefix(path: Option<&str>) -> Option<String> {
    let path = path?.split(['?', '#']).next().unwrap_or("").trim();
    if path.is_empty() || path == "/" {
        return None;
    }
    Some(path.to_string())
}

fn unique_rule_name(
    workload: &OperatorPolicyWorkload,
    host: &str,
    protocol: PublishedEndpointProtocol,
    port: u16,
) -> String {
    let protocol = match protocol {
        PublishedEndpointProtocol::Tcp => "tcp",
        PublishedEndpointProtocol::Http => "http",
        PublishedEndpointProtocol::Https => "https",
    };
    let mut base = host
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>();
    while base.contains("--") {
        base = base.replace("--", "-");
    }
    let base = base.trim_matches('-');
    let base = if base.is_empty() { "egress" } else { base };
    let candidate = format!("{base}-{protocol}-{port}");
    if !workload
        .network
        .egress
        .allow
        .iter()
        .any(|rule| rule.name == candidate)
    {
        return candidate;
    }
    let mut suffix = 2;
    loop {
        let candidate = format!("{base}-{protocol}-{port}-{suffix}");
        if !workload
            .network
            .egress
            .allow
            .iter()
            .any(|rule| rule.name == candidate)
        {
            return candidate;
        }
        suffix += 1;
    }
}

fn validate_draft_rule(rule: &OperatorSandboxEgressRulePolicy, workload_key: &str) -> Result<()> {
    let mut sandbox_rule = SandboxEgressRule::new(
        rule.name.clone(),
        rule.protocol,
        rule.host.clone(),
        rule.port,
    )
    .with_methods(rule.methods.clone())
    .with_path_prefixes(rule.path_prefixes.clone());
    if rule.allow_internal_ips {
        sandbox_rule = sandbox_rule.allow_internal_ips(true);
    }
    SandboxEgressPolicy::new([sandbox_rule])
        .validate()
        .map_err(|message| {
            Error::InvalidInput(format!(
                "operator policy draft for workload `{workload_key}` is invalid: {message}"
            ))
        })
}
