use std::sync::Arc;

use nimbus_core::{PrincipalContext, TableName, TableSchema};
use nimbus_engine::Engine;

use super::super::error::{BAD_VALUE, MongoError};
use super::tenant::{DEFAULT_TENANT, ensure_tenant, resolve_tenant_context};

pub fn create(
    body: &bson::Document,
    engine: &Arc<Engine>,
    principal: &PrincipalContext,
) -> Result<bson::Document, MongoError> {
    let collection = body.get_str("create").map_err(|_| MongoError::Command {
        code: BAD_VALUE.code,
        code_name: BAD_VALUE.code_name.into(),
        message: "missing collection name in create command".into(),
    })?;

    let db_name = body.get_str("$db").unwrap_or(DEFAULT_TENANT);
    let tenant_context = resolve_tenant_context(db_name, "mongodb create collection", principal)?;
    let tenant_id = tenant_context.tenant_id().clone();
    let table = TableName::new(collection).map_err(MongoError::from)?;

    ensure_tenant(engine, &tenant_context)?;

    let schema = engine.get_schema(&tenant_id).map_err(MongoError::from)?;
    if schema.tables.contains_key(&table) {
        return Err(MongoError::Command {
            code: 48,
            code_name: "NamespaceExists".into(),
            message: format!("Collection already exists. NS: {db_name}.{collection}"),
        });
    }

    let table_schema = TableSchema {
        table: table.clone(),
        fields: vec![],
        indexes: vec![],
        access_policy: None,
    };
    engine
        .set_table_schema(&tenant_id, table_schema)
        .map_err(MongoError::from)?;

    Ok(bson::doc! { "ok": 1.0 })
}

pub fn drop_collection(
    body: &bson::Document,
    engine: &Arc<Engine>,
    principal: &PrincipalContext,
) -> Result<bson::Document, MongoError> {
    let collection = body.get_str("drop").map_err(|_| MongoError::Command {
        code: BAD_VALUE.code,
        code_name: BAD_VALUE.code_name.into(),
        message: "missing collection name in drop command".into(),
    })?;

    let db_name = body.get_str("$db").unwrap_or(DEFAULT_TENANT);
    let tenant_context = resolve_tenant_context(db_name, "mongodb drop collection", principal)?;
    let tenant_id = tenant_context.tenant_id().clone();
    let table = TableName::new(collection).map_err(MongoError::from)?;

    ensure_tenant(engine, &tenant_context)?;

    let schema = engine.get_schema(&tenant_id).map_err(MongoError::from)?;
    if !schema.tables.contains_key(&table) {
        return Ok(bson::doc! {
            "ok": 0.0,
            "errmsg": format!("ns not found: {db_name}.{collection}"),
            "code": 26,
            "codeName": "NamespaceNotFound",
        });
    }

    let n_indexes = schema
        .tables
        .get(&table)
        .map(|s| s.indexes.len() + 1)
        .unwrap_or(1) as i32;

    engine
        .delete_table_schema(&tenant_id, &table)
        .map_err(MongoError::from)?;

    Ok(bson::doc! {
        "nIndexesWas": n_indexes,
        "ns": format!("{db_name}.{collection}"),
        "ok": 1.0,
    })
}

pub fn list_collections(
    body: &bson::Document,
    engine: &Arc<Engine>,
    principal: &PrincipalContext,
) -> Result<bson::Document, MongoError> {
    let db_name = body.get_str("$db").unwrap_or(DEFAULT_TENANT);
    let tenant_context = resolve_tenant_context(db_name, "mongodb list collections", principal)?;
    let tenant_id = tenant_context.tenant_id().clone();
    let name_only = body.get_bool("nameOnly").unwrap_or(false);
    let filter = body.get_document("filter").ok();

    ensure_tenant(engine, &tenant_context)?;

    let schema = engine.get_schema(&tenant_id).map_err(MongoError::from)?;

    let mut collections: Vec<bson::Bson> = Vec::new();
    for table_name in schema.tables.keys() {
        let name = table_name.as_str();

        if let Some(f) = filter
            && let Ok(filter_name) = f.get_str("name")
            && name != filter_name
        {
            continue;
        }

        if name_only {
            collections.push(bson::Bson::Document(bson::doc! { "name": name }));
        } else {
            collections.push(bson::Bson::Document(bson::doc! {
                "name": name,
                "type": "collection",
                "options": {},
                "info": { "readOnly": false },
            }));
        }
    }

    Ok(bson::doc! {
        "cursor": {
            "firstBatch": collections,
            "id": 0_i64,
            "ns": format!("{db_name}.$cmd.listCollections"),
        },
        "ok": 1.0,
    })
}

