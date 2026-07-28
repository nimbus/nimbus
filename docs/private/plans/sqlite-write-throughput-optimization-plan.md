# SQLite Write-Throughput Optimization Control Plane

Status: `active — CTRL0 merged (PR #241, squash commit 714f94437); SWT0 is the sole in_progress task`

Owner: this plan, and no other plan

Research:
`research/sqlite-write-overhead-and-opportunities-2026-07.md`

Proof root: `proof/sqlite-write-throughput/`

## Mission

Raise the canonical local SQLite result from the historical **21,433 durable
logical CRUD mutations/s** observation. SWT0 freezes the exact baseline source
commit and protocol as `B_ref`; after all production work merges, SWT5 freezes
the exact final candidate commit as `F_ref`. The final acceptance session
alternates immutable binaries from those two commits and requires all of:

- N=256 mean of the same-session paired `F_ref`/`B_ref` ratios **at least
  1.40**, with the lower 95% confidence bound of the paired percentage delta
  above zero;
- N=256 mean **at least 30,000 mutations/s** as an absolute floor;
- N=256 lower 95% confidence bound **at least 28,000 mutations/s**;
- CV at every accepted N=1/32/256 lane **at most 10%**;
- no unacceptable throughput, latency, memory, database/WAL, checkpoint,
  cold-start, contention, durability, or correctness regression.

The invariant target is a 40.0% end-to-end improvement over the
contemporaneous `B_ref` control; the 30k/28k floors prevent a uniformly
depressed session from making the campaign artificially easy. The target is
grounded in the clean layered planning reference: replay-guarded statement
reuse/invariant hoisting cut the controlled SQL fixture from 15.257 ms to
5.073 ms, and writer connection initialization accounts for an independently
measured upper bound of about 30% of production-storage elapsed time. SWT0
must freeze the baseline source/protocol and capture a hashed reference run.
That run diagnoses later drift but is not reused as a numeric denominator:
every candidate is accepted only against a contemporaneous control.

This plan optimizes Nimbus, not a benchmark-only path. It preserves the
journal, document/index versions, live state, authorization, validation,
conflict handling, publication, and FULL/WAL durability.

## Ownership

`archive/parallel-prepare-serial-commit-plan.md` is the complete predecessor;
it does not own follow-on SQLite efficiency work. The active architecture
review owns its July 6 finding ledger, not this benchmark-derived campaign.
No current plan owns the work below.

This file is the sole implementation owner and is routed from `README.md`.
Do not create another SQLite-performance plan or a second status ledger.

## Architecture In One Page

The detailed source census and statement accounting live in the research
document. The constraints implementation agents must carry are:

1. **Queued journal route:** parallel prepare → serial dense assignment →
   durable journal append transaction → materialization/apply transaction →
   write-log publication/cache invalidation/applied-head/fan-out. It
   intentionally has two SQLite commits per batch.
2. **Direct route:** prepare/revalidate/assign →
   `persist_prepared_write_batch` → one atomic SQLite transaction containing
   document/version/index/journal/watermark effects → publication.
3. **`MutationExecutionUnit` route:** stage one function invocation →
   conflict/schema validation and assignment →
   the same prepared-write persistence seam as direct → publication.

These are the three Engine-owned **client document mutation routes**. Direct
and queued mutations validate schema during preparation and detect stale
preparation through their conflict/reprepare machinery; execution units also
validate their staged write set inside the serial commit closure. All document
writes, index effects, durable record, and applicable watermarks must remain
atomic within each route's existing storage transaction. Fan-out must never
precede applied visibility.

This is not an inventory of every storage writer. Schema, scheduler, trigger,
and point-in-time-restore operations can use internal committer jobs. Object
manifests write through `TenantPointWrite` on the read executor, and libSQL
replica refresh reconciles its local cache through storage directly. Writer
ownership changes must coexist safely with those non-client transactions and
must not assume sole writership.

## Non-Negotiable Invariants

- Keep WAL and `synchronous=FULL`.
- Keep dense, gap-free sequence assignment.
- Keep durable-head/applied-head ordering and recovery of the durable
  unapplied tail.
- Keep the queued route's append COMMIT before apply COMMIT.
- Keep direct/execution-unit document, index, journal, and watermark effects in
  their single existing atomic transaction.
- Keep record integrity validation and divergent replay rejection.
- Keep document-version and index-version history.
- Keep schema, table-identity, resource-path, authorization, capability,
  conflict, cache-invalidation, subscription, and fan-out semantics.
- Keep all three client document mutation routes on the same storage
  primitives; do not add a fourth client route.
- Do not change the storage format in this campaign.
- Do not accept a performance result whose CV exceeds 10%.
- Do not accept a throughput win that hides a material regression elsewhere.

## Explicitly Rejected Shortcuts

These are `rejected` without further experimentation:

- `synchronous=NORMAL` or `OFF`;
- disabling WAL, durable acknowledgement, or a sync-bearing commit;
- removing or bypassing the journal, live rows, MVCC history, maintained
  indexes, validation, authorization, publication, or fan-out;
- combining queued append and apply into one transaction;
- a production fast path that exists only when a benchmark flag is set;
- comparing one raw row mutation with one Nimbus logical mutation as the same
  unit;
- accepting historical, cross-machine, cross-binding, or noisy numbers as an
  A/B result.

Checkpoint tuning is not authorized by the layered fixture: it produced only
337–341 WAL frames, below the 1,000-page automatic-checkpoint threshold.
SWT0 must capture Engine-scale checkpoint behavior before Decision D5 can be
kept or superseded for the full campaign.

## Worktree And PR Protocol

The control-plane PR must merge before implementation begins.

For every implementation PR:

1. update this ledger to mark exactly one task `in_progress`;
2. fetch clean `origin/main`;
3. verify the requested branch and worktree do not already exist;
4. create:
   - branch `codex/sqlite-write-throughput-p<N>-<concept>`;
   - worktree
     `/Users/jack/src/github.com/nimbus/nimbus-sqlite-write-throughput-p<N>`;
5. read this plan, the research document, the current source/tests/callers,
   and the preceding PR's proof;
6. capture fail-before evidence before changing behavior;
7. implement one concept, benchmark it against the same-session base, and keep
   it only if every gate passes;
8. update the ledger, decision log, accepted/rejected tables, and proof before
   handoff or compaction;
9. run structured `autoreview` before push/PR;
10. make hosted CI the merge source of truth;
11. after merge, remove only that PR's worktree and local/remote branch.

Never clean unrelated worktrees or branches. Never alter or discard
pre-existing user work in `/Users/jack/src/github.com/nimbus/nimbus`.

## Measurement Contract

### Canonical full Engine protocol

```bash
timeout 600 env \
  NIMBUS_CWB_WORKLOAD=crud \
  NIMBUS_CWB_LADDER=1,32,256 \
  NIMBUS_CWB_OPS_PER_WORKER=100 \
  NIMBUS_CWB_MAX_MUTATIONS_PER_ROUND=9000 \
  NIMBUS_CWB_MEASURE_ROUNDS=15 \
  NIMBUS_CWB_WARMUP_ROUNDS=3 \
  NIMBUS_CWB_SPLIT_PHASES=1 \
  NIMBUS_CWB_OUT=<proof-path> \
  <compiled-concurrent-write-throughput-binary>
```

