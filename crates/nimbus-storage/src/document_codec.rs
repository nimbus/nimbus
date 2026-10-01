use nimbus_core::Document;

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
    rmp_serde::to_vec(document)
}

pub fn decode_document_msgpack(bytes: &[u8]) -> Result<Document, rmp_serde::decode::Error> {
    rmp_serde::from_slice(bytes)
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use nimbus_core::{Document, TableName, Timestamp, TypedScalarValue};
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
    fn codec_map_key_order_is_insertion_order() {
        let mut object = serde_json::Map::new();
        object.insert("key_b".to_string(), json!(1));
        object.insert("key_a".to_string(), json!(2));
        let mut fields = serde_json::Map::new();
        fields.insert("zeta".to_string(), json!(1));
        fields.insert("alpha".to_string(), json!(2));
        fields.insert("object".to_string(), Value::Object(object));
        let document = Document::new(
            TableName::new("tasks").expect("table name should be valid"),
            fields,
        );

        let bytes = encode_document_msgpack(&document).expect("document should serialize");
        let offset = |key: &str| {
            bytes
                .windows(key.len())
                .position(|window| window == key.as_bytes())
                .unwrap_or_else(|| panic!("{key} should be persisted"))
        };
        for [before, after] in [["zeta", "alpha"], ["alpha", "object"], ["key_b", "key_a"]] {
            assert!(
                offset(before) < offset(after),
                "{before} should persist before {after}"
            );
        }

        let decoded = decode_document_msgpack(&bytes).expect("document should deserialize");
        assert_eq!(
            decoded.fields.keys().collect::<Vec<_>>(),
            ["zeta", "alpha", "object"]
        );
        let Some(Value::Object(object)) = decoded.fields.get("object") else {
            panic!("object field should decode as a JSON object");
        };
        assert_eq!(object.keys().collect::<Vec<_>>(), ["key_b", "key_a"]);
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
