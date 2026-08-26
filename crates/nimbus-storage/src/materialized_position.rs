use std::collections::BTreeSet;
use std::io::{self, Write};

use nimbus_core::{
    Document, DocumentLocator, Error, FieldType, IndexState, ResourcePathBinding, Result,
    SequenceNumber, SpecialDouble, StoredValue, TableSchema, TriggerDeliveryCursor,
    TypedScalarValue,
};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::TableIdentitySnapshotEntry;

/// The digest format for [`MaterializedPosition`].
///
/// Version 3 covers resource-path bindings and trigger-delivery progress.
pub const MATERIALIZED_POSITION_VERSION: u16 = 3;

const DIGEST_HEX_LEN: usize = 64;
const CODEC_DOMAIN: &[u8] = b"nimbus.materialized-position.v3";

/// A materialized snapshot's logical state in canonical order.
///
/// Construction stays inside the snapshot exporter. Consumers can inspect the
/// state for diagnostics, but cannot assemble a state that bypasses snapshot
/// validation and canonical collection ordering.
#[derive(Debug, Clone, PartialEq)]
pub struct CanonicalMaterializedState {
    snapshot_version: u16,
    table_identities: Vec<TableIdentitySnapshotEntry>,
    schema_tables: Vec<TableSchema>,
    documents: Vec<Document>,
    resource_path_bindings: Vec<ResourcePathBinding>,
    scheduled_execution_ids: Vec<String>,
    trigger_delivery_cursor: TriggerDeliveryCursor,
}

impl CanonicalMaterializedState {
    pub(crate) fn new(
        snapshot_version: u16,
        table_identities: Vec<TableIdentitySnapshotEntry>,
        schema_tables: Vec<TableSchema>,
        documents: Vec<Document>,
        resource_path_bindings: Vec<ResourcePathBinding>,
        scheduled_execution_ids: Vec<String>,
        trigger_delivery_cursor: TriggerDeliveryCursor,
    ) -> Self {
        Self {
            snapshot_version,
            table_identities,
            schema_tables,
            documents,
            resource_path_bindings,
            scheduled_execution_ids,
            trigger_delivery_cursor,
        }
    }

    pub fn snapshot_version(&self) -> u16 {
        self.snapshot_version
    }

    pub fn table_identities(&self) -> &[TableIdentitySnapshotEntry] {
        &self.table_identities
    }

    pub fn schema_tables(&self) -> &[TableSchema] {
        &self.schema_tables
    }

    pub fn documents(&self) -> &[Document] {
        &self.documents
    }

    pub fn resource_path_bindings(&self) -> &[ResourcePathBinding] {
        &self.resource_path_bindings
    }

    pub fn scheduled_execution_ids(&self) -> &[String] {
        &self.scheduled_execution_ids
    }

    pub fn trigger_delivery_cursor(&self) -> TriggerDeliveryCursor {
        self.trigger_delivery_cursor
    }

    pub fn digest(&self) -> Result<String> {
        let mut digest = Sha256::new();
        {
            let mut writer = DigestWriter(&mut digest);
            self.write_canonical(&mut writer)
                .map_err(|error| Error::Serialization(error.to_string()))?;
        }
        Ok(hex::encode(digest.finalize()))
    }

