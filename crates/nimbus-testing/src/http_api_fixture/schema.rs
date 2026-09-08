use reqwest::Response;
use serde_json::Value;

use super::HttpApiFixture;

impl<'a> HttpApiFixture<'a> {
    pub async fn set_table_schema(&self, tenant_id: &str, table: &str, schema: Value) -> Response {
        self.server
            .client()
            .put(self.tenant_url(tenant_id, &format!("/schema/{table}")))
            .json(&schema)
            .send()
            .await
            .expect("schema put request should succeed")
    }

    /// Applies a table schema through the checked route. The server scans
    /// the table first and refuses the schema when any document violates it.
    pub async fn apply_table_schema(
        &self,
        tenant_id: &str,
        table: &str,
        schema: Value,
        dry_run: bool,
    ) -> Response {
        let suffix = if dry_run { "?dry_run=true" } else { "" };
        self.server
            .client()
            .post(self.tenant_url(tenant_id, &format!("/schema/{table}/apply{suffix}")))
            .json(&schema)
            .send()
            .await
            .expect("schema apply request should succeed")
    }

    pub async fn get_schema(&self, tenant_id: &str) -> Response {
        self.server
            .client()
            .get(self.tenant_url(tenant_id, "/schema"))
            .send()
            .await
            .expect("schema get request should succeed")
    }

    pub async fn get_table_schema(&self, tenant_id: &str, table: &str) -> Response {
        self.server
            .client()
            .get(self.tenant_url(tenant_id, &format!("/schema/{table}")))
            .send()
            .await
            .expect("table schema get request should succeed")
    }

    pub async fn delete_table_schema(&self, tenant_id: &str, table: &str) -> Response {
        self.server
            .client()
            .delete(self.tenant_url(tenant_id, &format!("/schema/{table}")))
            .send()
            .await
            .expect("table schema delete request should succeed")
    }
}
