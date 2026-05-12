use std::sync::Arc;

use nimbus_core::{IndexDefinition, TableName, TableSchema, TenantId};
use nimbus_engine::Service;

use super::super::error::{BAD_VALUE, MongoError};
use super::tenant::{DEFAULT_TENANT, ensure_tenant, resolve_tenant};

pub fn create_indexes(
    body: &bson::Document,
    service: &Arc<Service>,
) -> Result<bson::Document, MongoError> {
    let collection = body
        .get_str("createIndexes")
        .map_err(|_| MongoError::Command {
            code: BAD_VALUE.code,
            code_name: BAD_VALUE.code_name.into(),
            message: "missing collection name in createIndexes command".into(),
        })?;

    let db_name = body.get_str("$db").unwrap_or(DEFAULT_TENANT);
    let tenant_id = resolve_tenant(db_name)?;
    let table = TableName::new(collection).map_err(MongoError::from)?;

    let indexes = body.get_array("indexes").map_err(|_| MongoError::Command {
        code: BAD_VALUE.code,
        code_name: BAD_VALUE.code_name.into(),
        message: "missing indexes array in createIndexes command".into(),
    })?;

    ensure_tenant(service, &tenant_id)?;

    let mut table_schema = get_or_create_schema(service, &tenant_id, &table)?;
    let num_before = table_schema.indexes.len();

    for idx_bson in indexes {
        let idx_doc = idx_bson.as_document().ok_or_else(|| MongoError::Command {
            code: BAD_VALUE.code,
            code_name: BAD_VALUE.code_name.into(),
            message: "index specification must be a document".into(),
        })?;

        let key_doc = idx_doc
            .get_document("key")
            .map_err(|_| MongoError::Command {
                code: BAD_VALUE.code,
                code_name: BAD_VALUE.code_name.into(),
                message: "index specification missing key field".into(),
            })?;

        let fields: Vec<String> = key_doc.keys().map(|k| k.to_string()).collect();
        if fields.is_empty() {
            return Err(MongoError::Command {
                code: BAD_VALUE.code,
                code_name: BAD_VALUE.code_name.into(),
                message: "index must have at least one key field".into(),
            });
        }

        let name = match idx_doc.get_str("name") {
            Ok(n) => n.to_string(),
            Err(_) => fields.join("_"),
        };

        if table_schema.indexes.iter().any(|i| i.name == name) {
            continue;
        }

        table_schema.indexes.push(IndexDefinition { name, fields });
    }

    let num_after = table_schema.indexes.len();

    service
        .set_table_schema(&tenant_id, table_schema)
        .map_err(MongoError::from)?;

    Ok(bson::doc! {
        "numIndexesBefore": num_before as i32,
        "numIndexesAfter": num_after as i32,
        "ok": 1.0,
    })
}

pub fn drop_indexes(
    body: &bson::Document,
    service: &Arc<Service>,
) -> Result<bson::Document, MongoError> {
    let collection = body
        .get_str("dropIndexes")
        .map_err(|_| MongoError::Command {
            code: BAD_VALUE.code,
            code_name: BAD_VALUE.code_name.into(),
            message: "missing collection name in dropIndexes command".into(),
        })?;

    let db_name = body.get_str("$db").unwrap_or(DEFAULT_TENANT);
    let tenant_id = resolve_tenant(db_name)?;
    let table = TableName::new(collection).map_err(MongoError::from)?;

    ensure_tenant(service, &tenant_id)?;

    let mut table_schema = match service.get_table_schema(&tenant_id, &table) {
        Ok(schema) => schema,
        Err(nimbus_core::Error::SchemaNotFound(_)) => {
            return Err(MongoError::Command {
                code: 26,
                code_name: "NamespaceNotFound".into(),
                message: format!("ns not found: {db_name}.{collection}"),
            });
        }
        Err(e) => return Err(MongoError::from(e)),
    };

    let index_spec = body.get("index").ok_or_else(|| MongoError::Command {
        code: BAD_VALUE.code,
        code_name: BAD_VALUE.code_name.into(),
        message: "missing index name in dropIndexes command".into(),
    })?;

    match index_spec {
        bson::Bson::String(name) if name == "*" => {
            table_schema.indexes.clear();
        }
        bson::Bson::String(name) => {
            let before = table_schema.indexes.len();
            table_schema.indexes.retain(|i| i.name != *name);
            if table_schema.indexes.len() == before {
                return Err(MongoError::Command {
                    code: 27,
                    code_name: "IndexNotFound".into(),
                    message: format!("index not found with name [{name}]"),
                });
            }
        }
        _ => {
            return Err(MongoError::Command {
                code: BAD_VALUE.code,
                code_name: BAD_VALUE.code_name.into(),
                message: "index to drop must be a string name or \"*\"".into(),
            });
        }
    }

    service
        .set_table_schema(&tenant_id, table_schema)
        .map_err(MongoError::from)?;

    Ok(bson::doc! { "ok": 1.0 })
}

