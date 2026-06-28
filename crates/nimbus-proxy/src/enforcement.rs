use std::io::Read;
use std::net::TcpStream;

use nimbus_egress::{CompiledEgressPolicy, EgressCredentialInjection, EgressDlpRule, EgressRule};

use crate::credentials::CredentialSecretStore;
use crate::decision_log::EgressDecisionLog;
use crate::request::{ParsedProxyRequest, PreparedProxyRequest, ProxyRequestMode};
use crate::response::HttpProxyResponse;

pub(crate) fn prepare_proxy_request_enforcement(
    client: &mut TcpStream,
    buffer: &mut Vec<u8>,
    parsed: &ParsedProxyRequest,
    policy: &CompiledEgressPolicy,
    matched_rule: Option<&str>,
    credential_store: &CredentialSecretStore,
) -> std::result::Result<PreparedProxyRequest, HttpProxyResponse> {
    let Some(rule) = matched_rule.and_then(|name| {
        policy
            .policy()
            .rules()
            .iter()
            .find(|rule| rule.name == name)
    }) else {
        return Ok(PreparedProxyRequest {
            header_lines: parsed.header_lines.clone(),
            inspected_body: None,
            decision_log: EgressDecisionLog::for_request(parsed, None),
        });
    };
    let mut header_lines = parsed.header_lines.clone();
    let credential_identity =
        apply_credential_injection(rule, &mut header_lines, parsed, credential_store)?;
    let inspected_body = enforce_dlp_rules(client, buffer, parsed, &rule.dlp)?;
    let decision_log = EgressDecisionLog::for_request(parsed, credential_identity.clone());
    Ok(PreparedProxyRequest {
        header_lines,
        inspected_body,
        decision_log,
    })
}

fn apply_credential_injection(
    rule: &EgressRule,
    header_lines: &mut Vec<String>,
    parsed: &ParsedProxyRequest,
    credential_store: &CredentialSecretStore,
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
    let Some(secret) = credential_store.get(&credential.credential_ref) else {
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
                || name.eq_ignore_ascii_case("cookie"))
        {
            return Err(HttpProxyResponse::forbidden(
                "credential-bearing caller header denied by egress policy",
            ));
        }
    }
    Ok(())
}

fn enforce_dlp_rules(
    client: &mut TcpStream,
    buffer: &mut Vec<u8>,
    parsed: &ParsedProxyRequest,
    dlp_rules: &[EgressDlpRule],
) -> std::result::Result<Option<Vec<u8>>, HttpProxyResponse> {
    if dlp_rules.is_empty() {
        return Ok(None);
    }
    if matches!(parsed.mode, ProxyRequestMode::ConnectTunnel) {
        return Err(HttpProxyResponse::forbidden(
            "DLP inspection input unavailable for CONNECT tunnels",
        ));
    }
    let content_length = content_length(&parsed.header_lines).ok_or_else(|| {
        HttpProxyResponse::forbidden("DLP inspection input unavailable: missing Content-Length")
    })??;
    if dlp_rules
        .iter()
        .any(|rule| content_length > rule.max_inspection_bytes)
    {
        return Err(HttpProxyResponse::forbidden(
            "DLP inspection input truncated by max_inspection_bytes",
        ));
    }
    let body = read_exact_request_body(client, buffer, parsed.body_offset, content_length)?;
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

fn content_length(
    header_lines: &[String],
) -> Option<std::result::Result<usize, HttpProxyResponse>> {
    header_lines.iter().find_map(|line| {
        let (name, value) = line.split_once(':')?;
        name.trim().eq_ignore_ascii_case("content-length").then(|| {
            value.trim().parse::<usize>().map_err(|_| {
                HttpProxyResponse::bad_request("Content-Length must be a non-negative integer")
            })
        })
    })
}

fn read_exact_request_body(
    client: &mut TcpStream,
    buffer: &mut Vec<u8>,
    body_offset: usize,
    content_length: usize,
) -> std::result::Result<Vec<u8>, HttpProxyResponse> {
    while buffer.len().saturating_sub(body_offset) < content_length {
        let mut chunk = [0_u8; 1024];
        let read = client.read(&mut chunk).map_err(|_| {
            HttpProxyResponse::forbidden("DLP inspection input unavailable while reading body")
        })?;
        if read == 0 {
            return Err(HttpProxyResponse::forbidden(
                "DLP inspection input unavailable: client closed early",
            ));
        }
        buffer.extend_from_slice(&chunk[..read]);
    }
    let mut body = buffer[body_offset..].to_vec();
    body.truncate(content_length);
    Ok(body)
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
