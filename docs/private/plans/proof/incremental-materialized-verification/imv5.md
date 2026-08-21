# IMV5 Bounded Verification Session Proof

Date: 2026-08-21.

## Fail-before

Each call to `verify_consistency_async` exported actual provider state. It then
rebuilt an authoritative view, a shadow materializer, and an embedded replica.
It sorted and hashed each complete logical state. A second unchanged check did
the same full work and could not name whether its evidence was a new scrub or a
reused anchor.

Verifier conditions 11 and 13 failed before IMV5.

## Contract

The engine now owns one process-local registry of per-tenant verification
sessions. A short registry lock protects cache metadata. An asynchronous lock
serializes checks for one tenant. Checks for different tenants can proceed
independently.

A cold session starts with one provider bootstrap export. This export is the
full scrub: it reads actual materialized state at the provider's applied head.
That snapshot is the only comparison and alignment cut. A concurrent write
cannot make a second metadata read move the expected root past this cut and
skip the comparison. Nimbus builds the authoritative, shadow, and
embedded-replica full-scrub evidence from that cut. It also builds three
verification indexes.

The session retains the indexes and small fingerprint evidence. It does not
retain three document snapshots.

The full scrub also replays the durable tail through three implementations.
It compares an authoritative store rebuild with `ShadowMaterializer` and
`EmbeddedReplica`. This replay check remains independent of the three retained
indexes, which share the storage-owned applied-record transition contract.

A warm check first reads only the provider's applied-sequence metadata. It
streams the exact contiguous journal suffix from the session sequence through
that applied head. It never advances a root from the durable head alone. Each
index applies the same canonical, storage-owned applied-record contract. The
check compares all roots only after they reach one exact sequence.

The warm result is an incremental witness. It is not a new provider-state
scrub. Its report therefore names `incremental`, the retained full-scrub
anchor, the anchor age, the applied event count, and no escalation reason. The
retained full-scrub fingerprints remain anchor evidence and do not claim a new
scan.

## Escalation and failure behavior

A cold start, idle deadline, anchor deadline, applied-sequence rewind,
retention gap, invalid index, or root mismatch runs a full scrub. The scrub
captures one bootstrap snapshot and advances the expected indexes through
that snapshot's applied head. It can therefore detect provider drift that
occurred after the old anchor. Capturing time after the per-tenant lock
prevents an older waiter from moving the session clock backward.

A root mismatch cannot become a successful full-scrub report. Nimbus compares
the prior roots with each other and compares each same-sequence prior root with
the rebuilt root. Any failed scrub discards its session. The next request must
scrub again, so a persistent provider or replay mismatch cannot become a warm
success. A session-only index fault can recover through a clean full rebuild.
A retention gap also discards the old assurance and starts a new full anchor
from actual state.

An unrelated provider error remains an error. It is not relabeled as a
retention rebuild. A failed operation can discard its disposable session, but
the registry records the remaining zero-byte state before returning the error.
Tenant deletion invalidates the registry entry. A replacement tenant with the
same ID cannot inherit a session from an older incarnation.

## Bounds

The retained cache admits at most 64 tenant entries and 256 MiB of combined
verification-index estimates. Admission and completion update one LRU order.
If the cache exceeds either bound, the registry evicts the inactive entry with
the oldest LRU order. The registry does not retain a session that exceeds the
byte budget by itself.

An idle session must scrub after five minutes. Every anchor must scrub after
15 minutes. These deadlines bound how long a fast result can reuse prior
provider evidence. Deadline evaluation is lazy at the next check. Count and
byte eviction bound retained cache memory even when a tenant is never checked
again.

Full-scrub scratch memory remains under the IMV2 measured peak-RSS gate.
The retained-session estimate does not include that scratch memory.

The registry does not use tenant IDs or state values as metric labels. IMV6
owns the bounded metrics and operator controls.

## Acceptance evidence

The consistency-verification lane reports 7 passed tests. It proves the cold
full mode, warm incremental mode, stable anchor identity, and an unchanged
zero-event recheck. It also proves mismatch failure, retention-gap rebuild,
expired-anchor advance before a scrub, and single-cut alignment during a
concurrent write. A lifecycle regression proves that a replacement tenant
starts with a cold scrub.

The session lane reports 3 passed tests. It proves count-based LRU eviction,
byte-based LRU eviction, and the exact idle and anchor deadlines. The storage
materialized-verification lane reports 22 passed tests and preserves batch
versus incremental root equivalence and fail-closed index invalidation.

Focused Clippy passes for all `nimbus-engine` and `nimbus-storage` targets. The
fixed verifier reports:

```text
PASS  11. verification sessions reuse state and remain bounded
PASS  13. every fast result names an anchor and unsafe states scrub or rebuild
Summary: 11 passed, 5 failed
```

The five remaining failures are the planned provider, fault, metrics, closeout,
and documentation conditions in IMV6 and IMV7.

The required local CI gate passed format, workspace Clippy, dependency,
runtime, and IMV-focused checks. Its non-runtime lane ran 7,572 tests. It passed
7,571 and skipped 110. The unchanged MongoDB CRUD report test failed once
during listener startup. It is outside the IMV5 paths and passed three isolated
reruns. Hosted CI remains the merge source of truth.

Commands:

```text
cargo test -p nimbus-engine consistency_verification -- --nocapture
cargo test -p nimbus-engine verification_session -- --nocapture
cargo test -p nimbus-storage materialized_verification -- --nocapture
cargo clippy -p nimbus-engine -p nimbus-storage --all-targets -- -D warnings
cargo fmt --all --check
bash docs/private/plans/proof/incremental-materialized-verification/verify.sh
make ci
cargo test -p nimbus-server --test mongodb_spec spec_executor_crud_execution_report -- --exact --nocapture
```
