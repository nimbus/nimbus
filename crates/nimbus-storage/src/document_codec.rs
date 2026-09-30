use nimbus_core::{
    Document, DocumentId, StoredValue, TableName, Timestamp, TypedFieldMap, TypedScalarValue,
};
use serde::{Serialize, Serializer};
use serde_json::Value;

pub(crate) const DOCUMENT_MSGPACK_REQUIRED_FIELD_COUNT: u32 = 5;
pub(crate) const DOCUMENT_MSGPACK_FIELD_COUNT_WITH_TYPED_FIELDS: u32 = 6;
pub(crate) const DOCUMENT_MSGPACK_FIELDS_INDEX: u32 = 4;

pub(crate) fn is_supported_document_msgpack_field_count(field_count: u32) -> bool {
    matches!(
        field_count,
        DOCUMENT_MSGPACK_REQUIRED_FIELD_COUNT | DOCUMENT_MSGPACK_FIELD_COUNT_WITH_TYPED_FIELDS
    )
}

pub fn encode_document_msgpack(document: &Document) -> Result<Vec<u8>, rmp_serde::encode::Error> {
    rmp_serde::to_vec(&SortedDocument::from(document))
}

pub fn decode_document_msgpack(bytes: &[u8]) -> Result<Document, rmp_serde::decode::Error> {
    rmp_serde::from_slice(bytes)
}

/// Borrowed mirror of the derived `Document` layout that writes every JSON
/// object in sorted key order.
///
/// The shipped Cargo graph enables `serde_json/preserve_order`, so
/// `serde_json::Map` iterates in insertion order there and in sorted order in
/// a storage-only graph. Sorting at this seam keeps persisted bytes independent
/// of feature unification. A document whose maps are already sorted encodes to
/// the same bytes as the derived layout.
#[derive(Serialize)]
#[serde(rename = "Document")]
struct SortedDocument<'a> {
    id: &'a DocumentId,
    table: &'a TableName,
    creation_time: &'a Timestamp,
    update_time: &'a Timestamp,
    fields: SortedObject<'a>,
    #[serde(skip_serializing_if = "SortedStoredMap::is_empty")]
    typed_fields: SortedStoredMap<'a>,
}

impl<'a> From<&'a Document> for SortedDocument<'a> {
    fn from(document: &'a Document) -> Self {
        // Exhaustive so that a new `Document` field cannot silently drop out of
        // the persisted bytes.
        let Document {
            id,
            table,
            creation_time,
            update_time,
            fields,
            typed_fields,
        } = document;
        Self {
            id,
            table,
            creation_time,
            update_time,
            fields: SortedObject(fields),
            typed_fields: SortedStoredMap(typed_fields),
        }
    }
}

struct SortedObject<'a>(&'a serde_json::Map<String, Value>);

impl Serialize for SortedObject<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut entries = self.0.iter().collect::<Vec<_>>();
        entries.sort_unstable_by_key(|(key, _)| *key);
        serializer.collect_map(
            entries
                .into_iter()
                .map(|(key, value)| (key, SortedValue(value))),
        )
    }
}

struct SortedValue<'a>(&'a Value);

impl Serialize for SortedValue<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self.0 {
            Value::Array(items) => serializer.collect_seq(items.iter().map(SortedValue)),
            Value::Object(entries) => SortedObject(entries).serialize(serializer),
            value => value.serialize(serializer),
        }
    }
}

struct SortedStoredMap<'a>(&'a TypedFieldMap);

impl SortedStoredMap<'_> {
    fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl Serialize for SortedStoredMap<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_map(
            self.0
                .iter()
                .map(|(key, value)| (key, SortedStoredValue::from(value))),
        )
    }
}

struct SortedStoredList<'a>(&'a [StoredValue]);

impl Serialize for SortedStoredList<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_seq(self.0.iter().map(SortedStoredValue::from))
    }
}

