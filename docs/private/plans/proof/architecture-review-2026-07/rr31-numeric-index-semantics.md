# RR31 Numeric Index Semantics

Status: deferred input from IMV1. RR31 remains nonterminal.

IMV1 work commit: `0da288204`.

## Confirmed collision

`crates/nimbus-storage/src/index/encoding.rs` converts every JSON number to
`f64` before it builds an index key. Integer `1` and floating-point `1.0`
therefore produce the same bytes. Large integers can also lose precision at
this boundary.

IMV1 makes document index maintenance consume the adapter-lowered
`StoredValue` when one exists. It deliberately preserves the current projected
key bytes. Changing the numeric type bands here would silently select one
protocol's equality and ordering rules for every adapter.

## Adapter consequences to inventory in RR31

- Convex distinguishes integer and floating-point value families in its sort
  key.
- Firestore compares mixed numeric values by numeric value.
- MongoDB has its own mixed numeric comparison and BSON type-order rules.
- DynamoDB retains arbitrary-precision number strings and does not admit the
  same numeric domain as JSON `f64`.
- Nimbus-native queries currently reach the same universal index encoding as
  the compatibility adapters.

RR31 must inventory every creation and query path. It must define the
engine-owned policy carrier and add large-integer and mixed-number conformance
tests before it changes the key format. Request transport must not determine
the meaning of stored index bytes.
