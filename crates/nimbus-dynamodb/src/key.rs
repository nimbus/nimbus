//! Composite primary-key encoding.
//!
//! DynamoDB's primary key is a partition key (`HASH`) plus an optional sort key
//! (`RANGE`); Nimbus's `DocumentId` is a single validated UTF-8 string (≤1500
//! bytes, no `/`, no NUL — `nimbus_core` `validate_document_key`). This module
//! encodes `(pk, sk)` into one reversible `DocumentId` for **exact** addressing
//! (PutItem/GetItem/DeleteItem).
//!
//! Encoding: each key attribute becomes `<type><base64url(value-bytes)>` where
//! `<type>` ∈ `{S,N,B}` and base64url is unpadded (alphabet `[A-Za-z0-9-_]`, so
//! it never contains `/`, NUL, or the `.` segment separator). The composite id
//! is `<pk-seg>.<sk-seg>` (sort key present) or `<pk-seg>`. The type tag keeps
//! `S "1"` distinct from `N "1"`.
//!
//! Range/ordering on the sort key does **not** use this id (base64url is not
//! order-preserving); Query sort conditions evaluate the order-preserving `_sk`
//! projection — see D0.3's sortable-key follow-up and D2.1.
//!
//! **Divergence (recorded in `docs/adapters/dynamodb/divergences.md`):** DynamoDB
//! allows pk ≤2,048 B + sk ≤1,024 B; base64url inflation puts the encoded id over
//! Nimbus's hard 1,500-byte `DocumentId` cap, so the combined raw key is bounded
//! to ~1,100 B and oversize keys are rejected with `ValidationException`.

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use extenddb_core::error::DynamoDbError;
use extenddb_core::types::{AttributeValue, Item};

/// Nimbus `DocumentId` byte ceiling (`validate_document_key`). The encoded
/// composite key must not exceed it.
pub const MAX_DOCUMENT_ID_BYTES: usize = 1500;

/// Encode a `(pk, sk)` pair into a reversible composite `DocumentId` string.
///
/// # Errors
/// `ValidationException` if a key attribute is not `S`/`N`/`B`, or if the
/// encoded id exceeds [`MAX_DOCUMENT_ID_BYTES`].
pub fn encode_key(
    pk: &AttributeValue,
    sk: Option<&AttributeValue>,
) -> Result<String, DynamoDbError> {
    let id = match sk {
        Some(sk) => format!("{}.{}", encode_segment(pk)?, encode_segment(sk)?),
        None => encode_segment(pk)?,
    };
    if id.len() > MAX_DOCUMENT_ID_BYTES {
        return Err(DynamoDbError::ValidationException(format!(
            "Composite primary key exceeds the maximum supported size: {} bytes encoded > {MAX_DOCUMENT_ID_BYTES} (Nimbus DocumentId limit)",
            id.len()
        )));
    }
    Ok(id)
}

/// Decode a composite `DocumentId` back into its `(pk, sk)` attribute values.
///
/// # Errors
/// `ValidationException` if the id is not a well-formed composite key.
pub fn decode_key(id: &str) -> Result<(AttributeValue, Option<AttributeValue>), DynamoDbError> {
    match id.split_once('.') {
        Some((pk, sk)) => Ok((decode_segment(pk)?, Some(decode_segment(sk)?))),
        None => Ok((decode_segment(id)?, None)),
    }
}

fn encode_segment(value: &AttributeValue) -> Result<String, DynamoDbError> {
    let (tag, bytes): (char, &[u8]) = match value {
        AttributeValue::S(s) => ('S', s.as_bytes()),
        AttributeValue::N(n) => ('N', n.as_bytes()),
        AttributeValue::B(b) => ('B', b.as_slice()),
        _ => {
            return Err(DynamoDbError::ValidationException(
                "Key attributes must be of type String (S), Number (N), or Binary (B)".to_owned(),
            ));
        }
    };
    Ok(format!("{tag}{}", URL_SAFE_NO_PAD.encode(bytes)))
}

