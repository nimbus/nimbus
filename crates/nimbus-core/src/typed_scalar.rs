use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::{Number, Value};

use crate::Timestamp;

/// Shared metadata for scalar values that plain JSON cannot carry without
/// losing database semantics.
///
/// This stays protocol-neutral and lives in `nimbus-core` so adapters can
/// translate transport-specific scalar encodings without inventing their own
/// storage-visible shims.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TypedScalarValue {
    Timestamp { value: Timestamp },
    SpecialDouble { value: SpecialDouble },
    ObjectId { hex: String },
    Binary { subtype: u8, data: Vec<u8> },
    Decimal128 { repr: String },
    Regex { pattern: String, options: String },
    MongoTimestamp { seconds: u32, increment: u32 },
    MinKey,
    MaxKey,
    JavaScriptCode { code: String },
}

impl TypedScalarValue {
    pub fn projected_json(&self) -> Value {
        match self {
            Self::Timestamp { value } => Value::Number(Number::from(value.0)),
            Self::SpecialDouble { value } => value.projected_json(),
            Self::ObjectId { hex } => Value::String(hex.clone()),
            Self::Binary { data, .. } => Value::String(base64_encode(data)),
            Self::Decimal128 { repr } => Value::String(repr.clone()),
            Self::Regex { pattern, .. } => Value::String(pattern.clone()),
            Self::MongoTimestamp { seconds, increment } => {
                Value::String(format!("Timestamp({seconds}, {increment})"))
            }
            Self::MinKey => Value::String("MinKey".to_string()),
            Self::MaxKey => Value::String("MaxKey".to_string()),
            Self::JavaScriptCode { code } => Value::String(code.clone()),
        }
    }
}

fn base64_encode(data: &[u8]) -> String {
    use std::fmt::Write;
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = if chunk.len() > 1 { chunk[1] as u32 } else { 0 };
        let b2 = if chunk.len() > 2 { chunk[2] as u32 } else { 0 };
        let n = (b0 << 16) | (b1 << 8) | b2;
        let _ = out.write_char(CHARS[((n >> 18) & 63) as usize] as char);
        let _ = out.write_char(CHARS[((n >> 12) & 63) as usize] as char);
        if chunk.len() > 1 {
            let _ = out.write_char(CHARS[((n >> 6) & 63) as usize] as char);
        } else {
            let _ = out.write_char('=');
        }
        if chunk.len() > 2 {
            let _ = out.write_char(CHARS[(n & 63) as usize] as char);
        } else {
            let _ = out.write_char('=');
        }
    }
    out
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
    Json { value: Value },
    TypedScalar { value: TypedScalarValue },
}

impl StoredValue {
    pub fn projected_json(&self) -> Value {
        match self {
            Self::Json { value } => value.clone(),
            Self::TypedScalar { value } => value.projected_json(),
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

impl NumericValue {
    pub fn projected_json(&self) -> Value {
        match self {
            Self::Integer { value } => Value::Number(Number::from(*value)),
            Self::Double { value } => Number::from_f64(*value)
                .map(Value::Number)
                .expect("finite numeric transform doubles should serialize"),
            Self::SpecialDouble { value } => value.projected_json(),
        }
    }

    pub fn into_stored_value(self) -> StoredValue {
        match self {
            Self::Integer { value } => StoredValue::Json {
                value: Value::Number(Number::from(value)),
            },
            Self::Double { value } => StoredValue::Json {
                value: Number::from_f64(value)
                    .map(Value::Number)
                    .expect("finite numeric transform doubles should serialize"),
            },
            Self::SpecialDouble { value } => StoredValue::TypedScalar {
                value: TypedScalarValue::SpecialDouble { value },
            },
        }
    }
}

pub type TypedFieldMap = BTreeMap<String, TypedScalarValue>;

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
}