pub fn list_indexes(
    body: &bson::Document,
    service: &Arc<Service>,
) -> Result<bson::Document, MongoError> {
    let collection = body
        .get_str("listIndexes")
        .map_err(|_| MongoError::Command {
            code: BAD_VALUE.code,
            code_name: BAD_VALUE.code_name.into(),
            message: "missing collection name in listIndexes command".into(),
        })?;

    let db_name = body.get_str("$db").unwrap_or(DEFAULT_TENANT);
    let tenant_id = resolve_tenant(db_name)?;
    let table = TableName::new(collection).map_err(MongoError::from)?;

    ensure_tenant(service, &tenant_id)?;

    let table_schema = match service.get_table_schema(&tenant_id, &table) {
        Ok(schema) => schema,
        Err(nimbus_core::Error::SchemaNotFound(_)) => {
            return Err(MongoError::Command {
                code: 26,
                code_name: "NamespaceNotFound".into(),
                message: format!("ns not found: {db_name}.{collection}"),
            });
        }
        Err(e) => return Err(MongoError::from(e)),
    };

    let mut indexes: Vec<bson::Bson> = Vec::new();

    indexes.push(bson::Bson::Document(bson::doc! {
        "v": 2,
        "key": { "_id": 1 },
        "name": "_id_",
    }));

    for idx in &table_schema.indexes {
        let mut key_doc = bson::Document::new();
        for field in &idx.fields {
            key_doc.insert(field, 1);
        }
        indexes.push(bson::Bson::Document(bson::doc! {
            "v": 2,
            "key": key_doc,
            "name": &idx.name,
        }));
    }

    let ns = format!("{db_name}.{collection}");
    Ok(bson::doc! {
        "cursor": {
            "firstBatch": indexes,
            "id": 0_i64,
            "ns": &ns,
        },
        "ok": 1.0,
    })
}

