# SIC1 — Atomic object conditions decided by the commit authority

Base: `main` @ `310007166fc8227eb3e73c7d993727f146640dd8`.
Machine: darwin 24.6.0, aarch64, rustc stable toolchain.
Production diff: 11 files, +873 / −104 (`git diff --stat -- crates`).

## 1. The defect

`nimbus-s3` decided every S3 write precondition against its own read:

```
put_object: get_manifest -> verify_write_preconditions -> put_manifest
```

`put_manifest` carried no expected state, so the decision and the write were
two independent trips to the tenant committer. Two concurrent
`If-None-Match: *` creates both observed the absent key and both committed.
The existing test `conditional_requests_enforce_s3_etag_preconditions` is
sequential and could never see it.

## 2. The change, by seam

| Seam | Owns |
|---|---|
| `nimbus-storage/src/traits/object_metadata.rs` | `ObjectExpectedState` (the typed clause) and `ObjectConditionOutcome` (committed or rejected). No `Default`: a writer cannot omit its expected state by silence. |
| `nimbus-engine/src/engine/objects.rs` | `evaluate_object_condition`, called inside `commit_object_meta_write_in_actor` against the actor's own read and **before** `let sequence = SequenceNumber…`. |
| `nimbus-s3/src/backend.rs` | `S3ObjectMeta::put_manifest_conditional`. The unconditional `put_manifest` is **removed**, not deprecated. |
| `nimbus-s3/src/service.rs` | Wire policy only: ETag syntax and the RFC 9110 strong/weak reduction into opaque-value clauses. It no longer reads the manifest before writing. |

`put_manifest_unconditional` (free fn in `nimbus-s3`, inherent method on
`TenantObjectMeta`) names the unconditional write explicitly instead of letting
it be the default.

### Weak `If-Match` is decided on the wire, not against state

Stored ETags are always `ETag::Strong` (`manifest_etag`). RFC 9110 requires
strong comparison for `If-Match`, so a weak `If-Match` can never match any
stored value. `write_condition_clauses` refuses it without consulting state.
That decision is state-independent, so it is not a read-then-write race.

### Invariant 6: a rejected condition leaves nothing behind

The actor returns `ConditionRejected` before sequence assignment, so there is
no sequence, no journal record, and no fan-out. The only residue is the blob
the request already wrote. `MemoryBlobStore::release` has no refcount
(`nimbus-blob/src/memory.rs:119`) and `put` is store-once idempotent
(`:78`), so a loser releasing its own hash can delete a **winner's identical
bytes**. The loser therefore releases only when the authority-reported
`current` manifest does not name that hash. On commit failure,
`release_uncommitted_blob` re-reads the authoritative manifest rather than
trusting a pre-read. Durable mark-and-sweep GC stays owned by NOS-A2.

One residual is unchanged from before this task: a release consults only the
manifest at the same logical key, so a blob that a **different** key dedups
onto could still be released. That hazard predates SIC1 and is not a condition
defect, so SIC1 narrows it rather than closing it: the release now consults the
authority's current manifest instead of a stale pre-read.

Closing it needs the lifecycle seam, not the write path. The parts already
exist — `nimbus_blob::BlobGc` mark-and-sweep (`nimbus-blob/src/gc.rs`), the
`BlobPinRegistry` write-intent pins (`nimbus-blob/src/pins.rs`), and
`nimbus_object_storage::object_gc_roots`, which enumerates every manifest and
every open multipart part as a live root
(`crates/nimbus-object-storage/src/gc.rs:26`). Nothing on the S3 write path
consults them: `release` drops the single claim directly, exactly as
`nimbus-blob/src/memory.rs:119` documents. Rooting object writes on that seam
is a lifecycle change with its own owner and is out of SIC1's scope.

## 3. Fail-before

The in-memory double was made faithful first. `get_manifest` and
`put_manifest_conditional` each `yield_now().await`, because every real
metadata call crosses to the committer actor and awaits it. Without that, the
probe cannot interleave and reports a false green.

**S3 seam** (`sic1-failbefore-s3.txt`) — `put_object` restored to
read → decide → write:

```
conditional_put_if_none_match_is_linearizable ... FAILED
  left: 75
 right: 1
test result: FAILED. 2 passed; 1 failed
```

75 of 100 concurrent `If-None-Match: *` creates were admitted. The sequential
probe and the blob-effect probe still passed, which confirms only the
concurrent probe can see this defect.

**Engine seam** (`sic1-failbefore-engine.txt`) — the in-actor check removed and
the decision taken on a read outside the actor:

```
object_meta_conditions_admit_one_concurrent_claimant ... FAILED
panicked at crates/nimbus-engine/src/tests/objects.rs:361:17:
the admitted claimant must find the key absent
test result: FAILED. 2 passed; 1 failed
```

Both patches were reverted from saved clean copies, verified by SHA-256.

## 4. Verification

Raw output: `sic1-verify.txt`.

