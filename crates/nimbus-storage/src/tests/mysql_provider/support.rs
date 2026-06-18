pub(super) use std::env;
pub(super) use std::future::Future;
pub(super) use std::ops::Bound;
pub(super) use std::sync::atomic::{AtomicU64, Ordering};
pub(super) use std::time::{SystemTime, UNIX_EPOCH};

pub(super) use mysql_async::prelude::Queryable;
pub(super) use mysql_async::{Opts, Pool};
pub(super) use nimbus_core::{
    CollectionName, CronJob, CronSchedule, Document, DocumentLocator, DocumentPath,
    IndexDefinition, Mutation, ResourcePathBinding, ScheduledJob, ScheduledJobOutcome,
    ScheduledJobResult, Schema, SchemaChangeEvent, SequenceNumber, TableId, TableName, TableSchema,
    TableState, TenantEventKind, TenantId, Timestamp, TriggerDeliveryCursor, WriteOp, WriteOpType,
};
pub(super) use testcontainers_modules::{
    mysql,
    testcontainers::{ContainerAsync, runners::AsyncRunner},
};

pub(super) use super::super::{
    Duration, FieldSchema, FieldType, MySqlProvider, MySqlProviderConfig, TenantEventRecord,
    TenantReadStorage, implicit_external_provider_fixtures_disabled,
    require_explicit_external_provider_fixture_envs, timeout,
};
pub(super) use crate::{ResolvedScheduleOp, ResolvedWrite};

pub(super) const MYSQL_URL_ENV: &str = "NIMBUS_MYSQL_URL";
pub(super) static TEST_SUFFIX_COUNTER: AtomicU64 = AtomicU64::new(0);

pub(super) async fn with_test_provider<F, Fut>(test: F)
where
    F: FnOnce(MySqlProvider, MySqlProviderConfig) -> Fut,
    Fut: Future<Output = ()>,
{
    let connection = match test_connection().await {
        Some(connection) => connection,
        None => return,
    };
    let suffix = unique_suffix();
    let metadata_database = format!("nimbus_meta_{}", &suffix[..16.min(suffix.len())]);
    let tenant_database_prefix = format!("tenant_{}_", &suffix[..12.min(suffix.len())]);
    let config = MySqlProviderConfig {
        connection_string: connection.connection_string().to_string(),
        metadata_database,
        tenant_database_prefix,
        min_connections: Some(1),
        max_connections: Some(4),
    };
    let provider = MySqlProvider::connect(config.clone())
        .await
        .expect("provider should connect");
    test(provider.clone(), config).await;
    provider
        .drop_provider_databases_for_test()
        .await
        .expect("test provider databases should drop");
    drop(connection);
}

pub(super) enum TestConnection {
    External(String),
    Container {
        connection_string: String,
        _container: Box<ContainerAsync<mysql::Mysql>>,
    },
}

impl TestConnection {
    fn connection_string(&self) -> &str {
        match self {
            Self::External(connection_string) => connection_string,
            Self::Container {
                connection_string, ..
            } => connection_string,
        }
    }
}

