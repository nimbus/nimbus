use reqwest::{Method, Response};
use serde_json::{Value, json};

use super::HttpApiFixture;

impl<'a> HttpApiFixture<'a> {
    pub async fn convex_query(&self, tenant_id: &str, query: Value) -> Response {
        self.server
            .client()
            .post(self.convex_url(tenant_id, "/query"))
            .json(&json!({ "query": query }))
            .send()
            .await
            .expect("convex query request should succeed")
    }

    pub async fn convex_named_query(&self, tenant_id: &str, name: &str, args: Value) -> Response {
        self.server
            .client()
            .post(self.convex_url(tenant_id, "/query"))
            .json(&json!({ "name": name, "args": args }))
            .send()
            .await
            .expect("convex named query request should succeed")
    }

    pub async fn convex_paginated_query(&self, tenant_id: &str, query: Value) -> Response {
        self.server
            .client()
            .post(self.convex_url(tenant_id, "/query/paginated"))
            .json(&json!({ "query": query }))
            .send()
            .await
            .expect("convex paginated query request should succeed")
    }

    pub async fn convex_named_paginated_query(
        &self,
        tenant_id: &str,
        name: &str,
        args: Value,
        page_size: usize,
        cursor: Option<&str>,
    ) -> Response {
        self.server
            .client()
            .post(self.convex_url(tenant_id, "/query/paginated"))
            .json(&json!({
                "name": name,
                "args": args,
                "page_size": page_size,
                "cursor": cursor,
            }))
            .send()
            .await
            .expect("convex named paginated query request should succeed")
    }

    pub async fn convex_mutation(&self, tenant_id: &str, mutation: Value) -> Response {
        self.server
            .client()
            .post(self.convex_url(tenant_id, "/mutation"))
            .json(&json!({ "mutation": mutation }))
            .send()
            .await
            .expect("convex mutation request should succeed")
    }

    pub async fn convex_named_mutation(
        &self,
        tenant_id: &str,
        name: &str,
        args: Value,
    ) -> Response {
        self.server
            .client()
            .post(self.convex_url(tenant_id, "/mutation"))
            .json(&json!({ "name": name, "args": args }))
            .send()
            .await
            .expect("convex named mutation request should succeed")
    }

    pub async fn convex_action(&self, tenant_id: &str, action: Value) -> Response {
        self.server
            .client()
            .post(self.convex_url(tenant_id, "/action"))
            .json(&json!({ "action": action }))
            .send()
            .await
            .expect("convex action request should succeed")
    }

    pub async fn convex_named_action(&self, tenant_id: &str, name: &str, args: Value) -> Response {
        self.server
            .client()
            .post(self.convex_url(tenant_id, "/action"))
            .json(&json!({ "name": name, "args": args }))
            .send()
            .await
            .expect("convex named action request should succeed")
    }

    pub async fn convex_http_json(
        &self,
        tenant_id: &str,
        method: Method,
        path: &str,
        body: Value,
    ) -> Response {
        self.server
            .client()
            .request(method, self.convex_http_url(tenant_id, path))
            .json(&body)
            .send()
            .await
            .expect("convex http json request should succeed")
    }

    pub async fn convex_http(&self, tenant_id: &str, method: Method, path: &str) -> Response {
        self.server
            .client()
            .request(method, self.convex_http_url(tenant_id, path))
            .send()
            .await
            .expect("convex http request should succeed")
    }

    pub async fn convex_schedule_after(&self, tenant_id: &str, request: Value) -> Response {
        self.server
            .client()
            .post(self.convex_url(tenant_id, "/schedule/run_after"))
            .json(&request)
            .send()
            .await
            .expect("convex schedule-after request should succeed")
    }

    pub async fn convex_schedule_at(&self, tenant_id: &str, request: Value) -> Response {
        self.server
            .client()
            .post(self.convex_url(tenant_id, "/schedule/run_at"))
            .json(&request)
            .send()
            .await
            .expect("convex schedule-at request should succeed")
    }

    pub async fn convex_cancel_scheduled_job(&self, tenant_id: &str, job_id: &str) -> Response {
        self.server
            .client()
            .delete(self.convex_url(tenant_id, &format!("/schedule/{job_id}")))
            .send()
            .await
            .expect("convex scheduled job cancel request should succeed")
    }
}
