pub(super) use std::env;
pub(super) use std::future::Future;
pub(super) use std::net::TcpListener;
pub(super) use std::ops::Bound;
pub(super) use std::sync::atomic::{AtomicU64, Ordering};
pub(super) use std::time::{SystemTime, UNIX_EPOCH};

pub(super) use libsql::{Builder, Database};
pub(super) use nimbus_core::{
    CollectionName, CronJob, CronSchedule, Document, DocumentId, DocumentLocator, DocumentPath,
    FieldSchema, FieldType, IndexDefinition, Mutation, ResourcePathBinding, ScheduledJob,
    ScheduledJobOutcome, ScheduledJobResult, SchemaChangeEvent, SequenceNumber, TableId, TableName,
    TableSchema, TableState, TenantEventKind, TenantEventRecord, TenantId, Timestamp,
    TriggerDeliveryCursor, WriteOp, WriteOpType,
};
pub(super) use serial_test::serial;
pub(super) use testcontainers_modules::testcontainers::core::{IntoContainerPort, WaitFor};
pub(super) use testcontainers_modules::testcontainers::{
    ContainerAsync, GenericImage, ImageExt, runners::AsyncRunner,
};

pub(super) use super::super::{
    Duration, LibsqlReplicaProvider, LibsqlReplicaProviderConfig, SqliteTenantStore,
    implicit_external_provider_fixtures_disabled, require_explicit_external_provider_fixture_envs,
    tempdir, timeout,
};
pub(super) use crate::async_storage::TenantReadStorage;
pub(super) use crate::libsql::libsql_transport_connector;
pub(super) use crate::{
    LibsqlReplicaBarrierPath, LibsqlReplicaRefreshCause, LibsqlReplicaRefreshPath,
    ResolvedScheduleOp, ResolvedWrite,
};

pub(super) const LIBSQL_URL_ENV: &str = "NIMBUS_LIBSQL_URL";
pub(super) const LIBSQL_AUTH_TOKEN_ENV: &str = "NIMBUS_LIBSQL_AUTH_TOKEN";
pub(super) const LIBSQL_ADMIN_URL_ENV: &str = "NIMBUS_LIBSQL_ADMIN_URL";
pub(super) const LIBSQL_ADMIN_AUTH_HEADER_ENV: &str = "NIMBUS_LIBSQL_ADMIN_AUTH_HEADER";
pub(super) static TEST_SUFFIX_COUNTER: AtomicU64 = AtomicU64::new(0);

pub(super) async fn with_test_provider<F, Fut>(test: F)
where
    F: FnOnce(LibsqlReplicaProvider, LibsqlReplicaProviderConfig) -> Fut,
    Fut: Future<Output = ()>,
{
    let connection = match test_connection().await {
        Some(connection) => connection,
        None => return,
    };
    let replica_cache_dir = tempdir().expect("replica cache dir should create");
    let suffix = unique_suffix();
    let metadata_namespace = format!("nimbus_meta_{}", &suffix[..16.min(suffix.len())]);
    let tenant_namespace_prefix = format!("tenant_{}_", &suffix[..12.min(suffix.len())]);
    let mut config = LibsqlReplicaProviderConfig::new(
        connection.primary_url().to_string(),
        connection.admin_api_url().to_string(),
        replica_cache_dir.path(),
    );
    config.auth_token = connection.auth_token().map(ToOwned::to_owned);
    config.admin_auth_header = connection.admin_auth_header().map(ToOwned::to_owned);
    config.metadata_namespace = metadata_namespace;
    config.tenant_namespace_prefix = tenant_namespace_prefix;

    let provider = LibsqlReplicaProvider::connect(config.clone())
        .await
        .expect("provider should connect");
    test(provider.clone(), config).await;
    provider
        .drop_provider_namespaces_for_test()
        .await
        .expect("provider namespaces should clean up");
    drop(connection);
}

pub(super) enum TestConnection {
    External {
        primary_url: String,
        auth_token: Option<String>,
        admin_api_url: String,
        admin_auth_header: Option<String>,
    },
    Container {
        primary_url: String,
        auth_token: Option<String>,
        admin_api_url: String,
        admin_auth_header: Option<String>,
        _container: Box<ContainerAsync<GenericImage>>,
    },
}

impl TestConnection {
    fn primary_url(&self) -> &str {
        match self {
            Self::External { primary_url, .. } => primary_url,
            Self::Container { primary_url, .. } => primary_url,
        }
    }