#[cfg(test)]
pub fn list_databases(
    _body: &bson::Document,
    engine: &Arc<Engine>,
    _principal: &PrincipalContext,
) -> Result<bson::Document, MongoError> {
    let tenants = engine.list_tenants().map_err(MongoError::from)?;
    Ok(render_databases(&tenants))
}

pub async fn list_databases_async(
    _body: &bson::Document,
    engine: &Arc<Engine>,
    _principal: &PrincipalContext,
) -> Result<bson::Document, MongoError> {
    let tenants = engine
        .list_tenants_async()
        .await
        .map_err(MongoError::from)?;
    Ok(render_databases(&tenants))
}

fn render_databases(tenants: &[nimbus_core::TenantId]) -> bson::Document {
    let mut databases: Vec<bson::Bson> = Vec::new();
    for tenant_id in tenants {
        let name = tenant_id.as_str();
        databases.push(bson::Bson::Document(bson::doc! {
            "name": name,
            "sizeOnDisk": 0_i64,
            "empty": false,
        }));
    }

    let total_size = 0_i64;
    bson::doc! {
        "databases": databases,
        "totalSize": total_size,
        "ok": 1.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::crud;
    use crate::connection::ConnectionState;
    use nimbus_core::TenantId;
    use nimbus_testing::EngineFixture;

    fn test_conn() -> ConnectionState {
        ConnectionState::new(([127, 0, 0, 1], 12345).into())
    }

    fn test_principal() -> PrincipalContext {
        PrincipalContext {
            authenticated: true,
            claims: serde_json::Map::from_iter([(
                "subject".to_string(),
                serde_json::json!("mongodb-test-user"),
            )]),
            verified_claims: serde_json::Map::new(),
        }
    }

    fn seed_collection(fixture: &EngineFixture<Engine>, collection: &str) {
        let principal = test_principal();
        let body = bson::doc! {
            "insert": collection,
            "$db": "testdb",
            "documents": [{ "_id": "tmp", "val": 1 }],
        };
        crud::insert(&body, &mut test_conn(), &fixture.engine(), &principal).unwrap();
    }

    #[test]
    fn create_collection_succeeds() {
        let fixture = EngineFixture::new(|path| Engine::new(path));
        let tenant_id = TenantId::new("testdb").unwrap();
        let _ = fixture.engine().create_tenant(tenant_id);

        let body = bson::doc! { "create": "newcol", "$db": "testdb" };
        let result = create(&body, &fixture.engine(), &test_principal()).unwrap();
        assert_eq!(result.get_f64("ok").unwrap(), 1.0);
    }

    #[test]
    fn create_duplicate_collection_returns_error() {
        let fixture = EngineFixture::new(|path| Engine::new(path));
        let tenant_id = TenantId::new("testdb").unwrap();
        let _ = fixture.engine().create_tenant(tenant_id);

        let body = bson::doc! { "create": "dupcol", "$db": "testdb" };
        create(&body, &fixture.engine(), &test_principal()).unwrap();

        let err = create(&body, &fixture.engine(), &test_principal()).unwrap_err();
        match err {
            MongoError::Command { code, .. } => assert_eq!(code, 48),
            other => panic!("expected Command, got {:?}", other),
        }
    }

    #[test]
    fn drop_existing_collection() {
        let fixture = EngineFixture::new(|path| Engine::new(path));
        let tenant_id = TenantId::new("testdb").unwrap();
        let _ = fixture.engine().create_tenant(tenant_id);

        let create_body = bson::doc! { "create": "todrop", "$db": "testdb" };
        create(&create_body, &fixture.engine(), &test_principal()).unwrap();

        let body = bson::doc! { "drop": "todrop", "$db": "testdb" };
        let result = drop_collection(&body, &fixture.engine(), &test_principal()).unwrap();
        assert_eq!(result.get_f64("ok").unwrap(), 1.0);
    }

    #[test]
    fn drop_nonexistent_collection_returns_not_found() {
        let fixture = EngineFixture::new(|path| Engine::new(path));
        let tenant_id = TenantId::new("testdb").unwrap();
        let _ = fixture.engine().create_tenant(tenant_id);

        let body = bson::doc! { "drop": "nosuch", "$db": "testdb" };
        let result = drop_collection(&body, &fixture.engine(), &test_principal()).unwrap();
        assert_eq!(result.get_f64("ok").unwrap(), 0.0);
        assert_eq!(result.get_i32("code").unwrap(), 26);
    }

    #[test]
    fn list_collections_returns_tables() {
        let fixture = EngineFixture::new(|path| Engine::new(path));
        seed_collection(&fixture, "alpha");
        seed_collection(&fixture, "beta");

        let body = bson::doc! { "listCollections": 1, "$db": "testdb" };
        let result = list_collections(&body, &fixture.engine(), &test_principal()).unwrap();
        assert_eq!(result.get_f64("ok").unwrap(), 1.0);
        let cursor = result.get_document("cursor").unwrap();
        let batch = cursor.get_array("firstBatch").unwrap();
        assert!(batch.len() >= 2);
    }

    #[test]
    fn list_collections_name_only() {
        let fixture = EngineFixture::new(|path| Engine::new(path));
        seed_collection(&fixture, "gamma");

        let body = bson::doc! {
            "listCollections": 1,
            "$db": "testdb",
            "nameOnly": true,
        };
        let result = list_collections(&body, &fixture.engine(), &test_principal()).unwrap();
        let cursor = result.get_document("cursor").unwrap();
        let batch = cursor.get_array("firstBatch").unwrap();
        assert!(!batch.is_empty());
        let doc = batch[0].as_document().unwrap();
        assert!(doc.get_str("name").is_ok());
        assert!(doc.get("type").is_none());
    }

    #[test]
    fn list_collections_with_name_filter() {
        let fixture = EngineFixture::new(|path| Engine::new(path));
        seed_collection(&fixture, "target");
        seed_collection(&fixture, "other");

        let body = bson::doc! {
            "listCollections": 1,
            "$db": "testdb",
            "filter": { "name": "target" },
        };
        let result = list_collections(&body, &fixture.engine(), &test_principal()).unwrap();
        let cursor = result.get_document("cursor").unwrap();
        let batch = cursor.get_array("firstBatch").unwrap();
        assert_eq!(batch.len(), 1);
        let doc = batch[0].as_document().unwrap();
        assert_eq!(doc.get_str("name").unwrap(), "target");
    }

    #[test]
    fn list_databases_returns_tenants() {
        let fixture = EngineFixture::new(|path| Engine::new(path));
        seed_collection(&fixture, "col1");

        let body = bson::doc! { "listDatabases": 1 };
        let result = list_databases(&body, &fixture.engine(), &test_principal()).unwrap();
        assert_eq!(result.get_f64("ok").unwrap(), 1.0);
        let databases = result.get_array("databases").unwrap();
        assert!(!databases.is_empty());
    }

    #[tokio::test]
    async fn list_databases_uses_provider_lifecycle() {
        let fixture = EngineFixture::new(|path| Engine::new_with_memory_persistence(path));
        fixture
            .engine()
            .ensure_tenant_ready_async(TenantId::new("testdb").unwrap())
            .await
            .expect("provider tenant admission");

        let body = bson::doc! { "listDatabases": 1 };
        let result = list_databases_async(&body, &fixture.engine(), &test_principal())
            .await
            .expect("provider-capable database listing");
        let databases = result.get_array("databases").unwrap();
        assert!(databases.iter().any(|database| {
            database
                .as_document()
                .and_then(|database| database.get_str("name").ok())
                == Some("testdb")
        }));
    }

    #[test]
    fn create_missing_name_returns_error() {
        let fixture = EngineFixture::new(|path| Engine::new(path));
        let body = bson::doc! { "$db": "testdb" };
        let err = create(&body, &fixture.engine(), &test_principal()).unwrap_err();
        match err {
            MongoError::Command { code, .. } => assert_eq!(code, BAD_VALUE.code),
            other => panic!("expected Command, got {:?}", other),
        }
    }

    #[test]
    fn drop_missing_name_returns_error() {
        let fixture = EngineFixture::new(|path| Engine::new(path));
        let body = bson::doc! { "$db": "testdb" };
        let err = drop_collection(&body, &fixture.engine(), &test_principal()).unwrap_err();
        match err {
            MongoError::Command { code, .. } => assert_eq!(code, BAD_VALUE.code),
            other => panic!("expected Command, got {:?}", other),
        }
    }
}
