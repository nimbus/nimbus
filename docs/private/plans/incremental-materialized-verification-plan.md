# Incremental Materialized Verification Plan

Status: `active` | Owner: this plan | Created: 2026-08-19.
Baseline: main @ 137cc632a1c8585545d200ea49f44bd236478175.
Proof root: `docs/private/plans/proof/incremental-materialized-verification/`.
Next action: run IMV1 from the retained IMV0 fail-before proof. Define the
normalized logical value tree and canonical leaf codec. Repair float encoding,
PITR preflight, opaque positions, and both Cargo-graph golden tests without
changing protocol numeric ordering.

## Outcome

> Nimbus can verify materialized state repeatedly at bounded cost while
> `MaterializedPosition` remains the canonical provider-neutral content
> binding. Each incremental result names its full anchor and exact applied
> sequence. Missing, stale, corrupt, or divergent incremental state causes a
> full state-derived verification instead of a false pass. The portable digest
> uses one explicit logical codec that is independent of Cargo features,
> provider layout, collection order, and stored-value spelling.

## Architecture

Before:

```text
ordered journal apply
        |
        v
[authoritative state]  [shadow state]  [replica state]
        |                    |                |
        +--------- full snapshot export -----+
                             |
                             v
               [sort + clone + JSON + SHA-256]
                             |
                             v
             [MaterializedPosition comparison]

Each repeated check rebuilds and hashes whole logical states.
```

After:

```text
ordered journal apply
        |
        +----> [authoritative state] ----> [MaterializedStateDelta]
        +----> [shadow state] ----------> [MaterializedStateDelta]
        +----> [replica state] ---------> [MaterializedStateDelta]
                                              |
                                              v
                                [MaterializedVerificationIndex]
                                  sequence + Merkle root
                                              |
                         +--------------------+-------------------+
                         |                                        |
                         v                                        v
              [incremental comparison]                  [full scrub]
              roots at one sequence             actual state -> canonical
                         |                       digest + rebuilt root
                         +--------------------+-------------------+
                                              |
                                              v
                           [mode, anchor, result, and metrics]

MaterializedPosition stays the artifact and recovery contract.
The Merkle root accelerates one bounded verification session.
```

## Scope

- Owns: the canonical logical leaf codec and a deterministic incremental
  Merkle root.
- Owns: `MaterializedPosition` correctness, PITR target preflight, and opaque
  validated construction for canonical materialized state.
- Owns: bounded sessions, full-scrub anchors, metrics, faults, benchmarks, and
  the repeated verification API.
- Owns: integration with authoritative journal replay, `ShadowMaterializer`,
  `EmbeddedReplica`, and the engine consistency verifier.
- Does not own: signed or authenticated artifacts. The horizontal scaling plan
  owns external epoch lineages and distributed snapshot transfer.
- Does not own: physical page checksums, WAL integrity, MVCC history proofs, or
  provider repair. Existing storage and recovery contracts own those items.
- Does not own: replacing `MaterializedPosition.state_digest` with a Merkle
  root.
- Does not own: object blob reclamation or S3 cleanup. The proposed
  `blob-lifecycle-integrity-plan.md` owns that work.
- Does not own: making every durable outcome and provider-capability set in
  the repository opaque. The routed residual below assigns that broader seam
  hardening to the active architecture-review control plane.
- Does not own: cross-adapter numeric query and index ordering. Successor row
  `RR31` owns that compatibility seam after IMV1 supplies the normalized value
  tree.
- Non-goals: a public inclusion-proof API, a general authenticated database, a
  consensus root, cross-tenant aggregation, or a new client mutation path.

## Promotion gate

Promote this plan to `active` only when every item holds:

1. The owner approves mandatory IMV1 correctness work even when IMV2 rejects
   the Merkle path.
2. The owner approves the IMV2 thresholds and the one-minute candidate
   verification interval.
3. Every task has one pull request boundary and a named reviewer gate.
4. The dedicated worktree is clean except for this plan, its index edit, and
   the RR30 and RR31 successor-ledger edits.

IMV0 runs first after promotion and creates the proof root and the
16-condition red verifier.

## Status ledger

| ID | Task | Status | Evidence |
|---|---|---|---|
| IMV0 | Pin the execution baseline, create the proof root, author the 16-condition verifier red, and capture full-verification fail-before cost. No production behavior changes. | `done` | Work `58e46b675`, fix `14227dc59`; proof `97949afa2`, refresh `6e643ada8`. Verifier: `Summary: 3 passed, 13 failed`. |
| IMV1 | Repair `MaterializedPosition` as mandatory correctness work: lower adapter values once into one normalized logical tree shared by persistence, equality, indexing, and hashing; define one canonical codec; make float encoding total; validate PITR before writes; and prove one golden digest in storage-only and shipped Cargo graphs. | `in_progress` | Started after IMV0 closed on 2026-08-20. |
| IMV2 | Run the benchmark matrix and decide `STREAMING_ACCEPTED`, `MERKLE_REQUIRED`, or `NO_ACCEPTABLE_DESIGN` from the ratified gate. | `todo` | |
| IMV3 | If required, implement the storage-owned deterministic Merkle treap and prove batch versus incremental equivalence. | `todo` | |
| IMV4 | If required, account for every materialized-state writer, repair its repository instruction, and publish exact deltas or invalidate the index. | `todo` | |
| IMV5 | If required, add bounded verification sessions, incremental reports, full-scrub anchors, and automatic escalation. | `todo` | |
| IMV6 | If required, complete six-provider parity, recovery faults, memory bounds, metrics, and operator controls. | `todo` | |
| IMV7 | Run closeout measurements and required gates, update governing architecture and operating docs, and publish the final verdict. | `todo` | |
| IMV9 | After the final pull request merges, archive this plan, retain its proof root, and remove its active index entry. | `todo` | |