Also run `NIMBUS_CWB_WORKLOAD=hotkey` at N=1/32/256 with the same
warmup/round policy. A candidate that changes only an isolated storage layer
still runs the complete Engine protocol before acceptance.
At N=256, hot-key intentionally saturates the bounded committer inbox.
`RetryableAfterBackoff` responses are retried with a short delay, and that
backpressure wait remains inside the measured mutation latency.

### Canonical layered protocol

```bash
timeout 600 env \
  NIMBUS_SWO_ROUNDS=12 \
  NIMBUS_SWO_REPETITIONS_PER_SAMPLE=60 \
  NIMBUS_SWO_OUT=<proof-path> \
  <compiled-sqlite-write-overhead-binary>
```

The exact fixture is 768 phased CRUD mutations and captured batches
`[5, 251, 90, 256, 20, 146]`.

### A/B rules

- Build release binaries for base and candidate from clean worktrees.
- Alternate base/candidate order by sample block where practical.
- SWT0 freezes `B_ref` as a source commit plus protocol, not a permanent
  numeric denominator. Every candidate gate reruns its applicable code base in
  the same session; SWT5 reruns `B_ref` itself against exact `F_ref`.
- Record commit SHA, dirty state, build command, binary SHA-256, report
  SHA-256, hardware/OS, raw samples, mean, median, Student-t 95% CI, CV,
  phase split, batch size, latency, SQL/row/transaction/sync units, DB/WAL
  bytes, frames/checkpoint result, peak RSS, and cold open.
- Layered DB/WAL/page/checkpoint cells are fieldwise maxima across every
  measured repetition and round; never substitute one convenient repetition.
- CV must be at most 10%. Otherwise quiet the host and rerun.
- Compute the 95% CI of matched percentage deltas. An accepted optimization's
  required primary delta must be positive at the lower bound.
- Preserve fixture bytes/row counts unless the task explicitly predicts an
  internal encoding reduction that does not change the storage format.
- Store every accepted and rejected run under this plan's proof root.

### Final paired block protocol

SWT5 predeclares and executes this protocol without adaptive stopping:

1. After every production optimization PR is merged, fetch clean
   `origin/main` and freeze that exact commit as `F_ref`. Record `B_ref`,
   `F_ref`, both clean statuses, build commands, and binary SHA-256 hashes
   before sampling.
2. Build each exact commit once in separate clean worktrees. Every sample block
   uses those same two immutable binaries. Any source change or rebuild
   invalidates the complete session and restarts it from block 1.
3. Run six adjacent base/final block pairs. Each side of each pair executes the
   complete canonical CRUD protocol above, including its own warmups and
   fifteen measured rounds. Use the fixed balanced order
   `B_ref/F_ref`, `F_ref/B_ref`, repeated three times.
4. For pair `i`, define `B_i` and `F_i` as that report's N=256 mean and
   `d_i = 100 × (F_i / B_i - 1)`. The primary ratio is the arithmetic mean of
   the six `F_i / B_i` values. Compute the paired Student-t 95% CI over the six
   predeclared `d_i` values; never pair rounds post hoc across reports.
5. Compute the final absolute mean, Student-t 95% CI, and CV over the six
   `F_i` block means. Every underlying report must also meet the per-lane
   CV≤10% gate. A failed process, noisy report, source change, or missing
   artifact rejects and retains the whole session; do not drop or repair a
   pair selectively.

The primary gate requires ratio mean ≥1.40 and paired-delta lower CI >0. The
absolute gates require final mean ≥30,000 and final lower CI ≥28,000. N=1,
N=32, hot-key, and resource regressions use predeclared same-session pairs and
balanced order as applicable; their task-specific thresholds remain unchanged.

### Cross-cutting regression gates

Unless a task states a stricter threshold:

- N=1 and N=32 throughput: no paired mean regression greater than 5%;
- N=256 throughput before final closeout: no paired mean regression greater
  than 2% for a candidate whose main benefit is another lane;
- p50/p95/p99 closed-loop wait: no regression greater than 10% at like N;
- hot-key N=32 mean: no regression greater than 5%;
- effective batch: no reduction greater than 5% at N=256;
- peak RSS: no increase greater than 10% or 32 MiB, whichever is larger;
- database and WAL bytes/frames: no increase greater than 5% without a proven
  and accepted reason;
- cold `SqliteTenantStore::open`: no regression greater than 5% or 100 µs,
  whichever is larger;
- no new foreground checkpoint at the fixed 768-mutation layered fixture;
- no unexplained Engine-scale checkpoint-count, checkpoint-time, or WAL-frame
  regression once SWT0 freezes those counters;
- every correctness and crash/recovery gate passes.

Closed-loop percentiles are not SLA latency. FINAL adds a below-saturation
open-loop companion before making a service-latency claim.

## Durable Status Ledger

Update this table before and after every material work session, before handoff,
and before likely compaction. There may be only one `in_progress` row. Resume
it; do not choose a different row.

| Phase/task | Status | Branch / worktree | PR / commit | Measurement result | Correctness evidence | Proof | Next action |
| --- | --- | --- | --- | --- | --- | --- | --- |
| CTRL0 Research, exact harness, and control-plane PR | `complete` | `codex/sqlite-write-throughput-plan` / `nimbus-sqlite-write-throughput-plan` | PR #241 merged / `714f94437` | Historical Engine observation 21,433; clean layered planning reference: storage 38,810, guarded SQL 151,485; independent quiet-host audit reported 25,862 and exposed host drift | Benchmark compiles and focused clippy passes; exact hot-key N=256 protocol completes under overload; deterministic layered fixture audits durable state; docs gates pass; rejected reports retained whole | `proof/sqlite-write-throughput/{environment,full-engine-baseline,layered-planning-reference,layered-review-reruns,layered-final-binary-rejected,layered-noisy-diagnostic-raw,hotkey-backpressure-validation,independent-audit-remediation}.md` | complete; planning worktree/branch removed |
| SWT0 Install diagnostics, then freeze same-session base and resources | `in_progress` | sequential `p0-observability` then `p0-baseline` branches/worktrees | — | — | — | `proof/sqlite-write-throughput/swt0/` | SWT0.1 observability counters on `codex/sqlite-write-throughput-p0-observability` |
| SWT1 Prepared statements + batch-invariant apply context | `planned` | `codex/sqlite-write-throughput-p1-batch-sql` / `nimbus-sqlite-write-throughput-p1` | — | target ≥5% paired N=256 gain; lower-layer delta lower CI >0 | queued/direct/unit parity + crash/replay + fail-before counters | `proof/sqlite-write-throughput/swt1/` | after SWT0 merge/cleanup |
| SWT2 Resident embedded writer connection | `planned` | `codex/sqlite-write-throughput-p2-writer-residency` / `nimbus-sqlite-write-throughput-p2` | — | target ≥5% storage gain and N=1 or N=32 gain; no N=256 loss >2% | two queued commits retained; route/fault/reopen parity | `proof/sqlite-write-throughput/swt2/` | after SWT1 merge/cleanup |
| SWT3 Reusable encoded persistence payload | `planned` | `codex/sqlite-write-throughput-p3-encoded-payload` / `nimbus-sqlite-write-throughput-p3` | — | execute only if measured remaining ceiling ≥3% and final relative-plus-absolute target unmet | integrity/storage-format/route parity | `proof/sqlite-write-throughput/swt3/` | decision after SWT2 |
| SWT4 Attribute and prove a guarded forward-apply optimization | `planned` | `codex/sqlite-write-throughput-p4-forward-apply` / `nimbus-sqlite-write-throughput-p4` | — | first isolate the combined ~7–11.5% cross-run lower-layer delta; implement only with ≥3% projected end-to-end safe gain | conditional-write + recovery full-validation corruption proof | `proof/sqlite-write-throughput/swt4/` | after SWT3 decision; measurement task runs even if the final target is already met |
| SWT5 Final target, regression, docs, archive, and cleanup | `planned` | `codex/sqlite-write-throughput-p5-final` / `nimbus-sqlite-write-throughput-p5` | — | Same-session `F_ref`/`B_ref` paired-ratio mean ≥1.40 and `F_ref` ≥30k; lower `F_ref` CI ≥28k; paired-delta lower CI >0; all cross-gates pass | full focused + `make ci` + hosted CI | `proof/sqlite-write-throughput/final/` | after SWT4 disposition |

