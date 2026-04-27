use std::sync::Arc;

use neovex_core::{
    AtomicWrite, AtomicWriteBatch, AtomicWriteBatchOutcome, AtomicWriteResult, Document,
    DocumentLocator, PrincipalContext, TableName, TenantId, Timestamp, WriteKey, WriteSetMode,
};
use neovex_engine::Service;

use super::super::bson_bridge;
use super::super::connection::ConnectionState;
use super::super::error::{BAD_VALUE, MongoError};
use super::tenant::{DEFAULT_TENANT, ensure_tenant, resolve_tenant};

mod filter;
mod update;

use filter::{has_operator_keys, query_documents, resolve_field_path, translate_sort};
use update::{build_operator_write, build_replacement_write};

fn execute_or_buffer_writes(
    body: &bson::Document,
    conn: &mut ConnectionState,
    service: &Arc<Service>,
    tenant_id: &TenantId,
    writes: Vec<AtomicWrite>,
) -> Result<AtomicWriteBatchOutcome, MongoError> {
    if conn
        .session_store
        .buffer_writes_if_in_transaction(body, writes.clone())
        .is_some()
    {
        return Ok(AtomicWriteBatchOutcome {
            commit: None,
            commit_time: Timestamp(0),
            write_results: (0..writes.len())
                .map(|_| AtomicWriteResult {
                    update_time: None,
                    transform_results: vec![],
                })
                .collect(),
        });
    }
    let batch = AtomicWriteBatch { writes };
    let principal = PrincipalContext::system();
    let eu = service
        .begin_mutation_execution_unit(tenant_id.clone(), principal)
        .map_err(MongoError::from)?;
    eu.execute_atomic_write_batch(batch)
        .map_err(MongoError::from)
}

pub fn insert(
    body: &bson::Document,
    conn: &mut ConnectionState,
    service: &Arc<Service>,
) -> Result<bson::Document, MongoError> {
    let collection = body.get_str("insert").map_err(|_| MongoError::Command {
        code: BAD_VALUE.code,
        code_name: BAD_VALUE.code_name.into(),
        message: "missing collection name in insert command".into(),
    })?;

    let db_name = body.get_str("$db").unwrap_or(DEFAULT_TENANT);
    let tenant_id = resolve_tenant(db_name)?;
    let table = TableName::new(collection).map_err(MongoError::from)?;

    let documents = body
        .get_array("documents")
        .map_err(|_| MongoError::Command {
            code: BAD_VALUE.code,
            code_name: BAD_VALUE.code_name.into(),
            message: "missing documents array in insert command".into(),
        })?;

    let ordered = body.get_bool("ordered").unwrap_or(true);

    ensure_tenant(service, &tenant_id)?;
    ensure_table_schema(service, &tenant_id, &table)?;

    if ordered {
        insert_ordered(body, conn, service, &tenant_id, &table, documents)
    } else {
        insert_unordered(body, conn, service, &tenant_id, &table, documents)
    }
}

