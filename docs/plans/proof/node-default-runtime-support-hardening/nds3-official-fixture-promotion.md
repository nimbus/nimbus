# NDS3 Official Fixture Promotion Proof

status: in_progress
date: 2026-06-01
branch: codex/node-default-runtime-support-hardening
worktree: /Users/jack/src/github.com/nimbus/nimbus-worktrees/node-default-runtime-support-hardening
pr: https://github.com/nimbus/nimbus/pull/10
verifier: scripts/verify-node-default-runtime-support-hardening.sh

## Row And Status

NDS3 is in progress. The first promotion wave used the required
wide-then-focused loop on Node24 official fixtures, found a stale accounting
bug in the evidence generator, corrected the pass numerator to count only
matching-lane non-ignored Rust tests that execute official fixtures, then
promoted the Node24 `core-semantics` broad group from ignored watchpoint to
regular green coverage.

The row is not done. Node24 is now `892 / 5198` full-corpus official fixtures
passed, and the NDS3 closeout gate remains `>= 2000`.

## Broad Pre-Run

Wide command:

```console
cargo test -p nimbus-runtime node24_default_lane_ -- --ignored --nocapture --test-threads=1
```

Observed before fixes:

| Broad group | Passed | Skipped | Failed | Failure inventory |
| --- | ---: | ---: | ---: | --- |
| `core-semantics` | 117 | 1 | 5 | `test-console-clear.js`, `test-events-add-abort-listener.mjs`, `test-url-parse-format.js`, `test-url-parse-invalid-input.js`, `test-url-pathtofileurl.js` |
| `loader-context` | 167 | 0 | 10 | `test-module-loading-globalpaths.js`, `test-async-hooks-*.js` count fixtures, `test-crypto-random.js`, `test-crypto-cipheriv-decipheriv.js`, `test-v8-version-tag.js`, `test-v8-serdes.js`, `test-v8-stats.js`, `test-v8-flag-type-check.js` |
| `networking` | 258 | 0 | 10 | `test-http-response-statuscode.js`, `test-http-response-splitting.js`, `test-https-client-get-url.js`, `test-http2-util-update-options-buffer.js`, `test-https-agent-sni.js`, `test-https-client-override-global-agent.js`, `test-https-resume-after-renew.js`, `test-https-pfx.js`, `test-https-strict.js`, `test-https-agent-keylog.js` |
| `process-and-timing` | 45 | 0 | 3 | `test-process-load-env-file.js`, `test-util-format.js`, `test-perf-hooks-resourcetiming.js` |
| `streams-and-local-io` | 300 | 0 | 8 | `test-stream-duplex-from.js`, `test-fs-append-file.js`, `test-fs-whatwg-url.js`, `test-fs-mkdir.js`, `test-fs-statfs.js`, `test-fs-truncate.js`, `test-fs-watch-enoent.js`, `test-fs-watch-encoding.js` |

The `core-semantics` skipped fixture is
`test/parallel/test-buffer-tostring-rangeerror.js`; the official fixture
self-skips under the current host memory limit.

## Failure Grouping

The first focused wave grouped the failures this way:

| Cluster | Fixtures | Owner repo | Fix path |
| --- | --- | --- | --- |
| Console TTY clearing semantics | `test-console-clear.js`, regression guard `test-console-methods.js` | `nimbus/nimbus` | Runtime bootstrap shim for `console.clear` that preserves Node method descriptor behavior. |
| Abort listener propagation semantics | `test-events-add-abort-listener.mjs` | `nimbus/deno` | Fork fix in Deno's Node events polyfill so `events.addAbortListener` is not blocked by user `stopImmediatePropagation`. |
| Legacy URL parser/path conversion semantics | `test-url-parse-format.js`, `test-url-parse-invalid-input.js`, `test-url-pathtofileurl.js` | `nimbus/deno` | Fork fix in Deno's Node URL polyfill for legacy host parsing, invalid-port warning, control-character stripping, and invalid Windows UNC hosts. |
| Dotenv fixture support | `test-process-load-env-file.js` | `nimbus/nimbus` | Vendor the official `.env` support fixture required by the manifest entry. |
| Streams/local I/O semantics | `test-stream-duplex-from.js`, `test-fs-append-file.js`, `test-fs-whatwg-url.js`, `test-fs-mkdir.js`, `test-fs-statfs.js`, `test-fs-truncate.js`, `test-fs-watch-enoent.js`, `test-fs-watch-encoding.js`, plus the broad-rerun-discovered `test-fs-glob.mjs` and `test-fs-readfile-flags.js` | `nimbus/nimbus`, `nimbus/deno` | Fix stream duplex abort/destroy propagation in the Deno fork; align statfs, appendFile validation, relative cwd path resolution, deleted-cwd mkdir/rmdir behavior, fs.watch no-entry/encoding behavior, symlink target semantics, and async open `EEXIST` normalization. |
| Remaining Node24 broad failures | loader and networking fixtures listed above | mixed | Still open for later NDS3 focused waves. |