    fn write_canonical(&self, writer: &mut impl Write) -> io::Result<()> {
        write_bytes(writer, CODEC_DOMAIN)?;
        write_u16(writer, self.snapshot_version)?;

        write_tag(writer, 0x10)?;
        write_len(writer, self.table_identities.len())?;
        for identity in &self.table_identities {
            write_table_identity(writer, identity)?;
        }

        write_tag(writer, 0x20)?;
        write_len(writer, self.schema_tables.len())?;
        for table in &self.schema_tables {
            write_table_schema(writer, table)?;
        }

        write_tag(writer, 0x30)?;
        write_len(writer, self.documents.len())?;
        for document in &self.documents {
            write_document(writer, document)?;
        }

        write_tag(writer, 0x40)?;
        write_len(writer, self.resource_path_bindings.len())?;
        for binding in &self.resource_path_bindings {
            write_resource_path_binding(writer, binding)?;
        }

        write_tag(writer, 0x50)?;
        write_len(writer, self.scheduled_execution_ids.len())?;
        for execution_id in &self.scheduled_execution_ids {
            write_scheduled_execution(writer, execution_id)?;
        }

        write_tag(writer, 0x60)?;
        write_trigger_delivery_cursor(writer, self.trigger_delivery_cursor)?;
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn reference_bytes(&self) -> Result<Vec<u8>> {
        let mut bytes = Vec::new();
        self.write_canonical(&mut bytes)
            .map_err(|error| Error::Serialization(error.to_string()))?;
        Ok(bytes)
    }
}

pub(crate) fn canonical_table_identity_identity(
    identity: &TableIdentitySnapshotEntry,
) -> Result<Vec<u8>> {
    canonical_bytes(|writer| {
        write_string(writer, &identity.namespace)?;
        write_string(writer, identity.table.as_str())
    })
}

pub(crate) fn canonical_table_identity_value(
    identity: &TableIdentitySnapshotEntry,
) -> Result<Vec<u8>> {
    canonical_bytes(|writer| write_table_identity(writer, identity))
}

pub(crate) fn canonical_schema_identity(table: &TableSchema) -> Result<Vec<u8>> {
    canonical_schema_identity_for_name(table.table.as_str())
}

pub(crate) fn canonical_schema_identity_for_name(table: &str) -> Result<Vec<u8>> {
    canonical_bytes(|writer| write_string(writer, table))
}

pub(crate) fn canonical_schema_value(table: &TableSchema) -> Result<Vec<u8>> {
    canonical_bytes(|writer| write_table_schema(writer, table))
}

pub(crate) fn canonical_document_identity(document: &Document) -> Result<Vec<u8>> {
    canonical_bytes(|writer| {
        write_string(writer, document.table.as_str())?;
        write_string(writer, document.id.as_str())
    })
}

pub(crate) fn canonical_document_value(document: &Document) -> Result<Vec<u8>> {
    canonical_bytes(|writer| write_document(writer, document))
}

pub(crate) fn canonical_resource_path_binding_identity(
    locator: &DocumentLocator,
) -> Result<Vec<u8>> {
    canonical_bytes(|writer| write_document_locator(writer, locator))
}

pub(crate) fn canonical_resource_path_binding_value(
    binding: &ResourcePathBinding,
) -> Result<Vec<u8>> {
    canonical_bytes(|writer| write_resource_path_binding(writer, binding))
}

pub(crate) fn canonical_scheduled_execution_identity(execution_id: &str) -> Result<Vec<u8>> {
    canonical_bytes(|writer| write_string(writer, execution_id))
}

pub(crate) fn canonical_scheduled_execution_value(execution_id: &str) -> Result<Vec<u8>> {
    canonical_bytes(|writer| write_scheduled_execution(writer, execution_id))
}

pub(crate) fn canonical_trigger_delivery_cursor_identity() -> Result<Vec<u8>> {
    canonical_bytes(|writer| write_string(writer, "trigger_delivery_cursor"))
}

pub(crate) fn canonical_trigger_delivery_cursor_value(
    cursor: TriggerDeliveryCursor,
) -> Result<Vec<u8>> {
    canonical_bytes(|writer| write_trigger_delivery_cursor(writer, cursor))
}

fn canonical_bytes(write: impl FnOnce(&mut Vec<u8>) -> io::Result<()>) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    write(&mut bytes).map_err(|error| Error::Serialization(error.to_string()))?;
    Ok(bytes)
}

/// Where a materialized artifact sits: the applied sequence plus the digest of
/// the state that sequence produced.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MaterializedPosition {
    version: u16,
    applied_sequence: SequenceNumber,
    state_digest: String,
}

impl MaterializedPosition {
    pub fn new(applied_sequence: SequenceNumber, state_digest: String) -> Result<Self> {
        let position = Self {
            version: MATERIALIZED_POSITION_VERSION,
            applied_sequence,
            state_digest,
        };
        position.validate()?;
        Ok(position)
    }

