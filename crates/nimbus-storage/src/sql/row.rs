//! Row serialization shared by the MySQL and PostgreSQL backends.
//!
//! Both providers store documents as JSON columns with identical encoding, so
//! these helpers are byte-for-byte dialect-independent. Each backend extracts
//! the primitive column values in its own driver's row type, then calls
//! [`row_to_document`] with the extracted values.

use nimbus_core::{Document, DocumentId, Error, Result, TableName, Timestamp};

pub(crate) fn serialize_json<T>(value: &T) -> Result<String>
where
    T: serde::Serialize,
{
    serde_json::to_string(value).map_err(|error| Error::Serialization(error.to_string()))
}

pub(crate) fn deserialize_json<T>(json: &str) -> Result<T>
where
    T: serde::de::DeserializeOwned,
{
    serde_json::from_str(json).map_err(|error| Error::Serialization(error.to_string()))
}

pub(crate) fn serialize_document_fields(document: &Document) -> Result<String> {
    serde_json::to_string(&document.fields).map_err(|error| Error::Serialization(error.to_string()))
}

pub(crate) fn serialize_document_typed_fields(document: &Document) -> Result<String> {
    serde_json::to_string(&document.typed_fields)
        .map_err(|error| Error::Serialization(error.to_string()))
}

/// Build a [`Document`] from already-extracted primitive column values. The
/// caller owns pulling `table`, `id`, and the timestamps out of its dialect's
/// row type; this function performs only the JSON decoding that is identical
/// across backends.
pub(crate) fn row_to_document(
    table: &TableName,
    id: &DocumentId,
    creation_time: u64,
    update_time: u64,
    data_json: String,
    typed_fields_json: String,
) -> Result<Document> {
    Ok(Document {
        id: id.clone(),
        table: table.clone(),
        creation_time: Timestamp(creation_time),
        update_time: Timestamp(update_time),
        fields: serde_json::from_str(&data_json)
            .map_err(|error| Error::Serialization(error.to_string()))?,
        typed_fields: serde_json::from_str(&typed_fields_json)
            .map_err(|error| Error::Serialization(error.to_string()))?,
    })
}
