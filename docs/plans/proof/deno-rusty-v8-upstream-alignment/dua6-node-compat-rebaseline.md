# DUA6 Node Compatibility Rebaseline

status: done
date: 2026-06-01
branch: codex/deno-rusty-v8-upstream-alignment
worktree: /Users/jack/src/github.com/nimbus/nimbus-worktrees/deno-rusty-v8-upstream-alignment
source worktree: /Users/jack/src/github.com/nimbus/deno
source branch: nimbus/v2.8.1
source tag: v2.8.1-nimbus.1
source commit: 18f76a9a19ab74d49d9a40037733cc4aec983d26
rusty_v8 tag: v149.2.0-nimbus.1
rusty_v8 commit: ce6663111a3ff8fde06bc04ba19bbbced60dbc8d
pr: https://github.com/nimbus/nimbus/pull/11
verifier: scripts/verify-deno-rusty-v8-upstream-alignment.sh

## Proof Contract Checklist

1. **Row and status.** DUA6 is done. Nimbus has been rebaselined against the
   immutable `nimbus/deno` `v2.8.1-nimbus.1` and `nimbus/rusty_v8`
   `v149.2.0-nimbus.1` stack.
2. **Input baseline.** The pre-DUA comparison baseline is the NDS3
   `v2.8.0-nimbus.15` evidence recorded in
   `docs/plans/proof/node-default-runtime-support-hardening/nds3-official-fixture-promotion.md`.
3. **Disposition table.** Every new DUA6 local compatibility change is
   classified below.
4. **Implementation evidence.** Runtime bootstrap fixes are listed below with
   exact behavior and owner.
5. **Focused verification.** Focused verification passed for the changed
   behavior and existing loader, CommonJS, crypto, `node:v8`, async_hooks,
   networking, fs, and stream watchpoints listed below.
6. **Broad verification.** Broad reruns compare pre-DUA and post-DUA counts.
   The two repin regressions were fixed, then closed only after the same broad
   groups reran green.
7. **Residual risks.** Remaining failures are owned and routed to the next NDS
   wave; none are hidden as positive compatibility claims.

## Input Baseline

| Field | Value |
| --- | --- |
| Pre-DUA Nimbus Deno tag | `v2.8.0-nimbus.15` |
| Pre-DUA Deno commit | `1f101bf0032a223463507f500ddd236afebd9fcc` |
| Post-DUA Nimbus Deno tag | `v2.8.1-nimbus.1` |
| Post-DUA Deno commit | `18f76a9a19ab74d49d9a40037733cc4aec983d26` |
| Post-DUA rusty_v8 tag | `v149.2.0-nimbus.1` |
| Post-DUA rusty_v8 commit | `ce6663111a3ff8fde06bc04ba19bbbced60dbc8d` |
| Nimbus Cargo state | `Cargo.toml` and `Cargo.lock` point at immutable fork tags; no local path override remains. |

Pre-DUA broad counts from NDS3:

| Family | Pre-DUA result |
| --- | --- |
| `core-semantics` | `122 passed, 1 skipped, 0 failed` |
| `process-and-timing` | `48 passed, 0 skipped, 0 failed` |
| `streams-and-local-io` | `308 passed, 0 skipped, 0 failed` |
| `networking` | `268 passed, 0 skipped, 0 failed` |
| `loader-context` | `173 passed, 0 skipped, 4 failed` |

The remaining pre-DUA loader/context failures were:

- `test/parallel/test-async-hooks-enable-recursive.js`
- `test/parallel/test-async-hooks-enable-before-promise-resolve.js`
- `test/parallel/test-async-hooks-enable-during-promise.js`
- `test/parallel/test-v8-serdes.js`

## Disposition Table

| Area | File | Disposition | Reason |
| --- | --- | --- | --- |
| `process.loadEnvFile()` inside the Nimbus embedded runtime | `crates/nimbus-runtime/src/runtime/bootstrap/js/node22_runtime_bootstrap.js` | `nimbus-embedding-specific` | Deno 2.8.1 correctly enforces its own read permission before `process.loadEnvFile()` reads from disk. Nimbus embeds Deno behind its own runtime host-policy file grants, so the wrapper now falls back to `op_nimbus_runtime_require_read_file` only after Deno denies the read, parses `.env` content with Node's no-overwrite behavior, and still returns `ERR_ACCESS_DENIED` when Nimbus host policy denies the path. |
| `fs.watch()` missing-entry synchronous throw | `crates/nimbus-runtime/src/runtime/bootstrap/js/node22_runtime_bootstrap.js` | `still-needed-node-gap` | Upstream Deno defers missing-path watch failures to an `error` event for editor/vite ergonomics. Node's `fs.watch()` contract throws synchronously when `throwIfNoEntry` is not `false`. Nimbus normalizes only that missing-entry case and leaves the Deno watcher path intact for existing paths and `throwIfNoEntry: false`. |