## Baseline And Accepted-Candidate Ledger

| Candidate | Commit | N=1 mut/s | N=32 mut/s | N=256 mut/s | N=256 95% CI / CV | Hot-key N=32 | Storage mut/s | Peak RSS | WAL bytes/frames | Verdict |
| --- | --- | ---: | ---: | ---: | --- | ---: | ---: | ---: | --- | --- |
| Historical observation | `e47b64eac` | 1,711 | 13,510 | 21,433 | 20,753–22,112 / 5.7% | not captured | 38,810 planning reference | not captured | 1,396,712 / 339 on layered fixture | Context only; host state was later shown to move N=256 materially |
| SWT0 frozen source/reference `B_ref` | — | — | — | — | — | — | — | — | — | pending; freeze commit/protocol and record reference run without replacing historical observation |
| SWT1 | — | — | — | — | — | — | — | — | — | pending |
| SWT2 | — | — | — | — | — | — | — | — | — | pending |
| SWT3 | — | — | — | — | — | — | — | — | — | pending / optional |
| FINAL | — | — | — | paired `F_ref`/`B_ref` ratio mean ≥1.40; `F_ref` ≥30,000 | `F_ref` lower CI ≥28,000; paired-delta lower CI >0; CV ≤10% | no >5% regression | record | record | no unexplained >5% | pending |

Never replace either the historical observation or the frozen SWT0
source/reference row. Add candidates and record rejections separately.

## Findings / Opportunity Ledger

| ID | Opportunity | Evidence | Risk | Campaign disposition |
| --- | --- | --- | --- | --- |
| O1 | Cache/prepare recurring batch statements | Guarded ablation is 3.01× current-loop SQL | low | SWT1 |
| O2 | Hoist document format, schemaless schema, and table identity checks per batch/distinct key | O1 ablation removes 3,048 statements/fixture while retaining replay guards | low–medium | SWT1; make invalidation explicit |
| O3 | Resident writer connection and persistent statement cache | 494.1 µs initialized open; twelve opens/fixture; resident-current lane 29.8% above storage | medium | SWT2 |
| O4 | Reuse encoded record/document forms | CPU-only ceiling 1.147 ms / 5.8% storage elapsed | medium | SWT3 only if still material |
| O5 | Avoid full forward preimage deserialization while retaining a conditional SQL guard and full replay validation | Combined preimage-query + delete-binding ablation saves ~7–11.5% of guarded lower-layer elapsed across accepted clean/audit runs; production additionally deserializes/compares | high correctness | SWT4: isolate, design, and implement only behind stronger proof |
| O6 | Checkpoint scheduling/tuning | Layered fixture remains at 337–341 frames, below 1,000; Engine N=256 runs are large enough to cross that scale | medium | defer decision until SWT0 Engine-scale counters |
| O7 | More queued coalescing | Avg batch already 142.22; apply dominates | latency/fairness | deferred |
| O8 | Transaction/journal/storage-format redesign | Could change sync/row amplification | high | rejected from easy-gains campaign |

## Decision Log

| ID | Date | Decision | Evidence / consequence |
| --- | --- | --- | --- |
| D1 | 2026-07-27 | Compare explicit units, never generic "writes/s" | Nimbus N=256 is approximately 64.6k physical row changes/s and 301 sync commits/s, not 21.4k raw inserts/s |
| D2 | 2026-07-27 | Set final local target at 30k mean / 28k lower CI | 40% above base and well within low-risk measured headroom |
| D3 | 2026-07-27 | Execute statement reuse and invariant hoisting together, with internal attribution counters | They share batch ownership and the clean replay-guarded planning ablation measures their combined safe mechanism |
| D4 | 2026-07-27 | Reuse a connection without combining queued transactions | Connection churn is measured; append-before-apply is a crash/replay invariant |
| D5 | 2026-07-27 | Reject checkpoint tuning absent new evidence | No fixed fixture reaches automatic checkpoint |
| D6 | 2026-07-27 | Treat forward apply as worthwhile but give it a separate higher-proof phase | 11.5% guarded lower-layer elapsed is material; combined ablation needs attribution, and corruption/recovery risk forbids simply deleting the read |
| D7 | 2026-07-27 | Plan/harness PR merges before any implementation branch | Every candidate must consume one canonical protocol and ledger |
| D8 | 2026-07-28 | Supersede D2's absolute-only gate with a same-session paired ≥40% gain over frozen source `B_ref`, plus 30k/28k floors | Identical production source measured 21,433 and 25,862 on different host states; rerunning `B_ref` contemporaneously prevents weather from deciding the ratio |
| D9 | 2026-07-28 | Scope D5 to the layered fixture pending SWT0 Engine-scale counters | The 768-mutation fixture stays below autocheckpoint, while full-Engine rounds cross that WAL scale |
| D10 | 2026-07-28 | Scope the three-route invariant to client document mutations | Internal committer jobs, object metadata, and replica-cache reconciliation are real additional storage-writing surfaces |
| D11 | 2026-07-28 | Treat the forward-apply magnitude as a cross-run range | Accepted planning/audit runs report approximately 7–11.5%; SWT4 retains its ≥3% attributed end-to-end gate |
| D12 | 2026-07-28 | Freeze immutable `B_ref`/`F_ref` commits and use six predeclared balanced block pairs for final acceptance | Prevent mutable-main drift, post-hoc pairing, and adaptive stopping from influencing the 1.40 ratio |
| D13 | 2026-07-28 | Merge baseline observation seams before freezing `B_ref` | The exact `B_ref` binary must expose every checkpoint/resource counter required of the final paired comparison |

Append decisions; do not rewrite historical rows.

## Rejected-Candidate Ledger

