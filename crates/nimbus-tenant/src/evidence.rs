pub const TENANT_ISOLATION_EVIDENCE_SCOPE: &str = "nimbus.tenant_isolation";
pub const NON_CANONICAL_REASON_CODE: &str = "non_canonical_reason_code";
pub const UNSPECIFIED_REASON_CODE: &str = "unspecified_reason_code";
pub const REDACTED_EVIDENCE_TEXT: &str = "[redacted evidence text]";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct CanonicalEvidenceCode {
    value: String,
    redacted: bool,
}

impl CanonicalEvidenceCode {
    #[cfg(test)]
    pub(super) fn value(&self) -> &str {
        &self.value
    }

    pub(super) fn into_value(self) -> String {
        self.value
    }

    pub(super) fn was_redacted(&self) -> bool {
        self.redacted
    }
}

pub(super) fn tenant_isolation_event_name(kind: &str, result: &str) -> String {
    format!("{TENANT_ISOLATION_EVIDENCE_SCOPE}.{kind}.{result}")
}

pub(super) fn canonical_evidence_reason_code(raw: impl AsRef<str>) -> CanonicalEvidenceCode {
    let trimmed = raw.as_ref().trim();
    if trimmed.is_empty() {
        return CanonicalEvidenceCode {
            value: UNSPECIFIED_REASON_CODE.to_owned(),
            redacted: true,
        };
    }
    if is_stable_evidence_code(trimmed) && !text_looks_sensitive(trimmed) {
        return CanonicalEvidenceCode {
            value: trimmed.to_owned(),
            redacted: false,
        };
    }
    CanonicalEvidenceCode {
        value: NON_CANONICAL_REASON_CODE.to_owned(),
        redacted: true,
    }
}

pub(super) fn is_stable_evidence_code(code: &str) -> bool {
    if code.len() > 96 {
        return false;
    }
    let mut previous_was_separator = false;
    for (index, byte) in code.bytes().enumerate() {
        let is_alnum = byte.is_ascii_lowercase() || byte.is_ascii_digit();
        let is_separator = byte == b'_';
        if !is_alnum && !is_separator {
            return false;
        }
        if index == 0 && !byte.is_ascii_lowercase() {
            return false;
        }
        if is_separator && previous_was_separator {
            return false;
        }
        previous_was_separator = is_separator;
    }
    !previous_was_separator
}

pub(super) fn redact_evidence_text(raw: impl AsRef<str>) -> String {
    let trimmed = raw.as_ref().trim();
    if trimmed.is_empty() {
        return UNSPECIFIED_REASON_CODE.to_owned();
    }
    if text_looks_sensitive(trimmed) {
        REDACTED_EVIDENCE_TEXT.to_owned()
    } else {
        trimmed.to_owned()
    }
}

fn text_looks_sensitive(text: &str) -> bool {
    let normalized = text.to_ascii_lowercase();
    normalized.contains("authorization:")
        || normalized.contains("bearer ")
        || normalized.contains("cookie:")
        || normalized.contains("set-cookie:")
        || normalized.contains("token=")
        || normalized.contains("token:")
        || normalized.contains("password=")
        || normalized.contains("password:")
        || normalized.contains("credential=")
        || normalized.contains("credential:")
        || normalized.contains("secret=")
        || normalized.contains("secret_handle")
        || normalized.contains("private key")
        || normalized.contains("begin private key")
        || normalized.contains("x-api-key")
        || normalized.contains("api_key=")
        || query_string_looks_sensitive(&normalized)
}

fn query_string_looks_sensitive(normalized: &str) -> bool {
    let Some((_, query)) = normalized.split_once('?') else {
        return false;
    };
    [
        "token",
        "password",
        "credential",
        "secret",
        "api_key",
        "key",
    ]
    .iter()
    .any(|name| query.contains(&format!("{name}=")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn evidence_codes_accept_only_stable_machine_names() {
        for code in [
            "policy_allowed",
            "secret_grant_denied",
            "tenant_cleanup_complete",
            "sandbox_manifest_tenant_mismatch",
        ] {
            let canonical = canonical_evidence_reason_code(code);
            assert_eq!(canonical.value(), code);
            assert!(!canonical.was_redacted());
        }

        for code in [
            "",
            "Policy Allowed",
            "policy-allowed",
            "policy__allowed",
            "_policy_allowed",
            "policy_allowed_",
            "bearer token=do-not-log",
        ] {
            let canonical = canonical_evidence_reason_code(code);
            assert!(canonical.was_redacted(), "{code:?}");
            assert_ne!(canonical.value(), code.trim());
        }
    }

    #[test]
    fn evidence_text_redacts_credentials_without_redacting_plain_reasons() {
        assert_eq!(
            redact_evidence_text("fixture policy denied"),
            "fixture policy denied"
        );
        assert_eq!(
            redact_evidence_text("secret grant denied"),
            "secret grant denied"
        );
        for text in [
            "Authorization: Bearer do-not-log",
            "https://example.test/path?token=do-not-log",
            "password=do-not-log",
            "BEGIN PRIVATE KEY do-not-log",
            "secret_handle=prod/db/password",
        ] {
            assert_eq!(redact_evidence_text(text), REDACTED_EVIDENCE_TEXT);
        }
    }
}
