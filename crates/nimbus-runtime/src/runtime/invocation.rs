use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::{NimbusRuntimeError, Result};
use crate::limits::RuntimeGuestSemantics;

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

impl InvocationRequest {
    pub(crate) fn runtime_invoke_expression(
        &self,
        module_specifier: Option<&str>,
        guest_semantics: RuntimeGuestSemantics,
    ) -> Result<String> {
        let request_json = serde_json::to_string(self)?;
        match self.kind {
            InvocationKind::CloudflareWorkerFetch => {
                let module_specifier = module_specifier.ok_or_else(|| {
                    NimbusRuntimeError::Contract(
                        "Cloudflare Worker fetch invocation requires a loaded runtime bundle"
                            .to_string(),
                    )
                })?;
                let module_specifier_json = serde_json::to_string(module_specifier)?;
                Ok(format!(
                    "globalThis.__nimbusInvokeCloudflareWorkerFetch(import({module_specifier_json}), {request_json})"
                ))
            }
            // The prelude reconfigures the guest-semantics surface (frozen
            // clock / seeded PRNG) for this invocation. It is emitted only on
            // ConvexDefault lanes: Host-semantics lanes never call the hook at
            // all, so a Host-lane bundle gets no per-invocation host callback
            // (the hook global itself is also host-frozen). The comma
            // expression still evaluates to the __nimbusInvoke result.
            _ if guest_semantics == RuntimeGuestSemantics::ConvexDefault => Ok(format!(
                "(globalThis.__nimbusBeginGuestInvocation?.(), globalThis.__nimbusInvoke({request_json}))"
            )),
            _ => Ok(format!("globalThis.__nimbusInvoke({request_json})")),
        }
    }
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