    pub fn validate(&self) -> Result<()> {
        if self.version != MATERIALIZED_POSITION_VERSION {
            return Err(Error::InvalidInput(format!(
                "unsupported materialized position version {}",
                self.version
            )));
        }
        if self.state_digest.len() != DIGEST_HEX_LEN
            || !self
                .state_digest
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(Error::InvalidInput(
                "materialized position digest must be 64 lowercase hexadecimal characters"
                    .to_string(),
            ));
        }
        Ok(())
    }

    pub fn version(&self) -> u16 {
        self.version
    }

    pub fn applied_sequence(&self) -> SequenceNumber {
        self.applied_sequence
    }

    pub fn state_digest(&self) -> &str {
        &self.state_digest
    }

    #[cfg(test)]
    pub(crate) fn from_unchecked(
        version: u16,
        applied_sequence: SequenceNumber,
        state_digest: String,
    ) -> Self {
        Self {
            version,
            applied_sequence,
            state_digest,
        }
    }
}

impl<'de> Deserialize<'de> for MaterializedPosition {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct WirePosition {
            version: u16,
            applied_sequence: SequenceNumber,
            state_digest: String,
        }

        let wire = WirePosition::deserialize(deserializer)?;
        let position = Self {
            version: wire.version,
            applied_sequence: wire.applied_sequence,
            state_digest: wire.state_digest,
        };
        position.validate().map_err(serde::de::Error::custom)?;
        Ok(position)
    }
}

struct DigestWriter<'a>(&'a mut Sha256);

impl Write for DigestWriter<'_> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0.update(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn write_document(writer: &mut impl Write, document: &Document) -> io::Result<()> {
    write_string(writer, document.table.as_str())?;
    write_string(writer, &document.id.to_string())?;
    write_u64(writer, document.creation_time.0)?;
    write_u64(writer, document.update_time.0)?;

    let keys = document
        .fields
        .keys()
        .chain(document.typed_fields.keys())
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    write_len(writer, keys.len())?;
    for key in keys {
        write_string(writer, key)?;
        if let Some(value) = document.typed_fields.get(key) {
            write_stored_value(writer, value)?;
        } else if let Some(value) = document.fields.get(key) {
            write_json_value(writer, value)?;
        }
    }
    Ok(())
}

fn write_table_identity(
    writer: &mut impl Write,
    identity: &TableIdentitySnapshotEntry,
) -> io::Result<()> {
    write_string(writer, &identity.namespace)?;
    write_string(writer, identity.table.as_str())?;
    write_string(writer, identity.table_id.as_str())?;
    write_string(writer, identity.state.as_str())
}

fn write_scheduled_execution(writer: &mut impl Write, execution_id: &str) -> io::Result<()> {
    write_string(writer, execution_id)
}

fn write_document_locator(writer: &mut impl Write, locator: &DocumentLocator) -> io::Result<()> {
    write_string(writer, locator.table.as_str())?;
    write_string(writer, locator.id.as_str())
}

fn write_resource_path_binding(
    writer: &mut impl Write,
    binding: &ResourcePathBinding,
) -> io::Result<()> {
    write_document_locator(writer, &binding.locator)?;
    let segments = binding.document_path.segments();
    write_len(writer, segments.len())?;
    for segment in segments {
        write_string(writer, &segment)?;
    }
    Ok(())
}

fn write_trigger_delivery_cursor(
    writer: &mut impl Write,
    cursor: TriggerDeliveryCursor,
) -> io::Result<()> {
    write_u64(writer, cursor.materialized_through.0)
}

fn write_table_schema(writer: &mut impl Write, table: &TableSchema) -> io::Result<()> {
    write_string(writer, table.table.as_str())?;
    write_len(writer, table.fields.len())?;
    for field in &table.fields {
        write_string(writer, &field.name)?;
        write_tag(
            writer,
            match field.field_type {
                FieldType::String => 0x01,
                FieldType::Number => 0x02,
                FieldType::Boolean => 0x03,
                FieldType::Array => 0x04,
                FieldType::Object => 0x05,
                FieldType::Any => 0x06,
            },
        )?;
        write_bool(writer, field.required)?;
    }
    write_len(writer, table.indexes.len())?;
    for index in &table.indexes {
        write_string(writer, index.id.as_str())?;
        write_string(writer, &index.name)?;
        write_len(writer, index.fields.len())?;
        for field in &index.fields {
            write_string(writer, field)?;
        }
        write_tag(
            writer,
            match index.state {
                IndexState::Pending => 0x01,
                IndexState::Backfilling => 0x02,
                IndexState::Enabled => 0x03,
                IndexState::Deleting => 0x04,
            },
        )?;
    }
    match &table.access_policy {
        Some(policy) => {
            write_tag(writer, 0x01)?;
            let value = serde_json::to_value(policy)
                .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
            write_json_value(writer, &value)?;
        }
        None => write_tag(writer, 0x00)?,
    }
    Ok(())
}