## Dependencies and coordination

- The archived storage integrity plan owns the shipped
  `MaterializedPosition` contract. This plan can version that contract, but it
  cannot weaken it.
- IMV1 owns the SIC4 digest errata. The blob lifecycle plan independently owns
  the SIC1 retained-blob errata. Neither plan waits for the other to fix its P1
  defect.
- `docs/private/architecture/time-and-ordering.md` governs the applied-sequence
  meaning.
- `docs/private/operating/verification.md` governs verification commands,
  bounded waits, and provider evidence.
- The horizontal scaling plan wins for cluster epochs, signed roots, and
  mixed-version fleet rollout.
- `RR30` in the active `architecture-review-2026-07-plan.md` owns routed
  residual `IMVR1`. RR30 inventories repository-wide
  storage durable outcomes and provider-capability types after IMV1 and BLI3
  merge. It preserves closed enums that cannot represent invalid state and
  creates repair rows for unsafe construction paths. IMV and BLI cannot mark
  RR30 complete.
- `RR31` in that plan owns routed residual `IMVR2`. It defines and verifies
  numeric equality and ordering across Nimbus, Convex, Firestore, MongoDB, and
  DynamoDB query and index paths. IMV1 records the current collision but does
  not invent a transport-derived index policy.
- If this plan and an active storage plan edit one apply seam, the active plan
  lands first. Rebase this plan and repeat IMV0 before implementation.
- Plan and index edits land directly on main. Land this plan before the blob
  lifecycle plan, then rebase that index edit. Code tasks keep their dedicated
  worktree and pull request boundaries.

## Invariants

1. `MaterializedPosition` remains the portable snapshot, bootstrap, PITR, and
   recovery comparison contract.
2. A `VerificationPosition` contains a format version, an applied sequence,
   and a Merkle root. It is a separate type.
3. A root never claims sequence `A` until all logical deltas through `A` are
   visible in that index.
   A durable journal head alone cannot advance the root.
4. A sequence gap, version mismatch, cache error, or retention gap invalidates
   the index. The verifier then runs a full scrub.
5. A full scrub reads actual materialized state. Journal replay alone cannot
   certify provider state.
6. An incremental result names its full-scrub anchor and reports its mode. It
   cannot report the assurance level of a full scrub.
7. The ordered journal apply seam produces index deltas. Nimbus does not add a
   fourth client mutation path.
8. Equal logical state produces equal leaves and roots on redb, SQLite,
   memory, PostgreSQL, MySQL, and libSQL.
9. Nimbus treats a root cache as derived and disposable. Its loss cannot block
   normal reads, writes, bootstrap, PITR, or recovery.
10. The first version does not persist Merkle nodes in provider schemas.
11. Metrics use bounded labels. They do not contain tenant IDs, document IDs,
    table names, SQL text, or state bytes.
12. The full verifier stays available after incremental verification ships.
13. The portable digest is identical in the storage-only and shipped binary
    Cargo feature graphs.
14. Every admitted logical state has one digest input. Distinct non-finite
    float classes never collapse to JSON `null`.
15. PITR rejects an invalid target binding before it writes destination state.
16. Every production materialized-state writer updates or invalidates the
    verification index before another fast result can pass.
17. Every adapter lowers a protocol value once into the normalized Nimbus
    logical value tree. Persistence, semantic equality, index-key derivation,
    and hashing consume that tree rather than normalizing parallel spellings.
18. IMV1 does not silently change protocol numeric ordering. RR31 owns the
    engine-level equality and ordering policy for integer and float values.
19. The materialized serving surface remains a derived read cache. It cannot
    certify provider state or advance a verification position.

## Decisions

### IMV-D1 Preserve two content bindings

- Keep the canonical SHA-256 digest and add `VerificationPosition`.
- Artifact restore needs a complete state digest. Repeated checks need an
  updateable witness.
- Re-open only if one versioned format can serve every persisted consumer.

### IMV-D2 Use one canonical leaf codec

- Each adapter lowers protocol values once into one normalized Nimbus logical
  value tree before persistence.
- Persistence, semantic equality, index-key derivation, the full digest, and
  the incremental root consume that same tree. A query policy can select
  protocol semantics over the tree. It cannot normalize a second value model.
- Storage owns one deterministic byte codec for every covered state family.
  The codec only serializes the normalized tree. It is not a second
  normalization system.
- Provider layout, collection order, adapter spelling, and stored-value
  spelling cannot change a leaf.

### IMV-D3 Use a deterministic Merkle treap

- Index each leaf by a domain-separated SHA-256 key.
- Derive treap priority from another domain. Hash both child roots, the key,
  and the value hash.
- Re-open if IMV3 finds excess depth or memory, or a smaller mature dependency.

### IMV-D4 Keep the first index process-local

- Build the index from a full snapshot. Retain it only in a bounded session.
- Persisted nodes would add six provider formats before measurement shows a
  cold-start need.
- Re-open if measured cold rebuild cost violates the ratified budget.

### IMV-D5 Keep two verification modes

- Expose `Full` and `Incremental` modes in reports and diagnostics.
- A root checks independent apply results. A state scan checks provider
  contents.
- Policy can schedule both modes without confusing their assurance.

### IMV-D6 Make canonical correctness mandatory

- IMV1 runs before the IMV2 continuation verdict.
- IMV2 can reject the Merkle work. It cannot reject or defer IMV1.
- Re-open only if another merged plan closes every IMV1 acceptance test first.