| Candidate | Commit / branch | Measurement | Correctness result | Reason rejected | Proof |
| --- | --- | --- | --- | --- | --- |
| Weaken SQLite durability | none | not run | violates mission | FULL/WAL is fixed | research |
| Combine queued append/apply | none | not run | violates crash/replay topology | not an easy optimization | research |
| Unguarded SQL lower-bound behavior | diagnostic only | 171,088 vs guarded 151,485 | production correctness not proven | useful price signal, but not itself an acceptable implementation; SWT4 must replace rather than remove the guard | `proof/sqlite-write-throughput/layered-planning-reference.md` |
| Checkpoint tuning from layered evidence alone | none | 337–341 frames < 1,000 | fixture does not reach Engine scale | no fixture-scale bottleneck evidence; reassess after SWT0 counters | layered planning reference |

Every implementation experiment that fails a gate gets a row before its branch
is removed.

## Open Blockers

| Blocker | Owner | Unblock condition | Status |
| --- | --- | --- | --- |
| Implementation cannot begin until CTRL0 merges | CTRL0 | plan/harness PR merged and worktree cleaned | closed 2026-07-28: PR #241 squash-merged as `714f94437`; planning worktree/branch removed |
| Final hashed layered binary has no quiet-host accepted report | SWT0 | same-session report and binary hashes recorded with every lane CV≤10% | open; planning reference is non-acceptance evidence |
| Fresh hot-key/resource baseline not captured in exact SWT0 session | SWT0 | accepted CV≤10% reports under `swt0/` | open |
| `B_ref` cannot be frozen before observability merges | SWT0 | SWT0.1 fail-before and Engine-scale WAL/checkpoint counter seams merged to `origin/main`, then the exact post-merge commit frozen per D13 | open |
| SWT3 authorization | plan owner via measured ledger | relative-plus-absolute final target remains unmet after SWT2 and encoded ceiling ≥3% | gated |
| SWT4 implementation authorization | SWT4.1 attribution and design review | isolated safe projected end-to-end gain ≥3%, conditional forward guard designed, recovery retains full validation | planned |

## Phase CTRL0 — Canonical Research And Plan PR

### CTRL0.1 — Land the measurement/control plane

- **Scope / files:** this plan; the research addendum; plan README route;
  `crates/nimbus-engine/Cargo.toml`;
  `crates/nimbus-engine/benches/sqlite-write-overhead.rs`; proof root.
- **Prerequisites:** clean sibling worktree from latest `origin/main`; no
  competing plan owner; primary dirty checkout preserved.
- **Mechanism:** make the exact layered workload executable and freeze the
  path census, units, baseline, opportunity rank, gates, workflow, and
  paste-ready goal before optimization.
- **Success:** layered and Engine benchmarks compile; the clean planning
  reference and every rejected final-binary rerun are labeled with their
  actual provenance; docs agree on all numbers; README routes exactly one
  owner. A cryptographically bound quiet-host layered baseline remains an
  explicit SWT0 prerequisite, not a CTRL0 acceptance claim.
- **Correctness/regression:** the process-stable fixture table id keeps base
  and candidate payload bytes identical. An untimed production validation
  asserts exact live documents after every captured batch. Every measured
  production repetition then audits durable/applied heads, all 768 journal
  payloads, all 768 version rows and contents, the exact one-row table
  catalog, three metadata rows, and final live/index/resource emptiness; no
  production behavior changes.
- **Focused verification:**

  ```bash
  cargo fmt --all --check
  timeout 900 cargo bench -p nimbus-engine --bench sqlite-write-overhead --no-run
  timeout 900 cargo bench -p nimbus-engine --bench concurrent-write-throughput --no-run
  bash scripts/check-docs.sh
  bash scripts/verify-nimbus-docs-site.sh
  ```

- **Benchmark evidence:** the proof root contains the fixed workload's clean
  planning reference, its report hash and executable-provenance limitation,
  plus whole rejected final-binary reports and hashes. SWT0 accepts the first
  quiet-host same-session run whose binary/report hashes exist and whose every
  lane has CV≤10%.
- **Rollback/rejection:** do not merge if the layered lanes silently change
  durability, batch distribution, row shape, or metric units.
- **Proof:** `proof/sqlite-write-throughput/`.
- **Branch/PR/cleanup:** current plan branch/worktree; structured autoreview;
  open plan PR; hosted CI; merge; remove only this planning worktree and
  branch. Mark CTRL0 `complete`, then SWT0 `in_progress`.

## Phase SWT0 — Freeze Base And Fail-Before Diagnostics

### SWT0.1 — Deterministic fail-before and resource counters

- **Scope / files:** test-hook diagnostics beside
  `nimbus-storage/src/sqlite/{config,journal,document_versions,index_versions}.rs`;
  focused SQLite tests; avoid a new generic helpers module.
- **Prerequisites:** CTRL0 merged; clean P0 observability worktree; no other
  active task.
- **Mechanism:** add test-only counters or trace evidence for writer opens,
  statement preparation/execution by concept, format/schema/table checks, and
  current-document encodes. Add a resettable Engine-scale WAL/checkpoint
  observation seam that distinguishes automatic/foreground checkpoint work
  from the post-run passive probe. Timed benchmark mode keeps statement
  tracing off; checkpoint observation overhead must be measured and either
  disabled or proven negligible.
- **Success:** fail-before assertions demonstrate current repeated behavior:
  two recurring writer opens per queued batch; per-record format/schema/table
  checks; two current-document encodes where current storage uses both version
  and live projections. N=256 CRUD proof records checkpoint counts/time and WAL
  high-water frames at Engine scale.
- **Correctness/regression:** counters are test-only, thread-safe, resettable,
  and do not change release behavior.
- **Focused verification:**

  ```bash
  timeout 300 cargo nextest run -p nimbus-storage -E 'test(sqlite)'
  cargo fmt --all --check
  timeout 600 cargo clippy -p nimbus-storage --all-targets -- -D warnings
  ```

- **Benchmark acceptance:** release layered/full Engine results remain within
  the same-session 95% noise band with diagnostics disabled.
- **Rollback/rejection:** reject instrumentation that requires unsafe global
  callbacks, changes the production dependency surface without justification,
  or perturbs timed runs.
- **Proof:** `proof/sqlite-write-throughput/swt0/fail-before.md`.
- **Branch/PR/cleanup:** P0 observability branch/worktree and an
  observability-only PR. Merge and remove only that worktree/branch before
  freezing `B_ref`.

### SWT0.2 — Freeze base, hot-key, resources, and artifact identity

- **Scope / files:** benchmark/report support only; this ledger and
  `proof/sqlite-write-throughput/swt0/`. Production SQL behavior is unchanged.
- **Prerequisites:** SWT0.1 observability PR merged; clean, fetched
  `origin/main`; no other active task.
- **Mechanism:** freeze that exact post-observability source commit and the
  canonical protocol as `B_ref`, then build the exact commit once. Record the
  clean status, build command, binary hash, contention, RSS, cold-open, WAL,
  DB-size, runtime SQLite/SQLCipher, and report hashes. The hot-key N=256 lane
  intentionally saturates the 128-slot committer inbox; client backoff is part
  of its measured latency rather than a fatal harness error. Later A/B
  sessions rebuild and rerun `B_ref` rather than comparing to this run's
  number.
- **Success:** disjoint CRUD N=1/32/256, hot-key N=1/32/256, and layered lanes
  all have CV≤10%; raw samples and hashes exist; the ledger records `B_ref`,
  its diagnostic reference result, and why that result differs from the
  historical observation when it does.
