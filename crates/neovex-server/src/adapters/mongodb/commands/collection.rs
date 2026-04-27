use std::sync::Arc;

use neovex_core::{TableName, TableSchema};
use neovex_engine::Service;

use super::super::error::{BAD_VALUE, MongoError};
use super::tenant::{DEFAULT_TENANT, ensure_tenant, resolve_tenant};

pub fn create(body: &bson::Document, service: &Arc<Service>) -> Result<bson::Document, MongoError> {
    let collection = body.get_str("create").map_err(|_| MongoError::Command {
        code: BAD_VALUE.code,
        code_name: BAD_VALUE.code_name.into(),
        message: "missing collection name in create command".into(),
    })?;

    let db_name = body.get_str("$db").unwrap_or(DEFAULT_TENANT);
    let tenant_id = resolve_tenant(db_name)?;
    let table = TableName::new(collection).map_err(MongoError::from)?;

    ensure_tenant(service, &tenant_id)?;

    let schema = service.get_schema(&tenant_id).map_err(MongoError::from)?;
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
    service
        .set_table_schema(&tenant_id, table_schema)
        .map_err(MongoError::from)?;

    Ok(bson::doc! { "ok": 1.0 })
}

pub fn drop_collection(
    body: &bson::Document,
    service: &Arc<Service>,
) -> Result<bson::Document, MongoError> {
    let collection = body.get_str("drop").map_err(|_| MongoError::Command {
        code: BAD_VALUE.code,
        code_name: BAD_VALUE.code_name.into(),
        message: "missing collection name in drop command".into(),
    })?;

    let db_name = body.get_str("$db").unwrap_or(DEFAULT_TENANT);
    let tenant_id = resolve_tenant(db_name)?;
    let table = TableName::new(collection).map_err(MongoError::from)?;

    ensure_tenant(service, &tenant_id)?;

    let schema = service.get_schema(&tenant_id).map_err(MongoError::from)?;
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

    service
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
    service: &Arc<Service>,
) -> Result<bson::Document, MongoError> {
    let db_name = body.get_str("$db").unwrap_or(DEFAULT_TENANT);
    let tenant_id = resolve_tenant(db_name)?;
    let name_only = body.get_bool("nameOnly").unwrap_or(false);
    let filter = body.get_document("filter").ok();

    ensure_tenant(service, &tenant_id)?;

    let schema = service.get_schema(&tenant_id).map_err(MongoError::from)?;

    let mut collections: Vec<bson::Bson> = Vec::new();
    for table_name in schema.tables.keys() {
        let name = table_name.as_str();

        if let Some(f) = filter {
            if let Ok(filter_name) = f.get_str("name") {
                if name != filter_name {
                    continue;
                }
            }
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

pub fn list_databases(
    _body: &bson::Document,
    service: &Arc<Service>,
) -> Result<bson::Document, MongoError> {
    let tenants = service.list_tenants().map_err(MongoError::from)?;

    let mut databases: Vec<bson::Bson> = Vec::new();
    for tenant_id in &tenants {
        let name = tenant_id.as_str();
        databases.push(bson::Bson::Document(bson::doc! {
            "name": name,
            "sizeOnDisk": 0_i64,
            "empty": false,
        }));
    }

    let total_size = 0_i64;
    Ok(bson::doc! {
        "databases": databases,
        "totalSize": total_size,
        "ok": 1.0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::mongodb::commands::crud;
    use crate::adapters::mongodb::connection::ConnectionState;
    use neovex_core::TenantId;
    use neovex_testing::ServiceFixture;

    fn test_conn() -> ConnectionState {
        ConnectionState::new(([127, 0, 0, 1], 12345).into())
    }

    fn seed_collection(fixture: &ServiceFixture<Service>, collection: &str) {
        let body = bson::doc! {
            "insert": collection,
            "$db": "testdb",
            "documents": [{ "_id": "tmp", "val": 1 }],
        };
        crud::insert(&body, &mut test_conn(), &fixture.service()).unwrap();
    }

    #[test]
    fn create_collection_succeeds() {
        let fixture = ServiceFixture::new(|path| Service::new(path));
        let tenant_id = TenantId::new("testdb").unwrap();
        let _ = fixture.service().create_tenant(tenant_id);

        let body = bson::doc! { "create": "newcol", "$db": "testdb" };
        let result = create(&body, &fixture.service()).unwrap();
        assert_eq!(result.get_f64("ok").unwrap(), 1.0);
    }

    #[test]
    fn create_duplicate_collection_returns_error() {
        let fixture = ServiceFixture::new(|path| Service::new(path));
        let tenant_id = TenantId::new("testdb").unwrap();
        let _ = fixture.service().create_tenant(tenant_id);

        let body = bson::doc! { "create": "dupcol", "$db": "testdb" };
        create(&body, &fixture.service()).unwrap();

        let err = create(&body, &fixture.service()).unwrap_err();
        match err {
            MongoError::Command { code, .. } => assert_eq!(code, 48),
            other => panic!("expected Command, got {:?}", other),
        }
    }

    #[test]
    fn drop_existing_collection() {
        let fixture = ServiceFixture::new(|path| Service::new(path));
        let tenant_id = TenantId::new("testdb").unwrap();
        let _ = fixture.service().create_tenant(tenant_id);

        let create_body = bson::doc! { "create": "todrop", "$db": "testdb" };
        create(&create_body, &fixture.service()).unwrap();

        let body = bson::doc! { "drop": "todrop", "$db": "testdb" };
        let result = drop_collection(&body, &fixture.service()).unwrap();
        assert_eq!(result.get_f64("ok").unwrap(), 1.0);
    }

    #[test]
    fn drop_nonexistent_collection_returns_not_found() {
        let fixture = ServiceFixture::new(|path| Service::new(path));
        let tenant_id = TenantId::new("testdb").unwrap();
        let _ = fixture.service().create_tenant(tenant_id);

        let body = bson::doc! { "drop": "nosuch", "$db": "testdb" };
        let result = drop_collection(&body, &fixture.service()).unwrap();
        assert_eq!(result.get_f64("ok").unwrap(), 0.0);
        assert_eq!(result.get_i32("code").unwrap(), 26);
    }

    #[test]
    fn list_collections_returns_tables() {
        let fixture = ServiceFixture::new(|path| Service::new(path));
        seed_collection(&fixture, "alpha");
        seed_collection(&fixture, "beta");

        let body = bson::doc! { "listCollections": 1, "$db": "testdb" };
        let result = list_collections(&body, &fixture.service()).unwrap();
        assert_eq!(result.get_f64("ok").unwrap(), 1.0);
        let cursor = result.get_document("cursor").unwrap();
        let batch = cursor.get_array("firstBatch").unwrap();
        assert!(batch.len() >= 2);
    }

    #[test]
    fn list_collections_name_only() {
        let fixture = ServiceFixture::new(|path| Service::new(path));
        seed_collection(&fixture, "gamma");

        let body = bson::doc! {
            "listCollections": 1,
            "$db": "testdb",
            "nameOnly": true,
        };
        let result = list_collections(&body, &fixture.service()).unwrap();
        let cursor = result.get_document("cursor").unwrap();
        let batch = cursor.get_array("firstBatch").unwrap();
        assert!(!batch.is_empty());
        let doc = batch[0].as_document().unwrap();
        assert!(doc.get_str("name").is_ok());
        assert!(doc.get("type").is_none());
    }

    #[test]
    fn list_collections_with_name_filter() {
        let fixture = ServiceFixture::new(|path| Service::new(path));
        seed_collection(&fixture, "target");
        seed_collection(&fixture, "other");

        let body = bson::doc! {
            "listCollections": 1,
            "$db": "testdb",
            "filter": { "name": "target" },
        };
        let result = list_collections(&body, &fixture.service()).unwrap();
        let cursor = result.get_document("cursor").unwrap();
        let batch = cursor.get_array("firstBatch").unwrap();
        assert_eq!(batch.len(), 1);
        let doc = batch[0].as_document().unwrap();
        assert_eq!(doc.get_str("name").unwrap(), "target");
    }

    #[test]
    fn list_databases_returns_tenants() {
        let fixture = ServiceFixture::new(|path| Service::new(path));
        seed_collection(&fixture, "col1");

        let body = bson::doc! { "listDatabases": 1 };
        let result = list_databases(&body, &fixture.service()).unwrap();
        assert_eq!(result.get_f64("ok").unwrap(), 1.0);
        let databases = result.get_array("databases").unwrap();
        assert!(!databases.is_empty());
    }

    #[test]
    fn create_missing_name_returns_error() {
        let fixture = ServiceFixture::new(|path| Service::new(path));
        let body = bson::doc! { "$db": "testdb" };
        let err = create(&body, &fixture.service()).unwrap_err();
        match err {
            MongoError::Command { code, .. } => assert_eq!(code, BAD_VALUE.code),
            other => panic!("expected Command, got {:?}", other),
        }
    }

    #[test]
    fn drop_missing_name_returns_error() {
        let fixture = ServiceFixture::new(|path| Service::new(path));
        let body = bson::doc! { "$db": "testdb" };
        let err = drop_collection(&body, &fixture.service()).unwrap_err();
        match err {
            MongoError::Command { code, .. } => assert_eq!(code, BAD_VALUE.code),
            other => panic!("expected Command, got {:?}", other),
        }
    }
}
