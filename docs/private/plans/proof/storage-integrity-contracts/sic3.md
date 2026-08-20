# SIC3 — Every storage writer declares its commit effects

Base: `main` @ `172d737a42539676677e325e14288480cd17975c` (SIC2 merged).
Machine: darwin 24.6.0, aarch64, rustc 1.96.1.
Diff: 3 files, test tree only. No production file changed.

## 1. The gap

U5 (SIC0) proved that the three composite SQL commit paths agree with each
other. It said nothing about the writers that do not go through them. Finding
F3: a direct or internal writer could gain — or silently lose — an effect and
every existing gate stayed green.

The gap is structural, not accidental. `SqlCommitEffects` is built by the three
composite paths. The point-write family (`insert_once`, `update_validated_once`,
`delete_with_indexes_validated_once_at`, and their siblings) never constructs
one: it calls `transaction.insert_document(...)` and lets the shared commit
sequence supply the journal, index and version effects. Those effects are real
and none of them is visible in the writer's own body.

Decision U8 stands: the direct path is deliberately **not** folded into
`SqlCommitEffects`. A `Default` would reintroduce exactly the silence this task
removes, and boxed closures would replace reviewer-visible variants with opaque
callbacks. SIC3 is therefore a declarative matrix plus a structural gate, not a
refactor of the direct path.

## 2. The change, by seam

| File | Owns |
|---|---|
| `crates/nimbus-storage/src/tests/commit_path_ownership/effect_matrix.rs` | The declaration. 13 enums, one `WriterEffects` struct, and `MATRIX`: 54 rows. |
| `crates/nimbus-storage/src/tests/commit_path_ownership/effect_gate.rs` | The check. Reads `sql/store_core.rs` as text, classifies every `SqlStoreCore` method, and requires the source and the matrix to agree. |
| `crates/nimbus-storage/src/tests/commit_path_ownership.rs` | Declares both child modules beside the two U4 gates it already owned. |

### The twelve columns

Every row declares `admission`, `lease`, `condition`, `document`, `index`,
`version`, `catalog`, `scheduler`, `trigger`, `journal`, `watermark`, and
`outcome`. The plan named eleven; `catalog` is the twelfth. Without it the
schema, table-lifecycle, resource-path, object-metadata and usage writers would
declare eleven no-ops each, and a row of no-ops is the silence the matrix
exists to remove — the gate rejects any row that declares nothing.

Each column is a closed enum with an explicit variant per decision. There is no
`Default`, no `Option`, and no callback anywhere in the file: a writer cannot
inherit an effect, leave one unset, or hide one behind a function pointer.
`-D unused` is enforced workspace-wide, so a variant no row constructs fails the
build — the enums cannot drift ahead of the matrix either.

### The 54 rows

| Shape | Rows | What it means |
|---|---|---|
| `Direct` | 26 | Opens its own transaction through `execute_write*`. |
| `Composes` | 13 | Forwards to other `SqlStoreCore` writers; declares only effects a delegate declares. |
| `ProviderBodied` | 5 | Bodiless in the trait; the body is provider- or feature-gated. |
| `External` | 10 | Writers outside `SqlStoreCore`, pinned by file path and symbol. |

The ten external rows are the engine and storage writers SIC0's census named:
the direct mutation route, the queued publisher, the execution-unit route,
object metadata in the committer actor, the committer lease, trigger
candidates, table lifecycle, resource-path bindings, the libSQL replica cache,
and the usage control database.

### Scan, do not register

The gate reads source text instead of registering writers at run time, and the
crate's own feature layout is why. `mod sql` is declared under
`#[cfg(any(feature = "libsql", feature = "mysql", feature = "postgres"))]`, and
the plan verifier runs a bare `cargo test -p nimbus-storage` with no features.
In that build `sql/store_core.rs` — every writer this matrix describes — is not
compiled at all. A runtime registry would therefore find nothing and report
success over an empty set, vacuous exactly where the coverage matters. Reading
the file as text sees all 52 methods in either build, which the verifier
confirms: 4 passed bare, and 4 passed again under `--features mysql,postgres`
with the identical scan line.

The scan is guarded against its own failure: floors require at least 26 direct
writers, 44 `SqlStoreCore` writers, and 10 external rows, so a scan that stops
matching cannot pass by finding nothing.

### What the gate checks

1. Every bodiless trait method is pinned in exactly one of `PRIMITIVES`,
   `PROVIDER_READERS`, `PROVIDER_MUTATORS`, and every pin still exists.
2. Every scanned writer has exactly one matrix row; every non-external row
   names a scanned writer that the scan classifies as a writer.
3. The declared shape matches the source, including the exact delegate set of
   each composing writer.
4. Source evidence implies a declaration: `insert_document` implies a point
   insert *and* index maintenance *and* a retained version *and* a journal
   effect; `begin_scheduled_execution` implies deduplicated admission;
   `advance_fenced_committer_lease` implies `Lease::Fenced`;
   `transaction.save_cron_job` implies `SchedulerEffect::CronSaved`, and so on
   through the scheduler, trigger and catalog families.
5. The declared outcome matches the parsed return type, and a
   `CommitterLeaseResult` wrapper matches `Lease::Fenced` in both directions.
6. A composing writer declares no effect that none of its delegates declares.
7. Every external row's pinned file still exists and still contains its symbol.
8. No row declares twelve no-ops.

The rules in check 4 are **one-directional** by construction. Evidence in a
body implies a declaration; the absence of evidence implies nothing, because
the effects the shared commit sequence supplies leave no token in the writer's
body. That asymmetry is precisely why the direct path could drift, and an `iff`
rule here would fail every point writer for effects it genuinely has.

## 3. Fail-before