/// Borrowed mirror of the derived `StoredValue` layout. A plain JSON value can
/// sit inside the typed sidecar, so its objects need the same key order.
#[derive(Serialize)]
#[serde(tag = "type", rename_all = "snake_case", rename = "StoredValue")]
enum SortedStoredValue<'a> {
    Json { value: SortedValue<'a> },
    TypedScalar { value: &'a TypedScalarValue },
    Map { entries: SortedStoredMap<'a> },
    List { items: SortedStoredList<'a> },
}

impl<'a> From<&'a StoredValue> for SortedStoredValue<'a> {
    fn from(value: &'a StoredValue) -> Self {
        match value {
            StoredValue::Json { value } => Self::Json {
                value: SortedValue(value),
            },
            StoredValue::TypedScalar { value } => Self::TypedScalar { value },
            StoredValue::Map { entries } => Self::Map {
                entries: SortedStoredMap(entries),
            },
            StoredValue::List { items } => Self::List {
                items: SortedStoredList(items),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::io::Cursor;

    use nimbus_core::{Document, StoredValue, TableName, Timestamp, TypedScalarValue};
    use rmp::decode::read_array_len;
    use serde_json::{Value, json};

    use super::{
        DOCUMENT_MSGPACK_FIELD_COUNT_WITH_TYPED_FIELDS, DOCUMENT_MSGPACK_REQUIRED_FIELD_COUNT,
        decode_document_msgpack, encode_document_msgpack,
    };

    #[test]
    fn document_msgpack_roundtrip_preserves_all_fields() {
        let document = Document::new(
            TableName::new("tasks").expect("table name should be valid"),
            serde_json::Map::from_iter([
                ("title".to_string(), json!("Hello")),
                ("rank".to_string(), json!(2)),
                ("active".to_string(), json!(true)),
            ]),
        );

        let bytes = encode_document_msgpack(&document).expect("document should serialize");
        let decoded = decode_document_msgpack(&bytes).expect("document should deserialize");

        assert_eq!(decoded, document);
    }

    #[test]
    fn codec_map_key_order_is_sorted() {
        let mut nested = serde_json::Map::new();
        nested.insert("nested_zulu".to_string(), json!(1));
        nested.insert("nested_alpha".to_string(), json!(2));
        let mut fields = serde_json::Map::new();
        fields.insert("field_zulu".to_string(), json!("z"));
        fields.insert("field_alpha".to_string(), json!([Value::Object(nested)]));
        fields.insert("field_mike".to_string(), json!(true));
        let mut typed_json = serde_json::Map::new();
        typed_json.insert("typed_zulu".to_string(), json!(1));
        typed_json.insert("typed_alpha".to_string(), json!(2));
        let mut document = Document::new(
            TableName::new("tasks").expect("table name should be valid"),
            fields,
        );
        document.typed_fields.insert(
            "meta".to_string(),
            StoredValue::Json {
                value: Value::Object(typed_json),
            },
        );

        let bytes = encode_document_msgpack(&document).expect("document should serialize");
        let offset = |key: &str| {
            bytes
                .windows(key.len())
                .position(|window| window == key.as_bytes())
                .unwrap_or_else(|| panic!("{key} should be persisted"))
        };
        for [before, after] in [
            ["field_alpha", "field_mike"],
            ["field_mike", "field_zulu"],
            ["nested_alpha", "nested_zulu"],
            ["typed_alpha", "typed_zulu"],
        ] {
            assert!(
                offset(before) < offset(after),
                "{before} should persist before {after}"
            );
        }
    }

    #[test]
    fn codec_bytes_match_derived_layout_for_sorted_documents() {
        let plain = Document::new(
            TableName::new("tasks").expect("table name should be valid"),
            serde_json::Map::from_iter([
                (
                    "alpha".to_string(),
                    json!({"inner_a": 1, "inner_b": [{"deep_a": null, "deep_b": 2.5}]}),
                ),
                ("beta".to_string(), json!("text")),
            ]),
        );
        let mut typed = plain.clone();
        typed.typed_fields.insert(
            "at".to_string(),
            StoredValue::TypedScalar {
                value: TypedScalarValue::Timestamp {
                    value: Timestamp(7),
                },
            },
        );
        typed.typed_fields.insert(
            "tree".to_string(),
            StoredValue::Map {
                entries: BTreeMap::from([
                    (
                        "json".to_string(),
                        StoredValue::Json {
                            value: json!({"a": 1, "b": 2}),
                        },
                    ),
                    (
                        "list".to_string(),
                        StoredValue::List {
                            items: vec![StoredValue::TypedScalar {
                                value: TypedScalarValue::Bytes { data: vec![1, 2] },
                            }],
                        },
                    ),
                ]),
            },
        );

        for document in [plain, typed] {
            assert_eq!(
                encode_document_msgpack(&document).expect("document should serialize"),
                rmp_serde::to_vec(&document).expect("derived layout should serialize")
            );
        }
    }

    #[test]
    fn document_msgpack_layout_constants_match_plain_and_typed_documents() {
        let plain = Document::new(
            TableName::new("tasks").expect("table name should be valid"),
            serde_json::Map::from_iter([("title".to_string(), json!("Hello"))]),
        );
        let plain_bytes = encode_document_msgpack(&plain).expect("document should serialize");
        assert_eq!(
            read_array_len(&mut Cursor::new(plain_bytes.as_slice()))
                .expect("document should start with an array"),
            DOCUMENT_MSGPACK_REQUIRED_FIELD_COUNT
        );

        let mut typed = Document::new(
            TableName::new("tasks").expect("table name should be valid"),
            serde_json::Map::new(),
        );
        typed.set_typed_field(
            "updatedAt",
            TypedScalarValue::Timestamp {
                value: Timestamp(42),
            },
        );
        let typed_bytes = encode_document_msgpack(&typed).expect("document should serialize");
        assert_eq!(
            read_array_len(&mut Cursor::new(typed_bytes.as_slice()))
                .expect("document should start with an array"),
            DOCUMENT_MSGPACK_FIELD_COUNT_WITH_TYPED_FIELDS
        );
    }
}