NDS3 also found an evidence-accounting bug: the prior `1002` Node24 pass count
was a source-topology count that included non-executing metadata/topology tests
and cross-lane references. The corrected pass numerator is now:

- non-ignored Rust tests only;
- the Rust test body must execute Node compatibility fixtures;
- the inferred fixture lane must match the reported lane;
- ignored watchpoints never count as passed;
- explicitly classified expected failures, known gaps, and runtime skips are
  not pass claims.

This correction lowered the honest Node24 execution numerator before fixes from
the historical `1002` source-topology number to `160`. After the first NDS3
promotion wave, Node24 is `276 / 5198`.

## Focused Work

Implemented local Nimbus fixes:

- `console.clear()` now writes the Node-compatible TTY clear sequence through
  the mutable process stdout object while preserving non-constructable console
  method behavior.
- `test/fixtures/dotenv/.env` is vendored for
  `test-process-load-env-file.js`.
- Direct Node24 fixture tests were added for console, events, URL, and dotenv
  coverage.
- `node24_default_lane_core_semantics_watchpoint` was renamed to
  `node24_default_lane_executes_core_semantics_subset` and unignored after the
  broad group was green.

Promoted fixes to the canonical Deno fork:

| Tag | Commit | Fix |
| --- | --- | --- |
| `v2.8.0-nimbus.10` | `ae79fb3e4b` | Align `events.addAbortListener` with Node abort-listener propagation semantics. |
| `v2.8.0-nimbus.11` | `5099d87414` | Align legacy Node URL parsing and `pathToFileURL(..., { windows: true })` error behavior. |
| `v2.8.0-nimbus.12` | `843e485fb9` | Align ArrayBuffer and SharedArrayBuffer inspect output with Node's non-enumerable `[byteLength]` label. |
| `v2.8.0-nimbus.13` | `663306f565` | Improve Node stream duplex lifecycle and local filesystem compatibility for statfs, appendFile validation, cwd-relative path handling, watch behavior, symlink target handling, and async open error normalization. |
| `v2.8.0-nimbus.14` | `d1c53e4315` | Improve Node24 networking compatibility: abort-listener observability, raw `writeHead` header validation, invalid status-code errors, `NODE_TLS_REJECT_UNAUTHORIZED`, PFX extraction, TLS keylog events, HTTP/2 state buffers, SNI preservation, `https.globalAgent` reassignment, and SecureContext ticket callback shape. |

Nimbus is repinned to `v2.8.0-nimbus.14` in `Cargo.toml` and `Cargo.lock`.

Focused verification:

```console
cargo test -p nimbus-runtime node24_console_ -- --nocapture
cargo test -p nimbus-runtime node24_events_add_abort_listener_fixture -- --nocapture
cargo test -p nimbus-runtime node24_url_ -- --nocapture
cargo test -p nimbus-runtime node24_process_load_env_file_fixture -- --nocapture
cargo test -p nimbus-runtime node24_fs_ -- --nocapture --test-threads=1
cargo test -p nimbus-runtime node24_stream_duplex_from_fixture -- --nocapture --test-threads=1
cargo test -p nimbus-runtime node24_fs_readfile_flags_fixture -- --nocapture --test-threads=1
```

Observed:

- `node24_console_`: `2 passed`.
- `node24_events_add_abort_listener_fixture`: `1 passed`.
- `node24_url_`: `3 passed`.
- `node24_process_load_env_file_fixture`: `1 passed`.
- `node24_fs_`: `8 passed, 7 ignored, 0 failed`.
- `node24_stream_duplex_from_fixture`: `1 passed`.
- `node24_fs_readfile_flags_fixture`: `1 passed`.

## Broad Final Rerun

Core-semantics broad rerun:

```console
cargo test -p nimbus-runtime node24_default_lane_executes_core_semantics_subset -- --nocapture --test-threads=1
```

Observed: `122 passed, 1 skipped, 0 failed`.

Process/timing partial rerun:

```console
cargo test -p nimbus-runtime node24_default_lane_process_and_timing_watchpoint -- --ignored --nocapture --test-threads=1
```

Observed: `46 passed, 0 skipped, 2 failed`.

Remaining process/timing failures:

- `test-util-format.js`
- `test-perf-hooks-resourcetiming.js`

Process/timing focused verification:

