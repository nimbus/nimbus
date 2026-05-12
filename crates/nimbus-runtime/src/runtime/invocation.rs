use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InvocationKind {
    Query,
    PaginatedQuery,
    Mutation,
    Action,
}

impl InvocationKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Query => "query",
            Self::PaginatedQuery => "paginated_query",
            Self::Mutation => "mutation",
            Self::Action => "action",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InvocationRequest {
    pub kind: InvocationKind,
    pub function_name: String,
    #[serde(default)]
    pub args: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub page_size: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auth: Option<InvocationAuth>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub services: InvocationServices,
}

pub type InvocationServices = BTreeMap<String, InvocationServiceBinding>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InvocationServiceProtocol {
    Tcp,
    Http,
    Https,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InvocationServiceBinding {
    pub host: String,
    pub port: u16,
    pub protocol: InvocationServiceProtocol,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub endpoints: BTreeMap<String, InvocationServiceEndpoint>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InvocationServiceEndpoint {
    pub host: String,
    pub port: u16,
    pub protocol: InvocationServiceProtocol,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeUserIdentity {
    pub token_identifier: String,
    pub subject: String,
    pub issuer: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub given_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub family_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nickname: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preferred_username: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub picture_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub email_verified: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gender: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub birthday: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timezone: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub phone_number: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub phone_number_verified: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub address: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
    #[serde(flatten)]
    pub custom_claims: Map<String, Value>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VerifiedUserIdentityKind {
    Oidc,
    CustomJwt,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VerifiedUserIdentity {
    pub kind: VerifiedUserIdentityKind,
    pub token_identifier: String,
    pub subject: String,
    pub issuer: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub given_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub family_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nickname: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preferred_username: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub picture_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub email_verified: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gender: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub birthday: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timezone: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub phone_number: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub phone_number_verified: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub address: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
    #[serde(flatten)]
    pub custom_claims: Map<String, Value>,
}

impl VerifiedUserIdentity {
    pub fn token_identifier(&self) -> &str {
        &self.token_identifier
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct InvocationAuth {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub identity: Option<RuntimeUserIdentity>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verified_identity: Option<VerifiedUserIdentity>,
    #[serde(default)]
    pub throw_on_missing_identity: bool,
}

impl InvocationAuth {
    pub fn with_identities(
        identity: RuntimeUserIdentity,
        verified_identity: VerifiedUserIdentity,
        throw_on_missing_identity: bool,
    ) -> Self {
        Self {
            identity: Some(identity),
            verified_identity: Some(verified_identity),
            throw_on_missing_identity,
        }
    }

    pub fn token_identifier(&self) -> Option<&str> {
        self.verified_identity
            .as_ref()
            .map(VerifiedUserIdentity::token_identifier)
            .or_else(|| {
                self.identity
                    .as_ref()
                    .map(|identity| identity.token_identifier.as_str())
            })
    }
}
