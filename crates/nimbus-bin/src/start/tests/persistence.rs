use super::*;

#[test]
fn cli_builds_postgres_typed_config_with_overrides() {
    let cli = parse_start([
        "nimbus",
        "start",
        "--tenant-provider",
        "postgres",
        "--control-data-dir",
        "./control",
        "--data-dir",
        "./ignored-for-postgres",
        "--postgres-url",
        "host=/tmp user=jack dbname=postgres",
        "--postgres-metadata-schema",
        "provider_meta",
        "--postgres-tenant-schema-prefix",
        "tenant_pg_",
        "--postgres-min-connections",
        "2",
        "--postgres-max-connections",
        "8",
    ]);
    let config = persistence_config_from_sources(
        &cli,
        &PersistenceFileConfig::default(),
        &PersistenceEnv::default(),
    )
    .expect("postgres config should build");
    assert_eq!(
        config.control_plane,
        nimbus::ControlPlaneConfig::embedded_redb("./control")
    );
    assert_eq!(
        config.tenant_provider.dialect,
        nimbus::PersistenceDialect::Postgres
    );
    assert_eq!(
        config.tenant_provider.topology,
        nimbus::PersistenceTopology::ExternalPrimary
    );
    assert_eq!(
        config.tenant_provider.credentials,
        nimbus::ProviderCredentials::ConnectionString(
            "host=/tmp user=jack dbname=postgres".to_string()
        )
    );
    assert_eq!(config.tenant_provider.pool.min_connections, Some(2));
    assert_eq!(config.tenant_provider.pool.max_connections, Some(8));
    assert_eq!(
        config.tenant_provider.routing,
        nimbus::TenantRoutingConfig::SchemaPerTenant {
            metadata_schema: "provider_meta".to_string(),
            tenant_schema_prefix: "tenant_pg_".to_string(),
        }
    );
}

#[test]
fn env_builds_postgres_typed_config_with_generic_resource_name() {
    let cli = parse_start(["nimbus", "start"]);
    let env = PersistenceEnv {
        tenant_provider: Some(CliTenantProvider::Postgres),
        control_data_dir: Some(PathBuf::from("./control-from-env")),
        postgres_url: Some("host=/tmp user=jack dbname=postgres".to_string()),
        postgres_min_connections: Some(3),
        postgres_max_connections: Some(9),
        ..PersistenceEnv::default()
    };

    let config = persistence_config_from_sources(&cli, &PersistenceFileConfig::default(), &env)
        .expect("env-backed postgres config should build");

    assert_eq!(
        config.control_plane,
        nimbus::ControlPlaneConfig::embedded_redb("./control-from-env")
    );
    assert_eq!(
        config.tenant_provider.credentials,
        nimbus::ProviderCredentials::ConnectionString(
            "host=/tmp user=jack dbname=postgres".to_string()
        )
    );
    assert_eq!(config.tenant_provider.pool.min_connections, Some(3));
    assert_eq!(config.tenant_provider.pool.max_connections, Some(9));
}

#[test]
fn cli_builds_libsql_replica_typed_config_with_overrides() {
    let cli = parse_start([
        "nimbus",
        "start",
        "--tenant-provider",
        "libsql-replica",
        "--control-data-dir",
        "./control",
        "--libsql-url",
        "libsql://127.0.0.1:8080",
        "--libsql-auth-token",
        "replica-secret",
        "--libsql-admin-url",
        "http://127.0.0.1:8081",
        "--libsql-admin-auth-header",
        "Bearer replica-admin",
        "--libsql-metadata-namespace",
        "provider_meta",
        "--libsql-tenant-namespace-prefix",
        "tenant_sqlite_",
        "--libsql-replica-cache-dir",
        "./replica-cache",
    ]);
    let config = persistence_config_from_sources(
        &cli,
        &PersistenceFileConfig::default(),
        &PersistenceEnv::default(),
    )
    .expect("libsql replica config should build");
    assert_eq!(
        config.control_plane,
        nimbus::ControlPlaneConfig::embedded_redb("./control")
    );
    assert_eq!(
        config.tenant_provider.dialect,
        nimbus::PersistenceDialect::Sqlite
    );
    assert_eq!(
        config.tenant_provider.topology,
        nimbus::PersistenceTopology::ExternalPrimaryWithReplicas
    );
    assert_eq!(
        config.tenant_provider.credentials,
        nimbus::ProviderCredentials::LibsqlReplica {
            primary_url: "libsql://127.0.0.1:8080".to_string(),
            auth_token: Some("replica-secret".to_string()),
            admin_api_url: "http://127.0.0.1:8081".to_string(),
            admin_auth_header: Some("Bearer replica-admin".to_string()),
        }
    );
    assert_eq!(
        config.tenant_provider.routing,
        nimbus::TenantRoutingConfig::NamespacePerTenant {
            metadata_namespace: "provider_meta".to_string(),
            tenant_namespace_prefix: "tenant_sqlite_".to_string(),
            replica_cache_dir: PathBuf::from("./replica-cache"),
        }
    );
}

