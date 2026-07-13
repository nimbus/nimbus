use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InvocationKind {
    Query,
    PaginatedQuery,
    Mutation,
    Action,
    CloudflareWorkerFetch,
}

impl InvocationKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Query => "query",
            Self::PaginatedQuery => "paginated_query",
            Self::Mutation => "mutation",
            Self::Action => "action",
            Self::CloudflareWorkerFetch => "cloudflare_worker_fetch",
        }
    }

    pub(crate) const fn is_convex_read_semantic_candidate(&self) -> bool {
        matches!(self, Self::Query | Self::PaginatedQuery)
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
    pub auth: Option<Value>,
    #[serde(default, skip_serializing)]
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
