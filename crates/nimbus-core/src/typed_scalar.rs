use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::{Number, Value};

use crate::Timestamp;
use crate::encoding::base64_encode_standard;

/// Shared metadata for scalar values that plain JSON cannot carry without
/// losing database semantics.
///
/// This stays protocol-neutral and lives in `nimbus-core` so adapters can
/// translate transport-specific scalar encodings without inventing their own
/// storage-visible shims.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TypedScalarValue {
    Timestamp { value: Timestamp },
    FirestoreTimestamp { rfc3339: String },
    Bytes { data: Vec<u8> },
    Reference { resource_name: String },
    GeoPoint { latitude: f64, longitude: f64 },
    SpecialDouble { value: SpecialDouble },
    ObjectId { hex: String },
    Binary { subtype: u8, data: Vec<u8> },
    Decimal128 { repr: String },
    Regex { pattern: String, options: String },
    MongoTimestamp { seconds: u32, increment: u32 },
    MinKey,
    MaxKey,
    JavaScriptCode { code: String },
    // DynamoDB-specific scalars (see docs/private/plans/archive/dynamodb-adapter-plan.md): `Number`
    // is an arbitrary-precision decimal kept as its exact string (38 sig digits,
    // beyond f64/i64); `StringSet`/`NumberSet`/`BinarySet` are DynamoDB SS/NS/BS.
    Number { repr: String },
    StringSet { values: Vec<String> },
    NumberSet { values: Vec<String> },
    BinarySet { values: Vec<Vec<u8>> },
}

impl TypedScalarValue {
    pub fn projected_json(&self) -> Value {
        match self {
            Self::Timestamp { value } => Value::Number(Number::from(value.0)),
            Self::FirestoreTimestamp { rfc3339 } => Value::String(rfc3339.clone()),
            Self::Bytes { data } => Value::String(base64_encode_standard(data)),
            Self::Reference { resource_name } => Value::String(resource_name.clone()),
            Self::GeoPoint {
                latitude,
                longitude,
            } => serde_json::json!({
                "latitude": latitude,
                "longitude": longitude,
            }),
            Self::SpecialDouble { value } => value.projected_json(),
            Self::ObjectId { hex } => Value::String(hex.clone()),
            Self::Binary { data, .. } => {
                // Clean JSON cannot represent BSON binary subtype metadata; the
                // typed sidecar remains the authoritative lossless value.
                Value::String(base64_encode_standard(data))
            }
            Self::Decimal128 { repr } => Value::String(repr.clone()),
            Self::Regex { pattern, .. } => Value::String(pattern.clone()),
            Self::MongoTimestamp { seconds, increment } => {
                Value::String(format!("Timestamp({seconds}, {increment})"))
            }
            Self::MinKey => Value::String("MinKey".to_string()),
            Self::MaxKey => Value::String("MaxKey".to_string()),
            Self::JavaScriptCode { code } => Value::String(code.clone()),
            Self::Number { repr } => number_repr_to_json(repr),
            Self::StringSet { values } => {
                Value::Array(values.iter().map(|s| Value::String(s.clone())).collect())
            }
            Self::NumberSet { values } => {
                Value::Array(values.iter().map(|s| number_repr_to_json(s)).collect())
            }
            Self::BinarySet { values } => Value::Array(
                values
                    .iter()
                    .map(|b| Value::String(base64_encode_standard(b)))
                    .collect(),
            ),
        }
    }
}

/// Project a DynamoDB `N` decimal string to clean JSON: a JSON number when the
/// repr fits, else the exact decimal string. The typed sidecar remains the
/// authoritative full-precision value; this projection is only for adapters that
/// read the plain JSON view.
fn number_repr_to_json(repr: &str) -> Value {
    match serde_json::from_str::<Value>(repr) {
        Ok(value @ Value::Number(_)) => value,
        _ => Value::String(repr.to_string()),
    }
}

/// Special floating-point values that do not round-trip through JSON numbers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SpecialDouble {
    NegativeZero,
    Nan,
    PositiveInfinity,
    NegativeInfinity,
}

impl SpecialDouble {
    pub fn sentinel(self) -> &'static str {
        match self {
            Self::NegativeZero => "-0",
            Self::Nan => "NaN",
            Self::PositiveInfinity => "Infinity",
            Self::NegativeInfinity => "-Infinity",
        }
    }

    pub fn projected_json(self) -> Value {
        Value::String(self.sentinel().to_string())
    }
}

/// One shared value that may still be plain JSON or may require typed scalar
/// metadata to round-trip correctly.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum StoredValue {
    Json {
        value: Value,
    },
    TypedScalar {
        value: TypedScalarValue,
    },
    // Nested containers whose children may themselves be plain JSON, typed
    // scalars, or further maps/lists — lets typed scalars survive inside
    // DynamoDB `M`/`L` and as set members (the flat top-level sidecar could not).
    Map {
        entries: BTreeMap<String, StoredValue>,
    },
    List {
        items: Vec<StoredValue>,
    },
}