pub fn find(
    body: &bson::Document,
    conn: &mut super::super::connection::ConnectionState,
    service: &Arc<Service>,
) -> Result<bson::Document, MongoError> {
    let collection = body.get_str("find").map_err(|_| MongoError::Command {
        code: BAD_VALUE.code,
        code_name: BAD_VALUE.code_name.into(),
        message: "missing collection name in find command".into(),
    })?;

    let db_name = body.get_str("$db").unwrap_or(DEFAULT_TENANT);
    let tenant_id = resolve_tenant(db_name)?;
    let table = TableName::new(collection).map_err(MongoError::from)?;

    let filter_doc = body.get_document("filter").ok();
    let order = translate_sort(body.get_document("sort").ok());

    let limit = match body.get("limit") {
        Some(bson::Bson::Int32(n)) if *n > 0 => Some(*n as usize),
        Some(bson::Bson::Int64(n)) if *n > 0 => Some(*n as usize),
        Some(bson::Bson::Double(n)) if *n > 0.0 => Some(*n as usize),
        _ => None,
    };

    let skip = match body.get("skip") {
        Some(bson::Bson::Int32(n)) if *n > 0 => *n as usize,
        Some(bson::Bson::Int64(n)) if *n > 0 => *n as usize,
        _ => 0,
    };

    let batch_size = match body.get("batchSize") {
        Some(bson::Bson::Int32(n)) if *n > 0 => Some(*n as usize),
        Some(bson::Bson::Int64(n)) if *n > 0 => Some(*n as usize),
        _ => None,
    };

    let projection = body.get_document("projection").ok();

    ensure_tenant(service, &tenant_id)?;

    let empty_filter = bson::Document::new();
    let effective_filter = filter_doc.unwrap_or(&empty_filter);
    let documents = query_documents(
        service,
        &tenant_id,
        &table,
        effective_filter,
        order,
        limit.map(|l| l + skip),
    )?;

    let bson_docs: Vec<bson::Document> = documents
        .into_iter()
        .skip(skip)
        .map(|doc| {
            let mut bson_doc = bson_bridge::document_to_bson_doc(&doc);
            if let Some(proj) = projection {
                apply_projection(&mut bson_doc, proj);
            }
            bson_doc
        })
        .collect();

    let effective_batch_size = batch_size.unwrap_or(101);
    let ns = format!("{}.{}", db_name, collection);

    let (cursor_id, first_batch) =
        conn.cursor_store
            .create(ns.clone(), bson_docs, effective_batch_size);

    Ok(bson::doc! {
        "cursor": {
            "firstBatch": first_batch,
            "id": cursor_id,
            "ns": &ns,
        },
        "ok": 1.0,
    })
}

pub fn apply_projection(doc: &mut bson::Document, projection: &bson::Document) {
    let is_inclusion = projection
        .values()
        .any(|v| matches!(v, bson::Bson::Int32(1) | bson::Bson::Boolean(true)));

    if is_inclusion {
        let mut keep: std::collections::HashSet<&str> = std::collections::HashSet::new();
        keep.insert("_id");
        for (key, val) in projection.iter() {
            match val {
                bson::Bson::Int32(1) | bson::Bson::Boolean(true) => {
                    keep.insert(key);
                }
                bson::Bson::Int32(0) | bson::Bson::Boolean(false) if key == "_id" => {
                    keep.remove("_id");
                }
                _ => {}
            }
        }
        let keys_to_remove: Vec<String> = doc
            .keys()
            .filter(|k| !keep.contains(k.as_str()))
            .cloned()
            .collect();
        for k in keys_to_remove {
            doc.remove(&k);
        }
    } else {
        for (key, val) in projection.iter() {
            if matches!(val, bson::Bson::Int32(0) | bson::Bson::Boolean(false)) {
                doc.remove(key);
            }
        }
    }
}

pub fn update(
    body: &bson::Document,
    conn: &mut ConnectionState,
    service: &Arc<Service>,
) -> Result<bson::Document, MongoError> {
    let collection = body.get_str("update").map_err(|_| MongoError::Command {
        code: BAD_VALUE.code,
        code_name: BAD_VALUE.code_name.into(),
        message: "missing collection name in update command".into(),
    })?;

    let db_name = body.get_str("$db").unwrap_or(DEFAULT_TENANT);
    let tenant_id = resolve_tenant(db_name)?;
    let table = TableName::new(collection).map_err(MongoError::from)?;

    let updates = body.get_array("updates").map_err(|_| MongoError::Command {
        code: BAD_VALUE.code,
        code_name: BAD_VALUE.code_name.into(),
        message: "missing updates array in update command".into(),
    })?;

    let ordered = body.get_bool("ordered").unwrap_or(true);

    ensure_tenant(service, &tenant_id)?;

    let mut n: i32 = 0;
    let mut n_modified: i32 = 0;
    let mut write_errors: Vec<bson::Document> = Vec::new();
    let mut upserted: Vec<bson::Document> = Vec::new();

    for (idx, update_bson) in updates.iter().enumerate() {
        let update_doc = match update_bson.as_document() {
            Some(d) => d,
            None => {
                write_errors.push(write_error_doc(
                    idx as i32,
                    BAD_VALUE.code,
                    "not a document",
                ));
                if ordered {
                    break;
                }
                continue;
            }
        };

        match execute_single_update(body, conn, service, &tenant_id, &table, update_doc) {
            Ok(result) => {
                n += result.n;
                n_modified += result.n_modified;
                if let Some(upserted_doc) = result.upserted {
                    upserted.push(bson::doc! {
                        "index": idx as i32,
                        "_id": upserted_doc,
                    });
                }
            }
            Err(e) => {
                let (code, msg) = error_to_code_msg(&e);
                write_errors.push(write_error_doc(idx as i32, code, &msg));
                if ordered {
                    break;
                }
            }
        }
    }

    let mut result = bson::doc! { "n": n, "nModified": n_modified, "ok": 1.0 };
    if !write_errors.is_empty() {
        result.insert(
            "writeErrors",
            write_errors
                .into_iter()
                .map(bson::Bson::Document)
                .collect::<Vec<_>>(),
        );
    }
    if !upserted.is_empty() {
        result.insert(
            "upserted",
            upserted
                .into_iter()
                .map(bson::Bson::Document)
                .collect::<Vec<_>>(),
        );
    }
    Ok(result)
}