`sic3-failbefore.txt`. Three independent mutations, each reverted after
capture.

**A new writer lands with no row.** A direct writer added to `SqlStoreCore`:

```
ownership scan: 53 SqlStoreCore methods, 27 direct writers, 45 writers total, 54 matrix rows
"purge_orphaned_documents_unowned writes storage and has 0 matrix rows; every writer declares its effects exactly once"
```

**An existing writer gains an undeclared effect** — the plan's fail-before:
`update_validated_once` gains `transaction.insert_scheduled_job(...)`.

```
"update_validated_once: writes scheduler state through insert_scheduled_job but declares None"
```

With that same mutation in place, the U5 coherence checks
(`cargo test -p nimbus-storage commit_effects`) stay **green**. That is F3
demonstrated directly: the pre-SIC3 gates cannot see a direct writer gaining an
effect. Only the new gate fails.

**A declared effect is dropped.** `insert_once`'s journal effect set to `None`:

```
"insert_once: writes a document through insert_document without declaring a journal effect"
"insert: declares a journal effect that no delegate in [\"insert_once\"] declares"
"insert_with_indexes_once: declares a journal effect that no delegate in [\"insert_once\"] declares"
```

One dropped effect fails three rows, because composition is checked as well as
declaration.

The mutation test `omitted_commit_effect_fails_the_ownership_matrix` runs the
whole gate as a pure function over damaged copies of `MATRIX` and requires a
violation for each: a removed row, five separately omitted effects on
`insert_once`, a row naming a writer that does not exist, an outcome that
contradicts the writer's return type, and a row reduced to no effects at all.
The gate cannot pass by agreeing with itself.

## 4. Verification

| Command | Result |
|---|---|
| `cargo test -p nimbus-storage commit_path_ownership -- --nocapture` | 4 passed, 0 failed |
| `cargo test -p nimbus-storage --features mysql,postgres commit_path_ownership -- --nocapture` | 4 passed, 0 failed (same scan line: 52 methods, 26 direct, 44 writers) |
| `cargo test -p nimbus-storage --features mysql,postgres commit_effects -- --nocapture` | 2 passed, 0 failed — U5's `effect_coherence_accepts_only_the_pairings_the_document_strategy_implies` and the new gate |
| `cargo test -p nimbus-engine mutation -- --nocapture` | 255 passed, 0 failed with `NIMBUS_DISABLE_IMPLICIT_EXTERNAL_PROVIDER_FIXTURES=1` |
| `cargo fmt --all --check` | clean |
| `make clippy` | clean |

U5 remains green, as the acceptance criteria require. Raw output:
`sic3-verify.txt`.

Without `NIMBUS_DISABLE_IMPLICIT_EXTERNAL_PROVIDER_FIXTURES=1`, four of those
255 fail closed on missing `NIMBUS_TEST_POSTGRES_URL` /
`NIMBUS_TEST_MYSQL_URL` / libSQL fixtures. That is the fixture contract
refusing to run a provider test without its pinned live provider, not a
regression: those four lanes are **UNVERIFIED** on this host and belong to
`make test-external-providers`.

Plan verifier (`sic3-verifier.txt`): `Summary: 8 passed, 5 failed`. Conditions
7 and 8 are SIC3's contract and are now green; 1–6 stay green; 9–13 belong to
SIC4–SIC6 and are red by design.

### The verifier's non-vacuity contract holds

`run_test` requires both a zero exit and at least one `^test .*<filter>.* ... ok`
line, so a filter that matches nothing fails. Both new conditions name a test
that exists in the bare build the verifier uses.

## 5. `make ci`

`make ci` aliases `ci-required`. Raw: `sic3-ci.txt`.

| Lane | Result |
|---|---|
| `fmt-check` | clean |
| `clippy` | clean |
| `deny` | `advisories ok, bans ok, licenses ok, sources ok` |
| `test-rust-runtime` | 517 passed, 0 failed, 134 ignored |
| `test-rust-workspace` | 7429 run: 7409 passed, 20 failed — all attributed to B2 below |
| `test-rust-docs` | pass |
| `verify-harness` | pass |
| `build-js` | pass |
| `typecheck-js` | pass |
| `test-js` | 51 files, 336 tests passed |
| `proof-helpers` | pass, install-script helper 44 tests |

`make ci` stops at the first failing lane, so the six lanes after
`test-rust-workspace` were run individually after it; each exited 0.

### All 20 workspace failures are blocker B2, and none touches storage

Zero failures in `nimbus-storage`. SIC3 changes only that crate's test tree.

**18 × `nimbus-cli machine::tests`** — non-hermetic. A live `nimbus dev`
(pid 4813, `127.0.0.1:3210`) owned the shared discovery record
`$TMPDIR/nimbus/server.json` throughout the run, so the CLI talked to it and
got `HTTP 404 … machine lifecycle endpoints require a server-owned machine
manager`. Re-run with an isolated `TMPDIR` while that same server stayed up:
**113 tests run, 113 passed** in 1.385s. This is B2's hermeticity defect
exactly as SIC1 recorded it.

**2 × `nimbus-server`** — wall-clock bounds missed under 7429-test
concurrency, both `Elapsed(())`:

| Test | Under full load | Serial |
|---|---|---|
| `construction::tests::nnc3_5_sibling_bind_is_claimed_before_guard_and_serves_identical_bytes` | FAIL at 7.150s | PASS 0.404s |
| `listener_group::tests::listener_projection_failure_keeps_every_listener_active_and_retries` | FAIL at 5.852s | PASS 0.392s |

Both are an order of magnitude inside their own 5s bound when not competing
for the host. Same class as SIC2's sandbox failure. B2 stays open; nothing was
skipped, weakened, or re-bounded.
