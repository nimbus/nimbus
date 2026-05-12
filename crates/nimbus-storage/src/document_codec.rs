use nimbus_core::Document;

pub fn encode_document_msgpack(document: &Document) -> Result<Vec<u8>, rmp_serde::encode::Error> {
    rmp_serde::to_vec(document)
}

pub fn decode_document_msgpack(bytes: &[u8]) -> Result<Document, rmp_serde::decode::Error> {
    rmp_serde::from_slice(bytes)
}

#[cfg(test)]
mod tests {
    use nimbus_core::{Document, TableName};
    use serde_json::json;

    use super::{decode_document_msgpack, encode_document_msgpack};

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
}