### IMV-D7 Use a total float encoding

- The codec uses explicit tags for finite values, NaN, positive infinity, and
  negative infinity. It normalizes negative zero where logical equality treats
  both zero spellings as one coordinate.
- Firestore admission rejects a non-finite or out-of-range GeoPoint on REST and
  gRPC. The stricter gRPC response is a deliberate pre-launch wire change.
- Re-open only if Nimbus removes GeoPoint from stored typed values.

### IMV-D8 Test both Cargo feature graphs

- One literal fixture runs in `nimbus-storage` and in a shipped dependency
  graph that enables `workspace-hack`.
- Both tests assert the same version and digest bytes.
- Re-open only if Cargo stops unifying dependency features across one graph.

### IMV-D9 Route cross-adapter numeric index semantics

- `encode_index_value` currently maps integers and floats through `f64`.
  Integer 1 and float 1.0 therefore share one key. Convex uses disjoint integer
  and float bands, while Firestore and MongoDB compare mixed numbers by numeric
  value.
- `IndexDefinition` and the query planner carry no protocol or ordering
  profile. Appending one universal type tag would not implement both contracts.
  Inferring a policy from the request transport would also make stored index
  meaning unstable.
- IMV1 records the collision and makes index derivation consume the normalized
  tree without changing numeric ordering. Successor row RR31 owns an explicit
  engine-level query and index semantics contract with adapter conformance
  tests.
- Re-open this routing only if RR31 lands first and closes its acceptance
  evidence.

## Rejected designs

- Replace `MaterializedPosition.state_digest` with the root. Rejected because
  it couples persisted artifacts to a cache algorithm.
- Hash the journal prefix only. Rejected because a correct input log does not
  prove correct output state.
- Update roots from client request paths. Rejected because Nimbus has three
  client commit routes and additional internal writers. The apply seam owns
  materialization.
- Persist provider nodes now. Rejected because it adds six formats before a
  cold-start measurement.
- Treat a fast root match as a full scrub. Rejected because both sides can
  preserve the same stale or incorrect incremental state.
- Keep `serde_json::to_vec` as the canonical codec. Rejected because map order
  and float behavior change the digest input without a format-version change.
- Add a digest-only normalizer. Rejected because persistence, equality,
  indexing, and hashing could then disagree about one logical value.
- Disable `serde_json/preserve_order`. Rejected because the shipped Deno fork
  enables it through `deno_core`, `deno_cache_dir`, and `deno_graph` at
  `2.9.3-nimbus.1`. The codec must be feature-independent.
- Validate only the final PITR digest after replay. Rejected because a malformed
  target can dirty an empty destination before the import fails.
- Use Convex `SetDigest` as the incremental witness. Rejected because its
  source states that it is not a strong cryptographic hash and that inputs can
  collide. A witness collision can produce a false pass. Re-open only for a
  mature cryptographic multiset accumulator with a reviewed security bound.

## Measurement gate

The candidate workload verifies each covered state once per minute while the
tenant keeps serving writes. That one-minute interval defines the
repeated-verification budget that every threshold below measures against.

No production caller runs periodic verification today. The only consumer is
the on-demand `GET /debug/tenants/{tenant_id}/consistency` operator endpoint.
The one-minute interval is a target service level for a new capability. A
`STREAMING_ACCEPTED` verdict means the streaming full digest meets that target,
not that the capability failed.

IMV2 records two verdict files, `raw.json` and `verdict.md`. Use document counts
of 10,000, 100,000, and 1,000,000. Use document payloads of 256 bytes, 1 KiB,
and 8 KiB. Measure 0%, 0.1%, 1%, and 10% churn between checks. Record p50, p95,
p99, CPU time, bytes read, allocations, peak RSS, and state bytes.

Continue into IMV3 only when all conditions hold:

1. At 100,000 documents with 1 KiB payloads, and at the one-minute interval,
   the full verifier exceeds one limit. The limits are 1 second p95 and
   256 MiB extra peak RSS.
2. The candidate treap prototype improves p95 by at least 5 times at
   100,000 documents with 0.1% churn.
3. The candidate improves p95 by at least 10 times at 1,000,000 documents with
   0.1% churn.
4. An active session changes median write throughput by no more than 5%.
   It changes p99 commit latency by no more than 5%.
5. The index uses no more than 192 resident bytes per logical leaf at the
   1,000,000-document rung.

The verdict has three states:

- `STREAMING_ACCEPTED`: the full verifier meets both condition 1 limits. Mark
  IMV3 through IMV6 `rejected(IMV2 measurement gate)`. IMV7 records the
  streaming full verifier as the accepted design and closes the plan.
- `MERKLE_REQUIRED`: the full verifier exceeds a condition 1 limit and the
  candidate meets conditions 2 through 5. Continue into IMV3.
- `NO_ACCEPTABLE_DESIGN`: the full verifier exceeds a condition 1 limit. The
  candidate also fails a gate in conditions 2 through 5. Mark IMV3 through
  IMV6 `blocked(NO_ACCEPTABLE_DESIGN)` and record every failed gate. Stop for
  an owner decision on revised thresholds or a new candidate structure. IMV7
  cannot record an accepted design or close the plan under this verdict.

## Test matrix