struct UpdateResult {
    n: i32,
    n_modified: i32,
    upserted: Option<bson::Bson>,
}

fn execute_single_update(
    body: &bson::Document,
    conn: &mut ConnectionState,
    service: &Arc<Service>,
    tenant_id: &TenantId,
    table: &TableName,
    update_doc: &bson::Document,
) -> Result<UpdateResult, MongoError> {
    let filter_doc = update_doc
        .get_document("q")
        .map_err(|_| MongoError::Command {
            code: BAD_VALUE.code,
            code_name: BAD_VALUE.code_name.into(),
            message: "missing q (query) in update entry".into(),
        })?;

    let u_doc = update_doc
        .get_document("u")
        .map_err(|_| MongoError::Command {
            code: BAD_VALUE.code,
            code_name: BAD_VALUE.code_name.into(),
            message: "missing u (update) in update entry".into(),
        })?;

    let upsert = update_doc.get_bool("upsert").unwrap_or(false);
    let multi = update_doc.get_bool("multi").unwrap_or(false);

    let is_replacement = !has_operator_keys(u_doc);

    if is_replacement && multi {
        return Err(MongoError::Command {
            code: BAD_VALUE.code,
            code_name: BAD_VALUE.code_name.into(),
            message: "multi update requires update operators".into(),
        });
    }

    let limit = if multi { None } else { Some(1) };
    let matched = query_documents(service, tenant_id, table, filter_doc, vec![], limit)?;

    if matched.is_empty() {
        if upsert {
            return execute_upsert(
                body,
                conn,
                service,
                tenant_id,
                table,
                filter_doc,
                u_doc,
                is_replacement,
            );
        }
        return Ok(UpdateResult {
            n: 0,
            n_modified: 0,
            upserted: None,
        });
    }

    let docs_to_update = if multi { &matched[..] } else { &matched[..1] };
    let mut n_modified = 0i32;

    for doc in docs_to_update {
        let locator = DocumentLocator::new(table.clone(), doc.id.clone());
        let write_key = WriteKey::from(locator);

        let write = if is_replacement {
            build_replacement_write(write_key, u_doc)?
        } else {
            build_operator_write(write_key, u_doc, Some(doc))?
        };

        execute_or_buffer_writes(body, conn, service, tenant_id, vec![write])?;
        n_modified += 1;
    }

    Ok(UpdateResult {
        n: docs_to_update.len() as i32,
        n_modified,
        upserted: None,
    })
}

