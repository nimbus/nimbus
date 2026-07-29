# SUC2.2 — Object-Metadata Writes On The Fenced Commit Path

## What Changed

`TenantObjectMeta`'s four write operations (`put_manifest`, `delete_manifest`,
`put_multipart_upload`, `delete_multipart_upload`) previously ran
get-then-insert/update on the tenant **read executor** via the storage-level
`ObjectMetaStore` write methods: sequence assignment happened outside the
committer, the engine's durable/applied watermarks never advanced, no fence
applied on provider-backed tenants, and two writers on the same key could
interleave (audit F3).

Each object write is now one committer-sequenced journal commit:

1. The write is submitted to the tenant committer actor
   (`submit_internal_committer_async`), which excludes every other sequence
   assigner — journal batches, trigger cursor advances, schema commits.
2. Inside the actor: committer lease check, previous-image read (race-free
   read-modify-write), `WriteOp` + `TenantEventRecord` built at
   `durable_head + 1`, staged in the write log, then persisted and applied
   through the SUC2.1 shared durable-batch core — the same fenced provider
   persist, fault windows, apply/recover, publish-frontier, and watermark
   sequence every journal batch uses.
3. On success the commit fans out to subscriptions and committed-mutation
   observers exactly like a journal batch. A persistence failure discards the
   staged write-log suffix; an ambiguous outcome begins crash-recovery
   eviction (scheduler-write pattern) and the caller awaits the eviction.

The engine-side unfenced write dispatch (`TenantPersistence::put_object_manifest`
etc.) is deleted; only read/list dispatch remains, with a comment forbidding
engine use of the storage-level write methods. Deleting an absent target
consumes no sequence and commits nothing.

Storage exposes the document mapping the engine needs
(`ObjectMultipartUpload::{to_document, from_document, document_id}` made pub;
`object_manifest_document_id` / `multipart_upload_document_id` helpers added).

## Fail-Before Evidence

The four new characterization tests
(`crates/nimbus-engine/src/tests/objects.rs`) were run against the old write
path (implementation stashed, tests kept): **3 of 4 RED** —

- `object_meta_writes_are_sequenced_journal_commits`: RED
  (`engine durable watermark must advance with the object commit:
  left: SequenceNumber(0), right: SequenceNumber(1)`)
- `manifest_replace_preserves_creation_identity_and_delete_returns_previous`: RED
- `concurrent_object_and_document_writers_serialize_without_conflict`: RED
- `multipart_upload_writes_commit_and_roundtrip`: green by accident on a fresh
  tenant (storage-assigned sequence 1 coincides with `baseline + 1`)

All 4 GREEN after. Raw output: session scratchpad `fail-before-objects.txt`,
reproduced here:

```
Summary [ 0.311s] 4 tests run: 1 passed, 3 failed, 660 skipped
  FAIL manifest_replace_preserves_creation_identity_and_delete_returns_previous
  FAIL concurrent_object_and_document_writers_serialize_without_conflict
  FAIL object_meta_writes_are_sequenced_journal_commits
```

## Publication/Subscription Classification (per event kind)

| Surface | Sequence consumption | Fencing | Publication |
| --- | --- | --- | --- |
| Object manifests / multipart (this change) | one journal sequence per write, assigned in the committer actor | committer lease + fenced provider batch via the durable-batch core | staged + published through the write log; document-write record on `_nimbus_objects` / `_nimbus_object_uploads`; subscription fan-out + committed-mutation observers on success |
| KV (`tenant_kv_*`) | none — redb-only keyspace, no `CommitEntry`, no journal record | redb write transaction locking; provider tenants reject KV | not published; invisible to subscriptions by design |
| Scheduler state | none — non-journal rows | fenced scheduler write under the committer lease (`persist_scheduler_write`) | not published (no document effects) |
| Trigger delivery cursor | one journal sequence (zero-write record) | committer actor + fenced trigger transition | zero-write record; provably-inert gap handling in subscriptions |