| Dimension | Required cases |
|---|---|
| Logical state | empty, one table, hidden table, deleting table, schema, document, scheduled execution |
| Stored values | null, bool, integer bounds, finite float, negative zero, NaN, positive and negative infinity, string, bytes, array, map, geo point |
| Index keys | normalized-tree input, integer versus equal-valued float evidence routed to RR31 |
| Mutation | three client routes, schema, scheduler, trigger, PITR, object metadata, libSQL replica refresh |
| Order | sorted build, reverse build, random build, repeated update, delete then reinsert |
| Progress | unchanged, contiguous suffix, duplicate record, gap, retention gap, version mismatch |
| Failure | index update error, corrupt node, stale root, process restart, session eviction, provider state tamper |
| Provider | redb, SQLite, memory, PostgreSQL, MySQL, libSQL |
| Mode | full anchor, incremental match, incremental mismatch, forced full scrub |
| Cargo graph | storage-only defaults, engine or binary graph with `workspace-hack` |

## Verifier contract

IMV0 creates
`docs/private/plans/proof/incremental-materialized-verification/verify.sh`.
It prints one line for each condition and ends with
`Summary: N passed, M failed`.

Conditions 1 through 7 cover baseline, both Cargo graphs, logical-value and
float correctness, PITR preflight, streaming digest equivalence, benchmark
evidence, and the IMV2 verdict. Conditions 8 through 14 cover the Merkle-only
contract: the incremental root, provider parity, session behavior, fail-closed
fallback, and incremental metrics. Conditions 15 and 16 cover closeout
performance and documentation. They apply under `STREAMING_ACCEPTED` and
`MERKLE_REQUIRED`.

A literal `STREAMING_ACCEPTED` IMV2 verdict marks conditions 8 through 14 not
applicable, and only when the ledger marks IMV3 through IMV6 rejected with the
same evidence. A not-applicable condition prints the verdict evidence it
checked and counts as passed. Conditions 15 and 16 still run under that
verdict: they verify the streaming full digest against the ratified budget and
the documented acceptance outcome. A `NO_ACCEPTABLE_DESIGN` verdict can never
reach `Summary: 16 passed, 0 failed`. The plan stays nonterminal under it
until the owner decides.

The completion gate requires `Summary: 16 passed, 0 failed`.

## Tasks

### IMV0 Baseline and red verifier

- Problem: Nimbus has no repeatable cost baseline. The existing SIC verifier
  cannot see digest differences caused by the shipped Cargo graph.
- Owning seam and paths: this plan, its proof root,
  `crates/nimbus-engine/src/engine/queries/verification.rs`, and a new
  `crates/nimbus-engine/benches/materialized-verification.rs` target. The
  baseline also covers `Cargo.toml`, `crates/workspace-hack/Cargo.toml`, and
  `crates/nimbus-storage/src/store/journal_snapshot.rs`.
- Steps:
  1. Pin current main and record all dirty-state attribution.
  2. Capture both resolved `serde_json` feature graphs with `cargo tree`.
  3. Add review-only probes for map insertion order, equivalent `Json` and
     `Map` spellings, and NaN against positive infinity. Run the
     map-insertion-order probe in a crate whose resolved graph enables
     `workspace-hack`, because the storage-only graph cannot exhibit the
     divergence.
  4. Add the benchmark harness without changing production behavior.
  5. Add the 16-condition verifier and record its red summary.
  6. Record current full verification time, bytes, allocations, and RSS.
  7. Remove every review-only probe after its output reaches the proof root.
- Acceptance: the proof records both feature graphs and three failing digest
  probes. The benchmark emits stable JSON for every required rung. The verifier
  reports the exact red count. At task close, the only changes under `crates`
  are the retained benchmark target and its `crates/nimbus-engine/Cargo.toml`
  registration. The task removes every review-only probe. It changes no
  `packages` file and no production code path.
- Fail-before: the shipped graph changes map-order digest input. Equivalent
  stored-value spellings hash differently. NaN and positive infinity GeoPoints
  serialize to the same `null` digest input. Two unchanged checks also export
  and hash complete logical states.
- Verification:
  `cargo tree -p nimbus-storage -e features`.
  `cargo tree -p nimbus-engine -e features`.
  `cargo bench -p nimbus-engine --bench materialized-verification -- --quick --output docs/private/plans/proof/incremental-materialized-verification/imv0-raw.json`.
  `bash docs/private/plans/proof/incremental-materialized-verification/verify.sh`.
  `diff <({ git diff --name-only 137cc632a -- crates packages; git ls-files --others --exclude-standard -- crates packages; } | LC_ALL=C sort -u) <(printf 'crates/nimbus-engine/Cargo.toml\ncrates/nimbus-engine/benches/materialized-verification.rs\n')`.

### IMV1 Canonical leaf codec and streaming digest

- Problem: shipped binaries enable `serde_json/preserve_order` through
  `workspace-hack`. Document fields hash in insertion order in production.
  Storage-only tests hash them in sorted order. The existing order-stability
  test runs only where the feature is off.

  The digest also separates equivalent values and collapses non-finite
  GeoPoints to JSON `null`. The gRPC path admits those GeoPoints today.
  Index-key encoding gives integer 1 and float 1.0 one key. RR31 owns that
  cross-adapter defect. PITR validates its target after writes.
- Owning seam and paths:
  `crates/nimbus-core/src/typed_scalar.rs`, adapter value-lowering seams,
  `crates/nimbus-storage/src/index/encoding.rs`,
  `crates/nimbus-storage/src/store/journal_snapshot.rs`, a concept-owned
  `crates/nimbus-storage/src/materialized_position.rs`, stored-value
  canonicalization, `crates/nimbus-firebase/src/grpc/write_stream.rs`,
  `crates/nimbus-cli/src/provision.rs`, one
  shipped-graph test under `nimbus-engine`, the archived SIC plan, and
  `docs/private/architecture/storage/persistence-engine-baseline.md`.