fn decode_segment(segment: &str) -> Result<AttributeValue, DynamoDbError> {
    let mut chars = segment.chars();
    let tag = chars
        .next()
        .ok_or_else(|| invalid_key("empty key segment"))?;
    let bytes = URL_SAFE_NO_PAD
        .decode(chars.as_str())
        .map_err(|_| invalid_key("key segment is not valid base64url"))?;
    match tag {
        'S' => Ok(AttributeValue::S(
            String::from_utf8(bytes).map_err(|_| invalid_key("S key is not valid UTF-8"))?,
        )),
        'N' => Ok(AttributeValue::N(
            String::from_utf8(bytes).map_err(|_| invalid_key("N key is not valid UTF-8"))?,
        )),
        'B' => Ok(AttributeValue::B(bytes)),
        _ => Err(invalid_key("unknown key type tag")),
    }
}

fn invalid_key(reason: &str) -> DynamoDbError {
    DynamoDbError::ValidationException(format!("The provided key is invalid: {reason}"))
}

/// Attribute names Nimbus reserves internally; incoming items may not use them.
///
/// `_pk`/`_sk` hold the order-preserving key projections; `_nimbus_*` is reserved
/// for internal markers.
#[must_use]
pub fn is_reserved_attribute_name(name: &str) -> bool {
    name == "_pk" || name == "_sk" || name.starts_with("_nimbus_")
}

/// Reject any top-level item attribute that collides with a Nimbus-reserved name.
///
/// # Errors
/// `ValidationException` naming the offending attribute.
pub fn validate_attribute_names(item: &Item) -> Result<(), DynamoDbError> {
    for name in item.keys() {
        if is_reserved_attribute_name(name) {
            return Err(DynamoDbError::ValidationException(format!(
                "Attribute name '{name}' is reserved for internal Nimbus use"
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn roundtrip(pk: AttributeValue, sk: Option<AttributeValue>) {
        let id = encode_key(&pk, sk.as_ref()).expect("encodes");
        assert!(
            !id.contains('/') && !id.contains('\0'),
            "id must satisfy DocumentId rules: {id}"
        );
        let (dpk, dsk) = decode_key(&id).expect("decodes");
        assert_eq!(dpk, pk);
        assert_eq!(dsk, sk);
    }

    #[test]
    fn roundtrips_across_types_and_unicode_plane() {
        roundtrip(AttributeValue::S("user#42".into()), None);
        roundtrip(
            AttributeValue::S("snowman ☃ 🦀 \u{1F4A9}".into()),
            Some(AttributeValue::S("2026-05-29T00:00:00Z".into())),
        );
        roundtrip(
            AttributeValue::N("-3.14159".into()),
            Some(AttributeValue::N(
                "99999999999999999999999999999999999999".into(),
            )),
        );
        roundtrip(
            AttributeValue::B(vec![0, 1, 2, 250, 255]),
            Some(AttributeValue::B(vec![255, 0, 127])),
        );
        // Mixed pk/sk types.
        roundtrip(
            AttributeValue::S("p".into()),
            Some(AttributeValue::N("7".into())),
        );
    }

    #[test]
    fn distinguishes_string_and_number_keys() {
        let s = encode_key(&AttributeValue::S("1".into()), None).unwrap();
        let n = encode_key(&AttributeValue::N("1".into()), None).unwrap();
        assert_ne!(s, n, "S \"1\" and N \"1\" must encode to distinct ids");
    }

    #[test]
    fn rejects_non_scalar_key() {
        let err = encode_key(&AttributeValue::Bool(true), None).unwrap_err();
        assert!(matches!(err, DynamoDbError::ValidationException(_)));
    }

    #[test]
    fn rejects_oversize_key() {
        let big = "x".repeat(MAX_DOCUMENT_ID_BYTES); // base64url inflates past the cap
        let err = encode_key(&AttributeValue::S(big), None).unwrap_err();
        assert!(matches!(err, DynamoDbError::ValidationException(_)));
    }

    #[test]
    fn reserved_attribute_names_rejected() {
        assert!(is_reserved_attribute_name("_pk"));
        assert!(is_reserved_attribute_name("_sk"));
        assert!(is_reserved_attribute_name("_nimbus_meta"));
        assert!(!is_reserved_attribute_name("userId"));

        let mut item: Item = BTreeMap::new();
        item.insert("_sk".to_string(), AttributeValue::S("x".into()));
        assert!(matches!(
            validate_attribute_names(&item),
            Err(DynamoDbError::ValidationException(_))
        ));

        let mut ok: Item = BTreeMap::new();
        ok.insert("userId".to_string(), AttributeValue::S("x".into()));
        assert!(validate_attribute_names(&ok).is_ok());
    }
}