impl StoredValue {
    pub fn projected_json(&self) -> Value {
        match self {
            Self::Json { value } => value.clone(),
            Self::TypedScalar { value } => value.projected_json(),
            Self::Map { entries } => Value::Object(
                entries
                    .iter()
                    .map(|(key, value)| (key.clone(), value.projected_json()))
                    .collect(),
            ),
            Self::List { items } => {
                Value::Array(items.iter().map(StoredValue::projected_json).collect())
            }
        }
    }

    /// Whether this value tree carries semantics that its plain JSON
    /// projection cannot represent losslessly.
    pub fn contains_typed_metadata(&self) -> bool {
        match self {
            Self::Json { .. } => false,
            Self::TypedScalar { .. } => true,
            Self::Map { entries } => entries.values().any(Self::contains_typed_metadata),
            Self::List { items } => items.iter().any(Self::contains_typed_metadata),
        }
    }

    /// Collapse every metadata-free subtree back to plain JSON.
    ///
    /// Producers disagree on how they spell a value that needs no typed
    /// metadata: `from_json_tree` builds `Map`/`List` nodes all the way down,
    /// while adapters that lower wire values collapse those nodes to `Json`.
    /// Both spellings mean the same value, so anything comparing two stored
    /// values for equality must canonicalize first or it will report a false
    /// difference.
    pub fn canonical(&self) -> Self {
        if !self.contains_typed_metadata() {
            return Self::Json {
                value: self.projected_json(),
            };
        }
        match self {
            Self::Map { entries } => Self::Map {
                entries: entries
                    .iter()
                    .map(|(key, value)| (key.clone(), value.canonical()))
                    .collect(),
            },
            Self::List { items } => Self::List {
                items: items.iter().map(Self::canonical).collect(),
            },
            value => value.clone(),
        }
    }

    /// Compares two stored spellings through the normalized logical tree.
    pub fn logical_eq(&self, other: &Self) -> bool {
        self.canonical() == other.canonical()
    }

    /// Build a navigable plain value tree. Adapters use this as the base when
    /// updating one nested path while retaining typed metadata on siblings.
    pub fn from_json_tree(value: Value) -> Self {
        match value {
            Value::Object(entries) => Self::Map {
                entries: entries
                    .into_iter()
                    .map(|(key, value)| (key, Self::from_json_tree(value)))
                    .collect(),
            },
            Value::Array(items) => Self::List {
                items: items.into_iter().map(Self::from_json_tree).collect(),
            },
            value => Self::Json { value },
        }
    }
}

impl From<Value> for StoredValue {
    fn from(value: Value) -> Self {
        Self::Json { value }
    }
}

impl From<TypedScalarValue> for StoredValue {
    fn from(value: TypedScalarValue) -> Self {
        Self::TypedScalar { value }
    }
}

/// Shared numeric transform operand/result representation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum NumericValue {
    Integer { value: i64 },
    Double { value: f64 },
    SpecialDouble { value: SpecialDouble },
}