- Steps:
  1. Give canonical materialized state and positions private fields with
     validated constructors and accessors.
  2. Define domain-tagged canonical leaves for every covered state family.
  3. Define one normalized Nimbus logical value tree at adapter ingress. Make
     each adapter lower protocol values into it once before persistence.
  4. Make persistence, semantic equality, index-key derivation, and canonical
     leaf encoding consume that same tree. The leaf codec serializes the tree.
     It does not normalize raw adapter or stored-value spellings again.
  5. Encode finite values, NaN, positive infinity, and negative infinity with
     explicit tags. Normalize negative zero where logical equality requires it.
  6. Record the numeric index collision and its adapter consequences in the
     RR31 proof input. Do not change the key format inside IMV1.
  7. Deliberately make gRPC reject invalid GeoPoints with the REST range rule.
  8. Stream deterministic bytes into SHA-256 and bump the position version.
  9. Validate the PITR target version and applied sequence before the first
     destination write. Keep the state-derived digest check after replay.
  10. Add one literal fixture to storage-only and shipped Cargo graphs.
  11. Delete duplicate, digest-only, or raw-JSON canonicalizers.
  12. Delete every incorrect claim that `preserve_order` stays disabled. This
      includes the workspace manifest and `crates/nimbus-cli/src/provision.rs`.
  13. Force-track the governing persistence specification and add the SIC4
      erratum with the failing mechanism and successor proof root.
- Acceptance:
  `normalized_logical_value_drives_persistence_equality_index_and_digest`,
  `canonical_leaf_equivalent_stored_values_hash_identically`,
  `canonical_leaf_order_is_provider_independent`,
  `canonical_leaf_nan_and_positive_infinity_do_not_collide`,
  `materialized_position_golden_matches_storage_graph`,
  `materialized_position_golden_matches_shipped_graph`,
  `pitr_rejects_invalid_target_position_before_first_write`,
  `index_key_derivation_consumes_normalized_logical_value`, and
  `streaming_materialized_digest_matches_reference` pass. Both golden tests
  assert the same literal version and digest. No digest path builds a full
  serialized payload. No covered persistence, equality, index, or hash path
  independently normalizes an adapter or stored-value spelling.
  `CanonicalMaterializedState` has no public fields. No file in the repository
  claims that `preserve_order` stays disabled.
- Fail-before: the IMV0 probes show build-graph drift, stored-value divergence,
  and a NaN versus positive-infinity collision. The RR31 input records that
  integer 1 and float 1.0 share one index key. A malformed PITR target writes
  state before the import rejects it.
- Verification:
  `cargo test -p nimbus-storage canonical_leaf -- --nocapture`.
  `cargo test -p nimbus-storage materialized_position -- --nocapture`.
  `cargo test -p nimbus-storage journal_snapshot -- --nocapture`.
  `cargo test -p nimbus-engine materialized_position_golden_matches_shipped_graph -- --nocapture`.
  `cargo test -p nimbus-firebase geo_point -- --nocapture`.
  `bash docs/private/plans/proof/incremental-materialized-verification/verify.sh`.

### IMV2 Measurement and continuation verdict

- Problem: Nimbus must not add a state index when the simpler streaming full
  digest meets the repeated-verification budget.
- Owning seam and paths: the benchmark target and
  `docs/private/plans/proof/incremental-materialized-verification/imv2-*`.
- Steps:
  1. Run every dataset, payload, churn, and mode rung.
  2. Run a minimal treap prototype only inside the benchmark target.
  3. Measure active-session write overhead and resident bytes per leaf.
  4. Write raw data separately from the verdict.
  5. Mark IMV3 through IMV6 eligible, rejected, or blocked from the literal
     three-state gate.
- Acceptance: `imv2-raw.json` contains every required field.
  `imv2-verdict.md` states `STREAMING_ACCEPTED`, `MERKLE_REQUIRED`, or
  `NO_ACCEPTABLE_DESIGN`. It records each threshold calculation and the
  measured margin against threshold 1.
- Fail-before: no current evidence shows that an incremental index beats the
  streamed full verifier within the write and memory budgets.
- Verification:
  `cargo bench -p nimbus-engine --bench materialized-verification -- --output docs/private/plans/proof/incremental-materialized-verification/imv2-raw.json`.
  `bash docs/private/plans/proof/incremental-materialized-verification/verify.sh`.

### IMV3 Deterministic Merkle treap

- Problem: a required accelerator needs one provider-neutral updateable root
  with a small, testable contract.
- Owning seam and paths: a new
  `crates/nimbus-storage/src/materialized_verification.rs` module.
- Steps:
  1. Define `VerificationRootVersion`, `LogicalLeafKey`, and
     `VerificationPosition` with private fields and validated constructors.
  2. Implement domain-separated key, priority, leaf, node, and empty hashes.
  3. Implement batch build, insert, update, delete, and root reads.
  4. Add structural depth and memory counters.
  5. Publish the arithmetic behind the 192-resident-bytes-per-leaf limit
     beside its constant.
  6. Screen a maintained crate before retaining custom tree code.
- Acceptance: `batch_and_incremental_verification_roots_match`,
  `verification_root_is_independent_of_update_order`,
  `delete_then_reinsert_restores_root`, and
  `verification_root_version_separates_formats` pass. The million-leaf depth
  and memory measurements meet IMV2 limits. The plan publishes the leaf-memory
  derivation beside its constant.
- Fail-before: the benchmark-only prototype has no production invariant,
  versioned type, property corpus, or dependency decision.
- Verification:
  `cargo test -p nimbus-storage materialized_verification -- --nocapture`.
  `cargo test -p nimbus-storage --test generated_history -- --ignored verification_root`.

