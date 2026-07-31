//! DynamoDB item sizing and the 400 KiB per-item ceiling.
//!
//! Sizing is DynamoDB's own accounting, not the wire encoding's: an item's size
//! is "the sum of the lengths of its attribute names and values, plus any
//! applicable overhead". The JSON an item travels in is a different and larger
//! number (binary attributes are base64 on the wire) and is bounded separately
//! by the transport's request-body limit.
//!
//! Nimbus owns this calculation rather than deferring to
//! `extenddb_core::types::item_size_bytes`, which omits the per-element
//! overhead AWS charges for `List` and `Map` entries and so undercounts every
//! item holding a nested collection (FU14). `extenddb-core` is a pinned git
//! dependency; the rule is small and fully specified, so it lives here.

use extenddb_core::error::DynamoDbError;
use extenddb_core::types::{AttributeValue, Item};

/// DynamoDB's maximum size for a single stored item.
///
/// Binary units, per the service quotas: "All size measurements in DynamoDB use
/// binary-based units. DynamoDB denotes 1 KB = 1024 bytes" — so 400 KB is
/// 409,600 bytes, not 400,000.
pub const MAX_ITEM_SIZE_BYTES: usize = 400 * 1024;

/// AWS's `ValidationException` message when a written item exceeds the ceiling.
///
/// Returned by `PutItem` and `BatchWriteItem`, where the oversized item is the
/// request payload itself.
pub const ITEM_TOO_LARGE_MESSAGE: &str = "Item size has exceeded the maximum allowed size";

/// AWS's message when the item an update *produces* exceeds the ceiling.
///
/// `UpdateItem` returns it as a `ValidationException`; `TransactWriteItems`
/// returns it as the `Message` of a `ValidationError` cancellation reason,
/// where it is the only size-related message AWS documents.
pub const UPDATED_ITEM_TOO_LARGE_MESSAGE: &str =
    "Item size to update has exceeded the maximum allowed size";

/// The DynamoDB-accounted size of `item` in bytes.
///
/// Every attribute contributes its name's UTF-8 length plus
/// [`attribute_value_size`] of its value.
#[must_use]
pub fn item_size_bytes(item: &Item) -> usize {
    item.iter()
        .map(|(name, value)| name.len() + attribute_value_size(value))
        .sum()
}

/// The DynamoDB-accounted size of one `AttributeValue`, excluding its attribute
/// name.
///
/// The rules are AWS's, from "DynamoDB item sizes and formats":
///
/// - `S` — the number of UTF-8-encoded bytes.
/// - `N` — "1 byte per two significant digits" plus 1 byte, with leading and
///   trailing zeroes trimmed, plus 1 byte for a negative sign.
/// - `B` — the raw byte length. Binary travels base64-encoded but is sized raw.
/// - `BOOL`, `NULL` — 1 byte.
/// - `L`, `M` — "3 bytes of overhead, regardless of its contents", plus the
///   sizes of the nested elements (a map entry's key counts as a name), plus
///   "1 byte of overhead" for each element.
/// - `SS`, `NS`, `BS` — the sum of the element sizes and nothing more; a set
///   carries no overhead of its own and its elements carry no names.
#[must_use]
pub fn attribute_value_size(value: &AttributeValue) -> usize {
    match value {
        AttributeValue::S(string) => string.len(),
        AttributeValue::N(number) => number_size(number),
        AttributeValue::B(bytes) => bytes.len(),
        AttributeValue::Bool(_) | AttributeValue::Null => 1,
        AttributeValue::L(elements) => {
            COLLECTION_OVERHEAD_BYTES
                + elements.len()
                + elements.iter().map(attribute_value_size).sum::<usize>()
        }
        AttributeValue::M(entries) => {
            COLLECTION_OVERHEAD_BYTES
                + entries.len()
                + entries
                    .iter()
                    .map(|(name, nested)| name.len() + attribute_value_size(nested))
                    .sum::<usize>()
        }
        AttributeValue::SS(set) => set.iter().map(String::len).sum(),
        AttributeValue::NS(set) => set.iter().map(|number| number_size(number)).sum(),
        AttributeValue::BS(set) => set.iter().map(Vec::len).sum(),
    }
}

/// Whether `item` is over the 400 KiB ceiling.
///
/// The comparison is strictly greater-than: an item at exactly
/// [`MAX_ITEM_SIZE_BYTES`] is legal.
#[must_use]
pub fn exceeds_max_item_size(item: &Item) -> bool {
    item_size_bytes(item) > MAX_ITEM_SIZE_BYTES
}