#[allow(clippy::too_many_arguments)]
fn execute_upsert(
    body: &bson::Document,
    conn: &mut ConnectionState,
    service: &Arc<Service>,
    tenant_id: &TenantId,
    table: &TableName,
    filter_doc: &bson::Document,
    u_doc: &bson::Document,
    is_replacement: bool,
) -> Result<UpdateResult, MongoError> {
    let mut merged = bson::Document::new();

    for (k, v) in filter_doc.iter() {
        if !k.starts_with('$') {
            match v {
                bson::Bson::Document(ops) if has_operator_keys(ops) => {}
                _ => {
                    merged.insert(k, v.clone());
                }
            }
        }
    }

    if is_replacement {
        for (k, v) in u_doc.iter() {
            if k != "_id" {
                merged.insert(k, v.clone());
            }
        }
    } else {
        if let Ok(set_doc) = u_doc.get_document("$set") {
            for (k, v) in set_doc.iter() {
                merged.insert(k, v.clone());
            }
        }
        if let Ok(set_on_insert) = u_doc.get_document("$setOnInsert") {
            for (k, v) in set_on_insert.iter() {
                merged.insert(k, v.clone());
            }
        }
    }

    let neovex_doc = bson_bridge::bson_doc_to_document(&merged, table)?;
    let upserted_id = bson_bridge::document_to_bson_doc(&neovex_doc)
        .get("_id")
        .cloned()
        .unwrap_or(bson::Bson::Null);

    let doc_id = neovex_doc.id.clone();
    let fields = neovex_doc.fields.clone();
    let locator = DocumentLocator::new(table.clone(), doc_id);
    let write_key = WriteKey::from(locator);

    let write = AtomicWrite::Set {
        key: write_key,
        document: fields,
        mode: WriteSetMode::Create,
        precondition: Default::default(),
        transforms: vec![],
    };

    execute_or_buffer_writes(body, conn, service, tenant_id, vec![write])?;

    Ok(UpdateResult {
        n: 1,
        n_modified: 0,
        upserted: Some(upserted_id),
    })
}

pub fn delete(
    body: &bson::Document,
    conn: &mut ConnectionState,
    service: &Arc<Service>,
) -> Result<bson::Document, MongoError> {
    let collection = body.get_str("delete").map_err(|_| MongoError::Command {
        code: BAD_VALUE.code,
        code_name: BAD_VALUE.code_name.into(),
        message: "missing collection name in delete command".into(),
    })?;

    let db_name = body.get_str("$db").unwrap_or(DEFAULT_TENANT);
    let tenant_id = resolve_tenant(db_name)?;
    let table = TableName::new(collection).map_err(MongoError::from)?;

    let deletes = body.get_array("deletes").map_err(|_| MongoError::Command {
        code: BAD_VALUE.code,
        code_name: BAD_VALUE.code_name.into(),
        message: "missing deletes array in delete command".into(),
    })?;

    let ordered = body.get_bool("ordered").unwrap_or(true);

    ensure_tenant(service, &tenant_id)?;

    let mut n: i32 = 0;
    let mut write_errors: Vec<bson::Document> = Vec::new();

    for (idx, del_bson) in deletes.iter().enumerate() {
        let del_doc = match del_bson.as_document() {
            Some(d) => d,
            None => {
                write_errors.push(write_error_doc(
                    idx as i32,
                    BAD_VALUE.code,
                    "not a document",
                ));
                if ordered {
                    break;
                }
                continue;
            }
        };

        match execute_single_delete(body, conn, service, &tenant_id, &table, del_doc) {
            Ok(count) => n += count,
            Err(e) => {
                let (code, msg) = error_to_code_msg(&e);
                write_errors.push(write_error_doc(idx as i32, code, &msg));
                if ordered {
                    break;
                }
            }
        }
    }

    let mut result = bson::doc! { "n": n, "ok": 1.0 };
    if !write_errors.is_empty() {
        result.insert(
            "writeErrors",
            write_errors
                .into_iter()
                .map(bson::Bson::Document)
                .collect::<Vec<_>>(),
        );
    }
    Ok(result)
}

