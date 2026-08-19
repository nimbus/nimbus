# SIC2 — Multipart upload merges fenced on the revision the writer observed

Base: `main` @ `b6b89c871224f26d6fd5e92bdff523e3db5b44ad` (SIC1 merged).
Machine: darwin 24.6.0, aarch64, rustc 1.96.1.
Production diff: 10 files, +867 / −123 (`git diff --stat -- crates`).

## 1. The defect

`UploadPart` merged one part into the whole upload record through a
read-modify-write that decided nothing:

```
upload_part: get_multipart_upload -> replace_part -> put_multipart_upload
```

`put_multipart_upload` carried no expected state. An unconditional write of an
upload row does not overwrite one field — it replaces the entire part list, so
it discards every part another request committed since this one read the row.
Concurrent `UploadPart` calls for *different* part numbers therefore all
returned `200` and all but the last-writer's part vanished. SIC1 closed the
same shape for manifests; the multipart row was the second site.

## 2. The change, by seam

| Seam | Owns |
|---|---|
| `nimbus-storage/src/traits/object_metadata.rs` | `ObjectMultipartUpload::revision` (monotonic, first value 1), `ObjectUploadExpectedState` (`Absent` / `AtRevision`), `ObjectUploadConditionOutcome`. No `Default`: a writer cannot omit its fence by silence. |
| `nimbus-engine/src/engine/objects.rs` | `ObjectMetaCondition` / `ObjectMetaUnmet` tag the manifest and upload condition families apart. `evaluate_object_condition` decides both inside the committer actor, against the actor's own read, **before** sequence assignment. |
| `nimbus-s3/src/backend.rs` | `S3ObjectMeta::put_multipart_upload_conditional` and `delete_multipart_upload_conditional`. The unconditional siblings are **removed**, not deprecated. |
| `nimbus-s3/src/service.rs` | The bounded re-merge policy, the completion/abort fences, and the blob-release decisions that follow from each outcome. |

### Why a revision and not a content comparison

A manifest has an ETag the client already names, so SIC1's clauses could be
written against it. An upload row has no client-visible version, and a part
list is not a value a client can fence on. The row therefore carries its own
monotonic `revision`, published by whoever commits it. Revision 0 never
exists, so "absent" and "present at the first revision" stay distinguishable
without an `Option` sentinel that a writer could leave unset.

`put_multipart_upload_conditional` rejects, as an internal contract violation,
any fenced write whose published revision is not the successor of the revision
it fenced on. A writer cannot fence on one image and publish another.

### The four multipart operations

| Operation | Fence | On rejection |
|---|---|---|
| `CreateMultipartUpload` | `Absent` | Internal error: a fresh ULID that already exists is a generator defect, not a client condition. |
| `UploadPart` | `AtRevision(observed)` | Reload the published image, re-apply this part, retry. Bounded by `UPLOAD_PART_MERGE_ATTEMPTS` (8). |
| `CompleteMultipartUpload` | `AtRevision(observed)`, on the delete, **before** the manifest write | `NoSuchUpload` if the row is gone, `OperationAborted` if it advanced. Nothing published. |
| `AbortMultipartUpload` | `AtRevision(observed)` | Absent row: success (abort stays idempotent). Advanced row: `OperationAborted`, no blob released. |

### Retry policy: only the pure merge

The retry re-runs `replace_part` against the reloaded image. It never re-runs
an ambiguous durable outcome. A transport or storage `Err` from the committer
means the write may still have landed, so that arm re-reads the authority,
decides blob retention from what it finds, and returns the error — it does not
loop. Only an explicit `Rejected` — which by invariant 6 has no sequence, no
journal record, and no fan-out — is retried.

### Completion consumes the upload before it publishes the manifest

The fenced delete runs first. A completion that lost the race to another
completion or to an abort is rejected there, having written no manifest and
released no blob. The reverse order would let a loser publish a manifest and
only then discover it had lost.

The delete returns the row it removed, read by the authority itself, and the
completion releases exactly those part blobs the new manifest does not retain.
Parts the client did not name in the completion are discarded by the S3
contract and had no other holder, so this closes a leak that predated SIC2:
the old code deleted the upload row and released nothing.

## 3. Fail-before

The in-memory double was made faithful first: `get_multipart_upload` yields,
because every real metadata call crosses to the committer actor and awaits it.
Without that the probe cannot interleave and reports a false green.

`sic2-failbefore-multipart.txt`, eight concurrent `UploadPart` calls for eight
distinct part numbers:

```
assertion `left == right` failed: every accepted part must survive in the durable upload record
  left: [1, 3, 8]
 right: [1, 2, 3, 4, 5, 6, 7, 8]
test result: FAILED. 0 passed; 1 failed
```

All eight calls returned success. Five acknowledged parts were lost.

## 4. Verification

| Command | Result |
|---|---|
| `cargo test -p nimbus-s3 multipart -- --nocapture` | 2 passed, 0 failed |
| `cargo test -p nimbus-s3` | 25 passed, 0 failed |
| `cargo test -p nimbus-engine objects -- --nocapture` | 6 passed, 0 failed |
| `cargo test -p nimbus-storage object_meta -- --nocapture` | 9 passed, 0 failed |
| `cargo fmt --all --check` | clean |
| `make clippy` | clean |

Plan verifier (`sic2-verifier.txt`): `Summary: 6 passed, 7 failed`. Conditions
1–6 are SIC1's and SIC2's contract and are green. Conditions 7–13 belong to
SIC3–SIC6 and stay red by design.

### Tests added

- `concurrent_upload_parts_preserve_all_accepted_parts` (`nimbus-s3`) — the
  acceptance probe above, on a 4-worker multi-thread runtime.
- `stale_multipart_fence_is_rejected_without_losing_parts` (`nimbus-s3`) — a
  delete and a merge fenced on a superseded revision are both rejected, the row
  and both accepted parts survive, and the current image still completes.
- `multipart_upload_revision_survives_sqlite_reopen` (`nimbus-storage`) — the
  fence field round-trips through the durable embedded provider.

### Same-part races have one documented winner

Two `UploadPart` calls for the *same* part number both commit; the one whose
fenced write the authority admits last is the winner, and the loser's blob is
released only when the committed row no longer names it. This matches S3: a
part number holds the bytes of the last upload the service accepted for it.
`replacing_duplicate_multipart_part_keeps_shared_blob_readable` covers the
shared-blob case, where the loser must release nothing.

### Provider fixtures

Embedded fixtures cover both stores the object metadata scenarios run on: redb
(`object_meta_store_round_trips_multipart_upload_through_redb`, full-value
equality, so the revision is included) and SQLite
(`multipart_upload_revision_survives_sqlite_reopen`, across a reopen).

Remote provider lanes (PostgreSQL, MySQL, libSQL) are **UNVERIFIED** on this
host: they need live containers through `make test-external-providers`, which
this machine does not run. They are not claimed green. Object rows travel the
same document write path those providers already qualify, and the complete
provider qualification matrix is SIC5's contract (verifier condition 12).

## 5. `make ci`

`make ci` aliases `ci-required`. Raw: `sic2-ci.txt` (deny, runtime, workspace
summary), `sic2-ci-lanes.txt` (every lane after the workspace one).

| Lane | Result |
|---|---|
| `fmt-check` | clean |
| `clippy` | clean |
| `deny` | `advisories ok, bans ok, licenses ok, sources ok` |
| `test-rust-runtime` | 517 passed, 0 failed, 134 ignored |
| `test-rust-workspace` | 7427 run: 7426 passed, 1 failed — attributed below |
| `test-rust-docs` | 0 failed |
| `verify-harness` | pass |
| `build-js` | pass |
| `typecheck-js` | pass |
| `test-js` | 51 files, 336 tests passed |
| `proof-helpers` | pass, including `runtime tenant-isolation static gate: 19 passed, 0 failed` |

### Blocker B1 is closed

SIC1 recorded the `deny` lane red on RUSTSEC-2026-0258 against transitive
`h2 0.3.27`. This run reports `advisories ok`. SIC2 changed no manifest and no
lockfile, so the advisory database moved, not the dependency graph. B1 is
therefore closed by observation, not by a change: hosted CI already passed this
lane throughout.

### The one workspace failure is host load, not SIC2

`nimbus-sandbox … fresh_process_converges_exact_runner_effect_matrix` failed
with `crash child did not reach the durable EffectsStarted boundary within 15s`
after 16.4s, while 7427 tests ran concurrently. SIC2 touches no sandbox path.
Re-run alone with `--test-threads=1`: **1 passed** in 1.60s, an order of
magnitude inside the same bound. This is the wall-clock-bound class already
recorded as blocker B2.

The 18 non-hermetic `nimbus-cli machine::tests` failures from SIC1's run did
not recur: no live server owned `$TMPDIR/nimbus/server.json` during this run.
B2 stays open because the hermeticity defect is unchanged.
