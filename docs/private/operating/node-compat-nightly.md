# Node Compatibility Nightly

This runbook owns the `Node Compatibility` workflow
(`.github/workflows/node-compat-nightly.yml`) and the recorded corpus baseline
that its Rust lane reads.

## What each job answers

The workflow has four jobs, and each one answers a single question. Keep them
separate. A job that answers two questions fails for the first one and never
reports the second.

| Job | Question | Red means |
| --- | --- | --- |
| `rust-corpus` | Does measured Node behavior match the recorded baseline? | A regression, or an improvement that is not recorded yet |
| `corpus-baseline-reconciliation` | Do the ignored watchpoints still describe reality? | A cataloged watchpoint passed, or the baseline is invalid |
| `release-train-freshness` | Is the vendored Node inventory current? | Upstream published a release, or a vendored fixture drifted |
| `node-compat-evidence` | What is the measured compatibility posture? | The dashboard, trends, or evidence artifact could not be produced |

`release-train-freshness` goes red on upstream's schedule, not on ours. Node
publishes patch releases continuously. That job must therefore never gate
measurement. It ran as step 4 of 11 inside the evidence job until 2026-09-16,
when a routine patch release (`v24.21.0`, `v26.8.2`) discarded a whole night of
compatibility evidence.

## The recorded corpus baseline

`tests/runtime/node/expectations/corpus-baseline.json` records which vendored
upstream fixtures are known to fail in the non-ignored Rust lane.

The vendored corpus is aspirational: it carries the whole upstream Node test
suite for four lanes, and much of it exercises APIs that the runtime does not
implement yet. A lane that requires every fixture to pass can never go green,
and a permanently red lane cannot report a regression, because a regression
looks exactly like the background failure.

The document holds five lane keys: `node20`, `node22`, `node24`, `node26`, and
`unspecified`. The last one holds the fixtures that are vendored once and run
without a declared lane. It is a key like any other, and the document always
states all five, even when a lane records nothing.

Every entry names three things: the fixture path inside the runtime bundle
(`test_relative_path`), where the fixture is vendored
(`fixture_source_relative_path`), and the observed `reason`.

The Rust seam
(`crates/nimbus-runtime/src/runtime/tests/node/corpus_baseline.rs`) compares
every observed result with the record:

| Recorded | Observed | Result |
| --- | --- | --- |
| no | pass | lane green |
| no | fail | **lane red** — a regression |
| yes | fail | lane green, counted as a known gap |
| yes | pass | **lane red** — remove the entry to record the improvement |

Three properties make this a gate rather than a mute button:

1. **The record only shrinks through a reviewed change.** An improvement fails
   the lane until a human removes the entry, so progress is visible in the diff
   and cannot silently reverse.
2. **A recorded gap is never a pass.** Batch summaries count it in a separate
   `known gaps` column, and every report mapper still classifies it as a
   measured failure. The published pass rate keeps measuring real Node
   compatibility.
3. **A required-surface fixture can never be recorded.** `make
   node-compat-baseline-verify` rejects any entry whose fixture has
   `support_denominator == v8_isolate_required` in
   `docs/private/architecture/runtime/node-default-support-posture.json`. The
   required surface is the gate that already means something, and a baseline
   entry must not retire it.
4. **Every recorded gap names a file that exists.** The seam writes the
   vendored path it actually read, and both guards resolve that path under
   `crates/nimbus-runtime/src/runtime/tests/node_compat_fixtures`. A test that
   supplies its own source writes no path, so the baseline cannot absorb it.

   Do not derive the vendored path from `test_relative_path`. The two differ.
   A lane vendors some fixtures under its own directory, the shared tree
   vendors others once at the fixture root, and a regression fixture can carry
   a different file name. Only the seam knows which one it read.

## Refreshing the baseline

Never hand-edit the baseline. Every entry must come from a real run.

1. Run the corpus with instrumentation. In CI, run the workflow through
   `workflow_dispatch` and download the `node-compat-observed-results`
   artifact. Locally:

   ```bash
   NIMBUS_NODE_COMPAT_OBSERVED_RESULTS=target/node-compat/observed/local.jsonl \
     cargo nextest run -p nimbus-runtime --lib runtime::tests::node_compat:: \
     --test-threads 1 --no-fail-fast
   ```

   The variable names a JSONL file, and each fixture appends one object.
   nextest runs every test in its own process, so an in-memory buffer would not
   survive.

   A relative path resolves against the repository root, not the working
   directory. Cargo and nextest run a test binary from the package root, so an
   unanchored path would write the shard under `crates/nimbus-runtime/target/`
   where no collector looks.

2. Merge the shards:

   ```bash
   make node-compat-baseline-aggregate \
     OBSERVED_SHARDS=target/node-compat/observed \
     OBSERVED_RESULTS=target/node-compat/observed-results.json
   ```