Remaining seam (not a production consumer today): `nimbus-fs`'s `ObjectFs`
takes `Arc<dyn ObjectMetaStore>` — the sync storage trait whose write half
bypasses the committer. Nothing in production constructs it (sandbox volume
`ObjectFs` backing is a planning enum); its only writers are nimbus-fs's own
tests against raw stores. When ObjectFs gains production wiring it must be
handed an engine-backed implementation; flagged for the SUC3 facade to gate
or delete the unfenced write half of the storage trait.

## Verification

- New tests: 4/4 GREEN (`tests::objects`), fail-before 3/4 RED as above.
- Engine suite: 3 consecutive full runs, 659 passed / 5 skipped each
  (fixtures opt-out). Storage 435 passed; nimbus-fs + nimbus-s3 suites green;
  `cargo clippy -p nimbus-engine -p nimbus-storage -p nimbus-fs -p nimbus-s3
  --all-targets -- -D warnings` clean; `cargo fmt --all --check` clean;
  `cargo check -p nimbus-server` clean (0 errors) against the changed API.
- Flake note: `arm_selection::opaque_internal_job_cannot_overtake_ordered_publisher`
  failed once during a battery that ran concurrently with a wedged
  `vite build` and a foreign session's `cargo test`. The test predates this
  branch (#226/#229), uses a timing-sensitive not-finished assertion, and
  passed 30/30 isolated repeats plus 3/3 full-suite runs on this branch.
  Attributed to load sensitivity, not this change.

## Paired A/B (campaign protocol, N=256 CRUD, sqlite)

Same session, balanced A/B/B/A order, 2 warmup + 5 measured rounds per block,
whole-block CV ≤ 10% admissibility. Baseline = branch point a082b9776
(worktree build); treatment = this branch (SUC2.1 + SUC2.2).

| Block | Arm | Mean mut/s | 95% CI | CV | Verdict |
| --- | --- | ---: | --- | ---: | --- |
| a1 | baseline | 47,078 | [43,100, 51,056] | 6.8% | admissible |
| b1 | branch | 46,229 | [42,067, 50,391] | 7.3% | admissible |
| b2 | branch | 47,254 | [37,868, 56,639] | 16.0% | **rejected** (one round collapsed to 33,890 — transient interference; retained per protocol) |
| a2 | baseline | 47,694 | [46,607, 48,782] | 1.8% | admissible |
| b3 | branch (rerun) | 48,369 | [44,706, 52,031] | 6.1% | admissible |

Baseline mean (a1, a2) = **47,386**; branch mean (b1, b3) = **47,299**;
ratio **0.9982** ≥ 0.98 — **PASS** (−0.18%, well inside noise). Raw block
reports: session scratchpad `ab-{a1,b1,b2,a2,b3}.md`.

## Structured Review (gpt-5.6-sol, high)

First pass returned three findings; each verified against the real code:

1. **Accepted (P1) — persistence-failure ambiguity.** The first cut discarded
   the staged suffix and returned the raw error on any persistence failure,
   treating it as definitively-not-committed. Fixed: failures now classify via
   `classify_durable_write_error` with a new `DurableWriteRoute::ObjectMetadata`
   (definitive only when the authoritative durable head equals the pre-write
   head); ambiguous outcomes begin crash-recovery eviction — the direct
   route's exact shape.
2. **Accepted (P2) — fan-out ordering.** Fan-out originally ran in the caller
   task after the committer returned, so concurrent object commits could
   notify out of order. Fixed: `process_commit_fanout` (the committer's
   documented ordered publication boundary) plus observer enqueue now run
   inside the committer task, so the actor cannot start the next commit until
   fan-out for this one is accepted.
3. **Rejected — "route through the client mutation path".** The three
   engine-owned client mutation routes are unchanged; this is an internal
   committer route in the same family as scheduler-state and trigger-cursor
   writes (the plan's own acceptance wording: "fenced through the committer or
   an explicitly serialized internal path"). After fixes 1–2 it shares the
   direct route's classification and fan-out seams; expressing
   upsert-with-previous-image as a client `Mutation` would expand the client
   mutation and authorization surface without additional safety. Rationale
   recorded as a code comment on `commit_object_meta_write_in_actor`.

Second review pass result recorded below.
