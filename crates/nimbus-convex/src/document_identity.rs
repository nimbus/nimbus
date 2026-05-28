use nimbus_core::{Document, DocumentId, Error, Page, ResolvedDocumentId, TableName};
use serde_json::Value;

pub fn encode_convex_document_id(
    table: &TableName,
    document_id: &DocumentId,
) -> Result<DocumentId, Error> {
    ResolvedDocumentId::encode_table_scoped(table, document_id)
}

pub fn resolve_convex_document_id(
    table: &TableName,
    document_id: DocumentId,
) -> Result<ResolvedDocumentId, Error> {
    ResolvedDocumentId::resolve_table_scoped(table, document_id)
}

pub fn document_to_convex_json(document: Document) -> Result<Value, Error> {
    let table = document.table.clone();
    let document_id = document.id.clone();
    let mut value = document.into_json();
    replace_top_level_id(&table, &document_id, &mut value)?;
    Ok(value)
}

pub fn documents_to_convex_json(documents: Vec<Document>) -> Result<Value, Error> {
    documents
        .into_iter()
        .map(document_to_convex_json)
        .collect::<Result<Vec<_>, _>>()
        .map(Value::Array)
}

pub fn page_to_convex_json(table: &TableName, mut page: Page) -> Result<Value, Error> {
    for item in &mut page.data {
        replace_id_in_value(table, item)?;
    }
    serde_json::to_value(page).map_err(|error| Error::Serialization(error.to_string()))
}

pub fn replace_id_in_value(table: &TableName, value: &mut Value) -> Result<(), Error> {
    match value {
        Value::Array(items) => {
            for item in items {
                replace_id_in_value(table, item)?;
            }
        }
        Value::Object(map) => {
            if let Some(Value::String(raw_id)) = map.get("_id") {
                let scoped =
                    encode_convex_document_id(table, &DocumentId::from_key(raw_id.clone())?)?;
                map.insert("_id".to_string(), Value::String(scoped.to_string()));
            }
            if let Some(data) = map.get_mut("data") {
                replace_id_in_value(table, data)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn replace_top_level_id(
    table: &TableName,
    document_id: &DocumentId,
    value: &mut Value,
) -> Result<(), Error> {
    let Value::Object(map) = value else {
        return Ok(());
    };
    let scoped = encode_convex_document_id(table, document_id)?;
    map.insert("_id".to_string(), Value::String(scoped.to_string()));
    Ok(())
}