### IMV4 Materialized-writer-owned state deltas

- Problem: an apply-only inventory can miss internal writers or publish a root
  ahead of state. Object metadata uses a sequenced internal committer route.
  LibSQL refresh also reconciles its local replica cache through storage. The
  durable journal can advance ahead of applied state.
- Owning seam and paths: `crates/nimbus-storage/src/sql/write_core.rs`,
  `crates/nimbus-storage/src/memory/journal.rs`,
  `crates/nimbus-storage/src/materializer/mod.rs`,
  `crates/nimbus-engine/src/engine/objects.rs`,
  `crates/nimbus-engine/src/replica.rs`, `crates/nimbus-storage/src/libsql.rs`,
  provider apply and replica-refresh adapters, and `AGENTS.md`.
- Steps:
  1. Define `MaterializedStateDelta` at the storage concept seam.
  2. Produce deltas from successful post-apply outcomes.
  3. Publish sequence `A` only after all deltas through `A` update the index.
  4. Invalidate the process-local index on any gap or update error.
  5. Inventory the three client routes plus schema, scheduler, trigger, PITR,
     object metadata, and replica-refresh writers.
  6. Emit exact deltas for the sequenced object-metadata route.
  7. Rebuild or invalidate the index when libSQL replaces or catches up its
     local replica cache outside the normal materializer.
  8. Inspect and preserve any concurrent `AGENTS.md` edits. Correct its writer
     inventory to name the sequenced internal object-metadata committer.
  9. Classify the materialized serving surface as a derived read cache. Do not
     let its durable catch-up position advance the verification index.
- Acceptance: `root_advances_with_applied_sequence`,
  `failed_apply_does_not_advance_root`,
  `replay_duplicate_keeps_root`, and
  `apply_gap_invalidates_verification_index` pass on local providers.
  The object route passes `object_manifest_commit_updates_verification_root`.
  The libSQL route passes
  `libsql_replica_refresh_invalidates_stale_verification_root`.
  `durable_head_ahead_of_apply_does_not_advance_verification_root` passes. The
  materialized serving surface has no verification-root write authority. The
  writer-ownership gate accounts for every listed production writer.
  `AGENTS.md` no longer claims that object manifests use raw
  `TenantPointWrite`. Unrelated concurrent edits remain intact.
- Fail-before: roots can precede materialized apply, and `AGENTS.md` names a
  stale object-metadata writer path.
- Verification:
  `cargo test -p nimbus-storage durable_journal -- --nocapture`.
  `cargo test -p nimbus-storage commit_path_ownership -- --nocapture`.
  `cargo test -p nimbus-engine mutation -- --nocapture`.
  `rg -n 'Object metadata|Object manifests|TenantPointWrite' AGENTS.md`.

### IMV5 Bounded verification sessions

- Problem: the current verifier rebuilds authoritative, shadow, and embedded
  replica state for each request. A root without a retained session saves no
  repeated work.
- Owning seam and paths:
  `crates/nimbus-engine/src/engine/queries/verification.rs`,
  `crates/nimbus-engine/src/verification.rs`, and a concept-owned session
  module under `crates/nimbus-engine/src/engine/queries/`.
- Steps:
  1. Build a session from one full bootstrap cut and full scrub.
  2. Retain independent authoritative, shadow, and replica indexes.
  3. Apply one contiguous journal suffix to each existing materializer.
  4. Compare roots only at one exact applied sequence.
  5. Report mode, anchor, anchor age, event count, and escalation reason.
  6. Bound sessions by count, bytes, idle time, and anchor age.
- Acceptance: `incremental_verifier_reports_mode_and_anchor`,
  `unchanged_recheck_reads_no_full_snapshot`,
  `root_mismatch_escalates_to_full_scrub`,
  `retention_gap_rebuilds_session`, and
  `bounded_sessions_evict_least_recently_used` pass.
- Fail-before: two calls to `verify_consistency_async` rebuild all three views.
- Verification:
  `cargo test -p nimbus-engine consistency_verification -- --nocapture`.
  `cargo test -p nimbus-engine verification_session -- --nocapture`.

### IMV6 Provider parity, faults, and observability

- Problem: a fast root is unsafe without real-provider parity, state-tamper
  detection, bounded cache behavior, and operator-visible fallback reasons.
- Owning seam and paths: shared provider contract scenarios, diagnostics,
  external provider tests, and the storage verification harness.
- Steps:
  1. Run one canonical leaf and root corpus on all six providers.
  2. Add process restart, cache loss, corrupt node, stale anchor, and retention
     gap cases.
  3. Tamper provider state at an unchanged sequence and prove a full scrub
     detects it.
  4. Add bounded metrics for mode, time, bytes, leaves, rebuilds, mismatches,
     sessions, and evictions.
  5. Add a force-full operator control and a safe cache-clear control.
- Acceptance:
  `materialized_verification_root_is_provider_independent`,
  `full_scrub_detects_state_tamper_at_same_sequence`,
  `corrupt_index_never_reports_success`, and
  `verification_metrics_have_bounded_labels` pass. Record PostgreSQL, MySQL,
  and libSQL results. Mark an unavailable lane `UNVERIFIED` with its hosted
  gate.
- Fail-before: an incremental root derived from journal events can match while
  a provider row is corrupt at the same sequence.
- Verification:
  `cargo test -p nimbus-storage materialized_verification -- --nocapture`.
  `make verify-harness SURFACE=storage`.
  Run the repository external-provider lanes.

### IMV7 Closeout, performance proof, and docs

- Problem: the accepted design needs final regression evidence and one
  operator contract.
