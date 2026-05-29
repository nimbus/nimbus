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

use std::str::FromStr;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use bigdecimal::BigDecimal;
use bigdecimal::num_bigint::Sign;
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

/// Project a key/index attribute into an **order-preserving** sortable string for
/// the `_pk`/`_sk` (and per-index) fields, so range conditions compare with
/// DynamoDB's type semantics even though Nimbus's index/compare path is `f64`
/// (lossy for big `N`) and cannot index binary:
///
/// - `S` → the raw UTF-8 string (byte-wise == DynamoDB string ordering).
/// - `B` → fixed-case lowercase hex (order-preserving for unsigned byte-wise).
/// - `N` → a full-precision lexicographically-sortable decimal encoding (below);
///   lexicographic order equals numeric order at the full 38 significant digits,
///   and numerically-equal numbers map to identical strings.
///
/// # Errors
/// `ValidationException` if the attribute is not `S`/`N`/`B`, or for an `N` whose
/// magnitude/precision is outside DynamoDB's supported range.
pub fn sortable_key(value: &AttributeValue) -> Result<String, DynamoDbError> {
    match value {
        AttributeValue::S(s) => Ok(s.clone()),
        AttributeValue::B(b) => Ok(hex_lower(b)),
        AttributeValue::N(n) => sortable_number(n),
        _ => Err(DynamoDbError::ValidationException(
            "Key and index attributes must be of type String (S), Number (N), or Binary (B)"
                .to_owned(),
        )),
    }
}

/// Lexicographically-sortable encoding of a DynamoDB `N` decimal.
///
/// Form: a 1-char class tag (`1` negative < `5` zero < `7` positive), then a
/// 3-digit biased adjusted-exponent, then the 38-digit zero-padded significant
/// mantissa. For negatives the exponent is inverted (`999 - biased`) and the
/// mantissa 9's-complemented, so larger-magnitude negatives sort first. Fixed
/// mantissa width removes the variable-length prefix hazard. Numerically-equal
/// inputs normalize to the same `(sign, mantissa, exponent)` and thus the same
/// string.
fn sortable_number(repr: &str) -> Result<String, DynamoDbError> {
    const EXP_BIAS: i64 = 200; // adjusted exponent in [-130, 125] -> biased [70, 325]
    const MANTISSA_WIDTH: usize = 38; // DynamoDB allows up to 38 significant digits

    let decimal = BigDecimal::from_str(repr).map_err(|_| invalid_number(repr))?;
    let (bigint, scale) = decimal.normalized().as_bigint_and_exponent();
    if bigint.sign() == Sign::NoSign {
        return Ok("5".to_owned()); // zero sorts between negatives and positives
    }

    let digits = bigint.magnitude().to_string(); // significant digits, no sign/leading/trailing zeros
    if digits.len() > MANTISSA_WIDTH {
        return Err(number_out_of_range(repr));
    }
    // Adjusted exponent: power of ten of the most-significant digit.
    let exponent = (digits.len() as i64 - 1) - scale;
    let biased = exponent + EXP_BIAS;
    if !(0..=999).contains(&biased) {
        return Err(number_out_of_range(repr));
    }
    let mantissa = format!("{digits:0<MANTISSA_WIDTH$}"); // right-pad with '0' to fixed width

    Ok(if bigint.sign() == Sign::Minus {
        let inverted_exponent = 999 - biased;
        let inverted_mantissa: String = mantissa
            .bytes()
            .map(|b| (b'9' + b'0' - b) as char)
            .collect();
        format!("1{inverted_exponent:03}{inverted_mantissa}")
    } else {
        format!("7{biased:03}{mantissa}")
    })
}

fn hex_lower(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(char::from_digit(u32::from(byte >> 4), 16).expect("nibble < 16"));
        out.push(char::from_digit(u32::from(byte & 0x0f), 16).expect("nibble < 16"));
    }
    out
}

fn invalid_number(repr: &str) -> DynamoDbError {
    DynamoDbError::ValidationException(format!(
        "The parameter cannot be converted to a numeric value: {repr}"
    ))
}

