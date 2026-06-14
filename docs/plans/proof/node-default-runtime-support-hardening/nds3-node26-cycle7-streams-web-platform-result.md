# NDS3 Node26 Cycle 7: Streams and WebStreams Promotion

## Scope

This checkpoint burns Node26 Current required-surface gaps in the
`streams-local-io/stream` and stream-shaped `node-compat/unpromoted-surface`
area. It adds a Node26 broad ignored streams/WebStreams watchpoint, promotes
only the dynamically stable Node26 fixture paths, and keeps broad skips and
residual failures counted as gaps. No Deno fork changes, rusty_v8 changes,
fixture edits, checker edits, or generated false-green JSON hand edits were
made.

Before this wave, Node26 `v8_isolate_required` posture was `855` gaps /
`61.12%`.

## Broad Pre-Run

Command:

```bash
NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nds-node26-streams-web-platform-wave1 \
  cargo test -p nimbus-runtime --lib node26_current_lane_streams_web_platform_watchpoint -- --ignored --nocapture
```

Result:

- Rust test result: failed, as expected for a broad diagnostic batch with
  residual failures and structural skips.
- Fixture summary: `selected=188`, `passed=73`, `skipped=69`, `failed=46`.
- Summary artifact:
  `/private/tmp/nds-node26-streams-web-platform-wave1/batch/node26__node26_current_lane_streams_web_platform_watchpoint__summary.json`

## Promoted Fixtures

The initial 73 broad-batch passes were tested as an enforced promotion batch.
One broad pass, `test/parallel/test-stream2-basic.js`, failed in the enforced
batch and was held back rather than promoted.

First promotion attempt:

```bash
NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nds-node26-streams-web-platform-promote1 \
  cargo test -p nimbus-runtime --lib node26_current_lane_executes_streams_web_platform_promoted_batch_fixture -- --nocapture
```

Result:

- Rust test result: failed.
- Fixture summary: `selected=73`, `passed=72`, `skipped=0`, `failed=1`.
- Failed fixture: `test/parallel/test-stream2-basic.js`
- Summary artifact:
  `/private/tmp/nds-node26-streams-web-platform-promote1/batch/node26__node26_current_lane_executes_streams_web_platform_promoted_batch__summary.json`

The stable 72 paths were then enforced by
`node26_current_lane_executes_streams_web_platform_promoted_batch_fixture`.

Final promotion command:

```bash
NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nds-node26-streams-web-platform-promote2 \
  cargo test -p nimbus-runtime --lib node26_current_lane_executes_streams_web_platform_promoted_batch_fixture -- --nocapture
```

Result:

- Rust test result: `1 passed; 0 failed; 0 ignored; 930 filtered out`.
- Fixture summary: `selected=72`, `passed=72`, `skipped=0`, `failed=0`.
- Summary artifact:
  `/private/tmp/nds-node26-streams-web-platform-promote2/batch/node26__node26_current_lane_executes_streams_web_platform_promoted_batch__summary.json`

## Failure Grouping

The 69 skipped broad-batch fixtures are all QUIC stream fixtures that self-skip
with `QUIC is not enabled`. They were not promoted in this wave.

The 46 non-promoted broad-batch failures group as follows:

- Missing `stream/iter`: 38 failures.
  - Dominant symptom: `Cannot find module 'stream/iter'`.
  - `test-stream-iter-disabled.js` additionally exposes unsupported
    subprocess flag handling for `--experimental-stream-iter`.
- AsyncResource / AsyncLocalStorage propagation through `stream.finished`: 2
  failures.
  - `test-stream-finished-async-local-storage.js`
  - `test-stream-finished-bindAsyncResource-path.js`
- Core stream behavior mismatches: 3 failures.
  - `test-stream-readable-readable-one.js`: buffer read size/reference mismatch.
  - `test-stream2-basic.js`: unstable chunk ordering/length mismatch in the
    enforced promotion batch.
  - `test-stream2-transform.js`: transform output mismatch (`abcdef` vs `abc`).
- WebStreams / inspection / termination mismatches: 3 failures.
  - `test-webstream-encoding-inspect.js`: inspect formatting and writable state
    shape mismatch.
  - `test-webstream-readable-from.js`: missing `ERR_ARG_NOT_ITERABLE` code.
  - `test-webstream-structured-clone-no-leftovers.mjs`: pending promise after
    event-loop resolution.
- WebStreams abort-controller singleton: 1 failure.
  - `test-webstreams-abort-controller.js`: JavaScript execution terminates while
    evaluating a dynamic import.

## Generated Evidence

Commands:

```bash
/opt/homebrew/bin/python3.12 scripts/runtime/node/classifications.py sync --lane all
/opt/homebrew/bin/python3.12 scripts/runtime/node/watchpoints.py sync
/opt/homebrew/bin/python3.12 scripts/runtime/node/watchpoints.py validate
/opt/homebrew/bin/python3.12 scripts/runtime/node/status.py
/opt/homebrew/bin/python3.12 scripts/runtime/node/dashboard.py
/opt/homebrew/bin/python3.12 scripts/runtime/node/trends.py
/opt/homebrew/bin/python3.12 scripts/runtime/node/publish_evidence.py
/opt/homebrew/bin/python3.12 scripts/runtime/node/default_support_posture.py
/opt/homebrew/bin/python3.12 scripts/runtime/node/required_surface_blockers.py
```

Results:

- `scripts/runtime/node/watchpoints.py validate`: `validated node-compat watchpoint catalog: 141 entries`
- `tests/runtime/node/compat/node-compat-evidence/latest/status-summary.json`: warnings `0`
- `tests/runtime/node/compat/node-compat-evidence/latest/dashboard-summary.json`: warnings `0`
- `scripts/runtime/node/required_surface_blockers.py`: `node22 required gaps: 0`, `node24 required gaps: 0`

Posture after regeneration:

- Node22 `v8_isolate_required`: `0` gaps, `100.0%`
- Node24 `v8_isolate_required`: `0` gaps, `100.0%`
- Node26 `v8_isolate_required`: `783` gaps, `64.39%`

The Node26 count moved from `855` gaps / `61.12%` to `783` gaps /
`64.39%`, burning 72 required-surface gaps in this wave.

## Next Node26 Work

The best stream follow-up is a Deno fork implementation wave for `stream/iter`,
which accounts for 38 of the residual failures and also explains fs-host-io
tail failures from cycle 5. The QUIC stream skips should remain unpromoted
until there is an explicit structural disposition or QUIC support decision.

Other high-yield Node26 waves remain `process-and-timing/timers`, loader/VM,
networking/http/http2/tls, and the CommonJS named-export interop root cause
seen in the fs-host-io tail.
