use reqwest::Response;
use serde_json::{Value, json};

use crate::ServerFixture;

pub struct HttpApiFixture<'a> {
    server: &'a ServerFixture,
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
        method: reqwest::Method,
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

    pub async fn convex_http(
        &self,
        tenant_id: &str,
        method: reqwest::Method,
        path: &str,
    ) -> Response {
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
            .delete(self.server.http_url(&format!("/api/tenants/{tenant_id}")))
            .send()
            .await
            .expect("tenant delete request should succeed")
    }

    pub async fn schedule_mutation(&self, tenant_id: &str, request: Value) -> Response {
        self.server
            .client()
            .post(
                self.server
                    .http_url(&format!("/api/tenants/{tenant_id}/schedule")),
            )
            .json(&request)
            .send()
            .await
            .expect("schedule request should succeed")
    }

    pub async fn list_scheduled_jobs(&self, tenant_id: &str) -> Response {
        self.server
            .client()
            .get(
                self.server
                    .http_url(&format!("/api/tenants/{tenant_id}/schedule")),
            )
            .send()
            .await
            .expect("scheduled jobs request should succeed")
    }

    pub async fn cancel_scheduled_job(&self, tenant_id: &str, job_id: &str) -> Response {
        self.server
            .client()
            .delete(
                self.server
                    .http_url(&format!("/api/tenants/{tenant_id}/schedule/{job_id}")),
            )
            .send()
            .await
            .expect("scheduled job cancel request should succeed")
    }

    pub async fn get_scheduled_job_result(&self, tenant_id: &str, job_id: &str) -> Response {
        self.server
            .client()
            .get(self.server.http_url(&format!(
                "/api/tenants/{tenant_id}/schedule/history/{job_id}"
            )))
            .send()
            .await
            .expect("scheduled job result request should succeed")
    }

    pub async fn create_cron_job(&self, tenant_id: &str, request: Value) -> Response {
        self.server
            .client()
            .post(
                self.server
                    .http_url(&format!("/api/tenants/{tenant_id}/crons")),
            )
            .json(&request)
            .send()
            .await
            .expect("cron create request should succeed")
    }

    pub async fn list_cron_jobs(&self, tenant_id: &str) -> Response {
        self.server
            .client()
            .get(
                self.server
                    .http_url(&format!("/api/tenants/{tenant_id}/crons")),
            )
            .send()
            .await
            .expect("cron list request should succeed")
    }

    pub async fn delete_cron_job(&self, tenant_id: &str, name: &str) -> Response {
        self.server
            .client()
            .delete(
                self.server
                    .http_url(&format!("/api/tenants/{tenant_id}/crons/{name}")),
            )
            .send()
            .await
            .expect("cron delete request should succeed")
    }

    pub async fn set_table_schema(&self, tenant_id: &str, table: &str, schema: Value) -> Response {
        self.server
            .client()
            .put(
                self.server
                    .http_url(&format!("/api/tenants/{tenant_id}/schema/{table}")),
            )
            .json(&schema)
            .send()
            .await
            .expect("schema put request should succeed")
    }

    pub async fn get_schema(&self, tenant_id: &str) -> Response {
        self.server
            .client()
            .get(
                self.server
                    .http_url(&format!("/api/tenants/{tenant_id}/schema")),
            )
            .send()
            .await
            .expect("schema get request should succeed")
    }

    pub async fn get_table_schema(&self, tenant_id: &str, table: &str) -> Response {
        self.server
            .client()
            .get(
                self.server
                    .http_url(&format!("/api/tenants/{tenant_id}/schema/{table}")),
            )
            .send()
            .await
            .expect("table schema get request should succeed")
    }

    pub async fn delete_table_schema(&self, tenant_id: &str, table: &str) -> Response {
        self.server
            .client()
            .delete(
                self.server
                    .http_url(&format!("/api/tenants/{tenant_id}/schema/{table}")),
            )
            .send()
            .await
            .expect("table schema delete request should succeed")
    }

    pub async fn insert_document(&self, tenant_id: &str, table: &str, fields: Value) -> Response {
        self.server
            .client()
            .post(
                self.server
                    .http_url(&format!("/api/tenants/{tenant_id}/documents")),
            )
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
            .patch(self.server.http_url(&format!(
                "/api/tenants/{tenant_id}/documents/{table}/{document_id}"
            )))
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
            .delete(self.server.http_url(&format!(
                "/api/tenants/{tenant_id}/documents/{table}/{document_id}"
            )))
            .send()
            .await
            .expect("document delete should succeed")
    }

    pub async fn list_documents(&self, tenant_id: &str, table: &str) -> Response {
        self.server
            .client()
            .get(
                self.server
                    .http_url(&format!("/api/tenants/{tenant_id}/documents/{table}")),
            )
            .send()
            .await
            .expect("document list should succeed")
    }

    pub async fn get_document(&self, tenant_id: &str, table: &str, document_id: &str) -> Response {
        self.server
            .client()
            .get(self.server.http_url(&format!(
                "/api/tenants/{tenant_id}/documents/{table}/{document_id}"
            )))
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
        let mut path = format!("/api/tenants/{tenant_id}/journal");
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
            .get(self.server.http_url(&path))
            .send()
            .await
            .expect("journal request should succeed")
    }

    pub async fn journal_bootstrap(&self, tenant_id: &str) -> Response {
        self.server
            .client()
            .get(
                self.server
                    .http_url(&format!("/api/tenants/{tenant_id}/journal/bootstrap")),
            )
            .send()
            .await
            .expect("journal bootstrap request should succeed")
    }

    pub async fn query_documents(&self, tenant_id: &str, query: Value) -> Response {
        self.server
            .client()
            .post(
                self.server
                    .http_url(&format!("/api/tenants/{tenant_id}/query")),
            )
            .json(&query)
            .send()
            .await
            .expect("query request should succeed")
    }

    pub async fn query_documents_paginated(&self, tenant_id: &str, query: Value) -> Response {
        self.server
            .client()
            .post(
                self.server
                    .http_url(&format!("/api/tenants/{tenant_id}/query/paginated")),
            )
            .json(&query)
            .send()
            .await
            .expect("paginated query request should succeed")
    }
}
