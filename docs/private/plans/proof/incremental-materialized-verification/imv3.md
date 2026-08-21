# IMV3 Deterministic Root Proof

Date: 2026-08-21.

## Fail-before

The IMV2 benchmark contained a treap prototype. Storage had no versioned root
type, logical leaf key, update and delete contract, generated history, or
million-leaf production measurement. Verifier condition 8 failed before this
task.

## Contract

`nimbus-storage` now owns the process-local index. It derives each logical key
from a state-family tag and canonical identity bytes. Separate SHA-256 domains
cover keys, priorities, values, nodes, empty roots, and the root-format
version. Equal logical leaves therefore produce one root independent of batch
or mutation order.

`VerificationPosition` contains only the root-format version, applied
sequence, and root hash. Its fields are private. Construction rejects an
unsupported version. `MaterializedPosition` remains unchanged as the portable
artifact and recovery binding.

The index supports batch build, insert, update, delete, root read, maximum
depth, and resident-memory accounting. Deleted node slots enter a free list
and later inserts reuse them.

## Dependency screen

The screen used maintained upstream repositories on 2026-08-21:

| Candidate | Evidence | Decision |
|---|---|---|
| `merkle-search-tree` 0.8.0 | [Upstream](https://github.com/domodwyer/merkle-search-tree) is active and property-tested. Its default digest is a non-cryptographic, potentially non-portable 128-bit SipHash. Its public mutation API has upsert but no delete. | Reject. Replacing its digest does not supply the required delete contract or the measured compact treap layout. |
| `rs-merkle-tree` | [Upstream](https://github.com/alrevuelta/rs-merkle-tree) is active and MIT licensed. Its contract is fixed-depth and append-only; existing leaves cannot change. | Reject. Materialized state needs keyed updates and deletes. |
| `nomt` | [Upstream](https://github.com/thrumdev/nomt) is active and extensively tested. It is a persistent embedded key-value database with its own disk tree and one-session writer model. | Reject. IMV3 requires a disposable process-local index and must not add a seventh provider format. |

The screen found no crate that supplies SHA-256 state identity, keyed updates
and deletes, provider independence, disposable memory, and the 192-byte limit.
Nimbus therefore retains the concept-owned implementation.

## Acceptance evidence

The focused unit lane reports eight passed tests. It includes all four named
acceptance tests and a generated corpus with 16 histories and 500 mixed
operations per history. Each checkpoint compares the incremental root with a
full rebuild.

The dedicated million-leaf lane reports:

```text
verification_root leaves=1000000 max_depth=55 resident_bytes_per_leaf=145 budgeted_bytes_per_leaf=160
test result: ok. 1 passed; 0 failed
```

The node layout is 144 bytes. IMV2 assigns 16 conservative allocator bytes per
leaf, for 160 budgeted bytes and 32 bytes of headroom below the approved
192-byte limit. The measured arena allocation uses 145 resident bytes per live
leaf after fixed index overhead.

Commands:

```text
cargo test -p nimbus-storage materialized_verification -- --nocapture
cargo test -p nimbus-storage --test generated_history verification_root -- --ignored --nocapture
```