pub type TypedFieldMap = BTreeMap<String, StoredValue>;

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn timestamp_typed_scalar_projects_to_epoch_millis_number() {
        let value = TypedScalarValue::Timestamp {
            value: Timestamp(1_234),
        };

        assert_eq!(value.projected_json(), json!(1234_u64));
    }

    #[test]
    fn special_double_projects_to_stable_string_sentinel() {
        assert_eq!(SpecialDouble::Nan.projected_json(), json!("NaN"));
        assert_eq!(
            SpecialDouble::PositiveInfinity.projected_json(),
            json!("Infinity")
        );
    }

    #[test]
    fn stored_value_roundtrips_plain_json_and_typed_scalars() {
        let json_value = StoredValue::from(json!(7));
        let typed_value = StoredValue::from(TypedScalarValue::SpecialDouble {
            value: SpecialDouble::NegativeInfinity,
        });

        assert_eq!(json_value.projected_json(), json!(7));
        assert_eq!(typed_value.projected_json(), json!("-Infinity"));
    }

    #[test]
    fn object_id_projects_to_hex_string() {
        let value = TypedScalarValue::ObjectId {
            hex: "507f1f77bcf86cd799439011".into(),
        };
        assert_eq!(value.projected_json(), json!("507f1f77bcf86cd799439011"));
    }

    #[test]
    fn binary_projects_to_base64() {
        let value = TypedScalarValue::Binary {
            subtype: 0,
            data: vec![0xDE, 0xAD, 0xBE, 0xEF],
        };
        assert_eq!(value.projected_json(), json!("3q2+7w=="));
    }

    #[test]
    fn binary_projection_is_lossy_but_roundtrip_preserves_subtype() {
        let generic = TypedScalarValue::Binary {
            subtype: 0,
            data: vec![1, 2, 3],
        };
        let user_defined = TypedScalarValue::Binary {
            subtype: 0x80,
            data: vec![1, 2, 3],
        };

        assert_eq!(generic.projected_json(), user_defined.projected_json());

        let encoded = serde_json::to_string(&user_defined).expect("typed binary should serialize");
        let decoded: TypedScalarValue =
            serde_json::from_str(&encoded).expect("typed binary should deserialize");
        assert_eq!(decoded, user_defined);
    }

    #[test]
    fn decimal128_projects_to_string() {
        let value = TypedScalarValue::Decimal128 {
            repr: "1234.5678".into(),
        };
        assert_eq!(value.projected_json(), json!("1234.5678"));
    }

    #[test]
    fn min_max_key_project_to_sentinel_strings() {
        assert_eq!(TypedScalarValue::MinKey.projected_json(), json!("MinKey"));
        assert_eq!(TypedScalarValue::MaxKey.projected_json(), json!("MaxKey"));
    }

    #[test]
    fn mongo_timestamp_projects_to_string() {
        let value = TypedScalarValue::MongoTimestamp {
            seconds: 1000,
            increment: 1,
        };
        assert_eq!(value.projected_json(), json!("Timestamp(1000, 1)"));
    }

    #[test]
    fn regex_projects_pattern() {
        let value = TypedScalarValue::Regex {
            pattern: "^test.*$".into(),
            options: "i".into(),
        };
        assert_eq!(value.projected_json(), json!("^test.*$"));
    }

    #[test]
    fn javascript_code_projects_to_string() {
        let value = TypedScalarValue::JavaScriptCode {
            code: "function() { return 1; }".into(),
        };
        assert_eq!(value.projected_json(), json!("function() { return 1; }"));
    }

    #[test]
    fn typed_scalar_serde_roundtrip() {
        let values = vec![
            TypedScalarValue::FirestoreTimestamp {
                rfc3339: "2024-01-02T03:04:05.123456789Z".into(),
            },
            TypedScalarValue::Bytes {
                data: vec![1, 2, 3],
            },
            TypedScalarValue::Reference {
                resource_name: "projects/demo/databases/(default)/documents/cities/SF".into(),
            },
            TypedScalarValue::GeoPoint {
                latitude: 37.7749,
                longitude: -122.4194,
            },
            TypedScalarValue::ObjectId {
                hex: "507f1f77bcf86cd799439011".into(),
            },
            TypedScalarValue::Binary {
                subtype: 5,
                data: vec![1, 2, 3],
            },
            TypedScalarValue::Decimal128 {
                repr: "Infinity".into(),
            },
            TypedScalarValue::Regex {
                pattern: "abc".into(),
                options: "im".into(),
            },
            TypedScalarValue::MongoTimestamp {
                seconds: 42,
                increment: 7,
            },
            TypedScalarValue::MinKey,
            TypedScalarValue::MaxKey,
            TypedScalarValue::JavaScriptCode { code: "1+1".into() },
        ];
        for value in values {
            let json = serde_json::to_string(&value).expect("should serialize");
            let back: TypedScalarValue = serde_json::from_str(&json).expect("should deserialize");
            assert_eq!(value, back);
        }
    }

    #[test]
    fn nested_typed_metadata_is_detected_recursively() {
        let plain = StoredValue::from_json_tree(json!({ "nested": [1, 2, 3] }));
        assert!(!plain.contains_typed_metadata());

        let typed = StoredValue::Map {
            entries: BTreeMap::from([(
                "nested".to_owned(),
                StoredValue::List {
                    items: vec![StoredValue::TypedScalar {
                        value: TypedScalarValue::Bytes { data: vec![1, 2] },
                    }],
                },
            )]),
        };
        assert!(typed.contains_typed_metadata());
    }

    #[test]
    fn canonical_collapses_metadata_free_subtrees_so_equal_values_compare_equal() {
        // The same metadata-free value spelled two ways: navigable tree nodes
        // versus collapsed plain JSON.
        let tree = StoredValue::from_json_tree(json!({ "a": [1, "x"], "b": 2 }));
        let collapsed = StoredValue::Json {
            value: json!({ "a": [1, "x"], "b": 2 }),
        };
        assert_ne!(tree, collapsed, "the two spellings differ structurally");
        assert_eq!(
            tree.canonical(),
            collapsed.canonical(),
            "canonical form must make the two spellings compare equal"
        );
        assert_eq!(tree.canonical(), collapsed);

        // Subtrees that carry typed metadata keep their structure; only the
        // metadata-free siblings collapse.
        let mixed = StoredValue::Map {
            entries: BTreeMap::from([
                (
                    "plain".to_owned(),
                    StoredValue::from_json_tree(json!({ "n": [1] })),
                ),
                (
                    "typed".to_owned(),
                    StoredValue::List {
                        items: vec![StoredValue::TypedScalar {
                            value: TypedScalarValue::Bytes { data: vec![7] },
                        }],
                    },
                ),
            ]),
        };
        assert_eq!(
            mixed.canonical(),
            StoredValue::Map {
                entries: BTreeMap::from([
                    (
                        "plain".to_owned(),
                        StoredValue::Json {
                            value: json!({ "n": [1] })
                        },
                    ),
                    (
                        "typed".to_owned(),
                        StoredValue::List {
                            items: vec![StoredValue::TypedScalar {
                                value: TypedScalarValue::Bytes { data: vec![7] },
                            }],
                        },
                    ),
                ]),
            }
        );

        // Canonicalization never changes what the value projects to.
        for value in [tree, collapsed, mixed] {
            assert_eq!(value.canonical().projected_json(), value.projected_json());
        }
    }

    #[test]
    fn dynamodb_scalar_variants_project_and_roundtrip() {
        // N projects to a JSON number when it fits; sets project to JSON arrays.
        assert_eq!(
            TypedScalarValue::Number {
                repr: "123.45".into()
            }
            .projected_json(),
            json!(123.45)
        );
        // A 38-digit number exceeds f64/i64: the clean projection is a best-effort
        // JSON number (lossy), while the typed sidecar's exact `repr` is preserved
        // — proven by the exact-roundtrip loop below.
        let big = "1234567890123456789012345678901234567.8";
        assert!(
            TypedScalarValue::Number { repr: big.into() }
                .projected_json()
                .is_number()
        );
        assert_eq!(
            TypedScalarValue::StringSet {
                values: vec!["a".into(), "b".into()]
            }
            .projected_json(),
            json!(["a", "b"])
        );
        assert_eq!(
            TypedScalarValue::NumberSet {
                values: vec!["1".into(), "2".into()]
            }
            .projected_json(),
            json!([1, 2])
        );
        assert_eq!(
            TypedScalarValue::BinarySet {
                values: vec![vec![0u8], vec![255u8]]
            }
            .projected_json(),
            json!([
                base64_encode_standard([0u8]),
                base64_encode_standard([255u8])
            ])
        );

        for value in [
            TypedScalarValue::Number { repr: big.into() },
            TypedScalarValue::StringSet {
                values: vec!["x".into()],
            },
            TypedScalarValue::NumberSet {
                values: vec!["9".into()],
            },
            TypedScalarValue::BinarySet {
                values: vec![vec![1, 2, 3]],
            },
        ] {
            let json = serde_json::to_string(&value).expect("serialize");
            let back: TypedScalarValue = serde_json::from_str(&json).expect("deserialize");
            assert_eq!(value, back, "exact roundtrip preserves DynamoDB precision");
        }
    }

    #[test]
    fn stored_value_carries_typed_scalars_nested_in_map_and_list() {
        // A Map containing a precise Number and a List of a Binary — exactly the
        // nesting the flat top-level sidecar could not express (MongoDB L8).
        let mut entries = BTreeMap::new();
        entries.insert(
            "amount".to_string(),
            StoredValue::TypedScalar {
                value: TypedScalarValue::Number {
                    repr: "10000000000000000000000000000000000001".into(),
                },
            },
        );
        entries.insert(
            "blobs".to_string(),
            StoredValue::List {
                items: vec![StoredValue::TypedScalar {
                    value: TypedScalarValue::Binary {
                        subtype: 0,
                        data: vec![1, 2, 3],
                    },
                }],
            },
        );
        entries.insert(
            "label".to_string(),
            StoredValue::Json { value: json!("ok") },
        );
        let tree = StoredValue::Map { entries };

        // Clean JSON projection: nested binary becomes base64, plain JSON passes
        // through, and the big number projects best-effort as a JSON number
        // (its exact value is preserved in the typed tree, asserted by the
        // roundtrip below).
        let projected = tree.projected_json();
        assert!(projected["amount"].is_number());
        assert_eq!(
            projected["blobs"],
            json!([base64_encode_standard([1, 2, 3])])
        );
        assert_eq!(projected["label"], json!("ok"));

        // The typed tree itself roundtrips losslessly, so the nested typed
        // leaves survive storage.
        let serialized = serde_json::to_string(&tree).expect("serialize");
        let back: StoredValue = serde_json::from_str(&serialized).expect("deserialize");
        assert_eq!(tree, back);
    }
}
