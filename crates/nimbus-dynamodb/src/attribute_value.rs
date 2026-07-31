//! AttributeValue ↔ persisted Nimbus document-field codec.
//!
//! DynamoDB items are typed (`S`/`N`/`B`/`SS`/`NS`/`BS`/`BOOL`/`NULL`/`M`/`L`).
//! Nimbus stores those values as AttributeValue wire JSON in `Document.fields`;
//! the reverse conversion reconstructs the DynamoDB item shape for reads,
//! expressions, stream images, and TTL sweeps.

use extenddb_core::error::DynamoDbError;
use extenddb_core::types::{AttributeValue, Item};
use serde_json::Value;

/// Serialize a DynamoDB item into the `Document.fields` map the engine persists.
///
/// Each attribute is stored as its **AttributeValue wire JSON** (e.g.
/// `{"N":"42"}`, `{"SS":["a"]}`), which is exactly lossless — N precision,
/// sets, binary, and nesting all survive — and rides the standard
/// `Mutation::Insert { fields }` path. The reverse is [`fields_to_item`].
///
/// # Errors
/// `InternalServerError` if an attribute fails to serialize (not expected for
/// well-formed `AttributeValue`s).
pub fn item_to_fields(item: &Item) -> Result<serde_json::Map<String, Value>, DynamoDbError> {
    item.iter()
        .map(|(k, v)| {
            serde_json::to_value(v)
                .map(|json| (k.clone(), json))
                .map_err(|error| DynamoDbError::InternalServerError(error.to_string()))
        })
        .collect()
}

/// Reconstruct a DynamoDB item from a persisted `Document.fields` map — the
/// reverse of [`item_to_fields`].
///
/// # Errors
/// `InternalServerError` if a stored field is not a valid serialized
/// `AttributeValue` (indicates storage corruption).
pub fn fields_to_item(fields: &serde_json::Map<String, Value>) -> Result<Item, DynamoDbError> {
    fields
        .iter()
        .map(|(k, v)| {
            serde_json::from_value::<AttributeValue>(v.clone())
                .map(|value| (k.clone(), value))
                .map_err(|error| {
                    DynamoDbError::InternalServerError(format!(
                        "corrupt stored item attribute '{k}': {error}"
                    ))
                })
        })
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
    // Reject top-level attribute names that collide with Nimbus-reserved
    // projection fields (`_pk`/`_sk`/`_gsi*`/…) before they reach storage (F12).
    crate::key::validate_attribute_names(item)?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{BTreeMap, BTreeSet};

    #[test]
    fn item_roundtrips_through_wire_json_fields() {
        // The persisted-fields form must be exactly lossless: N precision, sets,
        // binary, and nesting all survive fields -> item.
        let mut item: Item = BTreeMap::new();
        item.insert("pk".to_string(), AttributeValue::S("id-1".into()));
        item.insert(
            "big".to_string(),
            AttributeValue::N("99999999999999999999999999999999999999".into()),
        );
        item.insert(
            "tags".to_string(),
            AttributeValue::SS(["a", "b"].iter().map(|s| (*s).to_string()).collect()),
        );
        item.insert("bin".to_string(), AttributeValue::B(vec![0, 250, 7]));
        let mut nested = BTreeMap::new();
        nested.insert("n".to_string(), AttributeValue::N("3.5".into()));
        item.insert("m".to_string(), AttributeValue::M(nested));
        let fields = item_to_fields(&item).expect("serialize");
        let back = fields_to_item(&fields).expect("deserialize");
        assert_eq!(item, back, "item must roundtrip through wire-JSON fields");
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
    fn validate_item_rejects_reserved_attribute_names() {
        // F12: validate_attribute_names is now wired into the write path, so an
        // item carrying a Nimbus-reserved name (`_pk`/`_sk`/`_nimbus_*`) is
        // rejected before it can collide with the internal projection fields.
        for reserved in ["_pk", "_sk", "_nimbus_internal"] {
            let mut item: Item = BTreeMap::new();
            item.insert(reserved.to_string(), AttributeValue::S("x".into()));
            assert!(
                matches!(
                    validate_item(&item),
                    Err(DynamoDbError::ValidationException(_))
                ),
                "reserved attribute name {reserved} must be rejected"
            );
        }
        // A normal attribute name is fine.
        let mut ok: Item = BTreeMap::new();
        ok.insert("pk".to_string(), AttributeValue::S("a".into()));
        assert!(validate_item(&ok).is_ok());
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