fn execute_single_delete(
    body: &bson::Document,
    conn: &mut ConnectionState,
    service: &Arc<Service>,
    tenant_id: &TenantId,
    table: &TableName,
    del_doc: &bson::Document,
) -> Result<i32, MongoError> {
    let filter_doc = del_doc.get_document("q").map_err(|_| MongoError::Command {
        code: BAD_VALUE.code,
        code_name: BAD_VALUE.code_name.into(),
        message: "missing q (query) in delete entry".into(),
    })?;

    let limit_val = match del_doc.get("limit") {
        Some(bson::Bson::Int32(n)) => *n,
        Some(bson::Bson::Int64(n)) => *n as i32,
        Some(bson::Bson::Double(n)) => *n as i32,
        _ => 0,
    };

    let query_limit = if limit_val == 1 { Some(1) } else { None };
    let matched = query_documents(service, tenant_id, table, filter_doc, vec![], query_limit)?;

    if matched.is_empty() {
        return Ok(0);
    }

    let docs_to_delete = if limit_val == 1 {
        &matched[..1]
    } else {
        &matched[..]
    };

    for doc in docs_to_delete {
        let locator = DocumentLocator::new(table.clone(), doc.id.clone());
        let write_key = WriteKey::from(locator);

        let write = AtomicWrite::Delete {
            key: write_key,
            precondition: Default::default(),
            missing_ok: true,
        };

        execute_or_buffer_writes(body, conn, service, tenant_id, vec![write])?;
    }

    Ok(docs_to_delete.len() as i32)
}

pub fn find_and_modify(
    body: &bson::Document,
    conn: &mut ConnectionState,
    service: &Arc<Service>,
) -> Result<bson::Document, MongoError> {
    let collection = body
        .get_str("findAndModify")
        .or_else(|_| body.get_str("findandmodify"))
        .map_err(|_| MongoError::Command {
            code: BAD_VALUE.code,
            code_name: BAD_VALUE.code_name.into(),
            message: "missing collection name in findAndModify command".into(),
        })?;

    let db_name = body.get_str("$db").unwrap_or(DEFAULT_TENANT);
    let tenant_id = resolve_tenant(db_name)?;
    let table = TableName::new(collection).map_err(MongoError::from)?;

    let filter_doc = body.get_document("query").ok();
    let sort = translate_sort(body.get_document("sort").ok());
    let remove = body.get_bool("remove").unwrap_or(false);
    let return_new = body.get_bool("new").unwrap_or(false);
    let upsert = body.get_bool("upsert").unwrap_or(false);
    let fields = body.get_document("fields").ok();

    ensure_tenant(service, &tenant_id)?;

    let empty_filter = bson::Document::new();
    let effective_filter = filter_doc.unwrap_or(&empty_filter);
    let matched = query_documents(service, &tenant_id, &table, effective_filter, sort, Some(1))?;

    if remove {
        return find_and_remove(body, conn, service, &tenant_id, &table, &matched, fields);
    }

    let u_doc = body
        .get_document("update")
        .map_err(|_| MongoError::Command {
            code: BAD_VALUE.code,
            code_name: BAD_VALUE.code_name.into(),
            message: "findAndModify requires either remove or update".into(),
        })?;

    let is_replacement = !has_operator_keys(u_doc);

    if matched.is_empty() {
        if !upsert {
            return Ok(bson::doc! { "value": bson::Bson::Null, "ok": 1.0 });
        }
        return find_and_upsert(
            body,
            conn,
            service,
            &tenant_id,
            &table,
            effective_filter,
            u_doc,
            is_replacement,
            return_new,
            fields,
        );
    }

    let doc = &matched[0];
    let old_bson = bson_bridge::document_to_bson_doc(doc);

    let locator = DocumentLocator::new(table.clone(), doc.id.clone());
    let write_key = WriteKey::from(locator);

    let write = if is_replacement {
        build_replacement_write(write_key, u_doc)?
    } else {
        build_operator_write(write_key, u_doc, Some(doc))?
    };

    execute_or_buffer_writes(body, conn, service, &tenant_id, vec![write])?;

    let principal = PrincipalContext::system();
    let value = if return_new {
        match service.get_document_with_principal(&tenant_id, &table, doc.id.clone(), &principal) {
            Ok(new_doc) => {
                let mut bson_doc = bson_bridge::document_to_bson_doc(&new_doc);
                if let Some(proj) = fields {
                    apply_projection(&mut bson_doc, proj);
                }
                bson::Bson::Document(bson_doc)
            }
            Err(_) => bson::Bson::Null,
        }
    } else {
        let mut old = old_bson;
        if let Some(proj) = fields {
            apply_projection(&mut old, proj);
        }
        bson::Bson::Document(old)
    };

    Ok(bson::doc! {
        "value": value,
        "lastErrorObject": { "n": 1, "updatedExisting": true },
        "ok": 1.0,
    })
}

