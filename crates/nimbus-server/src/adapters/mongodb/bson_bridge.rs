use bson::oid::ObjectId;
use bson::spec::BinarySubtype;
use bson::{Bson, RawDocumentBuf};
use nimbus_core::typed_scalar::TypedScalarValue;
use nimbus_core::types::{DocumentId, TableName, Timestamp};
use nimbus_core::{Document, SpecialDouble};
use serde_json::{Map, Number, Value};

#[derive(Debug, thiserror::Error)]
pub enum BridgeError {
    #[error("invalid _id: {0}")]
    InvalidId(String),
    #[error("BSON conversion error: {0}")]
    Conversion(String),
}

/// Convert a raw BSON document (bytes) into a Nimbus `Document`.
///
/// Extracts `_id` for `DocumentId`, converts remaining fields to JSON with
/// typed scalar metadata for types that don't roundtrip through JSON.
pub fn bson_bytes_to_document(raw: &[u8], table: &TableName) -> Result<Document, BridgeError> {
    let raw_doc = RawDocumentBuf::from_bytes(raw.to_vec())
        .map_err(|e| BridgeError::Conversion(e.to_string()))?;
    let bson_doc: bson::Document = bson::deserialize_from_slice(raw_doc.as_bytes())
        .map_err(|e| BridgeError::Conversion(e.to_string()))?;

    bson_doc_to_document(&bson_doc, table)
}

/// Convert a `bson::Document` into a Nimbus `Document`.
pub fn bson_doc_to_document(
    bson_doc: &bson::Document,
    table: &TableName,
) -> Result<Document, BridgeError> {
    let (doc_id, id_typed) = extract_document_id(bson_doc)?;

    let mut fields = Map::new();
    let mut typed_fields = std::collections::BTreeMap::new();

    for (key, value) in bson_doc.iter() {
        if key == "_id" {
            continue;
        }
        let (json_val, typed_opt) = bson_to_json_with_metadata(value);
        fields.insert(key.to_string(), json_val);
        if let Some(typed) = typed_opt {
            typed_fields.insert(key.to_string(), typed);
        }
    }

    let mut doc = Document::with_id(doc_id, table.clone(), fields);
    if let Some(typed) = id_typed {
        typed_fields.insert("_id_type".to_string(), typed);
    }
    doc.typed_fields = typed_fields;
    Ok(doc)
}

/// Convert a Nimbus `Document` back to a `bson::Document` for wire responses.
pub fn document_to_bson_doc(document: &Document) -> bson::Document {
    let mut bson_doc = bson::Document::new();

    let id_bson = reconstruct_bson_id(document);
    bson_doc.insert("_id", id_bson);

    for (key, value) in &document.fields {
        let bson_val = match document.typed_field(key) {
            Some(typed) => typed_scalar_to_bson(typed),
            None => json_to_bson(value),
        };
        bson_doc.insert(key, bson_val);
    }

    bson_doc
}

/// Convert a Nimbus `Document` to raw BSON bytes for wire transmission.
pub fn document_to_bson_bytes(document: &Document) -> Vec<u8> {
    let doc = document_to_bson_doc(document);
    bson::serialize_to_vec(&doc).expect("bson::Document should always serialize")
}

/// Generate a new ObjectId and return it as a `DocumentId` plus typed metadata.
pub fn generate_object_id() -> (DocumentId, TypedScalarValue) {
    let oid = ObjectId::new();
    let hex = oid.to_hex();
    let doc_id = DocumentId::from_key(&hex).expect("ObjectId hex is always a valid document key");
    let typed = TypedScalarValue::ObjectId { hex };
    (doc_id, typed)
}

fn extract_document_id(
    bson_doc: &bson::Document,
) -> Result<(DocumentId, Option<TypedScalarValue>), BridgeError> {
    match bson_doc.get("_id") {
        None => {
            let (id, typed) = generate_object_id();
            Ok((id, Some(typed)))
        }
        Some(bson_val) => bson_id_to_document_id(bson_val),
    }
}

