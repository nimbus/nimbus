# SIC7 — Architecture reconciliation and campaign closeout

Base: `main` @ `8031cc581` (SIC0–SIC6 merged).
Machine: darwin 24.6.0, aarch64, rustc 1.96.1.
Branch: none. This task changes documentation only, so it commits direct to
`main` under the repository convention SIC0 already followed.

## 1. What the specs still said

Six SIC tasks changed how storage decides, records, and proves a write. The
four governing documents still described the world before them. The searches in
`sic7-failbefore.txt`, run at `8031cc581`, reproduce all six gaps:

| # | Gap | Evidence at `8031cc581` |
| --- | --- | --- |
| A | No spec said a condition is decided at the commit authority | zero matches for expected state, conditional admission, or precondition across all four files |
| B | Recovery was described by sequence alone | zero matches for `MaterializedPosition`, state digest, or `checkpoint_position`; the shadow materializer posture still read "tracks checkpoint and current sequence in a versioned manifest" |
| C | The U8 gap had no named successor | `U8` appeared exactly once, at `persistence-engine-baseline.md:62`, as a performance closeout with no commit-effect gate named |
| D | Provider qualification was undocumented | zero matches for `semantic_contract` or `UNVERIFIED` |
| E | Physical durability was undocumented | zero matches for physical durability, fault injection, or VFS |
| F | The HS deferrals were unrecorded | zero matches for epoch lineage or reader-first |

The `AFTER` block appended to the same file re-runs each search against the
reconciled tree. Every one now resolves, and each resolution names the file and
line that carries it.

### A trap worth recording

The first fail-before attempt reported "No such file or directory" for every
path. Two causes, both worth knowing:

1. The capture ran in a fresh `main` worktree. Two of the four governing specs
   are **untracked**, so they do not exist there. See §3.
2. `grep` in this shell is a function, not the binary, and it does not
   word-split an unquoted variable. `grep -rn ... $SPECS` treated a
   four-path list as one filename and reported a silent false negative. The
   paths are passed literally and quoted in the committed artifact.

## 2. What the reconciliation says

Nine edits across the four documents.

**`persistence-engine-baseline.md`** (five edits)

- A conditional write carries its expected state — `ObjectExpectedState` or
  `ObjectUploadExpectedState` — to the commit authority. The actor decides it
  against its own read, before sequence assignment, so a refusal takes no
  sequence and leaves no gap. There is no raw provider CAS escape hatch, and
  multipart is fenced on the revision the writer observed.
- Every storage writer declares its commit effects explicitly, over the
  complete `SqlStoreCore` writer set, as twelve closed enum concepts with no
  `Default`, no `Option`, and no callback, checked by a gate that reads the
  source. This is the successor to U8 that gap C asked for.
- Every materialized artifact is bound to a `MaterializedPosition` — state
  version, applied sequence, canonical state digest — compared by snapshot and
  bootstrap fingerprints, the shadow manifest, and every PITR import route.
  `durable_head` stays a separate comparison because it is a fact about the
  journal, not about materialized state. A sequence alone is not an identity.
- `StorageCapabilities` publishes a `semantic_contract` profile per store, over
  the closed seven-dimension matrix; "not owned" is checked against the real
  provider registrations, and a disabled feature or absent fixture reads
  `UNVERIFIED`.
- The shadow materializer posture now stores a `checkpoint_position`, so
  recovery rejects a checkpoint that diverges at an unchanged sequence.
- Explicit non-decisions gained one bullet: external epoch lineages,
  journal-format seals, and reader-first format rollout are binding
  prerequisites owned by `horizontal-scaling-plan.md`.

**`storage-seams-architecture.md`** (two edits)

- §6, Seam B, states that conditional writes are decided at the commit
  authority and not at the adapter.
- §14 records the storage-integrity risk as **CLOSED**, lists the five landed
  contracts and the proof root, and names HS as the single owner of the SIC-D4
  deferrals.

**`time-and-ordering.md`** (one edit)

- New section "Conditional admission and materialized position": a refused
  conditional write takes no sequence, so a rejection cannot leave a gap in
  durable order; a sequence orders writes but does not identify state; every
  materialized artifact is bound to a `MaterializedPosition`; `durable_head`
  stays separate.

**`verification.md`** (one edit)

- A lane that cannot run is `UNVERIFIED` and is never reported as green,
  followed by the storage semantic-qualification matrix and the physical
  durability lane with its command.

## 3. Two of the four specs cannot be committed

`docs/private` is gitignored except for a curated force-tracked subset.

| File | Tracked | Landed how |
| --- | --- | --- |
| `docs/private/architecture/time-and-ordering.md` | yes | commit below |
| `docs/private/operating/verification.md` | yes | commit below |
| `docs/private/plans/storage-seams-architecture.md` | **no** | edited in place, local only |
| `docs/private/architecture/storage/persistence-engine-baseline.md` | **no** | edited in place, local only |

