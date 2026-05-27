use nimbus_core::{PrincipalContext, Result};
use serde::{Deserialize, Serialize};

use super::context::{TenantIsolationContext, principal_tenant_claim};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum TenantIsolationAuthority {
    Operator,
    Application { principal: PrincipalContext },
    System,
}

impl TenantIsolationAuthority {
    pub(super) fn describe(&self) -> String {
        match self {
            Self::Operator => "operator".to_string(),
            Self::System => "system".to_string(),
            Self::Application { principal } if principal.authenticated => {
                "application(authenticated)".to_string()
            }
            Self::Application { .. } => "application(anonymous)".to_string(),
        }
    }

    #[cfg(test)]
    pub(super) fn is_system_or_operator(&self) -> bool {
        matches!(self, Self::Operator | Self::System)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TenantIsolationMode {
    LocalDevelopment,
    #[default]
    Production,
}

impl TenantIsolationMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::LocalDevelopment => "local-development",
            Self::Production => "production",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum TenantIsolationAuthorityDecision {
    Operator,
    Application {
        authenticated: bool,
        principal_snapshot_digest: String,
        tenant_claim_name: Option<&'static str>,
    },
    System,
}

impl TenantIsolationAuthorityDecision {
    pub(super) fn from_context(context: &TenantIsolationContext) -> Result<Self> {
        match &context.authority {
            TenantIsolationAuthority::Operator => Ok(Self::Operator),
            TenantIsolationAuthority::System => Ok(Self::System),
            TenantIsolationAuthority::Application { principal } => {
                let snapshot = principal.snapshot()?;
                Ok(Self::Application {
                    authenticated: principal.authenticated,
                    principal_snapshot_digest: snapshot.digest,
                    tenant_claim_name: principal_tenant_claim(principal).map(|claim| claim.name),
                })
            }
        }
    }

    pub fn class(&self) -> &'static str {
        match self {
            Self::Operator => "operator",
            Self::Application { .. } => "application",
            Self::System => "system",
        }
    }
}