fn bson_id_to_document_id(
    value: &Bson,
) -> Result<(DocumentId, Option<TypedScalarValue>), BridgeError> {
    match value {
        Bson::ObjectId(oid) => {
            let hex = oid.to_hex();
            let id =
                DocumentId::from_key(&hex).map_err(|e| BridgeError::InvalidId(e.to_string()))?;
            Ok((id, Some(TypedScalarValue::ObjectId { hex })))
        }
        Bson::String(s) => {
            let id = DocumentId::from_key(s.as_str())
                .map_err(|e| BridgeError::InvalidId(e.to_string()))?;
            Ok((id, None))
        }
        Bson::Int32(n) => {
            let s = n.to_string();
            let id = DocumentId::from_key(&s).map_err(|e| BridgeError::InvalidId(e.to_string()))?;
            Ok((id, None))
        }
        Bson::Int64(n) => {
            let s = n.to_string();
            let id = DocumentId::from_key(&s).map_err(|e| BridgeError::InvalidId(e.to_string()))?;
            Ok((id, None))
        }
        other => {
            let canonical = bson_to_canonical_id_string(other);
            let id = DocumentId::from_key(&canonical)
                .map_err(|e| BridgeError::InvalidId(e.to_string()))?;
            let (_, typed) = bson_to_json_with_metadata(other);
            Ok((id, typed))
        }
    }
}

fn bson_to_canonical_id_string(value: &Bson) -> String {
    let mut wrapper = bson::Document::new();
    wrapper.insert("v", value.clone());
    let raw = bson::serialize_to_vec(&wrapper).expect("bson should serialize");
    raw.iter()
        .fold(String::with_capacity(raw.len() * 2), |mut acc, b| {
            use std::fmt::Write;
            let _ = write!(acc, "{b:02x}");
            acc
        })
}

fn reconstruct_bson_id(document: &Document) -> Bson {
    if let Some(typed) = document.typed_fields.get("_id_type") {
        return typed_scalar_to_bson(typed);
    }
    let id_str = document.id.as_str();
    if id_str.len() == 24 {
        if let Ok(oid) = ObjectId::parse_str(id_str) {
            return Bson::ObjectId(oid);
        }
    }
    if let Ok(n) = id_str.parse::<i64>() {
        if n >= i32::MIN as i64 && n <= i32::MAX as i64 {
            return Bson::Int32(n as i32);
        }
        return Bson::Int64(n);
    }
    Bson::String(id_str.to_string())
}