/// Reject an item the caller is writing verbatim — `PutItem`'s and
/// `BatchWriteItem`'s payload item.
///
/// # Errors
/// `ValidationException` carrying [`ITEM_TOO_LARGE_MESSAGE`] if the item
/// exceeds [`MAX_ITEM_SIZE_BYTES`].
pub fn validate_item_size(item: &Item) -> Result<(), DynamoDbError> {
    if exceeds_max_item_size(item) {
        return Err(DynamoDbError::ValidationException(
            ITEM_TOO_LARGE_MESSAGE.to_owned(),
        ));
    }
    Ok(())
}

/// Reject the item an update *produces*.
///
/// The ceiling applies to the resulting item, not to the request: an
/// `UpdateItem` whose payload is a few bytes can still push a stored item past
/// 400 KiB, and AWS reports that with its own message.
///
/// # Errors
/// `ValidationException` carrying [`UPDATED_ITEM_TOO_LARGE_MESSAGE`] if the
/// resulting item exceeds [`MAX_ITEM_SIZE_BYTES`].
pub fn validate_updated_item_size(item: &Item) -> Result<(), DynamoDbError> {
    if exceeds_max_item_size(item) {
        return Err(DynamoDbError::ValidationException(
            UPDATED_ITEM_TOO_LARGE_MESSAGE.to_owned(),
        ));
    }
    Ok(())
}

/// Fixed overhead AWS charges for a `List` or `Map`, "regardless of its
/// contents".
const COLLECTION_OVERHEAD_BYTES: usize = 3;

/// How many `List` elements [`item_undersized_by_nested_elements`] carries, and
/// so how many bytes `extenddb_core`'s sizing loses on it.
#[cfg(test)]
pub(crate) const UNDERSIZED_ITEM_ELEMENTS: usize = 10;

/// FU14's subject: an item `extenddb_core`'s sizing puts at exactly
/// [`MAX_ITEM_SIZE_BYTES`] — and so accepted — while AWS's rules put it
/// [`UNDERSIZED_ITEM_ELEMENTS`] bytes over, one byte per `List` element.
///
/// Every write path's tests build the same item, so each of them is shown
/// rejecting the case the dependency's sizing lets through. `pk` carries `key`
/// so the item can be written to a table keyed on `pk`.
#[cfg(test)]
pub(crate) fn item_undersized_by_nested_elements(key: &str) -> Item {
    // Names "pk" (2) + "blob" (4) + "l" (1), the key's value, the list's fixed
    // 3-byte overhead, and one byte for each 1-byte BOOL element — everything
    // except the per-element charge the two sizings disagree about.
    let fixed = 2 + key.len() + 4 + 1 + COLLECTION_OVERHEAD_BYTES + UNDERSIZED_ITEM_ELEMENTS;
    Item::from([
        ("pk".to_owned(), AttributeValue::S(key.to_owned())),
        (
            "blob".to_owned(),
            AttributeValue::S("x".repeat(MAX_ITEM_SIZE_BYTES - fixed)),
        ),
        (
            "l".to_owned(),
            AttributeValue::L(vec![AttributeValue::Bool(true); UNDERSIZED_ITEM_ELEMENTS]),
        ),
    ])
}

/// The widest a DynamoDB number can be: 38 significant digits is 19 bytes, plus
/// the trailing byte and a sign.
const MAX_NUMBER_SIZE_BYTES: usize = 21;

/// Size a DynamoDB number: "(1 byte per two significant digits) + (1 byte)",
/// with leading and trailing zeroes trimmed, and one more byte for a sign.
///
/// Numbers are held as decimal strings, so 100 is 1E2 — one significant digit,
/// not three. Zero is a single byte.
fn number_size(number: &str) -> usize {
    let magnitude = number.trim_start_matches('-');
    if magnitude.chars().all(|c| c == '0' || c == '.') {
        return 1;
    }

    let significant = significant_digits(magnitude);
    let size = significant.div_ceil(2) + 1 + usize::from(number.starts_with('-'));
    size.min(MAX_NUMBER_SIZE_BYTES)
}