fn write_stored_value(writer: &mut impl Write, value: &StoredValue) -> io::Result<()> {
    match value {
        StoredValue::Json { value } => write_json_value(writer, value),
        StoredValue::TypedScalar { value } => write_typed_scalar(writer, value),
        StoredValue::Map { entries } => {
            write_tag(writer, 0x07)?;
            write_len(writer, entries.len())?;
            for (key, value) in entries {
                write_string(writer, key)?;
                write_stored_value(writer, value)?;
            }
            Ok(())
        }
        StoredValue::List { items } => {
            write_tag(writer, 0x06)?;
            write_len(writer, items.len())?;
            for value in items {
                write_stored_value(writer, value)?;
            }
            Ok(())
        }
    }
}

fn write_json_value(writer: &mut impl Write, value: &Value) -> io::Result<()> {
    match value {
        Value::Null => write_tag(writer, 0x00),
        Value::Bool(false) => write_tag(writer, 0x01),
        Value::Bool(true) => write_tag(writer, 0x02),
        Value::Number(number) => {
            if let Some(value) = number.as_i64() {
                write_tag(writer, 0x03)?;
                writer.write_all(&value.to_be_bytes())
            } else if let Some(value) = number.as_u64() {
                write_tag(writer, 0x04)?;
                writer.write_all(&value.to_be_bytes())
            } else {
                let value = number.as_f64().ok_or_else(|| {
                    io::Error::new(io::ErrorKind::InvalidData, "unsupported JSON number")
                })?;
                write_tag(writer, 0x05)?;
                write_f64(writer, value)
            }
        }
        Value::String(value) => {
            write_tag(writer, 0x08)?;
            write_string(writer, value)
        }
        Value::Array(items) => {
            write_tag(writer, 0x06)?;
            write_len(writer, items.len())?;
            for value in items {
                write_json_value(writer, value)?;
            }
            Ok(())
        }
        Value::Object(entries) => {
            write_tag(writer, 0x07)?;
            let mut entries = entries.iter().collect::<Vec<_>>();
            entries.sort_unstable_by_key(|(key, _)| *key);
            write_len(writer, entries.len())?;
            for (key, value) in entries {
                write_string(writer, key)?;
                write_json_value(writer, value)?;
            }
            Ok(())
        }
    }
}

