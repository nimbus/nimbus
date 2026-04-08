use reqwest::Response;
use serde_json::{Value, json};

use super::HttpApiFixture;

impl<'a> HttpApiFixture<'a> {
    pub async fn insert_document(&self, tenant_id: &str, table: &str, fields: Value) -> Response {
        self.server
            .client()
            .post(self.tenant_url(tenant_id, "/documents"))
            .json(&json!({
                "table": table,
                "fields": fields,
            }))
            .send()
            .await
            .expect("document insert should succeed")
    }

    pub async fn update_document(
        &self,
        tenant_id: &str,
        table: &str,
        document_id: &str,
        patch: Value,
    ) -> Response {
        self.server
            .client()
            .patch(self.tenant_url(tenant_id, &format!("/documents/{table}/{document_id}")))
            .json(&json!({ "patch": patch }))
            .send()
            .await
            .expect("document update should succeed")
    }

    pub async fn delete_document(
        &self,
        tenant_id: &str,
        table: &str,
        document_id: &str,
    ) -> Response {
        self.server
            .client()
            .delete(self.tenant_url(tenant_id, &format!("/documents/{table}/{document_id}")))
            .send()
            .await
            .expect("document delete should succeed")
    }

    pub async fn list_documents(&self, tenant_id: &str, table: &str) -> Response {
        self.server
            .client()
            .get(self.tenant_url(tenant_id, &format!("/documents/{table}")))
            .send()
            .await
            .expect("document list should succeed")
    }

    pub async fn get_document(&self, tenant_id: &str, table: &str, document_id: &str) -> Response {
        self.server
            .client()
            .get(self.tenant_url(tenant_id, &format!("/documents/{table}/{document_id}")))
            .send()
            .await
            .expect("document get should succeed")
    }

    pub async fn journal(
        &self,
        tenant_id: &str,
        after: Option<u64>,
        limit: Option<usize>,
    ) -> Response {
        let mut path = self.tenant_url(tenant_id, "/journal");
        let mut query = Vec::new();
        if let Some(after) = after {
            query.push(format!("after={after}"));
        }
        if let Some(limit) = limit {
            query.push(format!("limit={limit}"));
        }
        if !query.is_empty() {
            path.push('?');
            path.push_str(&query.join("&"));
        }
        self.server
            .client()
            .get(path)
            .send()
            .await
            .expect("journal request should succeed")
    }

    pub async fn journal_bootstrap(&self, tenant_id: &str) -> Response {
        self.server
            .client()
            .get(self.tenant_url(tenant_id, "/journal/bootstrap"))
            .send()
            .await
            .expect("journal bootstrap request should succeed")
    }
}