    fn auth_token(&self) -> Option<&str> {
        match self {
            Self::External { auth_token, .. } => auth_token.as_deref(),
            Self::Container { auth_token, .. } => auth_token.as_deref(),
        }
    }

    fn admin_api_url(&self) -> &str {
        match self {
            Self::External { admin_api_url, .. } => admin_api_url,
            Self::Container { admin_api_url, .. } => admin_api_url,
        }
    }

    fn admin_auth_header(&self) -> Option<&str> {
        match self {
            Self::External {
                admin_auth_header, ..
            } => admin_auth_header.as_deref(),
            Self::Container {
                admin_auth_header, ..
            } => admin_auth_header.as_deref(),
        }
    }
}

pub(super) async fn test_connection() -> Option<TestConnection> {
    if let Ok(primary_url) = env::var(LIBSQL_URL_ENV) {
        let admin_api_url = env::var(LIBSQL_ADMIN_URL_ENV).unwrap_or_else(|_| {
            panic!(
                "{LIBSQL_ADMIN_URL_ENV} is required when {LIBSQL_URL_ENV} is set for libsql provider tests"
            )
        });
        return Some(TestConnection::External {
            primary_url,
            auth_token: env::var(LIBSQL_AUTH_TOKEN_ENV).ok(),
            admin_api_url,
            admin_auth_header: env::var(LIBSQL_ADMIN_AUTH_HEADER_ENV).ok(),
        });
    }

    require_explicit_external_provider_fixture_envs(
        "libsql provider",
        &[LIBSQL_URL_ENV, LIBSQL_ADMIN_URL_ENV],
    );
    if implicit_external_provider_fixtures_disabled("libsql provider") {
        return None;
    }

    let image = GenericImage::new("ghcr.io/tursodatabase/libsql-server", "latest")
        // The image's wrapper/log stream is not a stable readiness seam for
        // testcontainers. We do a short startup delay here, then use a live
        // provider connect loop below as the authoritative readiness check.
        .with_wait_for(WaitFor::seconds(1))
        // The image entrypoint already injects --http-listen-addr from
        // SQLD_HTTP_LISTEN_ADDR; passing that flag again makes current images
        // exit with a duplicate-argument error before readiness probing starts.
        .with_env_var("SQLD_ADMIN_LISTEN_ADDR", "0.0.0.0:8081")
        .with_cmd(vec![
            "/bin/sqld".to_string(),
            "--enable-namespaces".to_string(),
            "--no-welcome".to_string(),
        ]);
    let host_http_port = allocate_host_port();
    let host_admin_port = allocate_host_port();
    let image = image
        .with_mapped_port(host_http_port, 8080.tcp())
        .with_mapped_port(host_admin_port, 8081.tcp());
    let container = match image.start().await {
        Ok(container) => container,
        Err(error) => {
            eprintln!(
                "skipping libsql provider test because no explicit libsql URL was provided and container startup failed: {error}"
            );
            return None;
        }
    };
    let host = container
        .get_host()
        .await
        .expect("container host should resolve");
    let primary_url = format!("http://{host}:{host_http_port}");
    let admin_api_url = format!("http://{host}:{host_admin_port}");

    if timeout(Duration::from_secs(60), async {
        loop {
            let replica_cache_dir = tempdir().expect("temporary replica cache dir should create");
            let config = LibsqlReplicaProviderConfig::new(
                primary_url.clone(),
                admin_api_url.clone(),
                replica_cache_dir.keep(),
            );
            if LibsqlReplicaProvider::connect(config).await.is_ok() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
    })
    .await
    .is_err()
    {
        eprintln!("skipping libsql provider test because the libsql container never became ready");
        return None;
    }

    Some(TestConnection::Container {
        primary_url,
        auth_token: None,
        admin_api_url,
        admin_auth_header: None,
        _container: Box::new(container),
    })
}

pub(super) fn unique_suffix() -> String {
    format!(
        "{:x}{:x}{:016x}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time should be after the unix epoch")
            .as_nanos(),
        TEST_SUFFIX_COUNTER.fetch_add(1, Ordering::Relaxed)
    )
}

