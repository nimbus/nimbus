# DynamoDB Adapter — Ground-Truth Corpus (H7 / F14)

## What this proves

The original parity runner exercised Nimbus through the official AWS SDK but had
**no executable ground truth** for the response *contract* itself — and the
divergences doc claimed the runner "classifies any unrecorded difference," which
the code did not back. H7 closes that gap with a committed **golden corpus** of
canonical DynamoDB operations whose expected responses are the values DynamoDB
Local / the AWS API reference return, replayed against the adapter and diffed
field-by-field on every `cargo test` run.

- **Corpus + replay:** `crates/nimbus-dynamodb/tests/ground_truth.rs`
- **Companion (official SDK):** `crates/nimbus-server/tests/dynamodb_spec` (27
  scenarios through `aws-sdk-dynamodb` + `aws-sdk-dynamodbstreams`)

## The corpus

`ground_truth.rs` pins the response contract for the core surfaces, each entry a
`(operation, request, status, [(json-pointer, expected-value)])` golden:

| Operation | Pinned contract |
| --- | --- |
| `CreateTable` | `TableStatus = ACTIVE`, `ItemCount = 0`, echoed `KeySchema` |
| `DescribeLimits` | account 80 000 / table 40 000 R+W capacity units |
| `PutItem` | empty body (no `Attributes`) without `ReturnValues` |
| `GetItem` | exact `Item` attribute-value wire JSON round-trips |
| `UpdateItem` | `ReturnValues=UPDATED_NEW` returns only the changed attribute |
| `Query` | `Count` / `ScannedCount` / `Items[*]` shape |
| `DeleteItem` | empty body without `ReturnValues` |
| `GetItem` (missing table) | `__type` = `…#ResourceNotFoundException`, HTTP 400 |

A `Value::Null` expectation asserts the field is **absent** (DynamoDB omits it),
so the corpus catches both wrong values and spurious fields.

## How it runs

The replay test drives each golden through the adapter's `dispatch` entrypoint
(no network, no Docker) and asserts an exact match. It is part of the normal
`cargo test -p nimbus-dynamodb` run, so a regression that drifts from the
documented contract — a changed status, a renamed field, a dropped attribute —
fails CI immediately.

```
cargo test -p nimbus-dynamodb --test ground_truth
# test ground_truth_corpus_matches_the_dynamodb_contract ... ok
```

## Refreshing / extending the corpus (optional Docker lane)

The corpus is distilled from DynamoDB Local's observed responses and the AWS API
reference. To regenerate or expand it against a live DynamoDB Local:

1. `docker run -p 8000:8000 amazon/dynamodb-local`
2. Issue the same requests with `aws dynamodb … --endpoint-url http://localhost:8000`.
3. Capture the JSON responses and add/adjust the corresponding `Golden` entries
   (request + expected pointers) in `ground_truth.rs`.

This Docker capture is the **optional refresh path** — the committed corpus is
the executable ground truth that gates CI without requiring Docker.