fn number_out_of_range(repr: &str) -> DynamoDbError {
    DynamoDbError::ValidationException(format!(
        "Number magnitude or precision is outside the supported range: {repr}"
    ))
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

    // -------- sortable key projection (DDB-DIV-002) --------

    /// The trust-critical invariant: sorting numbers by their sortable-key string
    /// (lexicographically) yields the exact same order as sorting them
    /// numerically. A failure here silently corrupts DynamoDB range queries.
    #[test]
    fn sortable_number_order_matches_numeric_order() {
        let mut samples: Vec<String> = vec![
            "-99999999999999999999999999999999999999".into(),
            "-1e20".into(),
            "-1000000".into(),
            "-1000".into(),
            "-100".into(),
            "-99".into(),
            "-10".into(),
            "-9.9".into(),
            "-1.23".into(),
            "-1.2".into(),
            "-1".into(),
            "-0.5".into(),
            "-0.05".into(),
            "-0.001".into(),
            "-1e-20".into(),
            "0".into(),
            "1e-20".into(),
            "0.001".into(),
            "0.05".into(),
            "0.5".into(),
            "1".into(),
            "1.2".into(),
            "1.23".into(),
            "9".into(),
            "9.9".into(),
            "10".into(),
            "99".into(),
            "100".into(),
            "1000".into(),
            "1000000".into(),
            "1e20".into(),
            "3.141592653589793238462643383279502884".into(),
            "99999999999999999999999999999999999999".into(),
        ];
        // Deterministic sweep over integers and a fractional family broadens coverage.
        for i in -120i64..=120 {
            samples.push(i.to_string());
            samples.push(format!("{i}.5"));
            samples.push(format!("0.{:03}", i.unsigned_abs() % 1000));
        }

        let mut pairs: Vec<(String, BigDecimal)> = samples
            .iter()
            .map(|s| {
                (
                    sortable_number(s).expect("encodes"),
                    BigDecimal::from_str(s).expect("parses"),
                )
            })
            .collect();
        pairs.sort_by(|a, b| a.0.cmp(&b.0)); // lexicographic by sortable key

        for window in pairs.windows(2) {
            assert!(
                window[0].1 <= window[1].1,
                "lexicographic key order must equal numeric order, but {} sorted before {}",
                window[0].1,
                window[1].1,
            );
        }
    }

    #[test]
    fn sortable_number_equal_for_numerically_equal_inputs() {
        // Trailing zeros, exponent form, and integer/decimal forms of the same
        // value must map to one key (equality/dedup correctness).
        for group in [
            vec!["1", "1.0", "1.00", "1e0", "10e-1"],
            vec!["0", "0.0", "0.000", "0e9"],
            vec!["-2.5", "-2.50", "-25e-1"],
            vec!["120", "1.2e2", "120.000"],
        ] {
            let keys: Vec<String> = group.iter().map(|s| sortable_number(s).unwrap()).collect();
            assert!(
                keys.windows(2).all(|w| w[0] == w[1]),
                "numerically-equal inputs {group:?} must share one sortable key, got {keys:?}"
            );
        }
    }

    #[test]
    fn sortable_string_is_raw_and_byte_wise() {
        assert_eq!(
            sortable_key(&AttributeValue::S("abc".into())).unwrap(),
            "abc"
        );
        let mut keys = ["banana", "apple", "cherry"]
            .map(|s| sortable_key(&AttributeValue::S(s.into())).unwrap());
        keys.sort();
        assert_eq!(keys, ["apple", "banana", "cherry"]);
    }

    #[test]
    fn sortable_binary_is_order_preserving_lowercase_hex() {
        assert_eq!(
            sortable_key(&AttributeValue::B(vec![0x00, 0x0a, 0xff])).unwrap(),
            "000aff"
        );
        // Byte-wise order is preserved by the hex projection.
        let mut keyed: Vec<(String, Vec<u8>)> = [
            vec![0xff],
            vec![0x00],
            vec![0x10],
            vec![0x0f],
            vec![0x00, 0x01],
        ]
        .into_iter()
        .map(|b| (sortable_key(&AttributeValue::B(b.clone())).unwrap(), b))
        .collect();
        keyed.sort_by(|a, b| a.0.cmp(&b.0));
        let ordered: Vec<Vec<u8>> = keyed.into_iter().map(|(_, b)| b).collect();
        assert_eq!(
            ordered,
            vec![
                vec![0x00],
                vec![0x00, 0x01],
                vec![0x0f],
                vec![0x10],
                vec![0xff]
            ]
        );
    }

    #[test]
    fn sortable_key_rejects_non_scalar() {
        assert!(matches!(
            sortable_key(&AttributeValue::Bool(true)),
            Err(DynamoDbError::ValidationException(_))
        ));
    }

    #[test]
    fn sortable_number_negatives_sort_before_zero_before_positives() {
        let neg = sortable_number("-1").unwrap();
        let zero = sortable_number("0").unwrap();
        let pos = sortable_number("1").unwrap();
        assert!(neg < zero && zero < pos, "neg={neg} zero={zero} pos={pos}");
    }
}
