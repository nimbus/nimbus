use reqwest::Response;
use serde_json::Value;

use super::HttpApiFixture;

impl<'a> HttpApiFixture<'a> {
    pub async fn schedule_mutation(&self, tenant_id: &str, request: Value) -> Response {
        self.server
            .client()
            .post(self.tenant_url(tenant_id, "/schedule"))
            .json(&request)
            .send()
            .await
            .expect("schedule request should succeed")
    }

    pub async fn list_scheduled_jobs(&self, tenant_id: &str) -> Response {
        self.server
            .client()
            .get(self.tenant_url(tenant_id, "/schedule"))
            .send()
            .await
            .expect("scheduled jobs request should succeed")
    }

    pub async fn cancel_scheduled_job(&self, tenant_id: &str, job_id: &str) -> Response {
        self.server
            .client()
            .delete(self.tenant_url(tenant_id, &format!("/schedule/{job_id}")))
            .send()
            .await
            .expect("scheduled job cancel request should succeed")
    }

    pub async fn get_scheduled_job_result(&self, tenant_id: &str, job_id: &str) -> Response {
        self.server
            .client()
            .get(self.tenant_url(tenant_id, &format!("/schedule/history/{job_id}")))
            .send()
            .await
            .expect("scheduled job result request should succeed")
    }

    pub async fn create_cron_job(&self, tenant_id: &str, request: Value) -> Response {
        self.server
            .client()
            .post(self.tenant_url(tenant_id, "/crons"))
            .json(&request)
            .send()
            .await
            .expect("cron create request should succeed")
    }

    pub async fn list_cron_jobs(&self, tenant_id: &str) -> Response {
        self.server
            .client()
            .get(self.tenant_url(tenant_id, "/crons"))
            .send()
            .await
            .expect("cron list request should succeed")
    }

    pub async fn delete_cron_job(&self, tenant_id: &str, name: &str) -> Response {
        self.server
            .client()
            .delete(self.tenant_url(tenant_id, &format!("/crons/{name}")))
            .send()
            .await
            .expect("cron delete request should succeed")
    }
}
