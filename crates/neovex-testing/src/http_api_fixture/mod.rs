mod convex;
mod debug;
mod documents;
mod queries;
mod schedule;
mod schema;
mod tenants;

use crate::ServerFixture;

pub struct HttpApiFixture<'a> {
    pub(super) server: &'a ServerFixture,
}

impl<'a> HttpApiFixture<'a> {
    pub fn new(server: &'a ServerFixture) -> Self {
        Self { server }
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
