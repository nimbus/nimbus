use reqwest::Response;

use super::HttpApiFixture;

impl<'a> HttpApiFixture<'a> {
    pub async fn health(&self) -> Response {
        self.server
            .client()
            .get(self.server.http_url("/health"))
            .send()
            .await
            .expect("health request should succeed")
    }

    pub async fn runtime_metrics(&self) -> Response {
        self.server
            .client()
            .get(self.server.http_url("/debug/runtime/metrics"))
            .send()
            .await
            .expect("runtime metrics request should succeed")
    }

    pub async fn license_status(&self) -> Response {
        self.server
            .client()
            .get(self.server.http_url("/debug/license/status"))
            .send()
            .await
            .expect("license status request should succeed")
    }

    pub async fn tenant_consistency_report(&self, tenant_id: &str) -> Response {
        self.server
            .client()
            .get(
                self.server
                    .http_url(&format!("/debug/tenants/{tenant_id}/consistency")),
            )
            .send()
            .await
            .expect("tenant consistency request should succeed")
    }

    pub async fn tenant_consistency_full_scrub(&self, tenant_id: &str) -> Response {
        self.server
            .client()
            .get(self.server.http_url(&format!(
                "/debug/tenants/{tenant_id}/consistency?force_full=true"
            )))
            .send()
            .await
            .expect("forced tenant consistency request should succeed")
    }

    pub async fn clear_tenant_consistency_session(&self, tenant_id: &str) -> Response {
        self.server
            .client()
            .delete(
                self.server
                    .http_url(&format!("/debug/tenants/{tenant_id}/consistency")),
            )
            .send()
            .await
            .expect("tenant consistency session clear should succeed")
    }

    pub async fn tenant_engine_metrics(&self, tenant_id: &str) -> Response {
        self.server
            .client()
            .get(
                self.server
                    .http_url(&format!("/debug/tenants/{tenant_id}/engine/metrics")),
            )
            .send()
            .await
            .expect("tenant engine metrics request should succeed")
    }
}