fn find_and_remove(
    body: &bson::Document,
    conn: &mut ConnectionState,
    service: &Arc<Service>,
    tenant_id: &TenantId,
    table: &TableName,
    matched: &[Document],
    fields: Option<&bson::Document>,
) -> Result<bson::Document, MongoError> {
    if matched.is_empty() {
        return Ok(bson::doc! { "value": bson::Bson::Null, "ok": 1.0 });
    }

    let doc = &matched[0];
    let mut old_bson = bson_bridge::document_to_bson_doc(doc);
    if let Some(proj) = fields {
        apply_projection(&mut old_bson, proj);
    }

    let locator = DocumentLocator::new(table.clone(), doc.id.clone());
    let write_key = WriteKey::from(locator);
    let write = AtomicWrite::Delete {
        key: write_key,
        precondition: Default::default(),
        missing_ok: true,
    };

    execute_or_buffer_writes(body, conn, service, tenant_id, vec![write])?;

    Ok(bson::doc! {
        "value": old_bson,
        "lastErrorObject": { "n": 1 },
        "ok": 1.0,
    })
}

#[allow(clippy::too_many_arguments)]
fn find_and_upsert(
    body: &bson::Document,
    conn: &mut ConnectionState,
    service: &Arc<Service>,
    tenant_id: &TenantId,
    table: &TableName,
    filter_doc: &bson::Document,
    u_doc: &bson::Document,
    is_replacement: bool,
    return_new: bool,
    fields: Option<&bson::Document>,
) -> Result<bson::Document, MongoError> {
    let mut merged = bson::Document::new();

    for (k, v) in filter_doc.iter() {
        if !k.starts_with('$') {
            match v {
                bson::Bson::Document(ops) if has_operator_keys(ops) => {}
                _ => {
                    merged.insert(k, v.clone());
                }
            }
        }
    }

    if is_replacement {
        for (k, v) in u_doc.iter() {
            if k != "_id" {
                merged.insert(k, v.clone());
            }
        }
    } else {
        if let Ok(set_doc) = u_doc.get_document("$set") {
            for (k, v) in set_doc.iter() {
                merged.insert(k, v.clone());
            }
        }
        if let Ok(set_on_insert) = u_doc.get_document("$setOnInsert") {
            for (k, v) in set_on_insert.iter() {
                merged.insert(k, v.clone());
            }
        }
    }

    let neovex_doc = bson_bridge::bson_doc_to_document(&merged, table)?;
    let doc_id = neovex_doc.id.clone();
    let doc_fields = neovex_doc.fields.clone();
    let locator = DocumentLocator::new(table.clone(), doc_id.clone());
    let write_key = WriteKey::from(locator);

    let write = AtomicWrite::Set {
        key: write_key,
        document: doc_fields,
        mode: WriteSetMode::Create,
        precondition: Default::default(),
        transforms: vec![],
    };

    execute_or_buffer_writes(body, conn, service, tenant_id, vec![write])?;

    let principal = PrincipalContext::system();
    let value = if return_new {
        match service.get_document_with_principal(tenant_id, table, doc_id, &principal) {
            Ok(new_doc) => {
                let mut bson_doc = bson_bridge::document_to_bson_doc(&new_doc);
                if let Some(proj) = fields {
                    apply_projection(&mut bson_doc, proj);
                }
                bson::Bson::Document(bson_doc)
            }
            Err(_) => bson::Bson::Null,
        }
    } else {
        bson::Bson::Null
    };

    let upserted_id = bson_bridge::document_to_bson_doc(&neovex_doc)
        .get("_id")
        .cloned()
        .unwrap_or(bson::Bson::Null);

    Ok(bson::doc! {
        "value": value,
        "lastErrorObject": { "n": 1, "updatedExisting": false, "upserted": upserted_id },
        "ok": 1.0,
    })
}