No Deno fork source changed in DUA6. These are Nimbus runtime bootstrap shims
over the already-published fork stack.

## Implementation Evidence

Changed file:

- `crates/nimbus-runtime/src/runtime/bootstrap/js/node22_runtime_bootstrap.js`

Implemented behavior:

- `process.loadEnvFile()` now:
  - keeps Deno's original implementation as the fast path;
  - detects Deno or Nimbus read-permission denial;
  - reads through the Nimbus host file-read policy when Deno's own permission
    model denies an otherwise allowed embedded-runtime path;
  - applies parsed dotenv entries without overwriting existing `process.env`
    values;
  - preserves `ERR_ACCESS_DENIED` for paths outside Nimbus grants and `ENOENT`
    for missing granted files.
- `fs.watch()` now:
  - pre-validates the watched path only when `throwIfNoEntry !== false`;
  - throws a Node-shaped `ENOENT` with `path`, `filename`, `syscall: "watch"`,
    and `errno: -2`;
  - leaves `throwIfNoEntry: false` on the Deno watcher path so missing paths can
    still return a closable watcher.

## Focused Verification

Focused verification passed for changed behavior:

```console
cargo test -p nimbus-runtime node24_process_load_env_file_fixture -- --test-threads=1
cargo test -p nimbus-runtime node22_process_load_env_file_fixture -- --test-threads=1
cargo test -p nimbus-runtime node24_fs_watch_enoent_fixture -- --test-threads=1
cargo test -p nimbus-runtime node22_fs_watch_enoent_fixture -- --test-threads=1
```

Observed:

- `node24_process_load_env_file_fixture`: `1 passed, 0 failed`.
- `node22_process_load_env_file_fixture`: `1 passed, 0 failed`.
- `node24_fs_watch_enoent_fixture`: `1 passed, 0 failed`.
- `node22_fs_watch_enoent_fixture`: `1 passed, 0 failed`.

Focused verification also passed for existing loader/changed-behavior guards:

```console
cargo test -p nimbus-runtime node24_loader_context_global_paths_preserve_local_precedence_regression -- --test-threads=1
cargo test -p nimbus-runtime node24_loader_context_followup_v8_green_batch_fixture -- --test-threads=1
```

Observed:

- `node24_loader_context_global_paths_preserve_local_precedence_regression`:
  `1 passed, 0 failed`.
- `node24_loader_context_followup_v8_green_batch_fixture`:
  `1 passed, 0 failed`.

Coverage terms for the DUA verifier: CommonJS loader, crypto, `node:v8`,
async_hooks, networking, fs, and stream behavior were either rerun broadly or
checked through focused guards. The async_hooks failures remain owned
watchpoints, not green claims.

## Broad Verification

Broad rerun evidence:

| Family | Post-repin first result | Focused action | Post-fix broad result |
| --- | --- | --- | --- |
| `core-semantics` | `122 passed, 1 skipped, 0 failed` | No action needed. | `122 passed, 1 skipped, 0 failed` |
| `process-and-timing` | `47 passed, 0 skipped, 1 failed` (`test-process-load-env-file.js`) | Added Nimbus host-policy fallback for `process.loadEnvFile()` after Deno read-permission denial. | `48 passed, 0 skipped, 0 failed` |
| `streams-and-local-io` | `307 passed, 0 skipped, 1 failed` (`test-fs-watch-enoent.js`) | Added Node-shaped missing-entry throw for `fs.watch()` when `throwIfNoEntry !== false`. | `308 passed, 0 skipped, 0 failed` |
| `networking` | `268 passed, 0 skipped, 0 failed` | No action needed. | `268 passed, 0 skipped, 0 failed` |
| `loader-context` | `173 passed, 0 skipped, 4 failed` | No new DUA6 fix; rebaseline matches the known NDS3 residual set. | `173 passed, 0 skipped, 4 failed` |