- **Correctness/regression:** benchmark fixture final state and watermarks
  asserted; the frozen source exposes every counter required by SWT5; no
  production files change after the freeze.
- **Verification/benchmarks:** canonical commands above, plus peak RSS via
  `/usr/bin/time -l`; record cold open, Engine-scale WAL high-water frames,
  automatic/passive checkpoint counts and time, and the layered fixture's
  passive-checkpoint state.
- **Rollback/rejection:** rerun noisy lanes; never normalize away drift or
  substitute a historical sample. Any source or instrumentation change
  invalidates `B_ref`: merge the change, freeze the new exact commit, rebuild,
  and rerun the complete reference protocol.
- **Proof:** `proof/sqlite-write-throughput/swt0/{environment,crud,hotkey,layered,resources}.md`.
- **Branch/PR/cleanup:** P0 baseline-evidence branch/worktree and an
  evidence-only follow-up PR. Record the frozen commit and cleanup in the
  ledger; remove only this worktree/branch after merge.

## Phase SWT1 — Prepared Statements And Batch-Invariant Apply Context

### SWT1.1 — Reuse recurring statements within append/apply

- **Scope / files:** `nimbus-storage/src/sqlite/journal.rs`,
  `document_versions.rs`, `index_versions.rs`, and a concept-owned
  `sqlite/write_batch.rs` only if ownership/testability warrants extraction.
- **Prerequisites:** SWT0 merged; fail-before statement/prepare evidence saved.
- **Mechanism:** prepare once per transaction/batch or use the connection cache
  for commit-log insert, version insert/tombstone, live insert/update/delete,
  metadata, preimage, resource-binding, and index-version statements. Preserve
  dynamic index SQL ownership and automatic schema invalidation.
- **Success:** deterministic prepare counter falls to the documented bound;
  fixed fixture SQL rows/bytes/transactions are identical; combined SWT1
  primary A/B gate is met.
- **Correctness/regression tests:** insert/update/delete; idempotent replay;
  divergent preimage; indexed and schemaless tables; resource bindings;
  schema change invalidates/reprepares correctly; queued/direct/unit parity.
- **Focused verification:**

  ```bash
  timeout 300 cargo nextest run -p nimbus-storage -E 'test(sqlite) & (test(journal) | test(version) | test(index) | test(resource))'
  timeout 300 cargo nextest run -p nimbus-engine -E 'test(ordered_publisher) | test(execution_unit) | test(direct)'
  cargo fmt --all --check
  timeout 600 cargo clippy -p nimbus-storage -p nimbus-engine --all-targets -- -D warnings
  ```

- **Benchmark acceptance:** measure an internal statement-only checkpoint after
  this task, but accept/reject the PR only after SWT1.2. No lane may regress
  beyond cross-gates.
- **Rollback/rejection:** revert any cache whose ownership outlives its
  connection, masks `SQLITE_SCHEMA`, or complicates errors without measurable
  benefit.
- **Proof:** `proof/sqlite-write-throughput/swt1/statement-reuse.md`.
- **Branch/PR/cleanup:** P1 branch/worktree; one combined SWT1 PR.

### SWT1.2 — Hoist only proven batch invariants

- **Scope / files:** same storage batch owners; focused tests. Keep
  `journal.rs` a composition root if extraction is needed; do not create
  `helpers.rs`, `common.rs`, `misc.rs`, or `utils.rs`.
- **Prerequisites:** SWT1.1 checkpoint measured; read batch callers and schema/
  lifecycle ordering before editing.
- **Mechanism:** construct a batch apply context that:
  - validates document-version format once per apply transaction;
  - loads schemaless/schema/index plans once per distinct table at the correct
    sequence boundary;
  - validates each distinct `(table, table_id)` once unless an event in the
    same batch changes its epoch/lifecycle;
  - retains one preimage check per write and all resource/index effects.
  Preserve route-specific validation placement: queued/direct work validates
  during prepare and conflict-mediated reprepare, while execution units retain
  serial write-set validation. Do not describe or implement one uniform
  serial schema-validation step.
- **Success:** the fixed one-table fixture reduces source/traced statements
  from the current bound toward the replay-guarded 3,401 bound; multi-table and
  schema/lifecycle batches prove correct invalidation; N=256 paired mean gain
  is at least 5% and paired-delta lower CI is positive.
- **Fail-before:** tests asserting one check per distinct invariant key must
  fail on the pre-change tree and pass after implementation; save both outputs.
- **Correctness/regression tests:**
  - durable append/apply, torn-tail recovery, duplicate/idempotent apply;
  - schema change followed by document write in the same and next batch;
  - table lifecycle/identity reuse and mismatch rejection;
  - maintained index open/close intervals;
  - `fanout_never_precedes_applied_head`;
  - `publisher_preserves_sequence_order_across_transient_retry`;
  - `publisher_torn_tail_recovery_replays_exactly_one_contiguous_prefix`;
  - `publisher_accumulator_preserves_fsync_amortization_when_assignment_gets_ahead`;
  - direct and `MutationExecutionUnit` route parity.
- **Benchmarks:** canonical layered + CRUD + hot-key A/B. Capture statement
  counts, phase split, batch, bytes/frames, RSS, and cold open.
- **Rollback/rejection:** reject the candidate if gain is below 5%, paired CI
  includes zero, any invariant needs speculative cache invalidation, or any
  cross-gate fails. Revert production changes but retain proof/rejected row.
- **Proof:** `proof/sqlite-write-throughput/swt1/{fail-before,ablation,crud,hotkey,resources,correctness}.md`.
- **Branch/PR/cleanup:** structured autoreview, push P1, hosted CI, merge only
  on PASS, then remove only P1 worktree/branch and update ledger.

## Phase SWT2 — Resident Embedded Writer

### SWT2.1 — Split initialization and own one writer connection

- **Scope / files:** `nimbus-storage/src/sqlite/config.rs`; a concept-owned
  `sqlite/writer.rs` if required; `SqliteTenantStore`; encrypted-open tests.
- **Prerequisites:** SWT1 merged/cleaned; fresh base rebased on main;
  fail-before open counts from SWT0.
- **Mechanism:** give one per-tenant storage owner a reusable writer connection
  and its statement cache. Separate one-time file/schema initialization from
  per-connection safety/key setup. Serialize the three client document routes
  under existing Engine ordering without introducing a second resident writer
  or broadening their concurrency. The resident owner must coexist safely with
  existing non-committer write transactions, including object-manifest writes
  and libSQL replica-cache reconciliation; it is not the database's sole
  writer.
- **Success:** recurring queued append/apply batches do not reopen/re-run full
  schema initialization; direct and execution-unit writes use the same owner;
  encrypted stores still key, harden temp storage, and verify correctly.
- **Fail-before:** a deterministic open-count test records current repeated
  opens and fails the resident bound before implementation.
- **Correctness/regression:** plain/encrypted open; poison/fatal error recovery;
  busy/locked error classification; connection disposal/reopen; read-pool
  independence; deterministic overlap tests with object-manifest and
  replica-reconciliation write transactions; no starvation and no lock held
  across Engine publication/fan-out.