/// Convert a BSON value to JSON, returning optional typed scalar metadata
/// for values that need it for roundtrip fidelity.
fn bson_to_json_with_metadata(value: &Bson) -> (Value, Option<TypedScalarValue>) {
    match value {
        Bson::Double(f) => {
            if f.is_nan() {
                let typed = TypedScalarValue::SpecialDouble {
                    value: SpecialDouble::Nan,
                };
                (typed.projected_json(), Some(typed))
            } else if f.is_infinite() {
                let typed = TypedScalarValue::SpecialDouble {
                    value: if f.is_sign_positive() {
                        SpecialDouble::PositiveInfinity
                    } else {
                        SpecialDouble::NegativeInfinity
                    },
                };
                (typed.projected_json(), Some(typed))
            } else if *f == 0.0 && f.is_sign_negative() {
                let typed = TypedScalarValue::SpecialDouble {
                    value: SpecialDouble::NegativeZero,
                };
                (typed.projected_json(), Some(typed))
            } else {
                (
                    Number::from_f64(*f)
                        .map(Value::Number)
                        .unwrap_or(Value::Null),
                    None,
                )
            }
        }
        Bson::String(s) => (Value::String(s.clone()), None),
        Bson::Boolean(b) => (Value::Bool(*b), None),
        Bson::Null => (Value::Null, None),
        Bson::Int32(n) => (Value::Number(Number::from(*n)), None),
        Bson::Int64(n) => (Value::Number(Number::from(*n)), None),
        Bson::ObjectId(oid) => {
            let typed = TypedScalarValue::ObjectId { hex: oid.to_hex() };
            (typed.projected_json(), Some(typed))
        }
        Bson::DateTime(dt) => {
            let millis = dt.timestamp_millis();
            let typed = TypedScalarValue::Timestamp {
                value: Timestamp(millis as u64),
            };
            (typed.projected_json(), Some(typed))
        }
        Bson::Binary(bin) => {
            let typed = TypedScalarValue::Binary {
                subtype: bin.subtype.into(),
                data: bin.bytes.clone(),
            };
            (typed.projected_json(), Some(typed))
        }
        Bson::Decimal128(d) => {
            let typed = TypedScalarValue::Decimal128 {
                repr: d.to_string(),
            };
            (typed.projected_json(), Some(typed))
        }
        Bson::RegularExpression(re) => {
            let typed = TypedScalarValue::Regex {
                pattern: re.pattern.as_str().to_string(),
                options: re.options.as_str().to_string(),
            };
            (typed.projected_json(), Some(typed))
        }
        Bson::Timestamp(ts) => {
            let typed = TypedScalarValue::MongoTimestamp {
                seconds: ts.time,
                increment: ts.increment,
            };
            (typed.projected_json(), Some(typed))
        }
        Bson::MinKey => {
            let typed = TypedScalarValue::MinKey;
            (typed.projected_json(), Some(typed))
        }
        Bson::MaxKey => {
            let typed = TypedScalarValue::MaxKey;
            (typed.projected_json(), Some(typed))
        }
        Bson::JavaScriptCode(code) => {
            let typed = TypedScalarValue::JavaScriptCode { code: code.clone() };
            (typed.projected_json(), Some(typed))
        }
        Bson::JavaScriptCodeWithScope(jsc) => {
            let typed = TypedScalarValue::JavaScriptCode {
                code: jsc.code.clone(),
            };
            (typed.projected_json(), Some(typed))
        }
        Bson::Document(inner) => {
            let json_map = bson_doc_to_json_map(inner);
            (Value::Object(json_map), None)
        }
        Bson::Array(arr) => {
            let json_arr: Vec<Value> = arr
                .iter()
                .map(|v| bson_to_json_with_metadata(v).0)
                .collect();
            (Value::Array(json_arr), None)
        }
        Bson::Symbol(s) => (Value::String(s.clone()), None),
        Bson::Undefined => (Value::Null, None),
        Bson::DbPointer(_) => (Value::Null, None),
    }
}

fn bson_doc_to_json_map(doc: &bson::Document) -> Map<String, Value> {
    let mut map = Map::new();
    for (k, v) in doc.iter() {
        map.insert(k.to_string(), bson_to_json_with_metadata(v).0);
    }
    map
}

/// Convert a JSON value back to BSON.
fn json_to_bson(value: &Value) -> Bson {
    match value {
        Value::Null => Bson::Null,
        Value::Bool(b) => Bson::Boolean(*b),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                if i >= i32::MIN as i64 && i <= i32::MAX as i64 {
                    Bson::Int32(i as i32)
                } else {
                    Bson::Int64(i)
                }
            } else if let Some(f) = n.as_f64() {
                Bson::Double(f)
            } else {
                Bson::Null
            }
        }
        Value::String(s) => Bson::String(s.clone()),
        Value::Array(arr) => Bson::Array(arr.iter().map(json_to_bson).collect()),
        Value::Object(map) => {
            let mut doc = bson::Document::new();
            for (k, v) in map {
                doc.insert(k, json_to_bson(v));
            }
            Bson::Document(doc)
        }
    }
}

