use crate::IdentitySourceKind;

/// Trust-domain and mode configuration for workload identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdentityTrustConfig {
    trust_domain: String,
    mode: TrustMode,
}

impl IdentityTrustConfig {
    pub fn local_dev(trust_domain: impl AsRef<str>) -> Result<Self, TrustConfigError> {
        Self::new(trust_domain, TrustMode::LocalDev)
    }

    pub fn production(trust_domain: impl AsRef<str>) -> Result<Self, TrustConfigError> {
        Self::new(trust_domain, TrustMode::Production)
    }

    pub fn trust_domain(&self) -> &str {
        &self.trust_domain
    }

    pub fn mode(&self) -> TrustMode {
        self.mode
    }

    pub fn admit_source(&self, source: &IdentitySourceKind) -> Result<(), TrustConfigError> {
        match (self.mode, source) {
            (TrustMode::LocalDev, IdentitySourceKind::LocalDev)
            | (TrustMode::Production, IdentitySourceKind::ClusterMembership(_)) => Ok(()),
            (mode, identity_source) => Err(TrustConfigError::SourceNotAdmitted {
                mode,
                identity_source: identity_source.clone(),
            }),
        }
    }

    fn new(trust_domain: impl AsRef<str>, mode: TrustMode) -> Result<Self, TrustConfigError> {
        Ok(Self {
            trust_domain: validate_trust_domain(trust_domain.as_ref())?.to_string(),
            mode,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrustMode {
    LocalDev,
    Production,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum TrustConfigError {
    #[error("identity trust domain cannot be empty")]
    EmptyTrustDomain,
    #[error(
        "identity trust domain `{trust_domain}` must contain only lowercase ASCII letters, digits, '.', '-', or '_'"
    )]
    InvalidTrustDomain { trust_domain: String },
    #[error("workload subject `{subject}` cannot be rendered as a SPIFFE workload identity")]
    InvalidWorkloadSubject { subject: String },
    #[error("identity source {identity_source:?} is not admitted in {mode:?} trust mode")]
    SourceNotAdmitted {
        mode: TrustMode,
        identity_source: IdentitySourceKind,
    },
}

fn validate_trust_domain(trust_domain: &str) -> Result<&str, TrustConfigError> {
    // Mirrors nimbus-tenant::identity::validate_spiffe_trust_domain locally so
    // workload identity does not import tenant-private validation internals.
    if trust_domain.is_empty() {
        return Err(TrustConfigError::EmptyTrustDomain);
    }
    if !trust_domain
        .bytes()
        .all(|byte| matches!(byte, b'a'..=b'z' | b'0'..=b'9' | b'.' | b'-' | b'_'))
    {
        return Err(TrustConfigError::InvalidTrustDomain {
            trust_domain: trust_domain.to_string(),
        });
    }
    Ok(trust_domain)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trust_domain_validation_rejects_spiffe_domain_violations() {
        for trust_domain in [
            "",
            "   ",
            "https://example.com",
            "example.com/ns",
            "example com",
            " example.com ",
            "Example.COM",
            "example@com",
            "example:com",
            "examplé.com",
        ] {
            assert!(
                IdentityTrustConfig::local_dev(trust_domain).is_err(),
                "trust domain `{trust_domain}` should be rejected"
            );
        }

        let config =
            IdentityTrustConfig::production("example.com").expect("lowercase domain is valid");
        assert_eq!(config.trust_domain(), "example.com");
        assert_eq!(config.mode(), TrustMode::Production);

        for trust_domain in ["example-1.com", "example_1.test"] {
            assert_eq!(
                IdentityTrustConfig::local_dev(trust_domain)
                    .expect("SPIFFE charset domain should be valid")
                    .trust_domain(),
                trust_domain
            );
        }
    }

    #[test]
    fn production_rejects_local_dev_source_and_local_dev_admits_local_dev() {
        let production =
            IdentityTrustConfig::production("example.com").expect("production config should build");
        assert_eq!(
            production.admit_source(&IdentitySourceKind::LocalDev),
            Err(TrustConfigError::SourceNotAdmitted {
                mode: TrustMode::Production,
                identity_source: IdentitySourceKind::LocalDev,
            })
        );

        let local_dev =
            IdentityTrustConfig::local_dev("example.test").expect("local dev config should build");
        assert_eq!(
            local_dev.admit_source(&IdentitySourceKind::LocalDev),
            Ok(())
        );
    }
}
