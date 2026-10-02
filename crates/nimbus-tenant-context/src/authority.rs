use nimbus_core::PrincipalContext;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TenantIsolationAuthority {
    Operator,
    Application { principal: PrincipalContext },
    System,
}

impl TenantIsolationAuthority {
    pub fn describe(&self) -> String {
        match self {
            Self::Operator => "operator".to_string(),
            Self::System => "system".to_string(),
            Self::Application { principal } if principal.authenticated => {
                "application(authenticated)".to_string()
            }
            Self::Application { .. } => "application(anonymous)".to_string(),
        }
    }

    pub fn is_system_or_operator(&self) -> bool {
        matches!(self, Self::Operator | Self::System)
    }
}
