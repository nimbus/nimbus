use std::time::Duration;

const WORKLOAD_SUBJECT_EXACT_PREFIX: &str = "nimbus-workload:v1/";
const WORKLOAD_SUBJECT_PREFIX: &str = "nimbus-workload:v1";
const WORKLOAD_AUDIT_PREFIX: &str = "nimbus-workload-audit:";
const PLACEMENT_SEGMENTS: [(&str, &str); 4] = [
    ("/node/", "node"),
    ("/machine/", "machine"),
    ("/sandbox/", "sandbox"),
    ("/invocation/", "invocation"),
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderAuthPolicy {
    rules: Vec<ProviderAuthRule>,
}

impl ProviderAuthPolicy {
    /// Builds a fail-closed provider authorization policy.
    pub fn try_new(rules: Vec<ProviderAuthRule>) -> Result<Self, PolicyValidationError> {
        for rule in &rules {
            validate_rule(rule)?;
        }
        Ok(Self { rules })
    }

    /// Empty policy that denies every mint.
    pub fn deny_all() -> Self {
        Self { rules: Vec::new() }
    }

    pub(crate) fn rules(&self) -> &[ProviderAuthRule] {
        &self.rules
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderAuthRule {
    subject: SubjectMatch,
    audiences: Vec<String>,
    max_ttl: Duration,
}

impl ProviderAuthRule {
    pub fn new(
        subject: SubjectMatch,
        audiences: impl IntoIterator<Item = impl Into<String>>,
        max_ttl: Duration,
    ) -> Self {
        Self {
            subject,
            audiences: audiences.into_iter().map(Into::into).collect(),
            max_ttl,
        }
    }

    pub fn subject(&self) -> &SubjectMatch {
        &self.subject
    }

    pub fn audiences(&self) -> &[String] {
        &self.audiences
    }

    pub fn max_ttl(&self) -> Duration {
        self.max_ttl
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SubjectMatch {
    Exact(String),
    SegmentPrefix(String),
}

impl SubjectMatch {
    pub(crate) fn matches(&self, subject: &str) -> bool {
        match self {
            Self::Exact(expected) => expected == subject,
            Self::SegmentPrefix(prefix) => segment_prefix_matches(prefix, subject),
        }
    }

    fn as_str(&self) -> &str {
        match self {
            Self::Exact(subject) | Self::SegmentPrefix(subject) => subject,
        }
    }
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum PolicyValidationError {
    #[error("provider auth subject `{subject}` must use the stable workload subject prefix")]
    SubjectPrefixInvalid { subject: String },
    #[error("provider auth subject `{subject}` must not include placement segment `/{segment}/`")]
    PlacementSegmentForbidden {
        subject: String,
        segment: &'static str,
    },
    #[error("provider auth subject `{subject}` must not use the workload audit projection")]
    AuditProjectionSubject { subject: String },
    #[error("provider auth rule for `{subject}` must allow at least one audience")]
    EmptyAudiences { subject: String },
    #[error("provider auth rule for `{subject}` contains an empty audience")]
    EmptyAudience { subject: String },
    #[error("provider auth rule for `{subject}` must have a positive max TTL")]
    ZeroMaxTtl { subject: String },
}

fn validate_rule(rule: &ProviderAuthRule) -> Result<(), PolicyValidationError> {
    let subject = rule.subject.as_str();
    if subject.starts_with(WORKLOAD_AUDIT_PREFIX) {
        return Err(PolicyValidationError::AuditProjectionSubject {
            subject: subject.to_string(),
        });
    }
    match rule.subject() {
        SubjectMatch::Exact(_) if !subject.starts_with(WORKLOAD_SUBJECT_EXACT_PREFIX) => {
            return Err(PolicyValidationError::SubjectPrefixInvalid {
                subject: subject.to_string(),
            });
        }
        SubjectMatch::SegmentPrefix(_)
            if subject != WORKLOAD_SUBJECT_PREFIX
                && !subject.starts_with(WORKLOAD_SUBJECT_EXACT_PREFIX) =>
        {
            return Err(PolicyValidationError::SubjectPrefixInvalid {
                subject: subject.to_string(),
            });
        }
        _ => {}
    }
    for (needle, segment) in PLACEMENT_SEGMENTS {
        if subject.contains(needle) {
            return Err(PolicyValidationError::PlacementSegmentForbidden {
                subject: subject.to_string(),
                segment,
            });
        }
    }
    if rule.audiences.is_empty() {
        return Err(PolicyValidationError::EmptyAudiences {
            subject: subject.to_string(),
        });
    }
    if rule.audiences.iter().any(String::is_empty) {
        return Err(PolicyValidationError::EmptyAudience {
            subject: subject.to_string(),
        });
    }
    if rule.max_ttl.is_zero() {
        return Err(PolicyValidationError::ZeroMaxTtl {
            subject: subject.to_string(),
        });
    }
    Ok(())
}

fn segment_prefix_matches(prefix: &str, subject: &str) -> bool {
    let Some(suffix) = subject.strip_prefix(prefix) else {
        return false;
    };
    suffix.is_empty() || prefix.ends_with('/') || suffix.starts_with('/')
}