pub(super) fn libsql_indexed_rank_schema(table: &TableName) -> (TableSchema, IndexDefinition) {
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

pub(super) fn libsql_ranked_document(table: &TableName, title: &str, rank: u64) -> Document {
    Document::new(
        table.clone(),
        serde_json::Map::from_iter([
            ("title".to_string(), serde_json::json!(title)),
            ("rank".to_string(), serde_json::json!(rank)),
        ]),
    )
}

pub(super) fn libsql_status_rank_schema(table: &TableName) -> TableSchema {
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

pub(super) fn libsql_status_rank_document(
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

pub(super) fn libsql_historical_read_shape(
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
        .read_shape_at(table, libsql_historical_snapshot(sequence))
        .expect("read shape should load")
        .expect("table should exist at historical read")
}

pub(super) fn libsql_historical_snapshot(
    sequence: SequenceNumber,
) -> nimbus_core::HistoricalReadSnapshot {
    let timestamp = Timestamp(sequence.0.saturating_mul(100));
    nimbus_core::HistoricalReadSnapshot::new(
        nimbus_core::ReadTimestamp::new(timestamp),
        nimbus_core::CommitSequence::new(sequence),
        nimbus_core::CommitTimestamp::new(timestamp),
    )
}

pub(super) fn libsql_rank_full_scan_oracle_titles(
    store: &crate::LibsqlReplicaTenantStore,
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
        .map(|document| libsql_document_title_string(&document))
        .collect::<Vec<_>>();
    titles.sort();
    titles
}

pub(super) fn libsql_status_rank_full_scan_oracle_titles(
    store: &crate::LibsqlReplicaTenantStore,
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
                Some((rank, libsql_document_title_string(&document)))
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    rows.sort_by(|left, right| left.0.cmp(&right.0).then_with(|| left.1.cmp(&right.1)));
    rows.into_iter().map(|(_, title)| title).collect()
}

pub(super) fn libsql_document_titles(documents: &[Document]) -> Vec<&str> {
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

pub(super) fn libsql_document_title_strings(documents: &[Document]) -> Vec<String> {
    documents.iter().map(libsql_document_title_string).collect()
}

pub(super) fn libsql_document_title_string(document: &Document) -> String {
    document
        .fields
        .get("title")
        .and_then(|value| value.as_str())
        .expect("document should have a string title")
        .to_string()
}

pub(super) fn libsql_active_table_id_for_diagnostic(
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

// Test-only helper mirroring `WriteOp` field-by-field; call sites pass
// distinctly-typed newtypes positionally, so a wrapper struct would only add
// call-site ceremony without reducing risk of mixups.
#[allow(clippy::too_many_arguments)]
pub(super) fn libsql_durable_write_record(
    sequence: SequenceNumber,
    timestamp: Timestamp,
    table: &TableName,
    table_id: &TableId,
    op_type: WriteOpType,
    doc_id: DocumentId,
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

pub(super) fn allocate_host_port() -> u16 {
    TcpListener::bind(("127.0.0.1", 0))
        .expect("temporary port probe should bind")
        .local_addr()
        .expect("temporary port probe should resolve")
        .port()
}

pub(super) async fn seed_remote_namespace(
    config: &LibsqlReplicaProviderConfig,
    namespace: &str,
    table_schema: &TableSchema,
    document_id: DocumentId,
    fields: serde_json::Value,
) {
    let database = open_remote_namespace_database(config, namespace)
        .await
        .expect("remote namespace database should open");
    let conn = database
        .connect()
        .expect("remote namespace connection should open");
    conn.execute(
        "INSERT INTO schemas (table_name, schema_json) VALUES (?, ?)",
        libsql::params![
            table_schema.table.as_str(),
            serde_json::to_string(table_schema).expect("schema should serialize")
        ],
    )
    .await
    .expect("remote schema insert should succeed");
    let table_id = TableId::new();
    conn.execute(
        "INSERT INTO table_catalog (namespace, table_name, table_id) VALUES (?, ?, ?)",
        libsql::params!["default", table_schema.table.as_str(), table_id.as_str()],
    )
    .await
    .expect("remote table catalog insert should succeed");
    conn.execute(
        "INSERT INTO documents (table_id, id, data_json, typed_fields_json, creation_time, update_time) VALUES (?, ?, ?, ?, ?, ?)",
        libsql::params![
            table_id.as_str(),
            document_id.to_string(),
            fields.to_string(),
            "{}",
            7_i64,
            7_i64
        ],
    )
    .await
    .expect("remote document insert should succeed");
}

pub(super) async fn open_remote_namespace_database(
    config: &LibsqlReplicaProviderConfig,
    namespace: &str,
) -> nimbus_core::Result<Database> {
    let builder = Builder::new_remote(
        config.primary_url.clone(),
        config.auth_token.clone().unwrap_or_default(),
    )
    .namespace(namespace.to_string())
    .connector(libsql_transport_connector()?);
    builder.build().await.map_err(|error| {
        nimbus_core::Error::storage(nimbus_core::StorageErrorKind::Other, error.to_string())
    })
}
