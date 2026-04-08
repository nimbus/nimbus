use reqwest::Response;
use serde_json::json;

use super::HttpApiFixture;

impl<'a> HttpApiFixture<'a> {
    pub async fn create_tenant(&self, id: &str) -> Response {
        self.server
            .client()
            .post(self.server.http_url("/api/tenants"))
            .json(&json!({ "id": id }))
            .send()
            .await
            .expect("tenant request should succeed")
    }

    pub async fn list_tenants(&self) -> Response {
        self.server
            .client()
            .get(self.server.http_url("/api/tenants"))
            .send()
            .await
            .expect("tenant list request should succeed")
    }

    pub async fn delete_tenant(&self, tenant_id: &str) -> Response {
        self.server
            .client()
            .delete(self.tenant_url(tenant_id, ""))
            .send()
            .await
            .expect("tenant delete request should succeed")
    }
}