fn write_typed_scalar(writer: &mut impl Write, value: &TypedScalarValue) -> io::Result<()> {
    if let TypedScalarValue::SpecialDouble { value } = value {
        write_tag(writer, 0x05)?;
        return match value {
            // This stored scalar projects to the client-visible "-0" sentinel.
            // Preserve its sign bit so replica verification can detect drift to
            // a plain positive-zero value.
            SpecialDouble::NegativeZero => write_finite_f64(writer, -0.0),
            SpecialDouble::Nan => write_f64(writer, f64::NAN),
            SpecialDouble::PositiveInfinity => write_f64(writer, f64::INFINITY),
            SpecialDouble::NegativeInfinity => write_f64(writer, f64::NEG_INFINITY),
        };
    }
    write_tag(writer, 0x09)?;
    match value {
        TypedScalarValue::Timestamp { value } => {
            write_tag(writer, 0x01)?;
            write_u64(writer, value.0)
        }
        TypedScalarValue::FirestoreTimestamp { rfc3339 } => {
            write_tag(writer, 0x02)?;
            write_string(writer, rfc3339)
        }
        TypedScalarValue::Bytes { data } => {
            write_tag(writer, 0x03)?;
            write_bytes(writer, data)
        }
        TypedScalarValue::Reference { resource_name } => {
            write_tag(writer, 0x04)?;
            write_string(writer, resource_name)
        }
        TypedScalarValue::GeoPoint {
            latitude,
            longitude,
        } => {
            write_tag(writer, 0x05)?;
            write_f64(writer, *latitude)?;
            write_f64(writer, *longitude)
        }
        TypedScalarValue::SpecialDouble { .. } => unreachable!("handled before the typed tag"),
        TypedScalarValue::ObjectId { hex } => {
            write_tag(writer, 0x07)?;
            write_string(writer, hex)
        }
        TypedScalarValue::Binary { subtype, data } => {
            write_tag(writer, 0x08)?;
            write_tag(writer, *subtype)?;
            write_bytes(writer, data)
        }
        TypedScalarValue::Decimal128 { repr } => {
            write_tag(writer, 0x09)?;
            write_string(writer, repr)
        }
        TypedScalarValue::Regex { pattern, options } => {
            write_tag(writer, 0x0a)?;
            write_string(writer, pattern)?;
            write_string(writer, options)
        }
        TypedScalarValue::MongoTimestamp { seconds, increment } => {
            write_tag(writer, 0x0b)?;
            writer.write_all(&seconds.to_be_bytes())?;
            writer.write_all(&increment.to_be_bytes())
        }
        TypedScalarValue::MinKey => write_tag(writer, 0x0c),
        TypedScalarValue::MaxKey => write_tag(writer, 0x0d),
        TypedScalarValue::JavaScriptCode { code } => {
            write_tag(writer, 0x0e)?;
            write_string(writer, code)
        }
        TypedScalarValue::Number { repr } => {
            write_tag(writer, 0x0f)?;
            write_string(writer, repr)
        }
        TypedScalarValue::StringSet { values } => {
            write_tag(writer, 0x10)?;
            let mut values = values.iter().map(String::as_str).collect::<Vec<_>>();
            values.sort_unstable();
            write_len(writer, values.len())?;
            for value in values {
                write_string(writer, value)?;
            }
            Ok(())
        }
        TypedScalarValue::NumberSet { values } => {
            write_tag(writer, 0x11)?;
            let mut values = values.iter().map(String::as_str).collect::<Vec<_>>();
            values.sort_unstable();
            write_len(writer, values.len())?;
            for value in values {
                write_string(writer, value)?;
            }
            Ok(())
        }
        TypedScalarValue::BinarySet { values } => {
            write_tag(writer, 0x12)?;
            let mut values = values.iter().map(Vec::as_slice).collect::<Vec<_>>();
            values.sort_unstable();
            write_len(writer, values.len())?;
            for value in values {
                write_bytes(writer, value)?;
            }
            Ok(())
        }
    }
}

fn write_f64(writer: &mut impl Write, value: f64) -> io::Result<()> {
    let tag = if value.is_nan() {
        0x01
    } else if value == f64::INFINITY {
        0x02
    } else if value == f64::NEG_INFINITY {
        0x03
    } else {
        let normalized = if value == 0.0 { 0.0 } else { value };
        return write_finite_f64(writer, normalized);
    };
    write_tag(writer, tag)
}

fn write_finite_f64(writer: &mut impl Write, value: f64) -> io::Result<()> {
    write_tag(writer, 0x00)?;
    writer.write_all(&value.to_bits().to_be_bytes())
}

fn write_bool(writer: &mut impl Write, value: bool) -> io::Result<()> {
    write_tag(writer, u8::from(value))
}

fn write_tag(writer: &mut impl Write, tag: u8) -> io::Result<()> {
    writer.write_all(&[tag])
}

fn write_u16(writer: &mut impl Write, value: u16) -> io::Result<()> {
    writer.write_all(&value.to_be_bytes())
}

fn write_u64(writer: &mut impl Write, value: u64) -> io::Result<()> {
    writer.write_all(&value.to_be_bytes())
}

fn write_len(writer: &mut impl Write, len: usize) -> io::Result<()> {
    let len = u64::try_from(len)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "length exceeds u64"))?;
    write_u64(writer, len)
}

fn write_bytes(writer: &mut impl Write, bytes: &[u8]) -> io::Result<()> {
    write_len(writer, bytes.len())?;
    writer.write_all(bytes)
}

fn write_string(writer: &mut impl Write, value: &str) -> io::Result<()> {
    write_bytes(writer, value.as_bytes())
}

