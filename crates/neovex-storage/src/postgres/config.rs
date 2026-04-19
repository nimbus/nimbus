use super::*;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PostgresProviderConfig {
    pub connection_string: String,
    pub metadata_schema: String,
    pub tenant_schema_prefix: String,
    pub min_connections: Option<usize>,
    pub max_connections: Option<usize>,
}

impl PostgresProviderConfig {
    pub fn new(connection_string: impl Into<String>) -> Self {
        Self {
            connection_string: connection_string.into(),
            metadata_schema: "neovex_provider".to_string(),
            tenant_schema_prefix: "tenant_".to_string(),
            min_connections: None,
            max_connections: None,
        }
    }

    pub fn derived_pool_application_name(&self) -> Result<String> {
        postgres_pool_application_name(self)
    }

    pub fn derived_notification_channel_name(&self) -> Result<String> {
        postgres_notification_channel_name(self)
    }
}

pub(super) fn build_pool(
    config: &PostgresProviderConfig,
    pool_application_name: &str,
) -> Result<Pool> {
    if let (Some(min_connections), Some(max_connections)) =
        (config.min_connections, config.max_connections)
        && min_connections > max_connections
    {
        return Err(Error::InvalidInput(
            "postgres pool min_connections cannot exceed max_connections".to_string(),
        ));
    }

    let mut connection_config =
        PostgresConfig::from_str(&config.connection_string).map_err(map_postgres_error)?;
    if connection_config.get_application_name().is_none() {
        connection_config.application_name(pool_application_name);
    }
    let manager = Manager::from_config(
        connection_config,
        NoTls,
        ManagerConfig {
            recycling_method: RecyclingMethod::Fast,
        },
    );
    let mut builder = Pool::builder(manager).runtime(Runtime::Tokio1);
    if let Some(max_connections) = config.max_connections {
        builder = builder.max_size(max_connections);
    }
    builder.build().map_err(map_build_error)
}

pub(super) fn postgres_notification_channel_name(
    config: &PostgresProviderConfig,
) -> Result<String> {
    let digest = Sha256::digest(
        format!("{}:{}", config.metadata_schema, config.tenant_schema_prefix).as_bytes(),
    );
    let available_hex_len = POSTGRES_IDENTIFIER_LIMIT
        .checked_sub(POSTGRES_NOTIFICATION_CHANNEL_PREFIX.len())
        .ok_or_else(|| {
            Error::InvalidInput(
                "notification channel prefix is too long for PostgreSQL".to_string(),
            )
        })?;
    let hex_len = available_hex_len.min(16);
    let mut suffix = String::with_capacity(hex_len);
    for byte in digest.iter().take(hex_len / 2) {
        let _ = write!(&mut suffix, "{byte:02x}");
    }
    Ok(format!("{POSTGRES_NOTIFICATION_CHANNEL_PREFIX}{suffix}"))
}

pub(super) fn postgres_pool_application_name(config: &PostgresProviderConfig) -> Result<String> {
    let digest = Sha256::digest(
        format!("{}:{}", config.metadata_schema, config.tenant_schema_prefix).as_bytes(),
    );
    let available_hex_len = POSTGRES_IDENTIFIER_LIMIT
        .checked_sub(POSTGRES_POOL_APPLICATION_NAME_PREFIX.len())
        .ok_or_else(|| {
            Error::InvalidInput(
                "postgres pool application-name prefix exceeds identifier budget".to_string(),
            )
        })?;
    let hex_len = available_hex_len.min(16);
    let mut suffix = String::with_capacity(hex_len);
    for byte in digest.iter().take(hex_len / 2) {
        let _ = write!(&mut suffix, "{byte:02x}");
    }
    Ok(format!("{POSTGRES_POOL_APPLICATION_NAME_PREFIX}{suffix}"))
}

