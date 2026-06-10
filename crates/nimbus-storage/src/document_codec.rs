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
    use serde_json::json;

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
