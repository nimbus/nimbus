# DynamoDB Adapter — Divergences

Intentional, recorded differences between the Nimbus DynamoDB adapter and real
DynamoDB / the ExtendDB reference. Every entry has a rationale and a regression
test asserting the chosen behavior. The parity runner (D8) classifies any
unrecorded difference as `nimbus-divergence` and fails until it appears here.

Classifications: `nimbus-divergence` (Nimbus differs from both DynamoDB Local and
ExtendDB — must be justified here) and `accept-extenddb-divergence` (Nimbus
matches ExtendDB but not real DynamoDB).

## DDB-DIV-001 — Composite primary key size limit (`nimbus-divergence`)

**Real DynamoDB:** partition key ≤ 2,048 bytes + sort key ≤ 1,024 bytes
(≤ 3,072 raw bytes combined).

**Nimbus:** the composite key is encoded into a single `DocumentId`
(`<type><base64url(value)>` per segment, joined by `.`), and `DocumentId` is
capped at 1,500 bytes (`nimbus_core::validate_document_key`). base64url inflates
by ~33%, so the supported combined **raw** key is ~1,100 bytes. Keys whose
encoded form exceeds 1,500 bytes are rejected with `ValidationException`.

**Rationale:** raising the core `DocumentId` limit is a cross-cutting storage
change affecting every backend; the adapter accepts the tighter bound until a
real workload needs full-size DynamoDB keys. Most keys are far below this.

**Regression test:** `crates/nimbus-dynamodb/src/key.rs` →
`tests::rejects_oversize_key`.

**Status:** accepted (D0.3).

## DDB-DIV-002 — Sort-key ordering uses an order-preserving projection (planned)

Real DynamoDB orders sort keys by type (`N` numeric, `S` UTF-8 byte-wise, `B`
byte-wise). Nimbus's index/compare path runs numbers through `f64` and cannot
index binary, so the adapter projects each key/index attribute into an
order-preserving sortable string in `_pk`/`_sk` (and per-index `_gsi1_*` fields):
`S` → raw UTF-8, `N` → a full-precision lexicographically-sortable decimal
encoding, `B` → fixed-case hex. Range conditions evaluate that projection, not
the opaque `DocumentId`.

**Status:** projection lands in the D0.3 sortable-key follow-up; range execution
in D2.1. This entry will gain its regression test (type-correct ordering,
including >17-digit numeric ranges that `f64` would collapse) when the projection
lands.
