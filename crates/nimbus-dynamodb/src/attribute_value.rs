//! AttributeValue ↔ Nimbus `StoredValue` codec.
//!
//! DynamoDB items are typed (`S`/`N`/`B`/`SS`/`NS`/`BS`/`BOOL`/`NULL`/`M`/`L`).
//! Nimbus stores clean JSON in `Document.fields` (so other adapters read natural
//! values) plus a `StoredValue` typed sidecar for lossless roundtrip. This
//! module maps `extenddb_core::types::AttributeValue` to/from the promoted
//! `nimbus_core` `StoredValue` tree (see D0.1b):
//!
//! | AttributeValue | StoredValue                              |
//! |----------------|------------------------------------------|
//! | `S`            | `Json(String)`                           |
//! | `N`            | `TypedScalar(Number{repr})` (exact)      |
//! | `B`            | `TypedScalar(Binary{subtype:0,data})`    |
//! | `SS`/`NS`/`BS` | `TypedScalar(StringSet/NumberSet/BinarySet)` |
//! | `BOOL`/`NULL`  | `Json(Bool/Null)`                        |
//! | `L`            | `List([...])`                            |
//! | `M`            | `Map({...})`                             |

use std::collections::{BTreeMap, BTreeSet};

use extenddb_core::error::DynamoDbError;
use extenddb_core::types::{AttributeValue, Item};
use nimbus_core::typed_scalar::{StoredValue, TypedScalarValue};
use serde_json::Value;

/// Convert a DynamoDB `AttributeValue` into a lossless Nimbus `StoredValue`.
#[must_use]
pub fn attribute_value_to_stored(value: &AttributeValue) -> StoredValue {
    match value {
        AttributeValue::S(s) => json(Value::String(s.clone())),
        AttributeValue::Bool(b) => json(Value::Bool(*b)),
        AttributeValue::Null => json(Value::Null),
        AttributeValue::N(repr) => scalar(TypedScalarValue::Number { repr: repr.clone() }),
        AttributeValue::B(data) => scalar(TypedScalarValue::Binary {
            subtype: 0,
            data: data.clone(),
        }),
        AttributeValue::SS(set) => scalar(TypedScalarValue::StringSet {
            values: set.iter().cloned().collect(),
        }),
        AttributeValue::NS(set) => scalar(TypedScalarValue::NumberSet {
            values: set.iter().cloned().collect(),
        }),
        AttributeValue::BS(set) => scalar(TypedScalarValue::BinarySet {
            values: set.iter().cloned().collect(),
        }),
        AttributeValue::L(items) => StoredValue::List {
            items: items.iter().map(attribute_value_to_stored).collect(),
        },
        AttributeValue::M(map) => StoredValue::Map {
            entries: map
                .iter()
                .map(|(k, v)| (k.clone(), attribute_value_to_stored(v)))
                .collect(),
        },
    }
}

/// Convert a Nimbus `StoredValue` back into a DynamoDB `AttributeValue`.
#[must_use]
pub fn stored_to_attribute_value(value: &StoredValue) -> AttributeValue {
    match value {
        StoredValue::Json { value } => json_to_attribute_value(value),
        StoredValue::TypedScalar { value } => typed_scalar_to_attribute_value(value),
        StoredValue::List { items } => {
            AttributeValue::L(items.iter().map(stored_to_attribute_value).collect())
        }
        StoredValue::Map { entries } => AttributeValue::M(
            entries
                .iter()
                .map(|(k, v)| (k.clone(), stored_to_attribute_value(v)))
                .collect(),
        ),
    }
}

/// Convert a whole DynamoDB item to the Nimbus typed-field sidecar map.
#[must_use]
pub fn item_to_stored(item: &Item) -> BTreeMap<String, StoredValue> {
    item.iter()
        .map(|(k, v)| (k.clone(), attribute_value_to_stored(v)))
        .collect()
}

/// Reject the cases DynamoDB rejects at the item boundary: an empty top-level
/// item and any empty `SS`/`NS`/`BS` (at any nesting depth), with
/// `ValidationException`. Duplicate set members and wire-shape errors are
/// rejected upstream by `extenddb-core`'s AttributeValue deserializer.
pub fn validate_item(item: &Item) -> Result<(), DynamoDbError> {
    if item.is_empty() {
        return Err(DynamoDbError::ValidationException(
            "The number of conditions on the keys is invalid; an item must contain at least one attribute".to_owned(),
        ));
    }
    for value in item.values() {
        validate_attribute_value(value)?;
    }
    Ok(())
}

fn validate_attribute_value(value: &AttributeValue) -> Result<(), DynamoDbError> {
    match value {
        AttributeValue::SS(set) if set.is_empty() => Err(DynamoDbError::ValidationException(
            "One or more parameter values were invalid: An string set  may not be empty".to_owned(),
        )),
        AttributeValue::NS(set) if set.is_empty() => Err(DynamoDbError::ValidationException(
            "One or more parameter values were invalid: An number set  may not be empty".to_owned(),
        )),
        AttributeValue::BS(set) if set.is_empty() => Err(DynamoDbError::ValidationException(
            "One or more parameter values were invalid: Binary sets should not be empty".to_owned(),
        )),
        AttributeValue::L(items) => items.iter().try_for_each(validate_attribute_value),
        AttributeValue::M(map) => map.values().try_for_each(validate_attribute_value),
        _ => Ok(()),
    }
}