- **Focused verification:**

  ```bash
  timeout 300 cargo nextest run -p nimbus-storage -E 'test(sqlite) & (test(open) | test(encrypt) | test(journal) | test(recover))'
  timeout 300 cargo nextest run -p nimbus-engine -E 'test(ordered_publisher) | test(direct) | test(execution_unit)'
  cargo fmt --all --check
  timeout 600 cargo clippy -p nimbus-storage -p nimbus-engine --all-targets -- -D warnings
  ```

- **Benchmark acceptance:** production storage paired gain at least 5%;
  either N=1 or N=32 paired gain at least 5%; N=256 no regression over 2%;
  cold store open no worse than cross-gate.
- **Rollback/rejection:** reject ownership that can deadlock read snapshots,
  broadens the critical section, shares a connection unsafely, or lacks clean
  recovery. Retain existing open-per-write behavior if performance gate fails.
- **Proof:** `proof/sqlite-write-throughput/swt2/writer-owner.md`.
- **Branch/PR/cleanup:** P2 branch/worktree and PR.

### SWT2.2 — Preserve both queued commits and all route semantics

- **Scope / files:** queued storage append/apply callers; direct
  `apply_prepared_write_batch`; execution-unit storage seam; fault tests.
- **Prerequisites:** SWT2.1 owner implemented locally.
- **Mechanism:** transact twice on the same connection for queued batches and
  once for direct/execution units. Reset/rollback cleanly on every error;
  preserve ambiguous-outcome recovery and publication order.
- **Success:** failpoint observation still exposes durable-unapplied state
  between queued commits; direct/execution unit remains atomic; connection
  counters and all performance gates pass.
- **Correctness tests:**
  - journal append before visibility;
  - append succeeds/apply fails/restart replays once;
  - torn tail applies exactly one contiguous prefix;
  - dense sequence/no holes across retry;
  - queued/direct/execution-unit serialization;
  - object-manifest and replica-reconciliation writers make progress while
    contending with the resident owner;
  - fan-out only after applied head;
  - kill-switch state equivalence;
  - encrypted restart/recovery.
- **Benchmarks:** canonical layered/CRUD/hot-key; additionally compare
  connection profile and transaction/s to prove topology is unchanged.
- **Rollback/rejection:** any one-commit queued trace, watermark inversion,
  ambiguous result divergence, or unexplained batch/RSS/WAL regression rejects
  the candidate.
- **Proof:** `proof/sqlite-write-throughput/swt2/{fail-before,crud,hotkey,connection-profile,crash-recovery,resources}.md`.
- **Branch/PR/cleanup:** same P2 PR; autoreview; hosted CI; merge and clean only
  P2 state; decide SWT3 from the ledger.

## Phase SWT3 — Reusable Encoded Persistence Payload (Conditional)

### SWT3.1 — Re-measure and decide

- **Scope / files:** no production edits; ledger and
  `proof/sqlite-write-throughput/swt3/decision.md`.
- **Prerequisites:** SWT2 merged and final-target protocol rerun.
- **Mechanism:** measure record-only MessagePack, fields JSON, typed-field JSON,
  preimage decode, clone/allocation, and ordered-publisher/storage shares on
  the optimized tree.
- **Success:** mark SWT3 `rejected` if N=256 already meets the final
  relative-plus-absolute target or the paired end-to-end opportunity is below
  3%. Mark implementation `in_progress` only if the target remains unmet and a
  ≥3% safe ceiling is demonstrated.
- **Correctness:** measurement-only; no format changes.
- **Verification/benchmark:** canonical protocols plus CPU component ablation.
- **Rollback/rejection:** complexity without ≥3% measured safe headroom is a
  rejection, recorded rather than implemented.
- **Proof:** `proof/sqlite-write-throughput/swt3/decision.md`.
- **Branch/PR/cleanup:** P3 worktree only if decision needs committed benchmark
  support; otherwise record rejection in P5 closeout.

### SWT3.2 — Carry one internal encoded representation

- **Scope / files:** concept owner around prepared commit/storage payload;
  `nimbus-core` only for protocol-neutral types; Engine preparation; SQLite
  version/live/journal consumers; no public API or storage-format change.
- **Prerequisites:** SWT3.1 explicitly authorizes implementation.
- **Mechanism:** produce deterministic encoded fields/typed fields/record blob
  once at the narrowest existing preparation boundary and borrow/reuse it
  through persistence. Bound lifetime to one commit/batch.
- **Success:** encode counters meet the documented once-per-value bound; paired
  N=256 gain at least 3% with positive lower CI; memory cross-gate passes.
- **Fail-before:** encoding-count assertion fails on the pre-change tree.
- **Correctness/regression:** byte-for-byte canonical record serialization;
  integrity SHA validation; typed values; large/nested payloads; update/delete;
  direct/execution-unit parity; retry/reprepare never reuses stale encoding;
  crash/replay remains compatible.
- **Focused verification:** focused nimbus-core/storage/engine tests; fmt;
  focused clippy; canonical benchmarks; `make clippy` and `make ci` before PR.
- **Rollback/rejection:** reject any storage-format change, duplicated
  long-lived representations, stale retry payload, >10% RSS growth, or gain
  below 3%.
- **Proof:** `proof/sqlite-write-throughput/swt3/{fail-before,encoding,crud,hotkey,resources,correctness}.md`.
- **Branch/PR/cleanup:** P3 branch/worktree; autoreview; hosted CI; merge on
  PASS; otherwise revert and record rejection; clean only P3 state.

## Phase SWT4 — Guarded Forward Apply

The combined guarded-to-lower-bound delta is material but run-sensitive:
accepted planning and independent-audit runs report approximately 7–11.5% of
guarded lower-layer elapsed. It is not yet attributed: the lower-bound lane
removes both the live preimage query and delete-side resource-binding cleanup,
and it does not perform production's Rust deserialization/equality comparison.
SWT4 therefore starts with measurement and design, even if SWT1–SWT3 already
meet the final target.

### SWT4.1 — Attribute preimage read, decode/compare, and binding cleanup

- **Scope / files:** benchmark/test-hook instrumentation around
  `nimbus-storage/src/sqlite/journal.rs`, `backend.rs`, and resource-path
  cleanup; proof only until the implementation gate passes.
- **Prerequisites:** SWT2 merged; SWT3 accepted or rejected; exact optimized
  base captured; no other active row.
- **Mechanism:** add separate ablation/counters for:
  - SQLite preimage query/row materialization;
  - JSON and typed-field deserialization;
  - Rust document equality comparisons;
  - delete-side resource locator encoding and binding `DELETE`;
  - insert, update, and delete independently.
- **Success:** report isolated time and paired CIs for every component, plus a
  production-storage and full-Engine Amdahl projection. The preimage candidate
  proceeds only if its safe projected end-to-end gain is at least 3% with a
  positive lower confidence bound. Binding cleanup becomes its own candidate;
  it is never silently removed.
- **Correctness/regression:** instrumentation off in normal release runs;
  fixture behavior and durable byte shape unchanged.
- **Focused verification:**

  ```bash
  timeout 300 cargo nextest run -p nimbus-storage -E 'test(sqlite) & (test(journal) | test(resource))'
  cargo fmt --all --check
  timeout 600 cargo clippy -p nimbus-storage --all-targets -- -D warnings
  ```