This is not a shortfall in the reconciliation; all nine edits are applied and
verified on disk. It means the two untracked specs are absent from a fresh
clone and from any other machine, exactly as SIC5's
`docs/private/operating/storage-backends.md` already is. It is the one piece of
this task that a reader elsewhere cannot see. Recorded here as the remaining
uncertainty rather than silently omitted.

Both apply scripts assert exactly one anchor match before writing and were
dry-run against copies of the real files first.

## 4. Campaign result

| Task | Work commit | Pull request | Merged as |
| --- | --- | --- | --- |
| SIC0 | `a2f34aec6` | none (docs only) | direct to `main` |
| SIC1 | `ed3585eec` | #281 | `b6b89c871` |
| SIC2 | `74bdaf7bd` | #284 | `09d1003d3` |
| SIC3 | `67775ab35` | #285 | `6de99e977` |
| SIC4 | `8cadaf7d0` | #287 | `f49abe93a` |
| SIC5 | `f5b562ffd` | #289 | `d6635b7b7` |
| SIC6 | `77d499f51` | #290 | `dc0c06b73` |
| SIC7 | this commit | none (docs only) | direct to `main` |

Pull request range #281 through #290. Every hosted run ended 53 or 54 checks
pass, 0 fail, 3 skipping.

## 5. Verification

| Command | Result |
| --- | --- |
| `bash .../verify.sh` | **`Summary: 13 passed, 0 failed`, exit 0** — `sic7-verifier.txt` |
| `bash scripts/check-docs.sh` | `PASS — 109 pages link-clean, source map resolves, private fence intact, titles unique` |
| `cargo fmt --all --check` | clean |
| `make clippy` | clean |
| `make ci` | workspace lane `7438 tests run: 7437 passed (5 slow, 1 leaky), 1 failed, 108 skipped`; all other lanes green |
| `cargo test -p nimbus-storage sqlite_physical_durability` | 5 passed, 0 failed, 1 ignored |
| `autoreview --gate pre-pr --mode auto` | see §6 |

This is the first run in the campaign where every one of the thirteen
conditions is present on `main` at once, and all thirteen pass:

```
PASS  1. typed ObjectExpectedState + ObjectConditionOutcome live in the storage seam
PASS  2. the condition crosses S3ObjectMeta and the S3 pre-read decision is gone
PASS  3. the committer actor decides the condition before sequence assignment
PASS  4. sequential + concurrent conditional probes pass
PASS  5. rejection has no commit or blob effect (1 test(s))
PASS  6. concurrent multipart writes preserve every accepted part (1 test(s))
PASS  7. all storage writers are inventoried (1 test(s))
PASS  8. an omitted effect fails the ownership gate (1 test(s))
PASS  9. storage owns the one canonical digest implementation
PASS  10. divergence + ordering tests pass
PASS  11. shadow recovery and PITR import both bind the position
PASS  12. provider qualification matrix is complete (1 test(s))
PASS  13. physical SQLite durability faults pass (4 cases)
```

### The one `make ci` failure

`redb_storage_engine_quality_performance_budget_covers_latest_historical_cdc_pitr_and_gc`,
a 1s wall-clock point-in-time-restore budget on the **redb** path. Re-run three
times in isolation on the same checkout: ok, ok, ok, at 1.75s, 1.66s, and 1.64s
of total test time with the budgeted step well inside its bound. It fails only
under 7438-test concurrency. This is blocker **B2**, already recorded by SIC4,
SIC5, and SIC6 against the same case. No bound was widened and no test was
skipped. This task changes no code, so it cannot have caused it.

### Skipped dependencies

- **Remote provider lanes.** PostgreSQL, MySQL, and libSQL have no live fixture
  on this host, so their cells read `UNVERIFIED`, never green, per invariant 12.
  Hosted CI runs all three; each SIC pull request that touched provider code
  passed them.
- **108 skipped tests** in the workspace lane are the standing feature-gated and
  fixture-gated set, unchanged by this campaign.
- **`crash_child_writes_and_parks`** stays `#[ignore]`d: it is the child body
  the SIC6 crash case re-executes, and it returns immediately without its
  environment variable.

## 6. Remaining uncertainty

1. The two untracked governing specs (§3) exist only on this machine.
2. Finding **F7** — external epoch lineages, journal-format seals, and
   reader-first format rollout — is deferred by decision SIC-D4 and owned by
   `horizontal-scaling-plan.md`. Both untracked specs now name that owner.
3. Blockers **B2** (local host wall-clock and hermeticity flakes), **B3** (one
   libsql wall-clock bound), and **B4** (hosted engine shard flakes) are test
   hermeticity, not storage-integrity defects. None was closed by this
   campaign, and none was worked around by weakening a bound. They remain open
   against the test suite.
