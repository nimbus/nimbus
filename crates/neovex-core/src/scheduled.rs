use serde::{Deserialize, Serialize};

use crate::{DocumentId, Mutation, Timestamp};

/// Unique identifier for a scheduled job.
pub type JobId = DocumentId;

/// A mutation scheduled to execute at a future time.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScheduledJob {
    pub id: JobId,
    pub run_at: Timestamp,
    pub mutation: Mutation,
    pub created_at: Timestamp,
}

/// A recurring cron job definition.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CronJob {
    pub name: String,
    pub schedule: CronSchedule,
    pub mutation: Mutation,
    pub enabled: bool,
    pub last_run: Option<Timestamp>,
    pub next_run: Timestamp,
    pub created_at: Timestamp,
}

/// Final execution outcome for a scheduled job.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScheduledJobOutcome {
    Completed,
    Failed,
}

/// Persisted result for a completed scheduled job.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScheduledJobResult {
    pub id: JobId,
    pub run_at: Timestamp,
    pub finished_at: Timestamp,
    pub mutation: Mutation,
    pub outcome: ScheduledJobOutcome,
    pub error: Option<String>,
}

/// Schedule type for cron jobs. Phase 3 supports interval schedules only.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CronSchedule {
    Interval { seconds: u64 },
}

impl CronSchedule {
    /// Calculates the next time this schedule should fire after the provided timestamp.
    pub fn next_after(&self, after: Timestamp) -> Timestamp {
        match self {
            Self::Interval { seconds } => Timestamp(after.0 + (seconds * 1000)),
        }
    }
}

/// Request to schedule a mutation after a delay.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScheduleRequest {
    pub run_after_ms: u64,
    pub mutation: Mutation,
}

/// Request to create a recurring cron job.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CreateCronRequest {
    pub name: String,
    pub schedule: CronSchedule,
    pub mutation: Mutation,
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{CronSchedule, ScheduledJobOutcome, ScheduledJobResult};
    use crate::{Mutation, TableName, Timestamp};

    #[test]
    fn cron_next_after_calculates_correctly() {
        let schedule = CronSchedule::Interval { seconds: 60 };
        let now = Timestamp(1_000_000);

        assert_eq!(schedule.next_after(now), Timestamp(1_060_000));
    }

    #[test]
    fn scheduled_types_roundtrip_via_json() {
        let request = super::ScheduleRequest {
            run_after_ms: 5_000,
            mutation: Mutation::Insert {
                table: TableName::new("tasks").expect("table name should be valid"),
                fields: serde_json::Map::from_iter([("title".to_string(), json!("Hello"))]),
            },
        };

        let serialized = serde_json::to_string(&request).expect("request should serialize");
        let decoded: super::ScheduleRequest =
            serde_json::from_str(&serialized).expect("request should deserialize");

        assert_eq!(decoded, request);
    }

    #[test]
    fn scheduled_job_result_roundtrip_via_json() {
        let result = ScheduledJobResult {
            id: crate::DocumentId::new(),
            run_at: Timestamp(5_000),
            finished_at: Timestamp(6_000),
            mutation: Mutation::Insert {
                table: TableName::new("tasks").expect("table name should be valid"),
                fields: serde_json::Map::from_iter([("title".to_string(), json!("Hello"))]),
            },
            outcome: ScheduledJobOutcome::Failed,
            error: Some("boom".to_string()),
        };

        let serialized = serde_json::to_string(&result).expect("result should serialize");
        let decoded: ScheduledJobResult =
            serde_json::from_str(&serialized).expect("result should deserialize");

        assert_eq!(decoded, result);
    }
}
