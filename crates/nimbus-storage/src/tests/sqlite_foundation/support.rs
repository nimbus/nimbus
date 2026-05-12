pub(super) use super::*;
pub(super) use nimbus_core::{
    CronJob, CronSchedule, Mutation, ScheduledJob, ScheduledJobOutcome, ScheduledJobResult,
};

pub(super) use crate::{ResolvedScheduleOp, ResolvedWrite};

pub(super) fn explain_query_plan<P>(
    conn: &rusqlite::Connection,
    statement: &str,
    params: P,
) -> Vec<String>
where
    P: rusqlite::Params,
{
    let explain = format!("EXPLAIN QUERY PLAN {statement}");
    let mut stmt = conn
        .prepare(explain.as_str())
        .expect("query plan statement should prepare");
    let mut rows = stmt
        .query(params)
        .expect("query plan statement should execute");
    let mut detail_rows = Vec::new();
    while let Some(row) = rows.next().expect("query plan row should advance") {
        detail_rows.push(
            row.get::<_, String>(3)
                .expect("query plan detail should read"),
        );
    }
    detail_rows
}

pub(super) fn scheduled_insert_job(run_at: Timestamp, title: &str) -> ScheduledJob {
    ScheduledJob {
        id: DocumentId::new(),
        run_at,
        mutation: Mutation::Insert {
            table: TableName::new("tasks").expect("table name should be valid"),
            id: None,
            fields: serde_json::Map::from_iter([("title".to_string(), json!(title))]),
        },
        created_at: Timestamp(1_000),
    }
}

pub(super) fn ranked_tasks_schema() -> TableSchema {
    TableSchema {
        table: TableName::new("tasks").expect("table name should be valid"),
        fields: vec![FieldSchema {
            name: "rank".to_string(),
            field_type: FieldType::Number,
            required: true,
        }],
        indexes: vec![IndexDefinition {
            name: "by_rank".to_string(),
            fields: vec!["rank".to_string()],
        }],
        access_policy: None,
    }
}

pub(super) fn ranked_document(table: &TableName, title: &str, rank: u64) -> Document {
    Document::new(
        table.clone(),
        serde_json::Map::from_iter([
            ("title".to_string(), json!(title)),
            ("rank".to_string(), json!(rank)),
        ]),
    )
}
