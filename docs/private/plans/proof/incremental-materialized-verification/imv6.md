# IMV6 Provider, Fault, and Observability Proof

Date: 2026-08-21.

## Fail-before

Nimbus had no shared verification-root test across all six storage providers.
The fixed verifier could not prove provider parity, same-sequence state-tamper
detection, or bounded metrics.

The IMV5 failed-scrub rule also removed the expected witness. A first scrub
detected persistent provider tamper at an unchanged sequence. The next request
then treated the missing session as a cold start and could accept the tampered
state as its new baseline.

The session registry had a second bound defect. It inserted a new protected
entry when all 64 prior slots were active. No slot was eligible for eviction,
so the registry could exceed its count limit. An active result could also stay
in the registry when all other active entries prevented byte-budget eviction.

Verifier conditions 10, 12, and 14 failed before IMV6.

## Provider parity

One shared scenario appends and applies the pinned canonical record corpus. It
exports actual materialized state through each provider and builds the same
storage-owned verification root. The root includes its format version,
applied sequence, root hash, and leaf count.

redb, memory, and SQLite execute the scenario locally and match the memory
reference. PostgreSQL, MySQL, and libSQL compile the same scenario through
their real provider types. Their fixture variables are not present on this
host, so their local result is `UNVERIFIED`. The provider matrix reports that
state instead of treating an omitted body as green.

The matrix now contains 48 explicit cells: six providers by eight semantic
dimensions. Each provider publishes `verification_root_parity` in its semantic
profile. Hosted external-provider jobs remain the qualification gate for the
three remote rows.

## Fault and recovery behavior

A memory-provider test changes one materialized document without changing the
applied sequence or durable journal. The rebuilt root keeps the sequence and
changes the state hash. This proves that a full state scrub can see corruption
that journal-only verification cannot see.

The engine regression establishes a clean anchor before the same provider
tamper. Two forced full scrubs then fail at the same sequence. A failed scrub
retains a consistent prior witness when provider state disagrees with that
witness. If the prior witness is corrupt, Nimbus retains the clean rebuilt
witness. The retained session carries a marker that forces the next request
through another full scrub. A persistent mismatch cannot become a cold or warm
success.

A process-restart regression drops the engine and its process-local root. The
reopened engine runs a cold full scrub. Existing cases also cover a corrupt
node, stale anchor, retention gap, session eviction, and tenant replacement.

## Bounds and metrics

The registry now checks the 64-session limit before it inserts a new tenant.
It evicts an inactive least-recently-used entry when one exists. If every slot
is active, it returns `ResourceExhausted` to the optional verification request.
It does not affect tenant reads or writes.

After a check, the registry applies the 256 MiB aggregate byte limit. If the
registry cannot evict another entry, it removes the just-completed protected
entry. The report stays valid, but the next request runs a full scrub. Six session
tests cover count eviction, byte eviction, active-slot refusal, active-result
discard, expiry, and tenant incarnation.

The process metrics use 11 fixed fields. They report full and incremental
counts, separate duration totals by mode, current and peak resident bytes,
verified leaves, rebuilds, mismatches, current sessions, and evictions. They
contain no tenant, document, table, SQL, or state labels.

## Operator controls

The local administration route keeps the normal on-demand check:

```text
GET /debug/tenants/{tenant_id}/consistency
```

The `force_full=true` query runs a full provider-state scrub even when a warm
session exists. Its report names `operator_forced` as the escalation reason.

```text
GET /debug/tenants/{tenant_id}/consistency?force_full=true
```

The cache-clear operation removes only the disposable process-local session.
It is idempotent, returns `204 No Content`, and does not change tenant data.

```text
DELETE /debug/tenants/{tenant_id}/consistency
```

The next normal check runs a cold full scrub.

## Acceptance evidence

Work commit `3567ba656` owns the implementation and regression tests. Focused
Clippy passes for all storage, engine, server, testing, and system targets.

The storage materialized-verification lane reports 26 passed tests. The engine
consistency lane reports 8 passed tests. The session lane reports 6 passed
tests. The server route regression passes the normal, forced, clear, and
post-clear sequence.

The all-feature provider build runs six named parity tests. Three local tests
execute their provider bodies. The three remote tests report omitted fixture
execution and remain `UNVERIFIED`. The provider matrix passes two tests and
prints the same status for all remote cells. The required storage harness
passes its generated-history model test.

The full `make ci` gate passes on the final implementation. An earlier run
stopped in an unchanged runtime wall-clock test while the host was under load.
That exact ignored test passed three isolated reruns. The complete rerun passed
its 517-test runtime lane, 7,586-test non-runtime lane, JavaScript checks, and
proof helpers.

Both Nimbus pre-PR autoreviews report no accepted or actionable finding. The
first confirms the retained-witness rule, hard registry bounds, fixed metrics,
local administration boundary, and test-only provider tamper hook. The second
also confirms the overlap-safe invalidator invariant after the hosted fix.

The first hosted run qualified PostgreSQL and MySQL. The libSQL verification
root test also passed, but four PPSC recovery cases failed. The new invalidator
rejected an overlapping replica-cache replacement as a storage error after
the durable head advanced. Correction `0fe5db256` gives overlapping replacement
guards one reference-counted non-current epoch. Derived verification work can
no longer reject valid storage concurrency.

The correction passes the 26-test materialized-verification lane, all eight
engine consistency tests, the libSQL-feature test compile, and focused Clippy.
Its real-provider rerun remains the hosted merge gate. A local fixture attempt
was `UNVERIFIED`: Docker Desktop's credential helper blocked before it started
the pinned libSQL image.

The fixed verifier reports:

```text
PASS  10. verification root is provider independent
PASS  11. verification sessions reuse state and remain bounded
PASS  12. corruption and provider tamper fail closed
PASS  14. incremental verification metrics are bounded
Summary: 14 passed, 2 failed
```

Conditions 15 and 16 remain the planned IMV7 performance and documentation
work.

Commands:

```text
cargo test -p nimbus-storage materialized_verification -- --nocapture
cargo test -p nimbus-engine consistency_verification -- --nocapture
cargo test -p nimbus-engine verification_session -- --nocapture
cargo test -p nimbus-engine consistency_full_scrub_rejects_persistent_same_sequence_provider_tamper -- --nocapture
NIMBUS_DISABLE_IMPLICIT_EXTERNAL_PROVIDER_FIXTURES=1 cargo test -p nimbus-storage --features postgres,mysql,libsql materialized_verification_root_is_provider_independent -- --nocapture
NIMBUS_DISABLE_IMPLICIT_EXTERNAL_PROVIDER_FIXTURES=1 cargo test -p nimbus-storage --features postgres,mysql,libsql provider_contract_matrix -- --nocapture
make verify-harness SURFACE=storage
cargo clippy -p nimbus-storage -p nimbus-engine -p nimbus-server -p nimbus-testing -p nimbus-system --all-targets -- -D warnings
bash docs/private/plans/proof/incremental-materialized-verification/verify.sh
make ci
```