- Owning seam and paths: `ARCHITECTURE.md`,
  `docs/private/architecture/time-and-ordering.md`,
  `docs/private/operating/verification.md`,
  `docs/private/architecture/storage/persistence-engine-baseline.md`, the
  archived SIC plan, this plan, and its proof root.
- Steps:
  1. Repeat IMV2 on final code and compare matched runs.
  2. Record full and incremental assurance in the operating guide.
  3. Record the root as a disposable derived index in architecture docs.
  4. Confirm Git tracks the persistence specification and that it matches the
     shipped codec and PITR behavior.
  5. Resolve the SIC4 erratum with the IMV1 work commit and golden-test proof.
  6. Require each determinism or canonicality invariant to run from a crate in
     the shipped binary dependency graph. Inspect and preserve concurrent
     verification-guide edits before adding this rule beside the existing
     feature-unification guidance.
  7. Run focused checks, required local CI, and the pre-PR autoreview gate.
  8. Publish a closeout proof with exact counts and remaining uncertainty.
- Acceptance: the final verdict meets the chosen branch of IMV2. The verifier
  reports `Summary: 16 passed, 0 failed`. Docs name when Nimbus runs a full
  scrub and when it can use an incremental check. The operating guide requires
  shipped-graph evidence for determinism and canonicality invariants.
- Fail-before: current docs define only the full `MaterializedPosition`
  comparison and no repeated-verification assurance level.
- Verification: `cargo fmt --all --check`. `make clippy`. `make ci`.
  `bash scripts/check-docs.sh`.
  `bash docs/private/plans/proof/incremental-materialized-verification/verify.sh`.
  `nimbus-autoreview --gate pre-pr --mode auto`.

### IMV9 Cleanup

- Problem: a merged campaign must not remain an active control plane.
- Owning seam and paths: this plan, its proof root, and
  `docs/private/plans/README.md`.
- Steps:
  1. Confirm the final pull request merge and every terminal ledger row.
  2. Move this plan to `docs/private/plans/archive/` with the merge date and
     pull request range.
  3. Retain the proof root and remove the active index entry.
  4. Confirm that successor row RR30 still owns IMVR1 before removing this
     plan's active index entry.
  5. Confirm that successor row RR31 still owns IMVR2.
- Acceptance: repository search finds only the archive record, retained proof,
  and named successor references.
- Fail-before: not applicable because the merge triggers cleanup.
- Verification:
  `rg -n 'incremental-materialized-verification' docs/private/plans`.

## Goal

```text
Execute docs/private/plans/incremental-materialized-verification-plan.md
to completion. This is a whole-plan goal, not a single-task goal. Read
the plan fully, then read README.md, ARCHITECTURE.md,
docs/private/plans/README.md,
docs/private/plans/archive/storage-integrity-contracts-plan.md,
docs/private/architecture/time-and-ordering.md,
docs/private/operating/verification.md,
docs/private/adapters/convex/ai-guidelines.md,
AGENTS.md,
crates/nimbus-storage/src/store/journal_snapshot.rs,
crates/nimbus-core/src/typed_scalar.rs,
crates/nimbus-storage/src/index/encoding.rs,
crates/nimbus-firebase/src/grpc/write_stream.rs,
crates/nimbus-storage/src/sql/write_core.rs,
crates/nimbus-storage/src/materializer/mod.rs,
crates/nimbus-storage/src/libsql.rs,
crates/nimbus-engine/src/engine/objects.rs,
crates/nimbus-engine/src/engine/queries/verification.rs, and
crates/nimbus-engine/src/replica.rs. Work in
/Users/jack/src/github.com/nimbus/nimbus-worktrees/incremental-materialized-verification
on branch codex/incremental-materialized-verification. Chat history is
not progress state. Resume from the status ledger, the execution log,
and git state. If compaction happens, continue from the plan and git
state rather than restarting. Loop: keep one task in_progress,
implement at the owning seam, capture fail-before evidence, run the
verification commands, commit the work per the commit policy, write the
proof file, append the execution log with the work commit, mark the task
terminal with evidence, commit the plan update the same way, then
advance to the next task. Decide rather than ask. Mark a wrong or
already-satisfied task no-action with a one-line reason. Record a
blocker and continue with the next eligible task. Binding constraints:
MaterializedPosition remains canonical, the root stays derived and
process-local, every fast result names its full anchor, full scrub stays
available, root loss fails to full verification, the digest is identical
in storage-only and shipped Cargo graphs, every adapter lowers a value once
into the normalized Nimbus logical tree used by persistence, equality,
indexing, and hashing, the codec only serializes that tree, distinct non-finite float
classes never collide, IMV1 does not change protocol numeric ordering, RR31
owns cross-adapter numeric query and index semantics, PITR preflight runs before destination writes,
every production materialized writer updates or invalidates the index,
the durable journal head never substitutes for applied state, the materialized
serving cache never certifies provider state,
and no new client mutation route is permitted. Commit policy: one reviewed pull request
per task, with separate work and proof commits when practical. Run the
Nimbus pre-PR autoreview gate after final checks for each substantive
code pull request. Stop only at a valid stop state from the plans skill.
A NO_ACCEPTABLE_DESIGN verdict from IMV2 is a valid stop state: mark IMV3
through IMV6 blocked, record every failed gate, and stop for the owner.
Before you stop, update the ledger and the log, and record the next
action in the status line. The goal is met when the chosen IMV2 branch
is complete, every non-cleanup row is terminal, the verifier reports
Summary: 16 passed, 0 failed, the required checks are recorded, and the
final pull request is ready to merge.
```

## Execution log