pub fn count(body: &bson::Document, service: &Arc<Service>) -> Result<bson::Document, MongoError> {
    let collection = body.get_str("count").map_err(|_| MongoError::Command {
        code: BAD_VALUE.code,
        code_name: BAD_VALUE.code_name.into(),
        message: "missing collection name in count command".into(),
    })?;

    let db_name = body.get_str("$db").unwrap_or(DEFAULT_TENANT);
    let tenant_id = resolve_tenant(db_name)?;
    let table = TableName::new(collection).map_err(MongoError::from)?;

    let filter_doc = body.get_document("query").ok();

    let limit = match body.get("limit") {
        Some(bson::Bson::Int32(n)) if *n > 0 => Some(*n as usize),
        Some(bson::Bson::Int64(n)) if *n > 0 => Some(*n as usize),
        _ => None,
    };

    let skip = match body.get("skip") {
        Some(bson::Bson::Int32(n)) if *n > 0 => Some(*n as usize),
        Some(bson::Bson::Int64(n)) if *n > 0 => Some(*n as usize),
        _ => None,
    };

    ensure_tenant(service, &tenant_id)?;

    let empty_filter = bson::Document::new();
    let effective_filter = filter_doc.unwrap_or(&empty_filter);

    let query_limit = match (skip, limit) {
        (Some(s), Some(l)) => Some(s.saturating_add(l)),
        (None, Some(l)) => Some(l),
        _ => None,
    };

    let documents = query_documents(
        service,
        &tenant_id,
        &table,
        effective_filter,
        vec![],
        query_limit,
    )?;

    let mut n = documents.len();
    if let Some(s) = skip {
        n = n.saturating_sub(s);
    }
    if let Some(l) = limit {
        n = n.min(l);
    }

    Ok(bson::doc! { "n": n as i64, "ok": 1.0 })
}

pub fn distinct(
    body: &bson::Document,
    service: &Arc<Service>,
) -> Result<bson::Document, MongoError> {
    let collection = body.get_str("distinct").map_err(|_| MongoError::Command {
        code: BAD_VALUE.code,
        code_name: BAD_VALUE.code_name.into(),
        message: "missing collection name in distinct command".into(),
    })?;

    let key = body.get_str("key").map_err(|_| MongoError::Command {
        code: BAD_VALUE.code,
        code_name: BAD_VALUE.code_name.into(),
        message: "missing key field in distinct command".into(),
    })?;

    let db_name = body.get_str("$db").unwrap_or(DEFAULT_TENANT);
    let tenant_id = resolve_tenant(db_name)?;
    let table = TableName::new(collection).map_err(MongoError::from)?;

    let filter_doc = body.get_document("query").ok();

    ensure_tenant(service, &tenant_id)?;

    let empty_filter = bson::Document::new();
    let effective_filter = filter_doc.unwrap_or(&empty_filter);
    let documents = query_documents(service, &tenant_id, &table, effective_filter, vec![], None)?;

    let mut seen_keys = std::collections::HashSet::<String>::new();
    let mut seen = Vec::<bson::Bson>::new();
    for doc in &documents {
        let bson_doc = bson_bridge::document_to_bson_doc(doc);
        let value = resolve_field_path(&bson_doc, key);
        match value {
            Some(bson::Bson::Array(arr)) => {
                for elem in arr {
                    if seen_keys.insert(format!("{elem:?}")) {
                        seen.push(elem);
                    }
                }
            }
            Some(val) => {
                if seen_keys.insert(format!("{val:?}")) {
                    seen.push(val);
                }
            }
            None => {}
        }
    }

    Ok(bson::doc! {
        "values": seen,
        "ok": 1.0,
    })
}