```console
cargo test -p nimbus-runtime node24_util_format_fixture -- --nocapture
cargo test -p nimbus-runtime node24_perf_hooks_resourcetiming_fixture -- --nocapture
```

Observed:

- `node24_util_format_fixture`: `1 passed`.
- `node24_perf_hooks_resourcetiming_fixture`: `1 passed`.

Process/timing broad promoted rerun:

```console
cargo test -p nimbus-runtime node24_default_lane_executes_process_and_timing_subset -- --nocapture --test-threads=1
```

Observed: `48 passed, 0 skipped, 0 failed`.

Streams/local I/O broad rerun before promotion:

```console
cargo test -p nimbus-runtime node24_default_lane_streams_and_local_io_watchpoint -- --ignored --nocapture --test-threads=1
```

Observed before the final focused fix: `307 passed, 0 skipped, 1 failed`.

Remaining failure:

- `test-fs-readfile-flags.js` expected `EEXIST` for exclusive-create readFile
  flags on an existing file, but async `open()` normalization returned
  `ENOENT`.

Focused fix:

- Added `node24_fs_readfile_flags_fixture`.
- Fixed Deno fork async `open()` normalization to stat the target and return
  `EEXIST` when `O_EXCL` opens an existing file.

Streams/local I/O broad rerun on the published Deno tag:

```console
cargo test -p nimbus-runtime node24_default_lane_streams_and_local_io_watchpoint -- --ignored --nocapture --test-threads=1
```

Observed: `308 passed, 0 skipped, 0 failed`.

Promoted non-ignored broad rerun:

```console
cargo test -p nimbus-runtime node24_default_lane_executes_streams_and_local_io_subset -- --nocapture --test-threads=1
```

Observed: `308 passed, 0 skipped, 0 failed`.

Networking broad rerun before fixes:

```console
cargo test -p nimbus-runtime node24_default_lane_networking_watchpoint -- --ignored --nocapture --test-threads=1
```

Observed: `254 passed, 0 skipped, 14 failed`.

Failure inventory:

- `test-http-agent-abort-controller.js`
- `test-http-response-statuscode.js`
- `test-http-response-splitting.js`
- `test-https-agent-abort-controller.js`
- `test-https-client-get-url.js`
- `test-http2-util-update-options-buffer.js`
- `test-https-agent-sni.js`
- `test-https-client-override-global-agent.js`
- `test-https-abortcontroller.js`
- `test-https-resume-after-renew.js`
- `test-https-pfx.js`
- `test-https-strict.js`
- `test-https-agent-keylog.js`
- `test-tls-connect-abort-controller.js`

Focused networking fixes:

- Deno `events.addAbortListener` now uses observable abort listeners while
  retaining stop-immediate-propagation resistance, fixing the HTTP/HTTPS/net/TLS
  abort-controller listener-count fixtures.
- Deno raw `ServerResponse.writeHead(status, headers)` now validates external
  header names and values before storing them, fixing response-splitting
  protection and odd flat-array validation.
- Deno invalid HTTP status-code errors now use Node's
  `ERR_HTTP_INVALID_STATUS_CODE` formatting.
- Deno TLS now reads `NODE_TLS_REJECT_UNAUTHORIZED` live and emits the Node
  warning once.
- Deno `SecureContext` now validates and extracts PFX/PKCS#12 cert/key/CA
  material for rustls-backed TLS.
- Deno TLS keylog lines are bridged into Node-style `keylog` events.
- Deno `internalBinding("http2")` now exposes runtime-bound HTTP/2 state
  buffers used by `internal/http2/util`.
- Deno TLS preserves Node/OpenSSL SNI behavior for names rustls rejects by
  using a same-length valid placeholder internally and restoring the plaintext
  ClientHello SNI.
- Deno `node:https` exposes a mutable accessor-backed `globalAgent`.
- Deno `SecureContext.context.enableTicketKeyCallback()` exists as a
  brand-checked no-op hook while rustls owns ticket rotation.

Focused networking verification:

```console
cargo test -p nimbus-runtime abort_controller_fixture -- --nocapture --test-threads=1
cargo test -p nimbus-runtime node24_http_response_statuscode_fixture -- --nocapture --test-threads=1
cargo test -p nimbus-runtime node24_http_response_splitting_fixture -- --nocapture --test-threads=1
cargo test -p nimbus-runtime node24_https_client_get_url_fixture -- --nocapture --test-threads=1
cargo test -p nimbus-runtime node24_https_strict_fixture -- --nocapture --test-threads=1
cargo test -p nimbus-runtime node24_https_pfx_fixture -- --nocapture --test-threads=1
cargo test -p nimbus-runtime node24_https_agent_keylog_fixture -- --nocapture --test-threads=1
cargo test -p nimbus-runtime node24_http2_util_update_options_buffer_fixture -- --nocapture --test-threads=1
cargo test -p nimbus-runtime node24_https_agent_sni_fixture -- --nocapture --test-threads=1
cargo test -p nimbus-runtime node24_https_client_override_global_agent_fixture -- --nocapture --test-threads=1
cargo test -p nimbus-runtime node24_https_resume_after_renew_fixture -- --nocapture --test-threads=1
```