fn get_or_create_schema(
    service: &Arc<Service>,
    tenant_id: &TenantId,
    table: &TableName,
) -> Result<TableSchema, MongoError> {
    match service.get_table_schema(tenant_id, table) {
        Ok(schema) => Ok(schema),
        Err(nimbus_core::Error::SchemaNotFound(_)) => Ok(TableSchema {
            table: table.clone(),
            fields: vec![],
            indexes: vec![],
            access_policy: None,
        }),
        Err(e) => Err(MongoError::from(e)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::mongodb::commands::crud;
    use crate::adapters::mongodb::connection::ConnectionState;
    use nimbus_testing::ServiceFixture;

    fn test_conn() -> ConnectionState {
        ConnectionState::new(([127, 0, 0, 1], 12345).into())
    }

    fn seed(fixture: &ServiceFixture<Service>, collection: &str) {
        let body = bson::doc! {
            "insert": collection,
            "$db": "testdb",
            "documents": [{ "_id": "doc1", "name": "Alice", "age": 30 }],
        };
        crud::insert(&body, &mut test_conn(), &fixture.service()).unwrap();
    }

    fn create_schema_with_fields(fixture: &ServiceFixture<Service>, collection: &str) {
        let tenant_id = TenantId::new("testdb").unwrap();
        let _ = fixture.service().create_tenant(tenant_id.clone());
        let table = TableName::new(collection).unwrap();
        let schema = TableSchema {
            table,
            fields: vec![
                nimbus_core::FieldSchema {
                    name: "name".into(),
                    field_type: nimbus_core::FieldType::String,
                    required: false,
                },
                nimbus_core::FieldSchema {
                    name: "age".into(),
                    field_type: nimbus_core::FieldType::Number,
                    required: false,
                },
            ],
            indexes: vec![],
            access_policy: None,
        };
        fixture
            .service()
            .set_table_schema(&tenant_id, schema)
            .unwrap();
    }

    #[test]
    fn create_indexes_adds_index() {
        let fixture = ServiceFixture::new(|path| Service::new(path));
        create_schema_with_fields(&fixture, "indexed");

        let body = bson::doc! {
            "createIndexes": "indexed",
            "$db": "testdb",
            "indexes": [{
                "key": { "name": 1 },
                "name": "name_1",
            }],
        };
        let result = create_indexes(&body, &fixture.service()).unwrap();
        assert_eq!(result.get_f64("ok").unwrap(), 1.0);
        assert_eq!(result.get_i32("numIndexesBefore").unwrap(), 0);
        assert_eq!(result.get_i32("numIndexesAfter").unwrap(), 1);
    }

    #[test]
    fn create_indexes_duplicate_is_noop() {
        let fixture = ServiceFixture::new(|path| Service::new(path));
        create_schema_with_fields(&fixture, "idx_dup");

        let body = bson::doc! {
            "createIndexes": "idx_dup",
            "$db": "testdb",
            "indexes": [{
                "key": { "name": 1 },
                "name": "name_1",
            }],
        };
        create_indexes(&body, &fixture.service()).unwrap();
        let result = create_indexes(&body, &fixture.service()).unwrap();
        assert_eq!(result.get_i32("numIndexesBefore").unwrap(), 1);
        assert_eq!(result.get_i32("numIndexesAfter").unwrap(), 1);
    }

    #[test]
    fn create_indexes_auto_generates_name() {
        let fixture = ServiceFixture::new(|path| Service::new(path));
        create_schema_with_fields(&fixture, "autoname");

        let body = bson::doc! {
            "createIndexes": "autoname",
            "$db": "testdb",
            "indexes": [{
                "key": { "name": 1 },
            }],
        };
        let result = create_indexes(&body, &fixture.service()).unwrap();
        assert_eq!(result.get_i32("numIndexesAfter").unwrap(), 1);
    }

    #[test]
    fn drop_indexes_by_name() {
        let fixture = ServiceFixture::new(|path| Service::new(path));
        create_schema_with_fields(&fixture, "dropme");

        let create_body = bson::doc! {
            "createIndexes": "dropme",
            "$db": "testdb",
            "indexes": [{
                "key": { "name": 1 },
                "name": "name_1",
            }],
        };
        create_indexes(&create_body, &fixture.service()).unwrap();

        let body = bson::doc! {
            "dropIndexes": "dropme",
            "$db": "testdb",
            "index": "name_1",
        };
        let result = drop_indexes(&body, &fixture.service()).unwrap();
        assert_eq!(result.get_f64("ok").unwrap(), 1.0);
    }

    #[test]
    fn drop_indexes_star_drops_all() {
        let fixture = ServiceFixture::new(|path| Service::new(path));
        create_schema_with_fields(&fixture, "dropall");

        let create_body = bson::doc! {
            "createIndexes": "dropall",
            "$db": "testdb",
            "indexes": [
                { "key": { "name": 1 }, "name": "name_1" },
                { "key": { "age": 1 }, "name": "age_1" },
            ],
        };
        create_indexes(&create_body, &fixture.service()).unwrap();

        let body = bson::doc! {
            "dropIndexes": "dropall",
            "$db": "testdb",
            "index": "*",
        };
        let result = drop_indexes(&body, &fixture.service()).unwrap();
        assert_eq!(result.get_f64("ok").unwrap(), 1.0);

        let list_body = bson::doc! { "listIndexes": "dropall", "$db": "testdb" };
        let list_result = list_indexes(&list_body, &fixture.service()).unwrap();
        let cursor = list_result.get_document("cursor").unwrap();
        let batch = cursor.get_array("firstBatch").unwrap();
        assert_eq!(batch.len(), 1);
    }

    #[test]
    fn drop_nonexistent_index_returns_error() {
        let fixture = ServiceFixture::new(|path| Service::new(path));
        create_schema_with_fields(&fixture, "noindex");

        let body = bson::doc! {
            "dropIndexes": "noindex",
            "$db": "testdb",
            "index": "nonexistent",
        };
        let err = drop_indexes(&body, &fixture.service()).unwrap_err();
        match err {
            MongoError::Command { code, .. } => assert_eq!(code, 27),
            other => panic!("expected Command, got {:?}", other),
        }
    }

    #[test]
    fn list_indexes_includes_id_index() {
        let fixture = ServiceFixture::new(|path| Service::new(path));
        create_schema_with_fields(&fixture, "listed");

        let body = bson::doc! { "listIndexes": "listed", "$db": "testdb" };
        let result = list_indexes(&body, &fixture.service()).unwrap();
        assert_eq!(result.get_f64("ok").unwrap(), 1.0);
        let cursor = result.get_document("cursor").unwrap();
        let batch = cursor.get_array("firstBatch").unwrap();
        assert_eq!(batch.len(), 1);
        let id_idx = batch[0].as_document().unwrap();
        assert_eq!(id_idx.get_str("name").unwrap(), "_id_");
    }

    #[test]
    fn list_indexes_includes_user_indexes() {
        let fixture = ServiceFixture::new(|path| Service::new(path));
        create_schema_with_fields(&fixture, "withidx");

        let create_body = bson::doc! {
            "createIndexes": "withidx",
            "$db": "testdb",
            "indexes": [{
                "key": { "name": 1 },
                "name": "name_1",
            }],
        };
        create_indexes(&create_body, &fixture.service()).unwrap();

        let body = bson::doc! { "listIndexes": "withidx", "$db": "testdb" };
        let result = list_indexes(&body, &fixture.service()).unwrap();
        let cursor = result.get_document("cursor").unwrap();
        let batch = cursor.get_array("firstBatch").unwrap();
        assert_eq!(batch.len(), 2);
    }

    #[test]
    fn list_indexes_nonexistent_collection_returns_error() {
        let fixture = ServiceFixture::new(|path| Service::new(path));
        let tenant_id = TenantId::new("testdb").unwrap();
        let _ = fixture.service().create_tenant(tenant_id);

        let body = bson::doc! { "listIndexes": "nosuch", "$db": "testdb" };
        let err = list_indexes(&body, &fixture.service()).unwrap_err();
        match err {
            MongoError::Command { code, .. } => assert_eq!(code, 26),
            other => panic!("expected Command, got {:?}", other),
        }
    }

    #[test]
    fn create_indexes_missing_collection_returns_error() {
        let fixture = ServiceFixture::new(|path| Service::new(path));
        let body = bson::doc! { "indexes": [] };
        let err = create_indexes(&body, &fixture.service()).unwrap_err();
        match err {
            MongoError::Command { code, .. } => assert_eq!(code, BAD_VALUE.code),
            other => panic!("expected Command, got {:?}", other),
        }
    }
}