#[test]
fn env_builds_libsql_replica_typed_config_with_generic_resource_name() {
    let cli = parse_start(["nimbus", "start"]);
    let env = PersistenceEnv {
        tenant_provider: Some(CliTenantProvider::LibsqlReplica),
        control_data_dir: Some(PathBuf::from("./control-from-env")),
        libsql_url: Some("libsql://127.0.0.1:8080".to_string()),
        libsql_admin_url: Some("http://127.0.0.1:8081".to_string()),
        libsql_replica_cache_dir: Some(PathBuf::from("./replica-cache-from-env")),
        ..PersistenceEnv::default()
    };

    let config = persistence_config_from_sources(&cli, &PersistenceFileConfig::default(), &env)
        .expect("env-backed libsql replica config should build");

    assert_eq!(
        config.control_plane,
        nimbus::ControlPlaneConfig::embedded_redb("./control-from-env")
    );
    assert_eq!(
        config.tenant_provider.credentials,
        nimbus::ProviderCredentials::LibsqlReplica {
            primary_url: "libsql://127.0.0.1:8080".to_string(),
            auth_token: None,
            admin_api_url: "http://127.0.0.1:8081".to_string(),
            admin_auth_header: None,
        }
    );
    assert_eq!(
        config.tenant_provider.routing,
        nimbus::TenantRoutingConfig::NamespacePerTenant {
            metadata_namespace: "nimbus_provider".to_string(),
            tenant_namespace_prefix: "tenant_".to_string(),
            replica_cache_dir: PathBuf::from("./replica-cache-from-env"),
        }
    );
}

#[test]
fn cli_builds_mysql_typed_config_with_overrides() {
    let cli = parse_start([
        "nimbus",
        "start",
        "--tenant-provider",
        "mysql",
        "--control-data-dir",
        "./control",
        "--data-dir",
        "./ignored-for-mysql",
        "--mysql-url",
        "mysql://root:password@127.0.0.1:3306/nimbus",
        "--mysql-metadata-database",
        "provider_meta",
        "--mysql-tenant-database-prefix",
        "tenant_mysql_",
        "--mysql-min-connections",
        "2",
        "--mysql-max-connections",
        "8",
    ]);
    let config = persistence_config_from_sources(
        &cli,
        &PersistenceFileConfig::default(),
        &PersistenceEnv::default(),
    )
    .expect("mysql config should build");
    assert_eq!(
        config.control_plane,
        nimbus::ControlPlaneConfig::embedded_redb("./control")
    );
    assert_eq!(
        config.tenant_provider.dialect,
        nimbus::PersistenceDialect::MySql
    );
    assert_eq!(
        config.tenant_provider.topology,
        nimbus::PersistenceTopology::ExternalPrimary
    );
    assert_eq!(
        config.tenant_provider.credentials,
        nimbus::ProviderCredentials::ConnectionString(
            "mysql://root:password@127.0.0.1:3306/nimbus".to_string()
        )
    );
    assert_eq!(config.tenant_provider.pool.min_connections, Some(2));
    assert_eq!(config.tenant_provider.pool.max_connections, Some(8));
    assert_eq!(
        config.tenant_provider.routing,
        nimbus::TenantRoutingConfig::DatabasePerTenant {
            metadata_database: "provider_meta".to_string(),
            tenant_database_prefix: "tenant_mysql_".to_string(),
        }
    );
}

#[test]
fn env_builds_mysql_typed_config_with_generic_resource_name() {
    let cli = parse_start(["nimbus", "start"]);
    let env = PersistenceEnv {
        tenant_provider: Some(CliTenantProvider::Mysql),
        control_data_dir: Some(PathBuf::from("./control-from-env")),
        mysql_url: Some("mysql://root:password@127.0.0.1:3306/nimbus".to_string()),
        mysql_min_connections: Some(3),
        mysql_max_connections: Some(9),
        ..PersistenceEnv::default()
    };

    let config = persistence_config_from_sources(&cli, &PersistenceFileConfig::default(), &env)
        .expect("env-backed mysql config should build");

    assert_eq!(
        config.control_plane,
        nimbus::ControlPlaneConfig::embedded_redb("./control-from-env")
    );
    assert_eq!(
        config.tenant_provider.credentials,
        nimbus::ProviderCredentials::ConnectionString(
            "mysql://root:password@127.0.0.1:3306/nimbus".to_string()
        )
    );
    assert_eq!(config.tenant_provider.pool.min_connections, Some(3));
    assert_eq!(config.tenant_provider.pool.max_connections, Some(9));
}

#[test]
fn config_file_builds_split_embedded_sqlite_config() {
    let path = write_test_config(
        r#"{
  "persistence": {
    "tenant_provider": "sqlite",
    "data_dir": "./tenant-data",
    "control_data_dir": "./control-data"
  }
}"#,
    );
    let cli = parse_start(["nimbus", "start", "--config", path.to_str().unwrap()]);
    let file_config =
        load_runtime_config_file(Some(path.as_path())).expect("config file should load");

    let config =
        persistence_config_from_sources(&cli, &file_config.persistence, &PersistenceEnv::default())
            .expect("config-backed sqlite config should build");

    assert_eq!(
        config.tenant_provider,
        nimbus::TenantProviderConfig::embedded(
            "./tenant-data",
            nimbus::EmbeddedProviderKind::Sqlite
        )
    );
    assert_eq!(
        config.control_plane,
        nimbus::ControlPlaneConfig::embedded_redb("./control-data")
    );
}

#[test]
fn cli_overrides_config_file_postgres_pool_settings() {
    let path = write_test_config(
        r#"{
  "persistence": {
    "tenant_provider": "postgres",
    "control_data_dir": "./control",
    "postgres_url": "host=/tmp user=jack dbname=postgres",
    "postgres_min_connections": 2,
    "postgres_max_connections": 4
  }
}"#,
    );
    let cli = parse_start([
        "nimbus",
        "start",
        "--config",
        path.to_str().unwrap(),
        "--postgres-max-connections",
        "8",
    ]);
    let file_config =
        load_runtime_config_file(Some(path.as_path())).expect("config file should load");

    let config =
        persistence_config_from_sources(&cli, &file_config.persistence, &PersistenceEnv::default())
            .expect("config + cli postgres config should build");

    assert_eq!(config.tenant_provider.pool.min_connections, Some(2));
    assert_eq!(config.tenant_provider.pool.max_connections, Some(8));
}
