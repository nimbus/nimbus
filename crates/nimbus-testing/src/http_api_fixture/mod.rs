mod convex;
mod debug;
mod documents;
mod queries;
mod schedule;
mod schema;
mod tenants;

use reqwest::{Method, RequestBuilder};

use crate::ServerFixture;

pub struct HttpApiFixture<'a> {
    pub(super) server: &'a ServerFixture,
    /// When set, every application-Convex request (`convex_*`) carries this
    /// `Authorization` header value. Used by the #41 team-binding migration so
    /// the data-access tests reach the gate as a *verified* principal bound to
    /// the silo's team. Native `/api/tenants/...` requests never carry it.
    pub(super) convex_bearer: Option<String>,
}

impl<'a> HttpApiFixture<'a> {
    pub fn new(server: &'a ServerFixture) -> Self {
        Self {
            server,
            convex_bearer: None,
        }
    }

    /// A fixture whose application-Convex requests carry `bearer` as the
    /// `Authorization` header (e.g. `"Bearer <token>"`). The anonymous,
    /// no-bearer refusal half of a migrated test still uses `server.client()`
    /// directly so the gate sees no principal.
    pub fn with_convex_bearer(server: &'a ServerFixture, bearer: impl Into<String>) -> Self {
        Self {
            server,
            convex_bearer: Some(bearer.into()),
        }
    }

    /// The configured application-Convex bearer header value, if any.
    pub fn convex_bearer(&self) -> Option<&str> {
        self.convex_bearer.as_deref()
    }

    /// Build a request to an application-Convex route, attaching the configured
    /// bearer (if any) so the `convex_*` helpers authenticate uniformly.
    pub(super) fn convex_request(&self, method: Method, url: String) -> RequestBuilder {
        let builder = self.server.client().request(method, url);
        match &self.convex_bearer {
            Some(bearer) => builder.header(reqwest::header::AUTHORIZATION, bearer),
            None => builder,
        }
    }

    pub fn ws_url(&self, path: &str) -> String {
        self.server.ws_url(path)
    }

    pub fn convex_url(&self, tenant_id: &str, suffix: &str) -> String {
        self.server
            .http_url(&format!("/convex/{tenant_id}{suffix}"))
    }

    pub fn convex_http_url(&self, tenant_id: &str, path: &str) -> String {
        if path.is_empty() || path == "/" {
            return self.convex_url(tenant_id, "/http");
        }
        let normalized = if path.starts_with('/') {
            path.to_string()
        } else {
            format!("/{path}")
        };
        self.convex_url(tenant_id, &format!("/http{normalized}"))
    }

    pub(super) fn tenant_url(&self, tenant_id: &str, suffix: &str) -> String {
        self.server
            .http_url(&format!("/api/tenants/{tenant_id}{suffix}"))
    }
}