- **Benchmark acceptance:** canonical layered protocol plus operation-specific
  insert/update/delete samples, CV≤10%, raw samples/hashes recorded.
- **Rollback/rejection:** if no isolated safe mechanism projects to ≥3%
  end-to-end, mark SWT4 implementation `rejected` and continue to SWT5; retain
  the attribution proof.
- **Proof:** `proof/sqlite-write-throughput/swt4/attribution.md`.
- **Branch/PR/cleanup:** P4 branch/worktree. An attribution-only result may
  merge with final evidence; production implementation requires SWT4.2.

### SWT4.2 — Design a conditional forward-apply guard

- **Scope / files:** a narrow storage-owned design beside SQLite journal/apply
  code; plan decision log; corruption/replay tests before production changes.
- **Prerequisites:** SWT4.1 meets the ≥3% projected safe-gain gate.
- **Mechanism:** replace forward-path full document reconstruction with a
  conditional SQL predicate that still proves the expected state—for example a
  canonical expected-preimage hash/version token plus affected-row check.
  Recovery, duplicate replay, ambiguous outcome, and any non-forward apply keep
  the full load/deserialization/equality validation.
- **Required design properties:**
  - no trust in Engine memory alone;
  - no unconditional update/delete;
  - zero affected rows distinguishes missing/mismatched preimage and falls back
    to full validation for a typed corruption/conflict result;
  - idempotent already-current/already-deleted replay remains distinguishable;
  - hash/token derivation is canonical and collision posture is documented;
  - no storage-format change unless separately promoted outside this plan;
  - resource-path cleanup remains correct and separately measured.
- **Success:** an explicit state table covers insert/update/delete × expected,
  already-current, missing, mismatched, duplicate, recovery, and ambiguous
  outcome cases. A fail-before test demonstrates current full reconstruction;
  new tests fail until the conditional mechanism exists.
- **Correctness verification:** adversarial design review plus structured
  autoreview before implementation proceeds; fault-injection and generated
  history cases named in proof.
- **Rollback/rejection:** reject designs needing a new persisted format,
  probabilistic-only correctness, removal of recovery validation, or a fourth
  mutation path.
- **Proof:** `proof/sqlite-write-throughput/swt4/design-and-fail-before.md`.
- **Branch/PR/cleanup:** same P4 worktree; record an explicit proceed/reject
  decision in D6.

### SWT4.3 — Implement and prove guarded forward apply

- **Scope / files:** the narrow SQLite apply owner selected by SWT4.2; focused
  storage/Engine tests; no transaction, journal, concurrency, or format change.
- **Prerequisites:** SWT4.2 PASS and proceed decision recorded.
- **Mechanism:** execute the conditional forward write, inspect affected rows,
  and take the full validation path on every ambiguous/non-matching state.
  Recovery/replay always retains the canonical full validation path.
- **Success:** paired full-Engine N=256 gain at least 3% with positive lower CI;
  operation-specific improvement matches attribution; every cross-gate passes.
- **Correctness/regression tests:**
  - expected update/delete applies once;
  - wrong/missing preimage is rejected with the same typed semantics;
  - already-current and duplicate replay remain idempotent;
  - append success/apply failure/restart validates and replays exactly once;
  - torn-tail and divergent durable record detection;
  - queued/direct/execution-unit parity;
  - fan-out after applied head;
  - resource binding present/absent deletion;
  - randomized/generated histories with restart points.
- **Focused verification:** full storage SQLite suite; focused Engine journal,
  publisher, direct, execution-unit, and fan-out groups; canonical
  layered/CRUD/hot-key/resource protocols; fmt and focused clippy.
- **Rollback/rejection:** any semantic divergence, fallback ambiguity,
  collision/format concern, or gain below 3% rejects and reverts production
  code. Record the result and continue to SWT5.
- **Proof:** `proof/sqlite-write-throughput/swt4/{attribution,design-and-fail-before,correctness,crud,hotkey,resources}.md`.
- **Branch/PR/cleanup:** autoreview; hosted CI; merge only on PASS; otherwise
  revert and record rejection. Remove only P4 worktree/branch after
  disposition.

Combining queued transactions, removing preimage protection, weakening
recovery validation, or trusting Engine memory alone remains prohibited.

## Phase SWT5 — Final Acceptance, Archive, And Cleanup

### SWT5.1 — Final performance and resource acceptance

- **Scope / files:** benchmark/report docs and only fixes required by a failing
  in-scope gate.
- **Prerequisites:** SWT4 disposition recorded; every accepted candidate
  merged; no candidate worktree active; exact post-merge `F_ref` frozen before
  any P5 evidence/doc edit.
- **Mechanism:** build immutable `B_ref` and `F_ref` binaries in separate clean
  worktrees, then execute the six-pair balanced final block protocol exactly as
  specified above. Record every report and the derived pair table.
- **Success:**
  - mean of the paired N=256 `F_ref`/`B_ref` ratios ≥1.40;
  - `F_ref` N=256 mean ≥30,000;
  - lower 95% confidence bound of the paired percentage delta is positive;
  - final N=256 lower 95% CI ≥28,000;
  - all CV≤10%;
  - N=1/N=32/hot-key/latency/batch/RSS/DB/WAL/Engine-scale checkpoint/
    cold-open gates pass;
  - layered ledger reports final unit rates and overhead retention;
  - below-saturation open-loop companion documents service latency without
    coordinated-omission claims.
- **Correctness/regression:** fixture row counts/watermarks, direct/unit
  coverage, crash/replay, and exact durable byte shape recorded.
- **Verification:** canonical benchmarks and hashes; focused tests; commands
  under SWT5.2.
- **Rollback/rejection:** a final target miss is not papered over. Record the
  miss and request an owner decision on scope or target; do not weaken gates.
- **Proof:** `proof/sqlite-write-throughput/final/{environment,crud,hotkey,layered,open-loop,resources}.md`.
- **Branch/PR/cleanup:** P5 branch/worktree; one final evidence/docs PR.

### SWT5.2 — Enterprise correctness closeout

- **Scope / files:** all touched code/tests/docs, this plan, research ledger,
  proof, README/archive routing.
- **Prerequisites:** SWT5.1 performance PASS.
- **Mechanism:** execute the complete proportional verification set and review
  the diff against every non-negotiable invariant.
- **Required focused behaviors:**
  - queued/direct/execution-unit route parity;
  - atomic journal/document/index/watermark effects;
  - durable-unapplied recovery and torn-tail handling;
  - dense sequencing across transient/ambiguous errors;
  - fan-out after applied visibility;
  - schema/index/table lifecycle invalidation;
  - encrypted store open/reuse/restart;
  - deterministic concurrency and kill-switch equivalence.
- **Commands:**

  ```bash
  cargo fmt --all --check
  timeout 900 cargo bench -p nimbus-engine --bench sqlite-write-overhead --no-run
  timeout 900 cargo bench -p nimbus-engine --bench concurrent-write-throughput --no-run
  timeout 600 cargo nextest run -p nimbus-storage
  timeout 600 cargo nextest run -p nimbus-engine -E 'test(journal) | test(publisher) | test(direct) | test(execution_unit) | test(fanout)'
  make clippy
  make ci
  bash scripts/check-docs.sh
  bash scripts/verify-nimbus-docs-site.sh
  ```

