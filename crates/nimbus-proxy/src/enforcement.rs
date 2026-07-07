use nimbus_egress::{CompiledEgressPolicy, EgressCredentialInjection, EgressDlpRule, EgressRule};

use crate::decision_log::EgressDecisionLog;
use crate::phase::{EgressProxyRequestPhase, RequestPhaseRecorder};
use crate::request::{ParsedProxyRequest, PreparedProxyRequest, ProxyRequestMode};
use crate::response::HttpProxyResponse;

pub(crate) struct ProxyRequestEnforcementContext<'a> {
    pub(crate) policy: &'a CompiledEgressPolicy,
    pub(crate) matched_rule: Option<&'a str>,
    pub(crate) reason: &'a str,
    pub(crate) credential_provider: &'a dyn crate::credentials::CredentialSecretProvider,
    pub(crate) phase_recorder: &'a RequestPhaseRecorder,
    pub(crate) request_id: &'a str,
}

pub(crate) struct ProxyRequestEnforcementPlan<'a> {
    parsed: &'a ParsedProxyRequest,
    header_lines: Vec<String>,
    dlp_rules: &'a [EgressDlpRule],
    decision_log: EgressDecisionLog,
}

impl ProxyRequestEnforcementPlan<'_> {
    pub(crate) fn requires_dlp(&self) -> bool {
        !self.dlp_rules.is_empty()
    }

    /// The PEP's whole-body read budget for this request: the tightest rule
    /// cap, clamped by the platform-owned [`MAX_DLP_INSPECTION_BYTES`]. The
    /// clamp is belt-and-suspenders — policy validation already rejects larger
    /// rule values — so a policy that bypassed validation can still never
    /// steer the PEP into unbounded buffering.
    pub(crate) fn dlp_max_inspection_bytes(&self) -> Option<usize> {
        self.dlp_rules
            .iter()
            .map(|rule| rule.max_inspection_bytes)
            .min()
            .map(|max| max.min(nimbus_egress::MAX_DLP_INSPECTION_BYTES))
    }

    pub(crate) fn finish(
        self,
        inspected_body: Option<Vec<u8>>,
    ) -> std::result::Result<PreparedProxyRequest, HttpProxyResponse> {
        let inspected_body = enforce_dlp_rules(self.parsed, self.dlp_rules, inspected_body)?;
        Ok(PreparedProxyRequest {
            header_lines: self.header_lines,
            inspected_body,
            decision_log: self.decision_log,
        })
    }
}

pub(crate) fn prepare_proxy_request_enforcement<'a>(
    parsed: &'a ParsedProxyRequest,
    context: ProxyRequestEnforcementContext<'a>,
) -> std::result::Result<ProxyRequestEnforcementPlan<'a>, HttpProxyResponse> {
    let Some(rule) = context.matched_rule.and_then(|name| {
        context
            .policy
            .policy()
            .rules()
            .iter()
            .find(|rule| rule.name == name)
    }) else {
        return Ok(ProxyRequestEnforcementPlan {
            parsed,
            header_lines: parsed.header_lines.clone(),
            dlp_rules: &[],
            decision_log: EgressDecisionLog::allowed(
                context.request_id,
                parsed,
                None,
                context.reason.to_owned(),
                context.matched_rule.map(ToOwned::to_owned),
            ),
        });
    };

    let mut header_lines = parsed.header_lines.clone();
    context
        .phase_recorder
        .record(EgressProxyRequestPhase::CredentialHeaderMutation);
    let credential_identity =
        apply_credential_injection(rule, &mut header_lines, parsed, context.credential_provider)?;
    context
        .phase_recorder
        .record(EgressProxyRequestPhase::BoundedDlpInspection);
    let decision_log = EgressDecisionLog::allowed(
        context.request_id,
        parsed,
        credential_identity.clone(),
        context.reason.to_owned(),
        context.matched_rule.map(ToOwned::to_owned),
    );
    Ok(ProxyRequestEnforcementPlan {
        parsed,
        header_lines,
        dlp_rules: &rule.dlp,
        decision_log,
    })
}

pub(crate) fn reject_unapproved_caller_credentials_for_rule(
    policy: &CompiledEgressPolicy,
    matched_rule: Option<&str>,
    header_lines: &[String],
) -> std::result::Result<(), HttpProxyResponse> {
    let credential = matched_rule
        .and_then(|name| {
            policy
                .policy()
                .rules()
                .iter()
                .find(|rule| rule.name == name)
        })
        .and_then(|rule| rule.credential.as_ref());
    deny_unapproved_credential_headers(header_lines, credential)
}

