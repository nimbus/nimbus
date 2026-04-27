use std::collections::BTreeMap;

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use neovex_core::{
    Document, NumericValue, SpecialDouble, StoredValue, Timestamp, TypedScalarValue,
};
use serde_json::{Map, Number, Value, json};
use thiserror::Error;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum FirestoreValue {
    Null,
    Boolean(bool),
    Integer(i64),
    Double(FirestoreDouble),
    Timestamp(String),
    String(String),
    Bytes(Vec<u8>),
    Reference(String),
    GeoPoint { latitude: f64, longitude: f64 },
    Array(Vec<FirestoreValue>),
    Map(BTreeMap<String, FirestoreValue>),
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum FirestoreDouble {
    Number(f64),
    NegativeZero,
    NaN,
    PositiveInfinity,
    NegativeInfinity,
}

#[derive(Debug, Error)]
pub(crate) enum FirestoreProtoJsonError {
    #[error("Firestore Value JSON must be an object with exactly one value type field")]
    InvalidShape,
    #[error("unsupported Firestore Value type `{0}`")]
    UnsupportedType(&'static str),
    #[error("invalid Firestore {field}: {reason}")]
    InvalidField { field: &'static str, reason: String },
}

pub(crate) fn decode_proto_json_value(value: &Value) -> Result<Value, FirestoreProtoJsonError> {
    FirestoreValue::from_proto_json(value)?.into_neovex_value()
}

pub(crate) fn decode_proto_json_numeric_value(
    value: &Value,
) -> Result<NumericValue, FirestoreProtoJsonError> {
    match FirestoreValue::from_proto_json(value)? {
        FirestoreValue::Integer(value) => Ok(NumericValue::Integer { value }),
        FirestoreValue::Double(value) => Ok(match value {
            FirestoreDouble::Number(value) => NumericValue::Double { value },
            value => NumericValue::SpecialDouble {
                value: special_double_from_firestore(value),
            },
        }),
        _ => Err(invalid_field(
            "doubleValue",
            "numeric transforms require Firestore integerValue or doubleValue operands".to_string(),
        )),
    }
}

pub(crate) fn encode_proto_json_value(value: &Value) -> Result<Value, FirestoreProtoJsonError> {
    FirestoreValue::try_from_neovex_value(value).map(|value| value.to_proto_json())
}

pub(crate) fn encode_proto_json_stored_value(
    value: &StoredValue,
) -> Result<Value, FirestoreProtoJsonError> {
    match value {
        StoredValue::Json { value } => encode_proto_json_value(value),
        StoredValue::TypedScalar { value } => encode_proto_json_typed_scalar(value),
    }
}

pub(crate) fn encode_proto_json_document_value(
    document: &Document,
    field_name: &str,
    value: &Value,
) -> Result<Value, FirestoreProtoJsonError> {
    match document.typed_field(field_name) {
        Some(value) => encode_proto_json_typed_scalar(value),
        None => encode_proto_json_value(value),
    }
}

impl FirestoreValue {
    pub(crate) fn from_proto_json(value: &Value) -> Result<Self, FirestoreProtoJsonError> {
        let Value::Object(object) = value else {
            return Err(FirestoreProtoJsonError::InvalidShape);
        };
        if object.len() != 1 {
            return Err(FirestoreProtoJsonError::InvalidShape);
        }
        let (kind, raw_value) = object.iter().next().expect("checked len == 1");
        match kind.as_str() {
            "nullValue" => {
                if raw_value.is_null() {
                    Ok(Self::Null)
                } else {
                    Err(invalid_field("nullValue", "expected null".to_string()))
                }
            }
            "booleanValue" => raw_value
                .as_bool()
                .map(Self::Boolean)
                .ok_or_else(|| invalid_field("booleanValue", "expected boolean".to_string())),
            "integerValue" => parse_integer_value(raw_value).map(Self::Integer),
            "doubleValue" => parse_double_value(raw_value).map(Self::Double),
            "timestampValue" => raw_value
                .as_str()
                .map(|value| Self::Timestamp(value.to_string()))
                .ok_or_else(|| invalid_field("timestampValue", "expected string".to_string())),
            "stringValue" => raw_value
                .as_str()
                .map(|value| Self::String(value.to_string()))
                .ok_or_else(|| invalid_field("stringValue", "expected string".to_string())),
            "bytesValue" => parse_bytes_value(raw_value).map(Self::Bytes),
            "referenceValue" => raw_value
                .as_str()
                .map(|value| Self::Reference(value.to_string()))
                .ok_or_else(|| invalid_field("referenceValue", "expected string".to_string())),
            "geoPointValue" => parse_geo_point_value(raw_value),
            "arrayValue" => parse_array_value(raw_value).map(Self::Array),
            "mapValue" => parse_map_value(raw_value).map(Self::Map),
            "fieldReferenceValue" => Err(FirestoreProtoJsonError::UnsupportedType(
                "fieldReferenceValue",
            )),
            "variableReferenceValue" => Err(FirestoreProtoJsonError::UnsupportedType(
                "variableReferenceValue",
            )),
            "functionValue" => Err(FirestoreProtoJsonError::UnsupportedType("functionValue")),
            "pipelineValue" => Err(FirestoreProtoJsonError::UnsupportedType("pipelineValue")),
            _ => Err(FirestoreProtoJsonError::InvalidShape),
        }
    }

    pub(crate) fn to_proto_json(&self) -> Value {
        match self {
            Self::Null => json!({ "nullValue": null }),
            Self::Boolean(value) => json!({ "booleanValue": value }),
            Self::Integer(value) => json!({ "integerValue": value.to_string() }),
            Self::Double(value) => json!({ "doubleValue": value.to_proto_json_value() }),
            Self::Timestamp(value) => json!({ "timestampValue": value }),
            Self::String(value) => json!({ "stringValue": value }),
            Self::Bytes(value) => json!({ "bytesValue": BASE64_STANDARD.encode(value) }),
            Self::Reference(value) => json!({ "referenceValue": value }),
            Self::GeoPoint {
                latitude,
                longitude,
            } => json!({
                "geoPointValue": {
                    "latitude": latitude,
                    "longitude": longitude,
                }
            }),
            Self::Array(values) => json!({
                "arrayValue": {
                    "values": values.iter().map(Self::to_proto_json).collect::<Vec<_>>()
                }
            }),
            Self::Map(fields) => {
                let fields = fields
                    .iter()
                    .map(|(key, value)| (key.clone(), value.to_proto_json()))
                    .collect::<Map<_, _>>();
                json!({
                    "mapValue": {
                        "fields": fields
                    }
                })
            }
        }
    }

    pub(crate) fn into_neovex_value(self) -> Result<Value, FirestoreProtoJsonError> {
        match self {
            Self::Null => Ok(Value::Null),
            Self::Boolean(value) => Ok(Value::Bool(value)),
            Self::Integer(value) => Ok(Value::Number(Number::from(value))),
            Self::Double(value) => value.into_neovex_value(),
            Self::String(value) => Ok(Value::String(value)),
            Self::Array(values) => values
                .into_iter()
                .map(Self::into_neovex_value)
                .collect::<Result<Vec<_>, _>>()
                .map(Value::Array),
            Self::Map(fields) => fields
                .into_iter()
                .map(|(key, value)| value.into_neovex_value().map(|value| (key, value)))
                .collect::<Result<Map<_, _>, _>>()
                .map(Value::Object),
            Self::Timestamp(_) => Err(FirestoreProtoJsonError::UnsupportedType("timestampValue")),
            Self::Bytes(_) => Err(FirestoreProtoJsonError::UnsupportedType("bytesValue")),
            Self::Reference(_) => Err(FirestoreProtoJsonError::UnsupportedType("referenceValue")),
            Self::GeoPoint { .. } => Err(FirestoreProtoJsonError::UnsupportedType("geoPointValue")),
        }
    }

    pub(crate) fn try_from_neovex_value(value: &Value) -> Result<Self, FirestoreProtoJsonError> {
        match value {
            Value::Null => Ok(Self::Null),
            Value::Bool(value) => Ok(Self::Boolean(*value)),
            Value::Number(value) => {
                if let Some(value) = value.as_i64() {
                    return Ok(Self::Integer(value));
                }
                if let Some(value) = value.as_u64() {
                    let value = i64::try_from(value).map_err(|_| {
                        invalid_field(
                            "integerValue",
                            "Neovex integer exceeds Firestore int64 range".to_string(),
                        )
                    })?;
                    return Ok(Self::Integer(value));
                }
                let value = value.as_f64().ok_or_else(|| {
                    invalid_field("doubleValue", "invalid JSON number".to_string())
                })?;
                Ok(Self::Double(FirestoreDouble::Number(value)))
            }
            Value::String(value) => Ok(Self::String(value.clone())),
            Value::Array(values) => values
                .iter()
                .map(Self::try_from_neovex_value)
                .collect::<Result<Vec<_>, _>>()
                .and_then(|values| {
                    if values.iter().any(|value| matches!(value, Self::Array(_))) {
                        Err(invalid_field(
                            "arrayValue",
                            "Firestore arrays cannot directly contain arrays".to_string(),
                        ))
                    } else {
                        Ok(Self::Array(values))
                    }
                }),
            Value::Object(fields) => fields
                .iter()
                .map(|(key, value)| {
                    Self::try_from_neovex_value(value).map(|value| (key.clone(), value))
                })
                .collect::<Result<BTreeMap<_, _>, _>>()
                .map(Self::Map),
        }
    }
}

impl FirestoreDouble {
    fn to_proto_json_value(&self) -> Value {
        match self {
            Self::Number(value) => json!(value),
            Self::NegativeZero => Value::String("-0".to_string()),
            Self::NaN => Value::String("NaN".to_string()),
            Self::PositiveInfinity => Value::String("Infinity".to_string()),
            Self::NegativeInfinity => Value::String("-Infinity".to_string()),
        }
    }

    fn into_neovex_value(self) -> Result<Value, FirestoreProtoJsonError> {
        match self {
            Self::Number(value) => Number::from_f64(value)
                .map(Value::Number)
                .ok_or_else(|| invalid_field("doubleValue", "invalid finite double".to_string())),
            Self::NegativeZero => Err(FirestoreProtoJsonError::UnsupportedType("doubleValue:-0")),
            Self::NaN => Err(FirestoreProtoJsonError::UnsupportedType("doubleValue:NaN")),
            Self::PositiveInfinity => Err(FirestoreProtoJsonError::UnsupportedType(
                "doubleValue:Infinity",
            )),
            Self::NegativeInfinity => Err(FirestoreProtoJsonError::UnsupportedType(
                "doubleValue:-Infinity",
            )),
        }
    }
}

fn encode_proto_json_typed_scalar(
    value: &TypedScalarValue,
) -> Result<Value, FirestoreProtoJsonError> {
    match value {
        TypedScalarValue::Timestamp { value } => {
            Ok(FirestoreValue::Timestamp(format_firestore_timestamp(*value)?).to_proto_json())
        }
        TypedScalarValue::SpecialDouble { value } => Ok(FirestoreValue::Double(
            firestore_double_from_special_double(*value),
        )
        .to_proto_json()),
        _ => Ok(FirestoreValue::String(value.projected_json().to_string()).to_proto_json()),
    }
}

fn special_double_from_firestore(value: FirestoreDouble) -> SpecialDouble {
    match value {
        FirestoreDouble::NegativeZero => SpecialDouble::NegativeZero,
        FirestoreDouble::NaN => SpecialDouble::Nan,
        FirestoreDouble::PositiveInfinity => SpecialDouble::PositiveInfinity,
        FirestoreDouble::NegativeInfinity => SpecialDouble::NegativeInfinity,
        FirestoreDouble::Number(_) => {
            unreachable!("finite doubles should not map to special doubles")
        }
    }
}

fn firestore_double_from_special_double(value: SpecialDouble) -> FirestoreDouble {
    match value {
        SpecialDouble::NegativeZero => FirestoreDouble::NegativeZero,
        SpecialDouble::Nan => FirestoreDouble::NaN,
        SpecialDouble::PositiveInfinity => FirestoreDouble::PositiveInfinity,
        SpecialDouble::NegativeInfinity => FirestoreDouble::NegativeInfinity,
    }
}

fn format_firestore_timestamp(timestamp: Timestamp) -> Result<String, FirestoreProtoJsonError> {
    OffsetDateTime::from_unix_timestamp_nanos(i128::from(timestamp.0) * 1_000_000)
        .map_err(|error| invalid_field("timestampValue", format!("invalid timestamp: {error}")))?
        .format(&Rfc3339)
        .map_err(|error| {
            invalid_field(
                "timestampValue",
                format!("failed to format timestamp: {error}"),
            )
        })
}

fn parse_integer_value(value: &Value) -> Result<i64, FirestoreProtoJsonError> {
    let Some(value) = value.as_str() else {
        return Err(invalid_field(
            "integerValue",
            "expected string-encoded int64".to_string(),
        ));
    };
    value.parse::<i64>().map_err(|error| {
        invalid_field(
            "integerValue",
            format!("expected string-encoded int64: {error}"),
        )
    })
}

fn parse_double_value(value: &Value) -> Result<FirestoreDouble, FirestoreProtoJsonError> {
    match value {
        Value::Number(number) => number
            .as_f64()
            .map(FirestoreDouble::Number)
            .ok_or_else(|| invalid_field("doubleValue", "invalid JSON number".to_string())),
        Value::String(special) => match special.as_str() {
            "-0" => Ok(FirestoreDouble::NegativeZero),
            "NaN" => Ok(FirestoreDouble::NaN),
            "Infinity" => Ok(FirestoreDouble::PositiveInfinity),
            "-Infinity" => Ok(FirestoreDouble::NegativeInfinity),
            _ => Err(invalid_field(
                "doubleValue",
                format!("unsupported double sentinel `{special}`"),
            )),
        },
        _ => Err(invalid_field(
            "doubleValue",
            "expected number or special string".to_string(),
        )),
    }
}

fn parse_bytes_value(value: &Value) -> Result<Vec<u8>, FirestoreProtoJsonError> {
    let Some(value) = value.as_str() else {
        return Err(invalid_field(
            "bytesValue",
            "expected base64 string".to_string(),
        ));
    };
    BASE64_STANDARD
        .decode(value)
        .map_err(|error| invalid_field("bytesValue", format!("invalid base64: {error}")))
}

fn parse_geo_point_value(value: &Value) -> Result<FirestoreValue, FirestoreProtoJsonError> {
    let Value::Object(object) = value else {
        return Err(invalid_field(
            "geoPointValue",
            "expected object".to_string(),
        ));
    };
    let latitude = object
        .get("latitude")
        .and_then(Value::as_f64)
        .ok_or_else(|| invalid_field("geoPointValue", "missing latitude".to_string()))?;
    let longitude = object
        .get("longitude")
        .and_then(Value::as_f64)
        .ok_or_else(|| invalid_field("geoPointValue", "missing longitude".to_string()))?;
    Ok(FirestoreValue::GeoPoint {
        latitude,
        longitude,
    })
}

fn parse_array_value(value: &Value) -> Result<Vec<FirestoreValue>, FirestoreProtoJsonError> {
    let Value::Object(object) = value else {
        return Err(invalid_field("arrayValue", "expected object".to_string()));
    };
    let values = object
        .get("values")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let values = values
        .iter()
        .map(FirestoreValue::from_proto_json)
        .collect::<Result<Vec<_>, _>>()?;
    if values
        .iter()
        .any(|value| matches!(value, FirestoreValue::Array(_)))
    {
        return Err(invalid_field(
            "arrayValue",
            "Firestore arrays cannot directly contain arrays".to_string(),
        ));
    }
    Ok(values)
}

fn parse_map_value(
    value: &Value,
) -> Result<BTreeMap<String, FirestoreValue>, FirestoreProtoJsonError> {
    let Value::Object(object) = value else {
        return Err(invalid_field("mapValue", "expected object".to_string()));
    };
    let fields = object
        .get("fields")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    fields
        .iter()
        .map(|(key, value)| {
            FirestoreValue::from_proto_json(value).map(|value| (key.clone(), value))
        })
        .collect::<Result<BTreeMap<_, _>, _>>()
}

fn invalid_field(field: &'static str, reason: String) -> FirestoreProtoJsonError {
    FirestoreProtoJsonError::InvalidField { field, reason }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn firestore_value_proto_json_roundtrips_supported_wire_types() {
        let cases = vec![
            json!({ "nullValue": null }),
            json!({ "booleanValue": true }),
            json!({ "integerValue": "42" }),
            json!({ "doubleValue": 1.5 }),
            json!({ "timestampValue": "2024-01-02T03:04:05.123456Z" }),
            json!({ "stringValue": "hello" }),
            json!({ "bytesValue": "AQID" }),
            json!({ "referenceValue": "projects/demo/databases/(default)/documents/cities/SF" }),
            json!({ "geoPointValue": { "latitude": 37.7749, "longitude": -122.4194 } }),
            json!({
                "arrayValue": {
                    "values": [
                        { "stringValue": "alpha" },
                        { "integerValue": "7" }
                    ]
                }
            }),
            json!({
                "mapValue": {
                    "fields": {
                        "name": { "stringValue": "Ada" },
                        "active": { "booleanValue": true }
                    }
                }
            }),
            json!({ "doubleValue": "NaN" }),
            json!({ "doubleValue": "Infinity" }),
            json!({ "doubleValue": "-Infinity" }),
        ];

        for case in cases {
            let parsed =
                FirestoreValue::from_proto_json(&case).expect("firestore proto json should parse");
            assert_eq!(parsed.to_proto_json(), case);
        }
    }

    #[test]
    fn firestore_value_converts_supported_neovex_json_values() {
        let source = json!({
            "name": "Ada",
            "age": 42,
            "score": 4.5,
            "flags": [true, null, "x"],
        });

        let parsed = FirestoreValue::try_from_neovex_value(&source)
            .expect("neovex json should convert to firestore value");
        let roundtrip = parsed
            .into_neovex_value()
            .expect("supported firestore values should convert back");

        assert_eq!(roundtrip, source);
    }

    #[test]
    fn firestore_value_rejects_firestore_only_types_for_neovex_conversion() {
        for value in [
            FirestoreValue::Timestamp("2024-01-02T03:04:05Z".to_string()),
            FirestoreValue::Bytes(vec![1, 2, 3]),
            FirestoreValue::Reference(
                "projects/demo/databases/(default)/documents/cities/SF".to_string(),
            ),
            FirestoreValue::GeoPoint {
                latitude: 1.0,
                longitude: 2.0,
            },
            FirestoreValue::Double(FirestoreDouble::NaN),
            FirestoreValue::Double(FirestoreDouble::PositiveInfinity),
            FirestoreValue::Double(FirestoreDouble::NegativeInfinity),
        ] {
            assert!(
                value.into_neovex_value().is_err(),
                "Firestore-only values should not silently coerce into Neovex JSON"
            );
        }
    }

    #[test]
    fn firestore_value_rejects_nested_arrays() {
        let nested_firestore_array = json!({
            "arrayValue": {
                "values": [
                    {
                        "arrayValue": {
                            "values": []
                        }
                    }
                ]
            }
        });
        assert!(FirestoreValue::from_proto_json(&nested_firestore_array).is_err());

        let nested_neovex_array = json!([[1, 2, 3]]);
        assert!(FirestoreValue::try_from_neovex_value(&nested_neovex_array).is_err());
    }

    #[test]
    fn firestore_value_rejects_unsupported_expression_types() {
        for case in [
            json!({ "fieldReferenceValue": "name" }),
            json!({ "variableReferenceValue": "pipeline_var" }),
            json!({ "functionValue": { "name": "like" } }),
            json!({ "pipelineValue": { "stages": [] } }),
        ] {
            assert!(FirestoreValue::from_proto_json(&case).is_err());
        }
    }
}