#[cfg(any(test, feature = "test-hooks"))]
#[doc(hidden)]
pub fn materialized_position_golden_fixture() -> Result<MaterializedPosition> {
    use std::str::FromStr;

    use nimbus_core::{DocumentId, TableId, TableName, Timestamp};

    let table = TableName::new("tasks")?;
    let mut document = Document::with_id_at(
        DocumentId::from_key("doc1")?,
        table.clone(),
        serde_json::Map::from_iter([
            ("zeta".to_string(), serde_json::json!([1, 2, 3])),
            (
                "alpha".to_string(),
                serde_json::json!({ "b": true, "a": null }),
            ),
        ]),
        Timestamp(1_700_000_000_000),
    );
    document.set_typed_field(
        "score",
        TypedScalarValue::SpecialDouble {
            value: SpecialDouble::Nan,
        },
    );

    let state = CanonicalMaterializedState::new(
        3,
        vec![TableIdentitySnapshotEntry::default_namespace(
            table,
            TableId::from_str("table1")?,
        )],
        Vec::new(),
        vec![document],
        Vec::new(),
        vec!["execution1".to_string()],
        TriggerDeliveryCursor::default(),
    );
    MaterializedPosition::new(SequenceNumber(7), state.digest()?)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use nimbus_core::{DocumentId, TableName, Timestamp};
    use serde_json::json;

    use super::*;

    fn state_with_document(document: Document) -> CanonicalMaterializedState {
        CanonicalMaterializedState::new(
            3,
            Vec::new(),
            Vec::new(),
            vec![document],
            Vec::new(),
            Vec::new(),
            TriggerDeliveryCursor::default(),
        )
    }

    fn document_with_value(value: Value) -> Document {
        Document::with_id_at(
            DocumentId::from_key("doc1").expect("fixed document id should be valid"),
            TableName::new("tasks").expect("fixed table name should be valid"),
            serde_json::Map::from_iter([("value".to_string(), value)]),
            Timestamp(1),
        )
    }

    #[test]
    fn canonical_leaf_equivalent_stored_values_hash_identically() {
        let plain = document_with_value(json!({ "b": [2, 3], "a": 1 }));
        let mut typed_spelling = plain.clone();
        typed_spelling.typed_fields.insert(
            "value".to_string(),
            StoredValue::Map {
                entries: BTreeMap::from([
                    ("a".to_string(), StoredValue::from(json!(1))),
                    (
                        "b".to_string(),
                        StoredValue::List {
                            items: vec![StoredValue::from(json!(2)), StoredValue::from(json!(3))],
                        },
                    ),
                ]),
            },
        );

        assert_eq!(
            state_with_document(plain)
                .digest()
                .expect("plain digest should compute"),
            state_with_document(typed_spelling)
                .digest()
                .expect("typed spelling digest should compute")
        );
    }

    #[test]
    fn normalized_logical_value_drives_persistence_equality_index_and_digest() {
        let alternate_plain = StoredValue::Map {
            entries: BTreeMap::from([("nested".to_string(), StoredValue::from(json!([1, 2])))]),
        };
        let plain_json = StoredValue::from(json!({ "nested": [1, 2] }));
        assert!(alternate_plain.logical_eq(&plain_json));

        let mut document = document_with_value(Value::Null);
        document.set_typed_field(
            "value",
            StoredValue::Map {
                entries: BTreeMap::from([(
                    "payload".to_string(),
                    StoredValue::from(TypedScalarValue::Bytes {
                        data: vec![0, 1, 2],
                    }),
                )]),
            },
        );
        document.set_typed_field(
            "score",
            TypedScalarValue::SpecialDouble {
                value: SpecialDouble::Nan,
            },
        );
        let persisted = crate::document_codec::encode_document_msgpack(&document)
            .expect("logical document should encode");
        let restored = crate::document_codec::decode_document_msgpack(&persisted)
            .expect("logical document should decode");
        assert_eq!(restored, document);

        let index = nimbus_core::IndexDefinition::new("by_score", ["score"]);
        let encoded = crate::index::encoded_index_tuple_for_document(&restored, &index)
            .expect("logical index value should encode")
            .expect("logical index value should exist");
        assert_eq!(
            encoded,
            crate::index::encode_index_value(&json!("NaN"))
                .expect("projected index value should encode")
        );
        assert_eq!(
            state_with_document(document)
                .digest()
                .expect("source digest should compute"),
            state_with_document(restored)
                .digest()
                .expect("restored digest should compute")
        );
    }

    #[test]
    fn canonical_leaf_order_is_provider_independent() {
        let forward = document_with_value(
            serde_json::from_str(r#"{"a":1,"b":2}"#).expect("forward object should deserialize"),
        );
        let reversed = document_with_value(
            serde_json::from_str(r#"{"b":2,"a":1}"#).expect("reversed object should deserialize"),
        );

        assert_eq!(
            state_with_document(forward)
                .digest()
                .expect("forward digest should compute"),
            state_with_document(reversed)
                .digest()
                .expect("reversed digest should compute")
        );

        let mut forward_set = document_with_value(Value::Null);
        forward_set.set_typed_field(
            "value",
            TypedScalarValue::StringSet {
                values: vec!["alpha".to_string(), "beta".to_string()],
            },
        );
        let mut reversed_set = document_with_value(Value::Null);
        reversed_set.set_typed_field(
            "value",
            TypedScalarValue::StringSet {
                values: vec!["beta".to_string(), "alpha".to_string()],
            },
        );
        assert_eq!(
            state_with_document(forward_set)
                .digest()
                .expect("forward set digest should compute"),
            state_with_document(reversed_set)
                .digest()
                .expect("reversed set digest should compute")
        );
    }

    #[test]
    fn canonical_leaf_nan_and_positive_infinity_do_not_collide() {
        let mut nan = document_with_value(Value::Null);
        nan.set_typed_field(
            "value",
            TypedScalarValue::SpecialDouble {
                value: SpecialDouble::Nan,
            },
        );
        let mut infinity = document_with_value(Value::Null);
        infinity.set_typed_field(
            "value",
            TypedScalarValue::SpecialDouble {
                value: SpecialDouble::PositiveInfinity,
            },
        );

        assert_ne!(
            state_with_document(nan)
                .digest()
                .expect("NaN digest should compute"),
            state_with_document(infinity)
                .digest()
                .expect("infinity digest should compute")
        );
    }

    #[test]
    fn canonical_leaf_distinguishes_scalar_negative_zero() {
        let zero = document_with_value(json!(0.0));
        let mut negative_zero = document_with_value(Value::Null);
        negative_zero.set_typed_field(
            "value",
            TypedScalarValue::SpecialDouble {
                value: SpecialDouble::NegativeZero,
            },
        );

        assert_ne!(
            state_with_document(zero)
                .digest()
                .expect("zero digest should compute"),
            state_with_document(negative_zero)
                .digest()
                .expect("negative-zero digest should compute")
        );
    }

    #[test]
    fn canonical_leaf_normalizes_geo_point_negative_zero() {
        let mut zero = document_with_value(Value::Null);
        zero.set_typed_field(
            "value",
            TypedScalarValue::GeoPoint {
                latitude: 0.0,
                longitude: 0.0,
            },
        );
        let mut negative_zero = document_with_value(Value::Null);
        negative_zero.set_typed_field(
            "value",
            TypedScalarValue::GeoPoint {
                latitude: -0.0,
                longitude: -0.0,
            },
        );

        assert_eq!(
            state_with_document(zero)
                .digest()
                .expect("zero GeoPoint digest should compute"),
            state_with_document(negative_zero)
                .digest()
                .expect("negative-zero GeoPoint digest should compute")
        );
    }

    #[test]
    fn streaming_materialized_digest_matches_reference() {
        let state = state_with_document(document_with_value(json!({ "a": [true, null, 3] })));
        let bytes = state
            .reference_bytes()
            .expect("reference encoding should compute");
        let reference = hex::encode(Sha256::digest(bytes));

        assert_eq!(
            state.digest().expect("streaming digest should compute"),
            reference
        );
    }

    #[test]
    fn materialized_position_golden_matches_storage_graph() {
        let position = materialized_position_golden_fixture()
            .expect("materialized position fixture should compute");
        assert_eq!(position.version(), 3);
        assert_eq!(
            position.state_digest(),
            "af6dfc9b7f93c73314ae70ff777d7b2f93bef82704b11d18c87a5bae9bee36f9"
        );
    }

    #[test]
    fn materialized_position_rejects_invalid_construction_and_deserialization() {
        assert!(
            MaterializedPosition::new(SequenceNumber(1), "A".repeat(DIGEST_HEX_LEN)).is_err(),
            "the constructor must reject non-lowercase digests"
        );

        let wire = serde_json::json!({
            "version": MATERIALIZED_POSITION_VERSION + 1,
            "applied_sequence": 1,
            "state_digest": "0".repeat(DIGEST_HEX_LEN),
        });
        assert!(
            serde_json::from_value::<MaterializedPosition>(wire).is_err(),
            "deserialization must not construct an unsupported position"
        );
    }
}