pub(super) fn tenant_schema_name(prefix: &str, tenant_id: &TenantId) -> Result<String> {
    let available_hex_len = POSTGRES_IDENTIFIER_LIMIT
        .checked_sub(prefix.len())
        .ok_or_else(|| {
            Error::InvalidInput("tenant schema prefix is too long for PostgreSQL".to_string())
        })?;
    let bounded_hex_len = available_hex_len.min(TARGET_TENANT_HASH_HEX_LEN);
    let hash_hex_len = bounded_hex_len - (bounded_hex_len % 2);
    if hash_hex_len < MIN_TENANT_HASH_HEX_LEN {
        return Err(Error::InvalidInput(
            "tenant schema prefix leaves too little room for a safe tenant hash".to_string(),
        ));
    }

    let digest = Sha256::digest(tenant_id.as_str().as_bytes());
    let mut hash = String::with_capacity(hash_hex_len);
    for byte in digest.iter().take(hash_hex_len / 2) {
        let _ = write!(&mut hash, "{byte:02x}");
    }
    Ok(format!("{prefix}{hash}"))
}

pub(super) fn validate_identifier_input(value: &str, label: &str) -> Result<()> {
    if value.is_empty() {
        return Err(Error::InvalidInput(format!("{label} cannot be empty")));
    }
    if value.len() >= POSTGRES_IDENTIFIER_LIMIT {
        return Err(Error::InvalidInput(format!(
            "{label} must be shorter than {POSTGRES_IDENTIFIER_LIMIT} bytes for PostgreSQL"
        )));
    }
    Ok(())
}

pub(super) fn quote_identifier(identifier: &str) -> String {
    let mut quoted = String::with_capacity(identifier.len() + 2);
    quoted.push('"');
    for character in identifier.chars() {
        if character == '"' {
            quoted.push('"');
        }
        quoted.push(character);
    }
    quoted.push('"');
    quoted
}

pub(super) fn quote_literal(value: &str) -> String {
    let mut quoted = String::with_capacity(value.len() + 2);
    quoted.push('\'');
    for character in value.chars() {
        if character == '\'' {
            quoted.push('\'');
        }
        quoted.push(character);
    }
    quoted.push('\'');
    quoted
}

pub(super) fn qualified_table(schema_name: &str, table_name: &str) -> String {
    format!(
        "{}.{}",
        quote_identifier(schema_name),
        quote_identifier(table_name)
    )
}

pub(super) fn tenant_init_sql(schema_name: &str) -> String {
    format!(
        "CREATE TABLE IF NOT EXISTS {} (\
            table_name TEXT NOT NULL,\
            id TEXT NOT NULL,\
            data_json TEXT NOT NULL,\
            creation_time BIGINT NOT NULL,\
            PRIMARY KEY (table_name, id)\
        );\
        CREATE TABLE IF NOT EXISTS {} (\
            table_name TEXT PRIMARY KEY,\
            schema_json TEXT NOT NULL\
        );\
        CREATE TABLE IF NOT EXISTS {} (\
            execution_id TEXT PRIMARY KEY\
        );\
        CREATE TABLE IF NOT EXISTS {} (\
            id TEXT PRIMARY KEY,\
            run_at BIGINT NOT NULL,\
            data_json TEXT NOT NULL\
        );\
        CREATE TABLE IF NOT EXISTS {} (\
            id TEXT PRIMARY KEY,\
            data_json TEXT NOT NULL\
        );\
        CREATE TABLE IF NOT EXISTS {} (\
            job_id TEXT PRIMARY KEY,\
            data_json TEXT NOT NULL\
        );\
        CREATE TABLE IF NOT EXISTS {} (\
            name TEXT PRIMARY KEY,\
            next_run BIGINT NOT NULL,\
            enabled BOOLEAN NOT NULL,\
            data_json TEXT NOT NULL\
        );\
        CREATE TABLE IF NOT EXISTS {} (\
            sequence BIGINT PRIMARY KEY,\
            record_blob BYTEA NOT NULL\
        );\
        CREATE TABLE IF NOT EXISTS {} (\
            key TEXT PRIMARY KEY,\
            value_blob BYTEA NOT NULL\
        );",
        qualified_table(schema_name, "documents"),
        qualified_table(schema_name, "schemas"),
        qualified_table(schema_name, "scheduled_job_executions"),
        qualified_table(schema_name, "scheduled_jobs"),
        qualified_table(schema_name, "running_scheduled_jobs"),
        qualified_table(schema_name, "scheduled_job_results"),
        qualified_table(schema_name, "cron_jobs"),
        qualified_table(schema_name, "commit_log"),
        qualified_table(schema_name, "metadata"),
    )
}