/// Convert a `TypedScalarValue` back to its original BSON representation.
fn typed_scalar_to_bson(typed: &TypedScalarValue) -> Bson {
    match typed {
        TypedScalarValue::Timestamp { value } => {
            Bson::DateTime(bson::DateTime::from_millis(value.0 as i64))
        }
        TypedScalarValue::SpecialDouble { value } => Bson::Double(match value {
            SpecialDouble::NegativeZero => -0.0_f64,
            SpecialDouble::Nan => f64::NAN,
            SpecialDouble::PositiveInfinity => f64::INFINITY,
            SpecialDouble::NegativeInfinity => f64::NEG_INFINITY,
        }),
        TypedScalarValue::ObjectId { hex } => match ObjectId::parse_str(hex) {
            Ok(oid) => Bson::ObjectId(oid),
            Err(_) => Bson::String(hex.clone()),
        },
        TypedScalarValue::Binary { subtype, data } => Bson::Binary(bson::Binary {
            subtype: BinarySubtype::from(*subtype),
            bytes: data.clone(),
        }),
        TypedScalarValue::Decimal128 { repr } => match repr.parse::<bson::Decimal128>() {
            Ok(d) => Bson::Decimal128(d),
            Err(_) => Bson::String(repr.clone()),
        },
        TypedScalarValue::Regex { pattern, options } => {
            match (
                bson::raw::CString::try_from(pattern.as_str()),
                bson::raw::CString::try_from(options.as_str()),
            ) {
                (Ok(p), Ok(o)) => Bson::RegularExpression(bson::Regex {
                    pattern: p,
                    options: o,
                }),
                _ => Bson::String(pattern.clone()),
            }
        }
        TypedScalarValue::MongoTimestamp { seconds, increment } => {
            Bson::Timestamp(bson::Timestamp {
                time: *seconds,
                increment: *increment,
            })
        }
        TypedScalarValue::MinKey => Bson::MinKey,
        TypedScalarValue::MaxKey => Bson::MaxKey,
        TypedScalarValue::JavaScriptCode { code } => Bson::JavaScriptCode(code.clone()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bson::{Bson, doc};

    fn test_table() -> TableName {
        TableName::new("test_collection").expect("valid table name")
    }

    #[test]
    fn roundtrip_simple_document() {
        let bson_doc = doc! {
            "_id": ObjectId::new(),
            "name": "Alice",
            "age": 30,
            "active": true,
        };

        let nimbus_doc = bson_doc_to_document(&bson_doc, &test_table()).unwrap();
        assert_eq!(
            nimbus_doc.get_field("name"),
            Some(&Value::String("Alice".into()))
        );
        assert_eq!(nimbus_doc.get_field("age"), Some(&Value::Number(30.into())));
        assert_eq!(nimbus_doc.get_field("active"), Some(&Value::Bool(true)));

        let back = document_to_bson_doc(&nimbus_doc);
        assert_eq!(back.get_str("name").unwrap(), "Alice");
        assert_eq!(back.get_i32("age").unwrap(), 30);
        assert!(back.get_bool("active").unwrap());
        assert!(back.get_object_id("_id").is_ok());
    }

    #[test]
    fn roundtrip_object_id() {
        let oid = ObjectId::new();
        let bson_doc = doc! { "_id": oid, "x": 1 };
        let nimbus_doc = bson_doc_to_document(&bson_doc, &test_table()).unwrap();

        assert_eq!(nimbus_doc.id.as_str(), oid.to_hex());
        assert!(nimbus_doc.typed_fields.contains_key("_id_type"));

        let back = document_to_bson_doc(&nimbus_doc);
        assert_eq!(back.get_object_id("_id").unwrap(), oid);
    }

    #[test]
    fn roundtrip_string_id() {
        let bson_doc = doc! { "_id": "my-custom-key", "x": 1 };
        let nimbus_doc = bson_doc_to_document(&bson_doc, &test_table()).unwrap();

        assert_eq!(nimbus_doc.id.as_str(), "my-custom-key");
        assert!(!nimbus_doc.typed_fields.contains_key("_id_type"));

        let back = document_to_bson_doc(&nimbus_doc);
        assert_eq!(back.get_str("_id").unwrap(), "my-custom-key");
    }

    #[test]
    fn roundtrip_integer_id() {
        let bson_doc = doc! { "_id": 42_i32, "x": 1 };
        let nimbus_doc = bson_doc_to_document(&bson_doc, &test_table()).unwrap();

        assert_eq!(nimbus_doc.id.as_str(), "42");

        let back = document_to_bson_doc(&nimbus_doc);
        assert_eq!(back.get_i32("_id").unwrap(), 42);
    }

    #[test]
    fn auto_generates_object_id_when_missing() {
        let bson_doc = doc! { "name": "Bob" };
        let nimbus_doc = bson_doc_to_document(&bson_doc, &test_table()).unwrap();

        assert_eq!(nimbus_doc.id.as_str().len(), 24);
        assert!(nimbus_doc.typed_fields.contains_key("_id_type"));
        assert!(matches!(
            nimbus_doc.typed_fields.get("_id_type"),
            Some(TypedScalarValue::ObjectId { .. })
        ));

        let back = document_to_bson_doc(&nimbus_doc);
        assert!(back.get_object_id("_id").is_ok());
    }

    #[test]
    fn roundtrip_special_doubles() {
        let bson_doc = doc! {
            "_id": ObjectId::new(),
            "nan_val": f64::NAN,
            "pos_inf": f64::INFINITY,
            "neg_inf": f64::NEG_INFINITY,
            "neg_zero": Bson::Double(-0.0),
            "normal": 3.14,
        };

        let nimbus_doc = bson_doc_to_document(&bson_doc, &test_table()).unwrap();
        assert!(nimbus_doc.typed_fields.contains_key("nan_val"));
        assert!(nimbus_doc.typed_fields.contains_key("pos_inf"));
        assert!(nimbus_doc.typed_fields.contains_key("neg_inf"));
        assert!(nimbus_doc.typed_fields.contains_key("neg_zero"));
        assert!(!nimbus_doc.typed_fields.contains_key("normal"));

        let back = document_to_bson_doc(&nimbus_doc);
        assert!(back.get_f64("nan_val").unwrap().is_nan());
        assert_eq!(back.get_f64("pos_inf").unwrap(), f64::INFINITY);
        assert_eq!(back.get_f64("neg_inf").unwrap(), f64::NEG_INFINITY);
        let neg_zero = back.get_f64("neg_zero").unwrap();
        assert!(neg_zero == 0.0 && neg_zero.is_sign_negative());
        assert_eq!(back.get_f64("normal").unwrap(), 3.14);
    }

    #[test]
    fn roundtrip_binary() {
        let bson_doc = doc! {
            "_id": ObjectId::new(),
            "data": Bson::Binary(bson::Binary {
                subtype: BinarySubtype::Generic,
                bytes: vec![0xCA, 0xFE, 0xBA, 0xBE],
            }),
        };

        let nimbus_doc = bson_doc_to_document(&bson_doc, &test_table()).unwrap();
        assert!(matches!(
            nimbus_doc.typed_fields.get("data"),
            Some(TypedScalarValue::Binary { subtype: 0, data }) if data == &[0xCA, 0xFE, 0xBA, 0xBE]
        ));

        let back = document_to_bson_doc(&nimbus_doc);
        let bin = back.get_binary_generic("data").unwrap();
        assert_eq!(bin, &[0xCA, 0xFE, 0xBA, 0xBE]);
    }

    #[test]
    fn roundtrip_regex() {
        let bson_doc = doc! {
            "_id": ObjectId::new(),
            "pattern": Bson::RegularExpression(bson::Regex {
                pattern: "^hello".try_into().unwrap(),
                options: "i".try_into().unwrap(),
            }),
        };

        let nimbus_doc = bson_doc_to_document(&bson_doc, &test_table()).unwrap();
        assert!(matches!(
            nimbus_doc.typed_fields.get("pattern"),
            Some(TypedScalarValue::Regex { pattern, options })
                if pattern == "^hello" && options == "i"
        ));

        let back = document_to_bson_doc(&nimbus_doc);
        match back.get("pattern") {
            Some(Bson::RegularExpression(re)) => {
                assert_eq!(re.pattern.as_str(), "^hello");
                assert_eq!(re.options.as_str(), "i");
            }
            other => panic!("expected regex, got: {:?}", other),
        }
    }

    #[test]
    fn roundtrip_datetime() {
        let millis = 1_700_000_000_000_i64;
        let bson_doc = doc! {
            "_id": ObjectId::new(),
            "created": bson::DateTime::from_millis(millis),
        };

        let nimbus_doc = bson_doc_to_document(&bson_doc, &test_table()).unwrap();
        assert!(matches!(
            nimbus_doc.typed_fields.get("created"),
            Some(TypedScalarValue::Timestamp { value: Timestamp(ms) }) if *ms == millis as u64
        ));

        let back = document_to_bson_doc(&nimbus_doc);
        assert_eq!(
            back.get_datetime("created").unwrap().timestamp_millis(),
            millis
        );
    }

    #[test]
    fn roundtrip_mongo_timestamp() {
        let bson_doc = doc! {
            "_id": ObjectId::new(),
            "ts": Bson::Timestamp(bson::Timestamp { time: 1000, increment: 5 }),
        };

        let nimbus_doc = bson_doc_to_document(&bson_doc, &test_table()).unwrap();
        assert!(matches!(
            nimbus_doc.typed_fields.get("ts"),
            Some(TypedScalarValue::MongoTimestamp {
                seconds: 1000,
                increment: 5
            })
        ));

        let back = document_to_bson_doc(&nimbus_doc);
        match back.get("ts") {
            Some(Bson::Timestamp(ts)) => {
                assert_eq!(ts.time, 1000);
                assert_eq!(ts.increment, 5);
            }
            other => panic!("expected Timestamp, got: {:?}", other),
        }
    }

    #[test]
    fn roundtrip_min_max_key() {
        let bson_doc = doc! {
            "_id": ObjectId::new(),
            "low": Bson::MinKey,
            "high": Bson::MaxKey,
        };

        let nimbus_doc = bson_doc_to_document(&bson_doc, &test_table()).unwrap();
        assert!(matches!(
            nimbus_doc.typed_fields.get("low"),
            Some(TypedScalarValue::MinKey)
        ));
        assert!(matches!(
            nimbus_doc.typed_fields.get("high"),
            Some(TypedScalarValue::MaxKey)
        ));

        let back = document_to_bson_doc(&nimbus_doc);
        assert_eq!(back.get("low"), Some(&Bson::MinKey));
        assert_eq!(back.get("high"), Some(&Bson::MaxKey));
    }

    #[test]
    fn roundtrip_nested_document() {
        let bson_doc = doc! {
            "_id": ObjectId::new(),
            "address": {
                "city": "Portland",
                "zip": "97201",
            },
        };

        let nimbus_doc = bson_doc_to_document(&bson_doc, &test_table()).unwrap();
        let addr = nimbus_doc.get_field("address").unwrap();
        assert_eq!(addr["city"], "Portland");
        assert_eq!(addr["zip"], "97201");

        let back = document_to_bson_doc(&nimbus_doc);
        let inner = back.get_document("address").unwrap();
        assert_eq!(inner.get_str("city").unwrap(), "Portland");
    }

    #[test]
    fn roundtrip_array() {
        let bson_doc = doc! {
            "_id": ObjectId::new(),
            "tags": ["rust", "mongodb", "nimbus"],
        };

        let nimbus_doc = bson_doc_to_document(&bson_doc, &test_table()).unwrap();
        let tags = nimbus_doc.get_field("tags").unwrap();
        assert_eq!(tags, &serde_json::json!(["rust", "mongodb", "nimbus"]));

        let back = document_to_bson_doc(&nimbus_doc);
        let arr = back.get_array("tags").unwrap();
        assert_eq!(arr.len(), 3);
    }

    #[test]
    fn roundtrip_null_and_bool() {
        let bson_doc = doc! {
            "_id": ObjectId::new(),
            "nothing": Bson::Null,
            "flag": true,
        };

        let nimbus_doc = bson_doc_to_document(&bson_doc, &test_table()).unwrap();
        assert_eq!(nimbus_doc.get_field("nothing"), Some(&Value::Null));
        assert_eq!(nimbus_doc.get_field("flag"), Some(&Value::Bool(true)));

        let back = document_to_bson_doc(&nimbus_doc);
        assert_eq!(back.get("nothing"), Some(&Bson::Null));
        assert!(back.get_bool("flag").unwrap());
    }

    #[test]
    fn roundtrip_javascript_code() {
        let bson_doc = doc! {
            "_id": ObjectId::new(),
            "validator": Bson::JavaScriptCode("function() { return true; }".into()),
        };

        let nimbus_doc = bson_doc_to_document(&bson_doc, &test_table()).unwrap();
        assert!(matches!(
            nimbus_doc.typed_fields.get("validator"),
            Some(TypedScalarValue::JavaScriptCode { code })
                if code == "function() { return true; }"
        ));

        let back = document_to_bson_doc(&nimbus_doc);
        match back.get("validator") {
            Some(Bson::JavaScriptCode(code)) => {
                assert_eq!(code, "function() { return true; }");
            }
            other => panic!("expected JavaScriptCode, got: {:?}", other),
        }
    }

    #[test]
    fn document_to_bson_bytes_produces_valid_bson() {
        let bson_doc = doc! {
            "_id": ObjectId::new(),
            "x": 42,
        };
        let nimbus_doc = bson_doc_to_document(&bson_doc, &test_table()).unwrap();
        let bytes = document_to_bson_bytes(&nimbus_doc);

        let parsed: bson::Document =
            bson::deserialize_from_slice(&bytes).expect("bytes should parse as BSON");
        assert_eq!(parsed.get_i32("x").unwrap(), 42);
    }

    #[test]
    fn generate_object_id_produces_valid_hex() {
        let (id, typed) = generate_object_id();
        assert_eq!(id.as_str().len(), 24);
        assert!(matches!(typed, TypedScalarValue::ObjectId { hex } if hex.len() == 24));
        assert!(ObjectId::parse_str(id.as_str()).is_ok());
    }

    #[test]
    fn json_numbers_prefer_int32_range() {
        let bson_doc = doc! {
            "_id": "test",
            "small": 42_i32,
            "big": i64::MAX,
        };
        let nimbus_doc = bson_doc_to_document(&bson_doc, &test_table()).unwrap();
        let back = document_to_bson_doc(&nimbus_doc);

        assert_eq!(back.get_i32("small").unwrap(), 42);
        assert_eq!(back.get_i64("big").unwrap(), i64::MAX);
    }

    #[test]
    fn decimal128_roundtrip() {
        let d128: bson::Decimal128 = "9876543210.123456789".parse().expect("valid decimal128");
        let bson_doc = doc! {
            "_id": ObjectId::new(),
            "price": d128,
        };

        let nimbus_doc = bson_doc_to_document(&bson_doc, &test_table()).unwrap();
        assert!(matches!(
            nimbus_doc.typed_fields.get("price"),
            Some(TypedScalarValue::Decimal128 { .. })
        ));

        let back = document_to_bson_doc(&nimbus_doc);
        assert!(matches!(back.get("price"), Some(Bson::Decimal128(_))));
    }
}
