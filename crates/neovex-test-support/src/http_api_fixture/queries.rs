use reqwest::Response;
use serde_json::Value;

use super::HttpApiFixture;

impl<'a> HttpApiFixture<'a> {
    pub async fn query_documents(&self, tenant_id: &str, query: Value) -> Response {
        self.server
            .client()
            .post(self.tenant_url(tenant_id, "/query"))
            .json(&query)
            .send()
            .await
            .expect("query request should succeed")
    }

    pub async fn query_documents_paginated(&self, tenant_id: &str, query: Value) -> Response {
        self.server
            .client()
            .post(self.tenant_url(tenant_id, "/query/paginated"))
            .json(&query)
            .send()
            .await
            .expect("paginated query request should succeed")
    }
}