- **Success:** name test counts/results in proof; structured autoreview has no
  unresolved P0/P1/P2; hosted CI passes; final decision table is complete.
- **Rollback/rejection:** fix root causes. Do not suppress warnings, weaken
  tests, delete failing cases, or call skipped provider lanes passing.
- **Proof:** `proof/sqlite-write-throughput/final/{correctness,verification,autoreview,hosted-ci}.md`.
- **Branch/PR/cleanup:** autoreview; push; PR; hosted CI; merge.

### SWT5.3 — Archive and leave no campaign residue

- **Scope / files:** this plan, plan README, research and proof index.
- **Prerequisites:** final PR merged.
- **Mechanism:** mark every row complete/rejected, move this file to
  `archive/sqlite-write-throughput-optimization-plan.md`, update README to one
  concise completed route, and verify local/GitHub cleanup.
- **Success checklist:**
  - [ ] all accepted PRs merged and commits/PRs recorded;
  - [ ] rejected experiments recorded with proof;
  - [ ] no `in_progress` row;
  - [ ] plan archived and README route updated;
  - [ ] no campaign worktree remains;
  - [ ] no unmerged local or remote `codex/sqlite-write-throughput-*` branch;
  - [ ] no temporary benchmark DB/report outside committed proof;
  - [ ] primary checkout has exactly its explicitly preserved pre-existing
    user work and no campaign edits;
  - [ ] GitHub has no open campaign PR or failed required check;
  - [ ] final binary/report hashes and hosted CI links are recorded.
- **Verification:**

  ```bash
  git worktree list
  git branch --list 'codex/sqlite-write-throughput-*'
  git ls-remote --heads origin 'codex/sqlite-write-throughput-*'
  git status --short
  ```

- **Rollback/rejection:** never delete an unrelated branch/worktree or
  pre-existing user file. Stop and record ambiguity instead.
- **Proof:** `proof/sqlite-write-throughput/final/cleanup.md`.
- **Branch/PR/cleanup:** cleanup follows final merge; no cleanup-only branch
  should remain.

## Session Discipline

At the start of every work session:

1. read this plan, the research document, README route, latest proof, git
   status, worktree list, and open PR/check state;
2. identify the sole `in_progress` row;
3. if no row is active, promote only the earliest unblocked row;
4. update branch/worktree/commit/PR state before editing;
5. read every production file, test, and caller to be changed.

Before ending, handing off, or likely compaction:

1. update the status, baseline/accepted, rejected, decision, blocker, and
   cleanup ledgers;
2. save raw measurements and hashes under the proof root;
3. state the exact next command/action;
4. never leave two tasks `in_progress`.

## Paste-Ready Campaign Goal

```text
/goal Execute the complete SQLite write-throughput optimization campaign
owned by docs/private/plans/sqlite-write-throughput-optimization-plan.md.

First load AGENTS.md, the plan, its linked research
docs/private/plans/research/sqlite-write-overhead-and-opportunities-2026-07.md,
the proof root, docs/private/plans/README.md, current git/worktree/branch/PR
state, and the durable status ledger. Resume the single in_progress row; do
not select a second task or create a competing plan owner.

The control-plane PR must be merged before implementation. For each subsequent
concept use a fresh clean origin/main branch
codex/sqlite-write-throughput-p<N>-<concept> and sibling worktree
/Users/jack/src/github.com/nimbus/nimbus-sqlite-write-throughput-p<N>. Verify
that neither exists before creation. Never disturb pre-existing user work in
/Users/jack/src/github.com/nimbus/nimbus. After each PR merges, remove only
that PR's worktree and local/remote branch.

Establish and preserve the exact layered and complete-Engine baselines using
the fixed rusqlite/SQLCipher build, WAL, synchronous=FULL, identical payload,
captured batches, release profile, N=1/32/256 disjoint CRUD and hot-key lanes,
raw samples, Student-t 95% confidence intervals, CV≤10%, artifact hashes,
hardware/OS, phase/batch/latency, SQL statements, row changes, transactions,
sync commits, RSS, database/WAL/checkpoint, and cold-open evidence. Never call
one raw row change one Nimbus logical mutation. Install and merge every
baseline checkpoint/resource observation seam before freezing the exact SWT0
source commit and protocol as `B_ref`; treat its SWT0 number as a diagnostic
reference, and rerun `B_ref` contemporaneously in every final A/B session.
After the last production PR merges, freeze exact clean `origin/main` as
`F_ref`, build both commits once, and run the six predeclared balanced block
pairs without adaptive stopping or post-hoc round pairing.

Implement only measured, low-risk, concept-owned optimizations in plan order:
SWT1 prepared-statement reuse plus batch-invariant format/schema/table checks;
SWT2 an actor-owned resident embedded writer connection while retaining the
queued append COMMIT then apply COMMIT; SWT3 encoded-payload reuse only if its
post-SWT2 decision gate proves at least 3% safe remaining headroom and the
relative-plus-absolute target is unmet. Run SWT4's attribution phase even if
the target is already met; implement guarded forward apply only when the
isolated safe candidate projects at least 3% end-to-end gain and its
conditional-write, fallback, corruption, and full-recovery-validation design
passes. Never simply remove the preimage guard or resource-binding cleanup.
Do not change transaction, journal, concurrency, durability, or storage-format
semantics.

For every behavioral optimization capture fail-before evidence, read source,
tests, and callers before editing, preserve queued/direct/MutationExecutionUnit
semantics, run the focused correctness/crash/concurrency gates, and A/B base
versus candidate in the same quiet session. Retain a candidate only when its
paired performance lower confidence bound is positive, it meets the task's
minimum gain, and all throughput, latency, batch, contention, RSS, DB/WAL,
checkpoint, cold-start, durability, recovery, ordering, and publication gates
pass. Otherwise revert the production change, record the rejection and proof,
clean its campaign branch/worktree after disposition, and continue only as the
plan permits.

Update the research, benchmark ledger, status table, decision log,
accepted/rejected candidate tables, blockers, and proof artifacts before and
after every material session, before handoff, and before likely compaction.
Keep exactly one in_progress row. Run cargo fmt --all --check, focused tests
and clippy during development, make clippy and make ci before PR when feasible,
structured autoreview before every push/PR, and treat hosted CI as merge truth.
Fix root causes; never suppress or weaken gates.

Finish only when the six-pair same-session mean N=256 `F_ref`/`B_ref` ratio is
at least 1.40, `F_ref` reaches at least 30,000 durable logical CRUD mutations/s
with lower 95% CI at least 28,000, the paired-delta lower 95% CI is positive,
and every plan gate passes; all approved PRs are merged; the final research
and proof are reproducible; the plan is archived; all campaign worktrees,
merged branches, temporary databases, and untracked artifacts are removed;
the primary checkout and GitHub are clean except for explicitly preserved
pre-existing user work. Leave Nimbus with a canonical implementation,
trustworthy overhead accounting, durable regression protection, and evidence
suitable for enterprise capacity decisions. If the target remains unmet after
all authorized low-risk work, record the exact blocker and request the owner
decision required by the plan; do not weaken durability, correctness, or
measurement gates.
```