fn json(value: Value) -> StoredValue {
    StoredValue::Json { value }
}

fn scalar(value: TypedScalarValue) -> StoredValue {
    StoredValue::TypedScalar { value }
}

fn json_to_attribute_value(value: &Value) -> AttributeValue {
    match value {
        Value::Null => AttributeValue::Null,
        Value::Bool(b) => AttributeValue::Bool(*b),
        Value::String(s) => AttributeValue::S(s.clone()),
        Value::Number(n) => AttributeValue::N(n.to_string()),
        Value::Array(items) => {
            AttributeValue::L(items.iter().map(json_to_attribute_value).collect())
        }
        Value::Object(map) => AttributeValue::M(
            map.iter()
                .map(|(k, v)| (k.clone(), json_to_attribute_value(v)))
                .collect(),
        ),
    }
}

fn typed_scalar_to_attribute_value(value: &TypedScalarValue) -> AttributeValue {
    match value {
        TypedScalarValue::Number { repr } => AttributeValue::N(repr.clone()),
        TypedScalarValue::Binary { data, .. } => AttributeValue::B(data.clone()),
        TypedScalarValue::StringSet { values } => {
            AttributeValue::SS(values.iter().cloned().collect::<BTreeSet<_>>())
        }
        TypedScalarValue::NumberSet { values } => {
            AttributeValue::NS(values.iter().cloned().collect::<BTreeSet<_>>())
        }
        TypedScalarValue::BinarySet { values } => {
            AttributeValue::BS(values.iter().cloned().collect::<BTreeSet<_>>())
        }
        // Foreign (MongoDB/Firebase) typed scalars only appear when DynamoDB
        // cross-adapter reads data another adapter wrote; project best-effort.
        other => json_to_attribute_value(&other.projected_json()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roundtrip(value: AttributeValue) {
        let stored = attribute_value_to_stored(&value);
        let back = stored_to_attribute_value(&stored);
        assert_eq!(
            value, back,
            "AttributeValue must roundtrip through StoredValue"
        );
    }

    #[test]
    fn every_attribute_value_variant_roundtrips() {
        roundtrip(AttributeValue::S("hello".into()));
        roundtrip(AttributeValue::Bool(true));
        roundtrip(AttributeValue::Null);
        // N preserves full 38-digit precision (the whole point of the typed sidecar).
        roundtrip(AttributeValue::N(
            "3.141592653589793238462643383279502884".into(),
        ));
        roundtrip(AttributeValue::B(vec![0, 127, 255]));
        roundtrip(AttributeValue::SS(
            ["a", "b", "c"].iter().map(|s| (*s).to_string()).collect(),
        ));
        roundtrip(AttributeValue::NS(
            ["1", "2", "30"].iter().map(|s| (*s).to_string()).collect(),
        ));
        roundtrip(AttributeValue::BS(
            [vec![1u8], vec![2u8, 3u8]].into_iter().collect(),
        ));
        // Nested M containing L containing typed scalars — exercises the
        // recursive StoredValue tree end to end.
        let mut inner = BTreeMap::new();
        inner.insert("n".to_string(), AttributeValue::N("42".into()));
        inner.insert("bin".to_string(), AttributeValue::B(vec![9, 8, 7]));
        inner.insert(
            "list".to_string(),
            AttributeValue::L(vec![
                AttributeValue::S("x".into()),
                AttributeValue::N("99999999999999999999999999999999999999".into()),
                AttributeValue::Null,
            ]),
        );
        roundtrip(AttributeValue::M(inner));
    }

    #[test]
    fn binary_roundtrips_via_base64_projection() {
        let stored = attribute_value_to_stored(&AttributeValue::B(vec![0, 1, 2, 250]));
        // Clean JSON projection is base64 (readable by other adapters)...
        assert!(stored.projected_json().is_string());
        // ...and the exact bytes survive the typed roundtrip.
        assert_eq!(
            stored_to_attribute_value(&stored),
            AttributeValue::B(vec![0, 1, 2, 250])
        );
    }

    #[test]
    fn validate_item_rejects_empty_top_level_item() {
        let empty: Item = BTreeMap::new();
        assert!(matches!(
            validate_item(&empty),
            Err(DynamoDbError::ValidationException(_))
        ));
    }

    #[test]
    fn validate_item_rejects_empty_sets_at_any_depth() {
        let mut item: Item = BTreeMap::new();
        item.insert("tags".to_string(), AttributeValue::SS(BTreeSet::new()));
        assert!(matches!(
            validate_item(&item),
            Err(DynamoDbError::ValidationException(_))
        ));

        // Nested empty set inside a list is also rejected.
        let mut nested: Item = BTreeMap::new();
        nested.insert(
            "wrap".to_string(),
            AttributeValue::L(vec![AttributeValue::NS(BTreeSet::new())]),
        );
        assert!(matches!(
            validate_item(&nested),
            Err(DynamoDbError::ValidationException(_))
        ));
    }

    #[test]
    fn validate_item_accepts_a_normal_item() {
        let mut item: Item = BTreeMap::new();
        item.insert("pk".to_string(), AttributeValue::S("id-1".into()));
        item.insert("count".to_string(), AttributeValue::N("3".into()));
        item.insert(
            "tags".to_string(),
            AttributeValue::SS(["x"].iter().map(|s| (*s).to_string()).collect()),
        );
        assert!(validate_item(&item).is_ok());
    }
}
