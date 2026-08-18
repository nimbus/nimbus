use std::io;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

use nimbus::Error;
use nimbus_operator::{LOCAL_ADMIN_HEADER_NAME, LocalServerPaths, load_local_admin_token};
use nimbus_server::read_live_server_discovery;
use reqwest::{Method, StatusCode};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Deserialize)]
pub(crate) struct MintedLaunchTicket {
    pub(crate) url: String,
}

#[derive(Clone)]
pub(crate) struct LocalServerHttpClient {
    client: reqwest::Client,
    base_url: String,
    token: String,
}

impl LocalServerHttpClient {
    pub(crate) fn discover(
        paths: &LocalServerPaths,
        client: reqwest::Client,
    ) -> Result<Option<Self>, Error> {
        let Some(base_url) = discover_local_server_base_url(paths)? else {
            return Ok(None);
        };
        let token = load_local_admin_token(paths)
            .map_err(|error| {
                Error::PermissionDenied(format!("local admin token unavailable: {error}"))
            })?
            .token;
        Ok(Some(Self {
            client,
            base_url,
            token,
        }))
    }

    pub(crate) fn base_url(&self) -> &str {
        &self.base_url
    }

    pub(crate) async fn post_empty<T>(&self, path: &str) -> Result<T, Error>
    where
        T: DeserializeOwned,
    {
        self.request(Method::POST, path, Option::<&()>::None).await
    }

    pub(crate) async fn mint_ui_launch_ticket(&self) -> Result<MintedLaunchTicket, Error> {
        self.post_empty::<MintedLaunchTicket>("/ui/auth/launch-ticket")
            .await
    }

    pub(crate) async fn post_json<T, B>(&self, path: &str, body: &B) -> Result<T, Error>
    where
        T: DeserializeOwned,
        B: Serialize + ?Sized,
    {
        self.request(Method::POST, path, Some(body)).await
    }

    pub(crate) async fn patch_json<T, B>(&self, path: &str, body: &B) -> Result<T, Error>
    where
        T: DeserializeOwned,
        B: Serialize + ?Sized,
    {
        self.request(Method::PATCH, path, Some(body)).await
    }

    pub(crate) async fn delete_empty<T>(&self, path: &str) -> Result<T, Error>
    where
        T: DeserializeOwned,
    {
        self.request(Method::DELETE, path, Option::<&()>::None)
            .await
    }

    async fn request<T, B>(&self, method: Method, path: &str, body: Option<&B>) -> Result<T, Error>
    where
        T: DeserializeOwned,
        B: Serialize + ?Sized,
    {
        let mut request = self.request_builder(method, path);
        if let Some(body) = body {
            request = request.json(body);
        }
        let response = request.send().await.map_err(|error| {
            Error::Internal(format!(
                "failed to call running local Nimbus server: {error}"
            ))
        })?;
        decode_response(response).await
    }

    fn request_builder(&self, method: Method, path: &str) -> reqwest::RequestBuilder {
        let path = path.trim_start_matches('/');
        self.client
            .request(method, format!("{}/{path}", self.base_url))
            .header(LOCAL_ADMIN_HEADER_NAME, &self.token)
    }
}

pub(crate) fn discover_local_server_base_url(
    paths: &LocalServerPaths,
) -> Result<Option<String>, Error> {
    let Some(discovery) = read_live_server_discovery(paths).map_err(|error| {
        Error::Internal(format!("failed to inspect local server discovery: {error}"))
    })?
    else {
        return Ok(None);
    };
    let connect_address =
        normalize_loopback_connect_address(&discovery.address).map_err(|error| {
            Error::Internal(format!(
                "failed to normalize local server discovery address: {error}"
            ))
        })?;
    Ok(Some(format!("http://{connect_address}")))
}

async fn decode_response<T>(response: reqwest::Response) -> Result<T, Error>
where
    T: DeserializeOwned,
{
    let status = response.status();
    let bytes = response.bytes().await.map_err(|error| {
        Error::Internal(format!(
            "failed to read local server response body: {error}"
        ))
    })?;
    if status.is_success() {
        return serde_json::from_slice(&bytes).map_err(|error| {
            Error::Internal(format!("failed to parse local server JSON: {error}"))
        });
    }
    let message = local_server_error_message(&bytes)
        .unwrap_or_else(|| String::from_utf8_lossy(&bytes).trim().to_owned());
    let message = if message.is_empty() {
        format!("local server returned HTTP {status}")
    } else {
        format!("local server returned HTTP {status}: {message}")
    };
    Err(http_status_error(status, message))
}

fn local_server_error_message(bytes: &[u8]) -> Option<String> {
    let value = serde_json::from_slice::<Value>(bytes).ok()?;
    value
        .get("error")
        .and_then(|error| error.get("message"))
        .and_then(Value::as_str)
        .map(str::to_owned)
}

fn http_status_error(status: StatusCode, message: String) -> Error {
    match status {
        StatusCode::BAD_REQUEST | StatusCode::NOT_FOUND | StatusCode::UNPROCESSABLE_ENTITY => {
            Error::InvalidInput(message)
        }
        StatusCode::CONFLICT => Error::conflict(message),
        StatusCode::PRECONDITION_FAILED => Error::PreconditionFailed(message),
        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => Error::PermissionDenied(message),
        StatusCode::TOO_MANY_REQUESTS => Error::ResourceExhausted(message),
        StatusCode::SERVICE_UNAVAILABLE => Error::Internal(message),
        _ => Error::Internal(message),
    }
}

pub(crate) fn normalize_loopback_connect_address(address: &str) -> io::Result<String> {
    let parsed: SocketAddr = address.parse().map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("server discovery address {address:?} is invalid: {error}"),
        )
    })?;
    let normalized = match parsed.ip() {
        IpAddr::V4(ip) if ip.is_unspecified() => {
            SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), parsed.port())
        }
        IpAddr::V6(ip) if ip.is_unspecified() => {
            SocketAddr::new(IpAddr::V6(Ipv6Addr::LOCALHOST), parsed.port())
        }
        _ => parsed,
    };
    Ok(normalized.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_wildcard_discovery_addresses_to_loopback() {
        let normalized =
            normalize_loopback_connect_address("0.0.0.0:3210").expect("address should normalize");
        assert_eq!(normalized, "127.0.0.1:3210");

        let ipv6 = normalize_loopback_connect_address("[::]:3210")
            .expect("ipv6 wildcard address should normalize");
        assert_eq!(ipv6, "[::1]:3210");
    }

    #[test]
    fn authenticated_local_requests_use_only_the_dedicated_admin_header() {
        let client = LocalServerHttpClient {
            client: reqwest::Client::new(),
            base_url: "http://127.0.0.1:3210".to_owned(),
            token: "local-admin-token".to_owned(),
        };

        let request = client
            .request_builder(Method::POST, "/api/tenants")
            .build()
            .expect("local request should build");

        assert_eq!(
            request
                .headers()
                .get(LOCAL_ADMIN_HEADER_NAME)
                .and_then(|value| value.to_str().ok()),
            Some("local-admin-token")
        );
        assert!(
            !request
                .headers()
                .contains_key(reqwest::header::AUTHORIZATION),
            "the host-admin credential must not occupy application Authorization"
        );
    }
}