pub(super) async fn test_connection() -> Option<TestConnection> {
    if let Ok(connection_string) = env::var(MYSQL_URL_ENV) {
        return Some(TestConnection::External(connection_string));
    }

    require_explicit_external_provider_fixture_envs("MySQL provider", &[MYSQL_URL_ENV]);
    if implicit_external_provider_fixtures_disabled("MySQL provider") {
        return None;
    }

    let container = match mysql::Mysql::default().start().await {
        Ok(container) => container,
        Err(error) => {
            eprintln!(
                "skipping mysql provider test because no explicit MySQL URL was provided and container startup failed: {error}"
            );
            return None;
        }
    };
    let host = container
        .get_host()
        .await
        .expect("container host should resolve");
    let port = container
        .get_host_port_ipv4(3306)
        .await
        .expect("container port should resolve");

    let url = format!("mysql://root@{host}:{port}/test");
    if timeout(Duration::from_secs(20), async {
        loop {
            if MySqlProvider::connect(MySqlProviderConfig::new(url.clone()))
                .await
                .is_ok()
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
    })
    .await
    .is_err()
    {
        eprintln!("skipping mysql provider test because the MySQL container never became ready");
        return None;
    }

    Some(TestConnection::Container {
        connection_string: url,
        _container: Box::new(container),
    })
}

pub(super) fn unique_suffix() -> String {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock should be after unix epoch")
        .as_nanos();
    let counter = TEST_SUFFIX_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{counter:08x}{:x}{timestamp:x}", std::process::id())
}

pub(super) fn mysql_indexed_rank_schema(table: &TableName) -> (TableSchema, IndexDefinition) {
    let index = IndexDefinition {
        id: nimbus_core::IndexId::new(),
        state: nimbus_core::IndexState::Enabled,
        name: "by_rank".to_string(),
        fields: vec!["rank".to_string()],
    };
    (
        TableSchema {
            table: table.clone(),
            fields: vec![FieldSchema {
                name: "rank".to_string(),
                field_type: FieldType::Number,
                required: true,
            }],
            indexes: vec![index.clone()],
            access_policy: None,
        },
        index,
    )
}

pub(super) fn mysql_ranked_document(table: &TableName, title: &str, rank: u64) -> Document {
    Document::new(
        table.clone(),
        serde_json::Map::from_iter([
            ("title".to_string(), serde_json::json!(title)),
            ("rank".to_string(), serde_json::json!(rank)),
        ]),
    )
}

pub(super) fn mysql_status_rank_schema(table: &TableName) -> TableSchema {
    TableSchema {
        table: table.clone(),
        fields: vec![
            FieldSchema {
                name: "status".to_string(),
                field_type: FieldType::String,
                required: true,
            },
            FieldSchema {
                name: "rank".to_string(),
                field_type: FieldType::Number,
                required: true,
            },
        ],
        indexes: vec![IndexDefinition {
            id: nimbus_core::IndexId::new(),
            state: nimbus_core::IndexState::Enabled,
            name: "by_status_rank".to_string(),
            fields: vec!["status".to_string(), "rank".to_string()],
        }],
        access_policy: None,
    }
}

pub(super) fn mysql_status_rank_document(
    table: &TableName,
    title: &str,
    status: &str,
    rank: u64,
) -> Document {
    Document::new(
        table.clone(),
        serde_json::Map::from_iter([
            ("title".to_string(), serde_json::json!(title)),
            ("status".to_string(), serde_json::json!(status)),
            ("rank".to_string(), serde_json::json!(rank)),
        ]),
    )
}

pub(super) fn mysql_historical_read_shape(
    table: &TableName,
    table_id: &TableId,
    schema: &TableSchema,
    sequence: SequenceNumber,
) -> nimbus_core::HistoricalReadShape {
    let registry = nimbus_core::VersionedRegistry::from_records([
        nimbus_core::TenantEventRecord::schema_change(
            SequenceNumber(1),
            Timestamp(100),
            SchemaChangeEvent::SetTable {
                table: table.clone(),
                table_id: table_id.clone(),
                previous: None,
                current: schema.clone(),
            },
        )
        .expect("schema change event should build"),
    ])
    .expect("registry should build");
    registry
        .read_shape_at(table, mysql_historical_snapshot(sequence))
        .expect("read shape should load")
        .expect("table should exist at historical read")
}

pub(super) fn mysql_historical_snapshot(
    sequence: SequenceNumber,
) -> nimbus_core::HistoricalReadSnapshot {
    let timestamp = Timestamp(sequence.0.saturating_mul(100));
    nimbus_core::HistoricalReadSnapshot::new(
        nimbus_core::ReadTimestamp::new(timestamp),
        nimbus_core::CommitSequence::new(sequence),
        nimbus_core::CommitTimestamp::new(timestamp),
    )
}

pub(super) fn mysql_rank_full_scan_oracle_titles(
    store: &crate::MySqlTenantStore,
    table: &TableName,
    table_id: &TableId,
    corpus: &[&Document],
    sequence: SequenceNumber,
    rank: u64,
) -> Vec<String> {
    let mut titles = corpus
        .iter()
        .filter_map(|document| {
            store
                .get_document_version_at(table, table_id, &document.id, sequence)
                .expect("document version oracle should load")
        })
        .filter(|document| {
            document.fields.get("rank").and_then(|value| value.as_u64()) == Some(rank)
        })
        .map(|document| mysql_document_title_string(&document))
        .collect::<Vec<_>>();
    titles.sort();
    titles
}

pub(super) fn mysql_status_rank_full_scan_oracle_titles(
    store: &crate::MySqlTenantStore,
    table_id: &TableId,
    corpus: &[&Document],
    sequence: SequenceNumber,
    status: &str,
    start_rank: Option<u64>,
    end_rank: Option<u64>,
) -> Vec<String> {
    let mut rows = corpus
        .iter()
        .filter_map(|document| {
            store
                .get_document_version_at(&document.table, table_id, &document.id, sequence)
                .expect("document version oracle should load")
        })
        .filter_map(|document| {
            let document_status = document.fields.get("status")?.as_str()?;
            let rank = document.fields.get("rank")?.as_u64()?;
            if document_status == status
                && start_rank.is_none_or(|start| rank >= start)
                && end_rank.is_none_or(|end| rank <= end)
            {
                Some((rank, mysql_document_title_string(&document)))
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    rows.sort_by(|left, right| left.0.cmp(&right.0).then_with(|| left.1.cmp(&right.1)));
    rows.into_iter().map(|(_, title)| title).collect()
}

pub(super) fn mysql_document_titles(documents: &[Document]) -> Vec<&str> {
    documents
        .iter()
        .map(|document| {
            document
                .fields
                .get("title")
                .and_then(|value| value.as_str())
                .expect("document should have a string title")
        })
        .collect()
}

pub(super) fn mysql_document_title_strings(documents: &[Document]) -> Vec<String> {
    documents.iter().map(mysql_document_title_string).collect()
}

pub(super) fn mysql_document_title_string(document: &Document) -> String {
    document
        .fields
        .get("title")
        .and_then(|value| value.as_str())
        .expect("document should have a string title")
        .to_string()
}

pub(super) fn mysql_active_table_id_for_diagnostic(
    diagnostics: &[crate::TableIdentityDiagnostic],
    table: &TableName,
) -> TableId {
    diagnostics
        .iter()
        .find(|diagnostic| {
            diagnostic.table_name == *table && diagnostic.state == TableState::Active
        })
        .expect("active table identity should exist")
        .table_id
        .clone()
}

#[allow(clippy::too_many_arguments)]
pub(super) fn mysql_durable_write_record(
    sequence: SequenceNumber,
    timestamp: Timestamp,
    table: &TableName,
    table_id: &TableId,
    op_type: WriteOpType,
    doc_id: nimbus_core::DocumentId,
    previous: Option<Document>,
    current: Option<Document>,
) -> TenantEventRecord {
    TenantEventRecord::new(
        sequence,
        timestamp,
        vec![WriteOp {
            table: table.clone(),
            table_id: table_id.clone(),
            op_type,
            doc_id,
            resource_path_binding: None,
            trigger_write_origin: None,
            previous,
            current,
        }],
        None,
    )
    .expect("durable record should build")
}

pub(super) fn binding(table: &str, id: &str, path: &[&str]) -> ResourcePathBinding {
    ResourcePathBinding::new(
        DocumentLocator::new(
            TableName::new(table).expect("table name should parse"),
            nimbus_core::DocumentId::from_key(id).expect("document id should parse"),
        ),
        DocumentPath::from_segments(path.iter().copied()).expect("document path should parse"),
    )
}

pub(super) fn scheduled_insert_job(run_at: Timestamp, title: &str) -> ScheduledJob {
    ScheduledJob {
        id: nimbus_core::DocumentId::new(),
        run_at,
        mutation: Mutation::Insert {
            table: TableName::new("tasks").expect("table name should build"),
            id: None,
            fields: serde_json::Map::from_iter([("title".to_string(), serde_json::json!(title))]),
        },
        created_at: Timestamp(100),
    }
}

pub(super) async fn document_index_counts(
    connection_string: &str,
    database_name: &str,
) -> (u64, u64) {
    let opts = Opts::from_url(connection_string).expect("connection string should parse");
    let pool = Pool::new(opts);
    let mut conn = pool.get_conn().await.expect("mysql connection should open");
    let generated_columns = conn
        .exec_first::<u64, _, _>(
            "SELECT COUNT(*) \
             FROM INFORMATION_SCHEMA.COLUMNS \
             WHERE TABLE_SCHEMA = ? \
               AND TABLE_NAME = 'documents' \
               AND COLUMN_NAME LIKE 'gcol\\_%' \
               AND EXTRA LIKE '%GENERATED%'",
            (database_name,),
        )
        .await
        .expect("generated column count should query")
        .expect("generated column count should return a row");
    let secondary_indexes = conn
        .exec_first::<u64, _, _>(
            "SELECT COUNT(DISTINCT INDEX_NAME) \
             FROM INFORMATION_SCHEMA.STATISTICS \
             WHERE TABLE_SCHEMA = ? \
               AND TABLE_NAME = 'documents' \
               AND INDEX_NAME LIKE 'idx\\_%'",
            (database_name,),
        )
        .await
        .expect("secondary index count should query")
        .expect("secondary index count should return a row");
    conn.disconnect()
        .await
        .expect("mysql connection should close");
    pool.disconnect().await.expect("mysql pool should close");
    (generated_columns, secondary_indexes)
}

pub(super) async fn document_generated_column_expressions(
    connection_string: &str,
    database_name: &str,
) -> Vec<String> {
    let opts = Opts::from_url(connection_string).expect("connection string should parse");
    let pool = Pool::new(opts);
    let mut conn = pool.get_conn().await.expect("mysql connection should open");
    let expressions = conn
        .exec::<String, _, _>(
            "SELECT GENERATION_EXPRESSION \
             FROM INFORMATION_SCHEMA.COLUMNS \
             WHERE TABLE_SCHEMA = ? \
               AND TABLE_NAME = 'documents' \
               AND COLUMN_NAME LIKE 'gcol\\_%' \
             ORDER BY COLUMN_NAME",
            (database_name,),
        )
        .await
        .expect("generated column expressions should query");
    conn.disconnect()
        .await
        .expect("mysql connection should close");
    pool.disconnect().await.expect("mysql pool should close");
    expressions
}