/// Count the significant digits of a non-zero, unsigned decimal string.
fn significant_digits(magnitude: &str) -> usize {
    let Some((integer, fraction)) = magnitude.split_once('.') else {
        return magnitude
            .trim_start_matches('0')
            .trim_end_matches('0')
            .len();
    };
    let integer = integer.trim_start_matches('0');
    if integer.is_empty() {
        // Below 1: leading zeroes of the fraction only set the exponent.
        return fraction.trim_start_matches('0').trim_end_matches('0').len();
    }
    // At or above 1: interior zeroes are significant, so the digits run from the
    // first integer digit through the last non-zero fraction digit.
    integer.len() + fraction.trim_end_matches('0').len()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{BTreeMap, BTreeSet};

    fn item(attributes: Vec<(&str, AttributeValue)>) -> Item {
        attributes
            .into_iter()
            .map(|(name, value)| (name.to_owned(), value))
            .collect()
    }

    fn map(entries: Vec<(&str, AttributeValue)>) -> AttributeValue {
        AttributeValue::M(
            entries
                .into_iter()
                .map(|(name, value)| (name.to_owned(), value))
                .collect::<BTreeMap<_, _>>(),
        )
    }

    fn string_set(members: &[&str]) -> AttributeValue {
        AttributeValue::SS(
            members
                .iter()
                .map(|s| (*s).to_owned())
                .collect::<BTreeSet<_>>(),
        )
    }

    #[test]
    fn attribute_value_size_follows_the_documented_rules() {
        // Each row is (value, expected size excluding the attribute name), with
        // the AWS rule it pins. Sizes are hand-derived from the rule text, not
        // from the implementation.
        let cases: Vec<(&str, AttributeValue, usize)> = vec![
            (
                "S is its UTF-8 byte length",
                AttributeValue::S("abcde".into()),
                5,
            ),
            (
                "S counts bytes, not chars",
                AttributeValue::S("é€".into()),
                5, // 2-byte é + 3-byte €
            ),
            ("BOOL is 1 byte", AttributeValue::Bool(true), 1),
            ("NULL is 1 byte", AttributeValue::Null, 1),
            (
                "B is its raw byte length",
                AttributeValue::B(vec![0, 1, 2, 3]),
                4,
            ),
            // Numbers: ceil(significant / 2) + 1, +1 when negative.
            ("N zero is 1 byte", AttributeValue::N("0".into()), 1),
            ("N one significant digit", AttributeValue::N("7".into()), 2),
            (
                "N trailing zeroes are not significant",
                AttributeValue::N("100".into()),
                2,
            ),
            (
                "N four significant digits",
                AttributeValue::N("1234".into()),
                3,
            ),
            (
                "N interior zeroes are significant",
                AttributeValue::N("1002".into()),
                3,
            ),
            (
                "N negative adds a byte",
                AttributeValue::N("-1234".into()),
                4,
            ),
            (
                "N leading fraction zeroes are exponent",
                AttributeValue::N("0.0025".into()),
                2,
            ),
            (
                "N is capped at 21 bytes",
                AttributeValue::N(format!("-1{}", "2".repeat(60))),
                21,
            ),
            // Sets carry no overhead of their own and their elements no names.
            (
                "SS is the sum of its members",
                string_set(&["ab", "cde"]),
                5,
            ),
            (
                "NS is the sum of its members",
                AttributeValue::NS(["7", "1234"].iter().map(|n| (*n).to_owned()).collect()),
                5, // 2 + 3
            ),
            (
                "BS is the sum of its members",
                AttributeValue::BS([vec![0_u8, 1], vec![2_u8]].into_iter().collect()),
                3,
            ),
            // Lists and maps: 3 bytes fixed, plus 1 byte per element.
            ("empty L is 3 bytes", AttributeValue::L(vec![]), 3),
            ("empty M is 3 bytes", map(vec![]), 3),
            (
                "L adds 1 byte per element",
                AttributeValue::L(vec![AttributeValue::Bool(true), AttributeValue::Null]),
                3 + 2 + 2, // overhead + per-element + two 1-byte values
            ),
            (
                "M adds 1 byte per entry and counts entry names",
                map(vec![
                    ("ab", AttributeValue::Bool(true)),
                    ("c", AttributeValue::Null),
                ]),
                3 + 2 + (2 + 1) + (1 + 1),
            ),
            (
                "nesting compounds both overheads",
                // { "in": [ true, { "k": "vv" } ] }
                map(vec![(
                    "in",
                    AttributeValue::L(vec![
                        AttributeValue::Bool(true),
                        map(vec![("k", AttributeValue::S("vv".into()))]),
                    ]),
                )]),
                // outer M: 3 + 1 entry + name "in" (2) + inner L
                // inner L: 3 + 2 elements + BOOL (1) + inner M
                // inner M: 3 + 1 entry + name "k" (1) + "vv" (2)
                3 + 1 + 2 + (3 + 2 + 1 + (3 + 1 + 1 + 2)),
            ),
        ];

        for (rule, value, expected) in cases {
            assert_eq!(
                attribute_value_size(&value),
                expected,
                "{rule}: sizing {value:?}"
            );
        }
    }

    #[test]
    fn deeply_nested_lists_charge_overhead_at_every_level() {
        // A chain of five singleton lists wrapping one BOOL. Each level costs
        // 3 bytes of collection overhead plus 1 byte for its single element,
        // so the whole chain is 5 * 4 + 1.
        let mut value = AttributeValue::Bool(true);
        for _ in 0..5 {
            value = AttributeValue::L(vec![value]);
        }
        assert_eq!(attribute_value_size(&value), 5 * (3 + 1) + 1);
    }

    #[test]
    fn item_size_counts_attribute_names_and_values() {
        let subject = item(vec![
            ("pk", AttributeValue::S("id-1".into())), // 2 + 4
            ("n", AttributeValue::N("42".into())),    // 1 + 2
            ("flag", AttributeValue::Bool(false)),    // 4 + 1
        ]);
        assert_eq!(item_size_bytes(&subject), (2 + 4) + (1 + 2) + (4 + 1));
    }

    #[test]
    fn the_ceiling_is_inclusive() {
        // An item at exactly 409,600 bytes is legal; one byte more is not.
        let at_limit = item(vec![(
            "b",
            AttributeValue::S("x".repeat(MAX_ITEM_SIZE_BYTES - 1)),
        )]);
        assert_eq!(item_size_bytes(&at_limit), MAX_ITEM_SIZE_BYTES);
        assert!(!exceeds_max_item_size(&at_limit));
        assert!(validate_item_size(&at_limit).is_ok());
        assert!(validate_updated_item_size(&at_limit).is_ok());

        let over_limit = item(vec![(
            "b",
            AttributeValue::S("x".repeat(MAX_ITEM_SIZE_BYTES)),
        )]);
        assert!(exceeds_max_item_size(&over_limit));
    }

    #[test]
    fn the_two_rejection_messages_are_the_aws_ones() {
        let over_limit = item(vec![(
            "b",
            AttributeValue::S("x".repeat(MAX_ITEM_SIZE_BYTES)),
        )]);

        match validate_item_size(&over_limit) {
            Err(DynamoDbError::ValidationException(message)) => {
                assert_eq!(message, "Item size has exceeded the maximum allowed size");
            }
            other => panic!("expected ValidationException, got {other:?}"),
        }

        match validate_updated_item_size(&over_limit) {
            Err(DynamoDbError::ValidationException(message)) => {
                assert_eq!(
                    message,
                    "Item size to update has exceeded the maximum allowed size"
                );
            }
            other => panic!("expected ValidationException, got {other:?}"),
        }
    }

    #[test]
    fn nested_collections_are_undercounted_by_extenddb_and_rejected_here() {
        // FU14's subject, with the dependency as the witness: the item lands at
        // exactly the ceiling under `extenddb_core`'s sizing — which omits
        // AWS's 1-byte-per-element charge — and ten bytes over it under this
        // crate's, one byte for each of the list's ten elements.
        const ELEMENTS: usize = UNDERSIZED_ITEM_ELEMENTS;
        let subject = item_undersized_by_nested_elements("k");

        assert_eq!(
            extenddb_core::types::item_size_bytes(&subject),
            MAX_ITEM_SIZE_BYTES,
            "the dependency must size this item at exactly the ceiling, and so accept it"
        );
        assert_eq!(
            item_size_bytes(&subject),
            MAX_ITEM_SIZE_BYTES + ELEMENTS,
            "AWS charges 1 byte per List element, putting this item over the ceiling"
        );
        assert!(exceeds_max_item_size(&subject));
    }

    #[test]
    fn a_map_of_many_entries_is_undercounted_by_exactly_its_entry_count() {
        // The same gap on the Map side, stated as a difference so it holds
        // whatever the surrounding sizes are.
        let entries: Vec<(&str, AttributeValue)> = vec![
            ("a", AttributeValue::S("one".into())),
            ("b", AttributeValue::N("2".into())),
            ("c", AttributeValue::Null),
        ];
        let entry_count = entries.len();
        let subject = item(vec![("m", map(entries))]);

        assert_eq!(
            item_size_bytes(&subject) - extenddb_core::types::item_size_bytes(&subject),
            entry_count
        );
    }
}