Append rows at the end. This section stays last.

| Date | Item | Action | Evidence |
|---|---|---|---|
| 2026-08-19 | meta | authored | Proposed plan created from the PR #287 architecture review. Baseline `8348556754446f5cd0f35a10619fa9169e45e2f2`. No implementation or benchmark run started. |
| 2026-08-19 | meta | refined | Accepted the independent Opus verification. IMV1 is mandatory correctness work and owns cross-Cargo-graph golden tests, stored-value normalization, total float encoding, gRPC GeoPoint admission, PITR preflight, opaque construction, the tracked persistence specification, and the SIC4 erratum. Blob reclamation stays outside IMV. No implementation started. |
| 2026-08-19 | meta | refined | Added object-metadata and libSQL replica-refresh writers, live gRPC collision evidence, the non-removable `preserve_order` refusal, probe cleanup, direct-main plan order, the deliberate wire change, and the shipped-graph verification rule. No implementation started. |
| 2026-08-19 | meta | refined | Assigned the stale `AGENTS.md` object-writer instruction to IMV4. Execution must preserve the user's pending rewrite before it corrects the raw `TenantPointWrite` claim. No implementation started. |
| 2026-08-19 | meta | refined | Owner approved mandatory IMV1 and the IMV2 thresholds. Defined the one-minute repeated-verification interval in the measurement gate and bound threshold 1 to it. No implementation started. |
| 2026-08-19 | meta | refined | Rebased the baseline onto `5fb9284cf7e313cfc0a4901835d7bd6144e297c8`, which committed the AGENTS.md rewrite and the verification-runbook hazards. The stale object-writer sentence survived that commit, so IMV4 still owns it. No implementation started. |
| 2026-08-19 | meta | refined | Strengthened IMV-D2 and IMV1: adapters lower values once into one normalized Nimbus logical tree used by persistence, semantic equality, index-key derivation, and hashing; the canonical codec only serializes that model. Routed repository-wide opaque durable outcomes and provider-capability sets to the active architecture-review control plane as IMVR1. No implementation started. |
| 2026-08-19 | meta | refined | Closed the IMVR1 ownership gap by adding deferred successor row RR30 to the active architecture-review plan. RR30 activates after IMV1 and BLI3 merge, preserves valid closed enums, and routes every constructible invalid state to a repair row. No implementation started. |
| 2026-08-19 | meta | refined | Applied the required Fable corrections. Led IMV1 with the shipped-graph `preserve_order` divergence and named the duplicate false claim in `crates/nimbus-cli/src/provision.rs`. Recorded that periodic verification does not exist today. Required the threshold 1 margin and the 192-bytes-per-leaf derivation. No implementation started. |
| 2026-08-19 | IMVR2 | routed | Rejected the proposed universal numeric type tag because `IndexDefinition` carries no ordering profile and the tag cannot satisfy both Convex and unified numeric contracts. Routed cross-adapter numeric query and index semantics to successor row RR31. Rejected Convex `SetDigest` because its source states that it is not cryptographically strong and can collide. No implementation started. |
| 2026-08-19 | meta | corrected | Updated the landing bundle and promotion gate to name both successor rows, RR30 and RR31. The owner-corroborated Fable audit adds no other open correction. No implementation started. |
| 2026-08-19 | meta | corrected | Applied the second contract audit. Removed IMV0 from the promotion gate and sequenced it first after promotion. Made IMV0 acceptance retain the benchmark target explicitly. Split verifier conditions 8 through 14 as Merkle-only with a not-applicable rule and required conditions 15 and 16 under both verdicts. Moved the status ledger before Dependencies for cold resume. Named RR31 in the successor plan's resume pointer. No implementation started. |
| 2026-08-19 | IMV2 | corrected | Replaced the two-state verdict with three states: `STREAMING_ACCEPTED`, `MERKLE_REQUIRED`, and `NO_ACCEPTABLE_DESIGN`. The third state blocks IMV3 through IMV6 and keeps the plan nonterminal for an owner decision, so rejection can no longer accept a full verifier that missed its own target. Made the IMV0 closing diff an enforced allowlist that permits only the benchmark target and its manifest registration. No implementation started. |
| 2026-08-19 | meta | corrected | Hardened the IMV0 closing allowlist into a baseline-anchored exact-set check. The check includes untracked files and requires both permitted paths. Split a long sentence in the `NO_ACCEPTABLE_DESIGN` bullet. Named the two successful verdicts for conditions 15 and 16. No implementation started. |
| 2026-08-20 | meta | rebased | Refreshed the baseline to `137cc632a` after 17 merged commits. PRs #293, #297, and #301 repaired materialized-read publication and made durable-versus-applied sequence drift explicit. IMV4 now preserves that distinction and keeps the serving surface outside verification authority. No BLI-owned blob path changed. No implementation started. |
| 2026-08-20 | IMV0 | started | The owner promoted the plan to `active`. IMV0 is the only `in_progress` task at baseline `137cc632a`. |
| 2026-08-20 | IMV0 | completed | Added the retained 36-rung full-verifier harness in work `58e46b675` and made default Cargo benchmark output safe in `14227dc59`. Proof commits `97949afa2` and `6e643ada8` record both feature graphs, three review-only digest probes, the quick cost baseline, and `Summary: 3 passed, 13 failed`. The closing allowlist, format, Clippy, docs, patch, and writing gates pass. Nimbus autoreview is clean. No production path or package changed, and no review-only probe remains. |
| 2026-08-20 | IMV1 | started | Advanced mandatory canonical-value, codec, float, PITR, opaque-position, and cross-graph golden work. IMV1 is the only `in_progress` task. |