3. Rewrite the baseline:

   ```bash
   make node-compat-baseline-refresh \
     OBSERVED_RESULTS=target/node-compat/observed-results.json
   ```

   The refresh refuses an observed-results document that does not cover every
   lane. Rewriting a lane from a run that never executed it would empty that
   lane's baseline and turn every future failure there into a fresh regression.
   Pass `LANES="node20 node22"` only when you intend a partial rewrite and the
   run covered exactly those lanes.

4. Review the diff. Removed entries are improvements, and they belong in the
   commit message. Added entries are regressions, and they need a cause before
   they are recorded.

The refresh reports, and does not record, two kinds of observed failure:

| Kind | Why it is not recordable |
| --- | --- |
| required surface (`v8_isolate_required`) | The gate that already means something must stay intact. Fix the runtime. |
| a result with no `fixture_source_relative_path` | The test supplied its own source. A synthetic `__nimbus-` probe tests Nimbus behavior, not upstream compatibility. Fix the probe or the runtime. |
| a recorded path that is not vendored | The named file is absent from the fixture tree. Vendor it, or fix the test that points at nothing. |

Both keep the lane red until the runtime changes, which is the intended
behavior. The refresh skips them so that it cannot write a baseline that
`verify` then rejects.

`make node-compat-baseline-verify` runs in the PR lane, so a stale or invalid
entry fails before merge, not at night.

## Reading a red `rust-corpus`

The failure names the fixture and the lane. Two messages matter:

- `upstream node_compat fixture ... should execute: ...` with no baseline
  mention is an unrecorded failure. Treat it as a regression: find the change
  that caused it. Record it only if the failure is an accepted, explained gap.
- `... is recorded as a known gap for lane ... but it passed` is good news. A
  gap closed. Remove that entry from the baseline in the same PR as the fix.

The `known gap` lines in a batch summary list what the baseline is currently
absorbing. That list is the compatibility backlog.

## When a partition dies

The corpus runs in six partitions. Each one writes two shards:
`partition-<i>.jsonl` from the main run, and `watchpoints-<i>.jsonl` from the
ignored-test run. A partition that dies before its tests start writes neither.

A missing shard is the dangerous failure, because the fixtures of that
partition are absent rather than reported. An unmeasured fixture then reads
exactly like a fixture with no finding. A baseline seeded from that
measurement leaves those fixtures unrecorded, and their next failure reads as
a fresh regression.

Two guards prevent it:

| Guard | Where | What it does |
| --- | --- | --- |
| `if-no-files-found: error` | the corpus upload step | fails the partition that measured nothing |
| `--expect-partitions 6` | the merge step | refuses a set that is missing any shard |

`EXPECT_PARTITIONS` must stay equal to the size of the corpus matrix. Change
both together.

The usual cause is the compiler cache. `RUSTC_WRAPPER: sccache` makes every
cargo command depend on the sccache server, and that server depends on the
Actions cache service. When the service returns 503, cargo exits in under a
second. The `Confirm sccache can serve this job` step retries three times,
then clears `RUSTC_WRAPPER` and writes a warning annotation. The job then
builds without the cache, which is slower but still produces the measurement.

When the merge refuses an incomplete set, rerun the corpus. Do not seed from
a partial measurement.

## When a batch is cut short

One batch test executes hundreds of upstream fixtures in a single process. The
test runner kills a test that outruns its timeout, and every fixture the batch
already measured is already in the shard. A kill therefore removes the rest of
the batch from the measurement without removing the batch, and nothing in the
shard says so.

That failure is silent and it moves. Run 35167962571 recorded 6930 fixtures and
run 35171841643 recorded 6908, with 99 fixtures only in the first and 77 only
in the second, in contiguous alphabetical groups. About 32 batch tests had been
killed at 135 seconds. A baseline seeded from one such run records where the
kill landed, and the next run reaches further and reports the fixtures behind
the old kill point as fresh regressions.

Two guards prevent it:

| Guard | Where | What it does |
| --- | --- | --- |
| `slow-timeout = { period = "10m", terminate-after = 1 }` | `.config/nextest.toml`, for `runtime::tests::node_compat::` | gives a batch the time it needs. The largest batch holds 312 fixtures and needs about three minutes |
| `batch_start` and `batch_complete` records | `corpus_baseline.rs`, checked by `corpus_baseline.py aggregate` | refuses a measurement in which any batch started and did not finish |

A batch writes one record when it starts and one when its fixture loop ends.
The end also carries how many fixtures the loop executed, so a shard that lost
records is refused as well. The timeout reduces how often the refusal fires; it
does not hide the refusal.

When the merge refuses a truncated measurement, read which batch was cut short,
then rerun the corpus. Do not seed from it. If the same batch is cut short
again, the batch outgrew its bound: measure it, then raise the bound in
`.config/nextest.toml` in a reviewed change.

## Prerequisites and cleanup

The corpus lane needs the Rust toolchain cache and `cargo-nextest`. The
reconciliation, freshness, and baseline commands need only `git` and
`python3`. The observed-results JSONL and the merged document are build
artifacts under `target/node-compat/`; `make clean` owns them, and nothing
outside `target/` is written.