| Command | Result |
|---|---|
| `cargo test -p nimbus-s3 conditional_ -- --nocapture` | 3 passed, 0 failed |
| `cargo test -p nimbus-engine object_meta -- --nocapture` | 3 passed, 0 failed |
| `cargo test -p nimbus-storage object_meta -- --nocapture` | 8 passed, 0 failed |
| `cargo test -p nimbus-s3` | 23 passed, 0 failed |
| `cargo fmt --all --check` | clean |
| `make clippy` | clean |

Plan verifier (`sic1-verifier.txt`): `Summary: 5 passed, 8 failed`. Conditions
1–5 are SIC1's contract and are green. Conditions 6–13 belong to SIC2–SIC6 and
stay red by design.

### Verifier correction

SIC0's `verify.sh` invoked `cargo test -q`. `-q` puts libtest in terse mode, so
no `^test NAME ... ok` line is ever printed and every test condition would have
reported a **vacuous FAIL**. All six invocations now drop `-q`. Without this,
conditions 4, 5, 6, 7, 8, 12, and 13 could never pass regardless of the code.

## 5. Blocker: `make ci` deny lane

`make ci` fails in `deny` only, on RUSTSEC-2026-0258 (h2 unbounded empty DATA
frames, low severity) against transitive `h2 0.3.27`:

```
advisories FAILED, bans ok, licenses ok, sources ok
```

SIC1 changed no manifest and no lockfile (`git status -- '*Cargo.toml'
Cargo.lock deny.toml` is empty), so this is pre-existing on `main`. h2 0.3.27
is the newest 0.3.x; the advisory is patched only in 0.4.16, which requires
moving `hyper` 0.14 → 1.x. `hyper` 0.14 is pinned by `libsql 0.9.30` and by
`x509-parser 0.15.1` through the deno fork. That dependency move is outside
SIC1's scope and is recorded as a campaign blocker rather than silenced with a
`deny.toml` ignore.

## 6. Every other `ci-required` lane

`make ci` aliases `ci-required`. With `deny` set aside as B1, each remaining
lane was run explicitly. Logs: `sic1-ci-rest.txt` (through the workspace lane),
`sic1-ci-tail.txt` (docs through build), `sic1-ci-js.txt` (JS and helpers).

| Lane | Result |
|---|---|
| `fmt-check` | clean |
| `clippy` | clean |
| `deny` | `advisories FAILED, bans ok, licenses ok, sources ok` — blocker B1 |
| `test-rust-runtime` | 517 passed, 0 failed, 134 ignored |
| `test-rust-workspace` | 7424 run: 7404 passed, 20 failed — every failure attributed to host state below |
| `test-rust-docs` | 0 failed |
| `verify-harness` | pass (`scripts/verification-harness.sh required all`) |
| `build-js` | pass |
| `typecheck-js` | pass |
| `test-js` | 51 files, 336 tests passed |
| `proof-helpers` | pass, including `runtime tenant-isolation static gate: 19 passed, 0 failed` |

`typecheck-js` first failed with `TS2307: Cannot find module 'firebase-admin/app'`
in `examples/cloud-functions/tasks/functions`. Those dependencies are declared
in that workspace's `package.json` but were not installed on this machine.
`npm install` fixed it and left `package-lock.json` byte-identical
(sha256 `4d30cf33…`, unchanged before and after).

### The 20 workspace-lane failures are host state, not SIC1

SIC1 touches four crates: `nimbus-engine`, `nimbus-s3`, `nimbus-storage`, and
one file in `nimbus-server` (`adapters/s3/listener.rs`). None of the 20
failures is in a path it changed. Raw attribution: `sic1-attribution.txt`,
`sic1-attribution-cli.txt`.

**18 in `nimbus-cli` `machine::tests::*` — not hermetic.** The handler calls
`try_run_lifecycle_command_via_live_server` first
(`crates/nimbus-cli/src/machine/handlers.rs:215`), which resolves
`LocalServerPaths` from the **real host** `$TMPDIR/nimbus/server.json`
(`crates/nimbus-operator/src/paths.rs:219`), not from the test's `TempDir`.
During the run that file named a live `nimbus` server on `127.0.0.1:3210`, so
each test's `machine init` was forwarded to the developer's server, which
answered `HTTP 404: machine lifecycle endpoints require a server-owned machine
manager` (`crates/nimbus-compute/src/machines.rs:354`). Re-run against a
`TMPDIR` with no `server.json`: **113 tests run, 113 passed**.

**1 in `nimbus-sandbox`, 1 in `nimbus-server` — wall-clock bounds under load.**
`runtime_observed_rejects_missing_creator_attempt_annotation` failed with
`provider command exceeded 2s`; `fresh_process_reopens_engine_and_plans_every_workload_saga_phase_without_snapshot_handoff`
failed with `timed out waiting for checkpoint` and a SIGKILLed harness role.
Both bounds were exceeded while 7424 tests ran concurrently beside a live
server. Re-run serially (`-j1`): both pass.

Test hermeticity in `nimbus-cli` is a real defect, but it is outside this
campaign's seams. It is recorded as B2 so a future local red is not misread as
a regression.