fn insert_ordered(
    body: &bson::Document,
    conn: &mut ConnectionState,
    service: &Arc<Service>,
    tenant_id: &TenantId,
    table: &TableName,
    documents: &[bson::Bson],
) -> Result<bson::Document, MongoError> {
    let mut n: i32 = 0;
    let mut write_errors: Vec<bson::Document> = Vec::new();

    for (idx, doc_bson) in documents.iter().enumerate() {
        let bson_doc = match doc_bson.as_document() {
            Some(d) => d,
            None => {
                write_errors.push(write_error_doc(
                    idx as i32,
                    BAD_VALUE.code,
                    "not a document",
                ));
                break;
            }
        };

        match insert_single_doc(body, conn, service, tenant_id, table, bson_doc) {
            Ok(()) => n += 1,
            Err(e) => {
                let (code, msg) = error_to_code_msg(&e);
                write_errors.push(write_error_doc(idx as i32, code, &msg));
                break;
            }
        }
    }

    Ok(insert_response(n, write_errors))
}

fn insert_unordered(
    body: &bson::Document,
    conn: &mut ConnectionState,
    service: &Arc<Service>,
    tenant_id: &TenantId,
    table: &TableName,
    documents: &[bson::Bson],
) -> Result<bson::Document, MongoError> {
    let mut n: i32 = 0;
    let mut write_errors: Vec<bson::Document> = Vec::new();

    for (idx, doc_bson) in documents.iter().enumerate() {
        let bson_doc = match doc_bson.as_document() {
            Some(d) => d,
            None => {
                write_errors.push(write_error_doc(
                    idx as i32,
                    BAD_VALUE.code,
                    "not a document",
                ));
                continue;
            }
        };

        match insert_single_doc(body, conn, service, tenant_id, table, bson_doc) {
            Ok(()) => n += 1,
            Err(e) => {
                let (code, msg) = error_to_code_msg(&e);
                write_errors.push(write_error_doc(idx as i32, code, &msg));
            }
        }
    }

    Ok(insert_response(n, write_errors))
}

fn insert_single_doc(
    body: &bson::Document,
    conn: &mut ConnectionState,
    service: &Arc<Service>,
    tenant_id: &TenantId,
    table: &TableName,
    bson_doc: &bson::Document,
) -> Result<(), MongoError> {
    let neovex_doc = bson_bridge::bson_doc_to_document(bson_doc, table)?;
    let doc_id = neovex_doc.id.clone();
    let fields = neovex_doc.fields.clone();

    let locator = neovex_core::DocumentLocator::new(table.clone(), doc_id);
    let write_key = WriteKey::from(locator);

    let write = AtomicWrite::Set {
        key: write_key,
        document: fields,
        mode: WriteSetMode::Create,
        precondition: Default::default(),
        transforms: vec![],
    };

    execute_or_buffer_writes(body, conn, service, tenant_id, vec![write])?;
    Ok(())
}

fn ensure_table_schema(
    service: &Arc<Service>,
    tenant_id: &TenantId,
    table: &TableName,
) -> Result<(), MongoError> {
    match service.get_table_schema(tenant_id, table) {
        Ok(_) => Ok(()),
        Err(neovex_core::Error::SchemaNotFound(_)) => {
            let schema = neovex_core::TableSchema {
                table: table.clone(),
                fields: vec![],
                indexes: vec![],
                access_policy: None,
            };
            service
                .set_table_schema(tenant_id, schema)
                .map_err(MongoError::from)
        }
        Err(e) => Err(MongoError::from(e)),
    }
}

fn insert_response(n: i32, write_errors: Vec<bson::Document>) -> bson::Document {
    let mut doc = bson::doc! { "n": n, "ok": 1.0 };
    if !write_errors.is_empty() {
        doc.insert(
            "writeErrors",
            write_errors
                .into_iter()
                .map(bson::Bson::Document)
                .collect::<Vec<_>>(),
        );
    }
    doc
}

fn write_error_doc(index: i32, code: i32, errmsg: &str) -> bson::Document {
    bson::doc! {
        "index": index,
        "code": code,
        "errmsg": errmsg,
    }
}

fn error_to_code_msg(err: &MongoError) -> (i32, String) {
    match err {
        MongoError::Command { code, message, .. } => (*code, message.clone()),
        MongoError::Wire(w) => (BAD_VALUE.code, w.to_string()),
    }
}

#[cfg(test)]
mod tests;
