use std::fmt;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use url::Url;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EgressProtocol {
    Http,
    Https,
    Tcp,
    Udp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EgressSubstrate {
    Isolate,
    Wasm,
    Container,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EgressRequest {
    pub substrate: EgressSubstrate,
    pub protocol: EgressProtocol,
    pub method: Option<String>,
    pub url: Option<String>,
    pub host: String,
    pub port: u16,
    pub path_and_query: Option<String>,
    pub tenant_label: Option<String>,
    pub session_id: Option<String>,
    pub invocation_id: Option<u64>,
    pub uses_custom_client: bool,
}

impl EgressRequest {
    pub fn from_fetch_url(
        method: impl Into<String>,
        url: &str,
    ) -> std::result::Result<Self, EgressRequestError> {
        Self::from_fetch_url_with_context(method, url, false, None, None, None)
    }

    pub(crate) fn from_fetch_url_with_context(
        method: impl Into<String>,
        url: &str,
        uses_custom_client: bool,
        tenant_label: Option<String>,
        session_id: Option<String>,
        invocation_id: Option<u64>,
    ) -> std::result::Result<Self, EgressRequestError> {
        let parsed = Url::parse(url).map_err(|source| EgressRequestError {
            message: format!("invalid fetch URL: {source}"),
        })?;
        let protocol = match parsed.scheme() {
            "http" => EgressProtocol::Http,
            "https" => EgressProtocol::Https,
            scheme => {
                return Err(EgressRequestError {
                    message: format!("unsupported fetch egress scheme `{scheme}`"),
                });
            }
        };
        if !parsed.username().is_empty() || parsed.password().is_some() {
            return Err(EgressRequestError {
                message: "fetch egress URL must not contain userinfo".to_string(),
            });
        }
        let host = parsed
            .host_str()
            .ok_or_else(|| EgressRequestError {
                message: "fetch egress URL must contain a host".to_string(),
            })?
            .to_string();
        let port = parsed
            .port_or_known_default()
            .ok_or_else(|| EgressRequestError {
                message: "fetch egress URL must contain a known port".to_string(),
            })?;
        let path_and_query = match parsed.query() {
            Some(query) => format!("{}?{query}", parsed.path()),
            None => parsed.path().to_string(),
        };
        Ok(Self {
            substrate: EgressSubstrate::Isolate,
            protocol,
            method: Some(method.into()),
            url: Some(parsed.to_string()),
            host,
            port,
            path_and_query: Some(path_and_query),
            tenant_label,
            session_id,
            invocation_id,
            uses_custom_client,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EgressRequestError {
    message: String,
}

impl fmt::Display for EgressRequestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for EgressRequestError {}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EgressAuthorization {
    allowed: bool,
    reason: String,
    matched_rule: Option<String>,
    proxy_url: Option<String>,
}

impl EgressAuthorization {
    pub fn allow(reason: impl Into<String>) -> Self {
        Self {
            allowed: true,
            reason: reason.into(),
            matched_rule: None,
            proxy_url: None,
        }
    }

    pub fn allow_with_proxy(reason: impl Into<String>, proxy_url: impl Into<String>) -> Self {
        Self {
            allowed: true,
            reason: reason.into(),
            matched_rule: None,
            proxy_url: Some(proxy_url.into()),
        }
    }

    pub fn deny(reason: impl Into<String>) -> Self {
        Self {
            allowed: false,
            reason: reason.into(),
            matched_rule: None,
            proxy_url: None,
        }
    }

    pub fn with_matched_rule(mut self, matched_rule: impl Into<String>) -> Self {
        self.matched_rule = Some(matched_rule.into());
        self
    }

    pub fn is_allowed(&self) -> bool {
        self.allowed
    }

    pub fn reason(&self) -> &str {
        &self.reason
    }

    pub fn matched_rule(&self) -> Option<&str> {
        self.matched_rule.as_deref()
    }

    pub fn proxy_url(&self) -> Option<&str> {
        self.proxy_url.as_deref()
    }
}

pub trait EgressGateway: Send + Sync + 'static {
    fn authorize(&self, request: &EgressRequest) -> EgressAuthorization;
}

#[derive(Debug, Default)]
pub struct DenyAllEgressGateway;

impl EgressGateway for DenyAllEgressGateway {
    fn authorize(&self, _request: &EgressRequest) -> EgressAuthorization {
        EgressAuthorization::deny("egress gateway denied by default")
    }
}

#[derive(Debug, Default)]
pub struct AllowAllEgressGateway;

impl EgressGateway for AllowAllEgressGateway {
    fn authorize(&self, _request: &EgressRequest) -> EgressAuthorization {
        EgressAuthorization::allow("egress gateway allowed by explicit test gateway")
    }
}

pub fn authorize_fetch_egress(
    gateway: Option<&dyn EgressGateway>,
    request: &EgressRequest,
) -> EgressAuthorization {
    match gateway {
        Some(gateway) => gateway.authorize(request),
        None => EgressAuthorization::deny("fetch egress gateway is not installed"),
    }
}

#[derive(Clone)]
pub(crate) enum RuntimeEgressGatewayBinding {
    CoarsePermissions,
    Gateway(Arc<dyn EgressGateway>),
    Missing,
}

impl RuntimeEgressGatewayBinding {
    pub(crate) fn coarse_permissions() -> Self {
        Self::CoarsePermissions
    }

    pub(crate) fn gateway(gateway: Arc<dyn EgressGateway>) -> Self {
        Self::Gateway(gateway)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use super::*;
    use serde_json::Value;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    #[test]
    fn fetch_egress_request_canonicalizes_authority_and_default_port() {
        let request =
            EgressRequest::from_fetch_url("GET", "https://Example.COM/api?q=secret").unwrap();

        assert_eq!(request.substrate, EgressSubstrate::Isolate);
        assert_eq!(request.protocol, EgressProtocol::Https);
        assert_eq!(request.method.as_deref(), Some("GET"));
        assert_eq!(request.host, "example.com");
        assert_eq!(request.port, 443);
        assert_eq!(request.path_and_query.as_deref(), Some("/api?q=secret"));
    }

    #[test]
    fn fetch_egress_request_rejects_non_http_schemes() {
        let error = EgressRequest::from_fetch_url("GET", "file:///tmp/data").unwrap_err();

        assert!(
            error
                .to_string()
                .contains("unsupported fetch egress scheme `file`")
        );
    }

    #[test]
    fn fetch_egress_request_rejects_userinfo_authority() {
        let error = EgressRequest::from_fetch_url("GET", "https://token@example.com").unwrap_err();

        assert!(error.to_string().contains("must not contain userinfo"));
    }

    #[test]
    fn missing_fetch_egress_gateway_fails_closed() {
        let request = EgressRequest::from_fetch_url("GET", "https://api.example.test/").unwrap();

        let authorization = authorize_fetch_egress(None, &request);

        assert!(!authorization.is_allowed());
        assert_eq!(
            authorization.reason(),
            "fetch egress gateway is not installed"
        );
    }

    #[test]
    fn fetch_egress_gateway_sees_request_and_can_allow() {
        #[derive(Default)]
        struct SpyGateway {
            seen: Mutex<Vec<EgressRequest>>,
        }

        impl EgressGateway for SpyGateway {
            fn authorize(&self, request: &EgressRequest) -> EgressAuthorization {
                self.seen
                    .lock()
                    .expect("spy gateway lock should not be poisoned")
                    .push(request.clone());
                EgressAuthorization::allow("allowed by spy")
            }
        }

        let gateway = Arc::new(SpyGateway::default());
        let request =
            EgressRequest::from_fetch_url("POST", "https://uploads.example.test/v1").unwrap();

        let authorization = authorize_fetch_egress(Some(gateway.as_ref()), &request);

        assert!(authorization.is_allowed());
        assert_eq!(authorization.reason(), "allowed by spy");
        let seen = gateway
            .seen
            .lock()
            .expect("spy gateway lock should not be poisoned");
        assert_eq!(seen.as_slice(), std::slice::from_ref(&request));
    }

    #[test]
    fn fetch_egress_gateway_can_deny_custom_clients() {
        struct DenyCustomClientGateway;

        impl EgressGateway for DenyCustomClientGateway {
            fn authorize(&self, request: &EgressRequest) -> EgressAuthorization {
                if request.uses_custom_client {
                    EgressAuthorization::deny("custom fetch clients must route through the PEP")
                } else {
                    EgressAuthorization::allow("standard fetch path")
                }
            }
        }

        let request = EgressRequest::from_fetch_url_with_context(
            "GET",
            "https://api.example.test/",
            true,
            None,
            None,
            None,
        )
        .unwrap();

        let authorization = authorize_fetch_egress(Some(&DenyCustomClientGateway), &request);

        assert!(!authorization.is_allowed());
        assert_eq!(
            authorization.reason(),
            "custom fetch clients must route through the PEP"
        );
    }

    #[tokio::test]
    async fn isolate_fetch_consults_egress_gateway_and_raw_net_remains_denied() {
        #[derive(Default)]
        struct NoopHost;

        impl crate::HostBridge for NoopHost {
            fn call(&self, _request: crate::HostCallRequest) -> crate::Result<Value> {
                Ok(Value::Null)
            }
        }

        struct LocalFetchGateway {
            port: u16,
            seen: Mutex<Vec<EgressRequest>>,
        }

        impl EgressGateway for LocalFetchGateway {
            fn authorize(&self, request: &EgressRequest) -> EgressAuthorization {
                self.seen
                    .lock()
                    .expect("gateway spy lock should not be poisoned")
                    .push(request.clone());
                if request.host == "127.0.0.1"
                    && request.port == self.port
                    && !request.uses_custom_client
                {
                    EgressAuthorization::allow("local test gateway allow")
                } else {
                    EgressAuthorization::deny("local test gateway deny")
                }
            }
        }

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("test listener should bind");
        let port = listener
            .local_addr()
            .expect("listener address should resolve")
            .port();
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener
                .accept()
                .await
                .expect("fetch should connect to the local listener");
            let mut request_bytes = Vec::new();
            let mut buf = [0_u8; 1024];
            loop {
                let read = tokio::time::timeout(Duration::from_secs(5), socket.read(&mut buf))
                    .await
                    .expect("fetch should send an HTTP request before timeout")
                    .expect("server read should succeed");
                if read == 0 {
                    break;
                }
                request_bytes.extend_from_slice(&buf[..read]);
                if request_bytes.windows(4).any(|window| window == b"\r\n\r\n") {
                    break;
                }
            }
            let request_text = String::from_utf8_lossy(&request_bytes);
            assert!(
                request_text.starts_with("GET /allowed?secret=value HTTP/1.1"),
                "unexpected request received by test server: {request_text}"
            );
            socket
                .write_all(
                    b"HTTP/1.1 200 OK\r\ncontent-type: text/plain\r\ncontent-length: 2\r\nconnection: close\r\n\r\nok",
                )
                .await
                .expect("server response should write");
        });

        let tempdir = tempfile::tempdir().expect("tempdir should build");
        let bundle_path = tempdir.path().join("bundle.mjs");
        std::fs::write(
            &bundle_path,
            format!(
                r#"
globalThis.__nimbusInvoke = async function () {{
  let rawNet = "not-attempted";
  try {{
    const net = await import("node:net");
    rawNet = await new Promise((resolve) => {{
      const socket = net.connect({{ host: "127.0.0.1", port: {port} }}, () => {{
        socket.destroy();
        resolve("connected");
      }});
      socket.once("error", (error) => {{
        resolve(String((error && error.message) || error));
      }});
      setTimeout(() => {{
        socket.destroy();
        resolve("timeout");
      }}, 1000);
    }});
  }} catch (error) {{
    rawNet = String((error && error.message) || error);
  }}
  const response = await fetch("http://127.0.0.1:{port}/allowed?secret=value");
  return {{
    rawNet,
    status: response.status,
    body: await response.text(),
  }};
}};

export {{}};
"#
            ),
        )
        .expect("bundle should write");

        let gateway = Arc::new(LocalFetchGateway {
            port,
            seen: Mutex::default(),
        });
        let runtime = crate::NimbusRuntime::with_policy(
            Arc::new(NoopHost),
            Arc::new(crate::RuntimePolicy::new(
                crate::RuntimeLimits::application_node22(),
            )),
        )
        .with_egress_gateway(gateway.clone());
        let request = crate::InvocationRequest {
            kind: crate::InvocationKind::Query,
            function_name: "messages:egressFetch".to_string(),
            args: Value::Null,
            page_size: None,
            cursor: None,
            auth: None,
            services: Default::default(),
        };

        let result = runtime
            .invoke_bundle(&crate::RuntimeBundle::new(&bundle_path), &request)
            .await
            .expect("bundle fetch should execute through the egress gateway");

        tokio::time::timeout(Duration::from_secs(5), server)
            .await
            .expect("server should finish after fetch")
            .expect("server task should not panic");
        assert_eq!(result["status"], 200);
        assert_eq!(result["body"], "ok");
        let raw_net = result["rawNet"]
            .as_str()
            .expect("raw net result should be a string");
        assert_ne!(raw_net, "connected");
        assert!(
            raw_net.contains("Requires net access") || raw_net.contains("permission"),
            "raw Deno.connect should fail closed on net permission, got: {raw_net}"
        );
        let seen = gateway
            .seen
            .lock()
            .expect("gateway spy lock should not be poisoned");
        assert_eq!(seen.len(), 1);
        let fetch_request = &seen[0];
        assert_eq!(fetch_request.substrate, EgressSubstrate::Isolate);
        assert_eq!(fetch_request.protocol, EgressProtocol::Http);
        assert_eq!(fetch_request.method.as_deref(), Some("GET"));
        assert_eq!(fetch_request.host, "127.0.0.1");
        assert_eq!(fetch_request.port, port);
        assert_eq!(
            fetch_request.path_and_query.as_deref(),
            Some("/allowed?secret=value")
        );
        assert!(!fetch_request.uses_custom_client);
    }
}