Commands run:

```console
cargo test -p nimbus-runtime node24_default_lane_executes_core_semantics_subset -- --test-threads=1
cargo test -p nimbus-runtime node24_default_lane_executes_process_and_timing_subset -- --test-threads=1
cargo test -p nimbus-runtime node24_default_lane_executes_streams_and_local_io_subset -- --test-threads=1
cargo test -p nimbus-runtime node24_default_lane_networking_watchpoint -- --test-threads=1
cargo test -p nimbus-runtime node24_default_lane_loader_context_watchpoint -- --ignored --nocapture --test-threads=1
```

Observed command outcomes:

- `node24_default_lane_executes_core_semantics_subset`: cargo test passed;
  `1 passed, 0 failed` at the Rust harness level, with the Node fixture batch
  corresponding to `122 passed, 1 skipped, 0 failed`.
- `node24_default_lane_executes_process_and_timing_subset`: initially failed
  with `test-process-load-env-file.js`; after the focused fix, cargo test
  passed with `1 passed, 0 failed` at the Rust harness level, corresponding to
  `48 passed, 0 skipped, 0 failed`.
- `node24_default_lane_executes_streams_and_local_io_subset`: initially failed
  with `test-fs-watch-enoent.js`; after the focused fix, cargo test passed in
  `580.06s` with `1 passed, 0 failed` at the Rust harness level,
  corresponding to `308 passed, 0 skipped, 0 failed`.
- `node24_default_lane_networking_watchpoint`: cargo test passed in `531.74s`
  with `1 passed, 0 failed` at the Rust harness level, corresponding to
  `268 passed, 0 skipped, 0 failed`.
- `node24_default_lane_loader_context_watchpoint`: intentionally remains an
  ignored watchpoint; the run produced `173 passed, 0 skipped, 4 failed`.

Newly green fixtures were not promoted from focused tests alone. The two DUA6
regressions closed only after the same broad groups that found them were rerun.

## Remaining Failure Ownership

| Fixture | Owner repo | Classification | Follow-up trigger |
| --- | --- | --- | --- |
| `test/parallel/test-async-hooks-enable-recursive.js` | `nimbus/nimbus` runtime first, then `nimbus/deno` if Deno promise-hook semantics are the root cause | embedded async_hooks promise-count watchpoint | NDS should isolate the async resource overcount under the current `v2.8.1-nimbus.1` stack and either fix the runtime bootstrap hook lifecycle or promote a Deno fork issue/patch. |
| `test/parallel/test-async-hooks-enable-before-promise-resolve.js` | `nimbus/nimbus` runtime first, then `nimbus/deno` | embedded async_hooks promise-count watchpoint | Same async_hooks cluster as above; do not count as green until the broad loader/context rerun passes. |
| `test/parallel/test-async-hooks-enable-during-promise.js` | `nimbus/nimbus` runtime first, then `nimbus/deno` | embedded async_hooks promise-count watchpoint | Same async_hooks cluster as above; follow the wide-then-focused NDS loop. |
| `test/parallel/test-v8-serdes.js` | `nimbus/rusty_v8` / V8 wire-format boundary, documented by `nimbus/nimbus` | V8 serialization wire-format boundary | Nimbus uses the `v8_deno_core` V8 line, currently `v149.2.0`; exact Node24 serialized bytes are not a portable in-isolate support claim. NDS should keep the functional V8 helper subset green and document the wire-format limit unless a versioned engine strategy owns exact bytes. |

These failures do not block DUA closeout, because DUA6 is a rebaseline row and
the same residual set existed before the upstream-aligned fork repin. They do
remain NDS blockers for any claim that Node24 has reached the full default
support target.

## Residual Risks

- DUA6 restored the promoted Node24 foundation groups to their pre-DUA pass
  posture but did not increase the NDS full-corpus numerator.
- The exact V8 serialization byte fixture is intentionally not papered over.
  Treating it as green would misrepresent a V8 wire-format difference between
  the Deno-compatible V8 line and Node24's own line.
- The async_hooks failures remain real embedded-runtime behavior gaps and must
  be fixed or classified by NDS before a stronger Node24 default claim.
- DUA7 must update docs/ledgers/dashboards or record a no-public-count-change
  decision based on these results.