fn apply_credential_injection(
    rule: &EgressRule,
    header_lines: &mut Vec<String>,
    parsed: &ParsedProxyRequest,
    credential_provider: &dyn crate::credentials::CredentialSecretProvider,
) -> std::result::Result<Option<String>, HttpProxyResponse> {
    deny_unapproved_credential_headers(header_lines, rule.credential.as_ref())?;
    let Some(credential) = &rule.credential else {
        return Ok(None);
    };
    if matches!(parsed.mode, ProxyRequestMode::ConnectTunnel) {
        return Err(HttpProxyResponse::forbidden(
            "credential injection is unavailable for CONNECT tunnels",
        ));
    }
    if header_lines
        .iter()
        .any(|line| header_name_matches(line, &credential.header_name))
    {
        if credential.allow_caller_header {
            return Ok(Some(credential.credential_ref.clone()));
        }
        return Err(HttpProxyResponse::forbidden(
            "credential-bearing caller header denied by egress policy",
        ));
    }
    let Some(secret) = credential_provider.resolve_credential_secret(&credential.credential_ref)
    else {
        return Err(HttpProxyResponse::forbidden(
            "credential injection failed closed: credential material is unavailable",
        ));
    };
    header_lines.push(format!(
        "{}: {}{}",
        credential.header_name,
        credential.value_prefix.as_deref().unwrap_or_default(),
        secret
    ));
    Ok(Some(credential.credential_ref.clone()))
}

fn deny_unapproved_credential_headers(
    header_lines: &[String],
    credential: Option<&EgressCredentialInjection>,
) -> std::result::Result<(), HttpProxyResponse> {
    for line in header_lines {
        let Some((name, _)) = line.split_once(':') else {
            continue;
        };
        let name = name.trim();
        let approved = credential.is_some_and(|credential| {
            credential.allow_caller_header && name.eq_ignore_ascii_case(&credential.header_name)
        });
        if !approved
            && (name.eq_ignore_ascii_case("authorization")
                || name.eq_ignore_ascii_case("proxy-authorization")
                || name.eq_ignore_ascii_case("cookie")
                || credential
                    .is_some_and(|credential| name.eq_ignore_ascii_case(&credential.header_name)))
        {
            return Err(HttpProxyResponse::forbidden(
                "credential-bearing caller header denied by egress policy",
            ));
        }
    }
    Ok(())
}

fn enforce_dlp_rules(
    parsed: &ParsedProxyRequest,
    dlp_rules: &[EgressDlpRule],
    inspected_body: Option<Vec<u8>>,
) -> std::result::Result<Option<Vec<u8>>, HttpProxyResponse> {
    if dlp_rules.is_empty() {
        return Ok(None);
    }
    if matches!(parsed.mode, ProxyRequestMode::ConnectTunnel) {
        return Err(HttpProxyResponse::forbidden(
            "DLP inspection input unavailable for CONNECT tunnels",
        ));
    }
    let content_length = parsed.content_length.ok_or_else(|| {
        HttpProxyResponse::forbidden("DLP inspection input unavailable: missing Content-Length")
    })?;
    if content_length > nimbus_egress::MAX_DLP_INSPECTION_BYTES {
        return Err(HttpProxyResponse::forbidden(
            "DLP inspection input exceeds the proxy inspection cap",
        ));
    }
    if dlp_rules
        .iter()
        .any(|rule| content_length > rule.max_inspection_bytes)
    {
        return Err(HttpProxyResponse::forbidden(
            "DLP inspection input truncated by max_inspection_bytes",
        ));
    }
    let Some(body) = inspected_body else {
        return Err(HttpProxyResponse::forbidden(
            "DLP inspection input unavailable while reading body",
        ));
    };
    for rule in dlp_rules {
        if contains_bytes(&body, rule.pattern.as_bytes()) {
            return Err(HttpProxyResponse::forbidden(&format!(
                "DLP rule `{}` blocked request body",
                rule.name
            )));
        }
    }
    Ok(Some(body))
}

fn contains_bytes(haystack: &[u8], needle: &[u8]) -> bool {
    !needle.is_empty()
        && haystack
            .windows(needle.len())
            .any(|window| window == needle)
}

fn header_name_matches(line: &str, header_name: &str) -> bool {
    line.split_once(':')
        .map(|(name, _)| name.trim().eq_ignore_ascii_case(header_name))
        .unwrap_or(false)
}