Observed: all focused networking fixtures passed.

Networking broad rerun on local Deno path:

```console
cargo test -p nimbus-runtime node24_default_lane_networking_watchpoint -- --ignored --nocapture --test-threads=1
```

Observed: `268 passed, 0 skipped, 0 failed`.

Networking broad rerun after publishing and repinning to
`v2.8.0-nimbus.14`:

```console
cargo test -p nimbus-runtime node24_default_lane_networking_watchpoint -- --ignored --nocapture --test-threads=1
```

Observed: `268 passed, 0 skipped, 0 failed`.

Promoted non-ignored networking rerun:

```console
cargo test -p nimbus-runtime node24_default_lane_networking_watchpoint -- --nocapture --test-threads=1
```

Observed: `268 passed, 0 skipped, 0 failed`.

Evidence generator correction:

- Promoting the networking batch exposed an overcount: the status generator
  expanded explicit `NodeCompatBatchEntry` values without honoring
  `node24_fixture_source_path: None`, so it reported `270` networking fixtures
  even though the runner executed `268`.
- `scripts/runtime/node/classifications.py` now masks explicit batch-entry
  structs and counts a fixture only when the lane-specific fixture source is
  `Some(...)`.
- The corrected status reports Node24 networking as `268`, matching the
  runner output. This also corrected historical Node20/Node22 totals where
  lane-disabled entries had been counted as green.

Regenerated evidence after the networking promotion and generator correction:

```console
python3 scripts/runtime/node/classifications.py sync --lane all
python3 scripts/runtime/node/watchpoints.py sync
make node-compat-status
make node-compat-dashboard
make node-compat-publish-evidence
python3 scripts/runtime/node/default_support_posture.py
make node-compat-publish-docs
```

Current control-plane verifier:

```console
bash scripts/verify-node-default-runtime-support-hardening.sh
```

Observed: `15 passed, 19 failed`. The remaining failures are the expected NDS3
closeout/future-row gates: Node24 still needs `>= 2000` full-corpus passes,
Node22 parity is not yet proven for the final NDS3 denominator, Node26 and
NDS5..NDS10 are still pending, and the plan is not closed.

Current generated full-corpus official fixture posture:

| Lane | Passed | Vendored | Pass rate |
| --- | ---: | ---: | ---: |
| `node20` | 893 | 1308 | 68.3% |
| `node22` | 1023 | 4773 | 21.4% |
| `node24` | 892 | 5198 | 17.2% |
| `node26` | 0 | 5578 | 0.0% |

Current Node24 default-support posture:

| Metric | Count |
| --- | ---: |
| Current passed | 892 |
| Required gaps | 1477 |
| Optional promotable gaps | 422 |
| Diagnostic gaps | 1781 |
| Harness-only gaps | 602 |
| Upstream/platform gaps | 24 |
| Estimated reachable pass ceiling | 2791 |

## Evidence Links

- `docs/architecture/runtime/node-compat-evidence/latest/status-summary.md`
- `docs/architecture/runtime/node-default-support-posture.md`
- `docs/runtimes/nodejs/evidence/node24.md`
- `tests/runtime/node/classifications/node24.json`
- `tests/runtime/node/expectations/rust-watchpoints.json`
- `crates/nimbus-runtime/src/runtime/tests/node/cases/watchpoints_core.rs`
- `crates/nimbus-runtime/src/runtime/tests/node/cases/watchpoints_extended.rs`
- `crates/nimbus-runtime/src/runtime/bootstrap/js/node22_runtime_bootstrap.js`
- Deno fork tag `v2.8.0-nimbus.14` (`d1c53e4315fff77c279db3c68933fab6ec2e84d5`)

## Residual Risks

- NDS3 is still far below the `2000` Node24 full-corpus closeout threshold.
- The next focused waves must continue from the broad failure inventory rather
  than adding isolated green tests disconnected from the official corpus.
- Remaining NDS3 work is concentrated in loader/context plus additional
  non-foundation fixture clusters outside the promoted Node24 networking
  manifest.
  They must be fixed, reclassified as stricter non-isolate diagnostics, or moved
  to a documented blocked state with exact fixture ownership.
- Node22 parity is not yet proven for the final NDS3 state; the proof must
  include a same-denominator Node22 rerun before NDS3 can close.
