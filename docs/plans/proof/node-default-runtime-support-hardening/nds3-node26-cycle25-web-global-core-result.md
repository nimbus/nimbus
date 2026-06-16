# NDS3 node26 cycle 25 - Web/global/core promotion

Date: 2026-06-15
Worktree: `/Users/jack/src/github.com/nimbus/nimbus-worktrees/node-default-runtime-support-hardening`
Branch / PR: `codex/node-default-runtime-support-hardening` / PR #10

## Result

This wave burns seven Node26 Current required gaps across coherent web/global,
core-semantics, and URL inspect surfaces. Node26 `v8_isolate_required` posture
moved from `106` gaps / `94.95%` (`1991 / 2097`) to `99` gaps / `95.28%`
(`1998 / 2097`). Node22 and Node24 remain green at `0` gaps / `100.0%`.

The Deno fork was advanced from `v2.8.3-nimbus.57` to published lightweight tag
`v2.8.3-nimbus.58`:

- Deno branch: `/Users/jack/src/github.com/nimbus/deno`, `nimbus/v2.8.3`
- Deno commit: `cf321f2394ffd51ca56fffe7636f52beb7174f2a`
- Deno commit subject: `node: align Node26 assert and URL semantics`
- Published tag: `v2.8.3-nimbus.58`

Nimbus is pinned to immutable `https://github.com/nimbus/deno`, tag
`v2.8.3-nimbus.58`. `Cargo.lock` records
`#cf321f2394ffd51ca56fffe7636f52beb7174f2a`.

rusty_v8 was unchanged at `v149.4.0-nimbus.2`
(`8f70a59de9b1b1db41996e2ac1c68eede4449208`).

No V8 or rusty_v8 changes were made. No official upstream Node fixture or
checker was edited. No generated JSON was hand-edited to fake a green. No
`git add -A` was used.

## Fixed Surfaces

The Deno fork changes cover:

- Node26 `assert` tuple/printf/function/error message semantics for equality
  methods plus `match` and `doesNotMatch`.
- Node26 assert diff inspect behavior for symbol-key output and proxy target
  formatting.
- Node26 `url.parse()` `DEP0169` warning behavior outside `node_modules`, once
  per process.

The Nimbus-local bootstrap changes cover:

- In-memory `sessionStorage` exposure for the isolate global.
- `EventSource` exposure only when `process.execArgv` contains
  `--experimental-eventsource`.
- Node-major-aware URL custom inspect context-label normalization.
- Preservation of the computed `Symbol(nodejs.util.inspect.custom)` method name
  on URL custom inspect wrappers.
- Node-like `ERR_INVALID_ARG_TYPE` behavior for
  `performance.setResourceTimingBufferSize()` BigInt/Symbol inputs.
- Parser/test support for `--experimental-eventsource`.

## Local Deno-Path Proof

The local Deno-path focused batches proved the Deno fork implementation before
tagging:

```bash
NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nds-node26-wave25-local-deno-core-semantics4 \
  cargo test -p nimbus-runtime --lib node26_current_lane_core_semantics_util_required_gap_watchpoint -- --ignored --nocapture
# selected=3, passed=3, skipped=0, failed=0

NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nds-node26-wave25-local-wave-event2 \
  cargo test -p nimbus-runtime --lib node26_current_lane_event_required_gap_watchpoint -- --ignored --nocapture
# selected=1, passed=1, skipped=0, failed=0

NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nds-node26-wave25-local-wave-parallel-js3 \
  cargo test -p nimbus-runtime --lib node26_current_lane_parallel_js_platform_required_gap_watchpoint -- --ignored --nocapture
# selected=2, passed=2, skipped=0, failed=0

NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nds-node26-wave25-local-wave-url-inspect5 \
  cargo test -p nimbus-runtime --lib node26_current_lane_whatwg_web_platform_required_gap_watchpoint -- --ignored --nocapture
# selected=1, passed=1, skipped=0, failed=0
```

## Immutable-Tag Proof

After Deno commit/tag/push and Nimbus repin to `v2.8.3-nimbus.58`, the same
focused batches were rerun on the immutable tag:

```bash
NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nds-node26-wave25-tag-deno-core-semantics1 \
  cargo test -p nimbus-runtime --lib node26_current_lane_core_semantics_util_required_gap_watchpoint -- --ignored --nocapture
# selected=3, passed=3, skipped=0, failed=0

NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nds-node26-wave25-tag-deno-event1 \
  cargo test -p nimbus-runtime --lib node26_current_lane_event_required_gap_watchpoint -- --ignored --nocapture
# selected=1, passed=1, skipped=0, failed=0

NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nds-node26-wave25-tag-deno-parallel-js1 \
  cargo test -p nimbus-runtime --lib node26_current_lane_parallel_js_platform_required_gap_watchpoint -- --ignored --nocapture
# selected=2, passed=2, skipped=0, failed=0

NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nds-node26-wave25-tag-deno-url-inspect1 \
  cargo test -p nimbus-runtime --lib node26_current_lane_whatwg_web_platform_required_gap_watchpoint -- --ignored --nocapture
# selected=1, passed=1, skipped=0, failed=0
```

The first URL inspect promoted batch caught a Nimbus wrapper regression:

```bash
NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nds-node26-wave25-promoted-whatwg1 \
  cargo test -p nimbus-runtime --lib node26_current_lane_executes_whatwg_web_platform_promoted_batch_fixture -- --nocapture
# selected=46, passed=45, skipped=0, failed=1
```

`test/parallel/test-whatwg-url-properties.js` expected the custom inspect
descriptor value name to remain `[nodejs.util.inspect.custom]`, but the wrapper
had changed it to `value`. The wrapper was changed to use computed-symbol
method definitions, and the focused immutable-tag URL inspect batch was rerun:

```bash
NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nds-node26-wave25-tag-deno-url-inspect2 \
  cargo test -p nimbus-runtime --lib node26_current_lane_whatwg_web_platform_required_gap_watchpoint -- --ignored --nocapture
# selected=1, passed=1, skipped=0, failed=0
```

## Promoted Batch Proof

After promotion into the non-ignored Node26 batches:

```bash
NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nds-node26-wave25-promoted-whatwg2 \
  cargo test -p nimbus-runtime --lib node26_current_lane_executes_whatwg_web_platform_promoted_batch_fixture -- --nocapture
# selected=46, passed=46, skipped=0, failed=0

NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nds-node26-wave25-promoted-event1 \
  cargo test -p nimbus-runtime --lib node26_current_lane_executes_event_promoted_batch_fixture -- --nocapture
# selected=37, passed=37, skipped=0, failed=0

NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nds-node26-wave25-promoted-parallel-js1 \
  cargo test -p nimbus-runtime --lib node26_current_lane_executes_parallel_js_platform_promoted_batch_fixture -- --nocapture
# selected=51, passed=51, skipped=0, failed=0

NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nds-node26-wave25-promoted-core1 \
  cargo test -p nimbus-runtime --lib node26_current_lane_executes_core_semantics_util_promoted_batch_fixture -- --nocapture
# selected=27, passed=27, skipped=0, failed=0
```

Promoted paths from this wave:

- `test/parallel/test-assert-deep.js`
- `test/parallel/test-assert.js`
- `test/parallel/test-eventsource.js`
- `test/parallel/test-global.js`
- `test/parallel/test-performance-resourcetimingbuffersize.js`
- `test/parallel/test-url-parse-deprecation.js`
- `test/parallel/test-whatwg-url-custom-inspect.js`

## Generator and Integrity Checks

```bash
/opt/homebrew/bin/python3.12 scripts/runtime/node/classifications.py sync --lane all
# wrote node20, node22, node24, node26 classification catalogs

/opt/homebrew/bin/python3.12 scripts/runtime/node/watchpoints.py sync
# wrote tests/runtime/node/expectations/rust-watchpoints.json

/opt/homebrew/bin/python3.12 scripts/runtime/node/status.py
# wrote target/node-compat/status/status-summary.{json,md}

/opt/homebrew/bin/python3.12 scripts/runtime/node/dashboard.py
# wrote target/node-compat/dashboard/dashboard-summary.{json,md}

/opt/homebrew/bin/python3.12 scripts/runtime/node/trends.py
# wrote target/node-compat/trends/trend-summary.{json,md}

/opt/homebrew/bin/python3.12 scripts/runtime/node/publish_evidence.py
# published tests/runtime/node/compat/node-compat-evidence/latest/*

/opt/homebrew/bin/python3.12 scripts/runtime/node/default_support_posture.py
# wrote private and public node-default-support-posture artifacts

/opt/homebrew/bin/python3.12 scripts/runtime/node/required_surface_blockers.py
# node22 required gaps: 0
# node24 required gaps: 0

/opt/homebrew/bin/python3.12 -B scripts/runtime/node/classifications.py sync --preserve-existing --check
# node20.json, node22.json, node24.json, node26.json are up to date

/opt/homebrew/bin/python3.12 -B scripts/runtime/node/default_support_posture.py --check
# node default support posture: pass

/opt/homebrew/bin/python3.12 -B scripts/runtime/node/required_surface_blockers.py --check
# node required-surface blocker inventory: pass

/opt/homebrew/bin/python3.12 -B scripts/runtime/node/watchpoints.py validate
# validated node-compat watchpoint catalog: 151 entries

cargo fmt --all --check
# no output

/opt/homebrew/bin/python3.12 -B scripts/runtime/node/docs_guard.py
# Node LTS docs guard passed: public docs avoid stale pass-rate, support-priority, and host-heavy overclaim prose

git diff --check
# no output
```

Two broader docs checks were attempted and remain red for known repository
state outside this wave:

```bash
npm run docs:validate-refs:strict
# 32 broken reference(s) found
```

The broken references are missing private/staging docs links in package README
and node-compat reference files, not files changed by this wave.

```bash
make node-compat-publish-docs CHECK=1
# stale Node.js runtime evidence docs:
# tests/runtime/node/published/nodejs/evidence/latest.md
# tests/runtime/node/published/nodejs/evidence/node22.md
# tests/runtime/node/published/nodejs/evidence/node24.md
# tests/runtime/node/published/nodejs/evidence/node26.md
# FileNotFoundError: docs/architecture/runtime/node-isolate-shim-inventory.json
```

This matches the known `publish_docs.py` blocker already recorded in
`nds3-cycle85-webcrypto-aes-encrypt-decrypt-result.md`: the worktree lacks the
untracked public shim inventory input required by the publish-docs renderer.
The gate-critical classification, posture, blocker, watchpoint, and latest
evidence artifacts were regenerated and validated.

Regenerated public posture:

- Node22 `v8_isolate_required`: `0` gaps, `100.0%`, `2363 / 2363`.
- Node24 `v8_isolate_required`: `0` gaps, `100.0%`, `2400 / 2400`.
- Node26 `v8_isolate_required`: `99` gaps, `95.28%`, `1998 / 2097`.

Verifier checkpoint:

```bash
bash scripts/verify-node-default-runtime-support-hardening.sh
# Summary: 14 passed, 20 failed
```

Step 9 remains green for Node22/Node24. The verifier remains red honestly
because the broader NDS closeout proof rows are incomplete and Node26 still has
`99` required gaps.

## Diagnostics

Diagnostic roots retained:

- `/private/tmp/nds-node26-wave25-local-deno-core-semantics4`
- `/private/tmp/nds-node26-wave25-local-wave-event2`
- `/private/tmp/nds-node26-wave25-local-wave-parallel-js3`
- `/private/tmp/nds-node26-wave25-local-wave-url-inspect5`
- `/private/tmp/nds-node26-wave25-tag-deno-core-semantics1`
- `/private/tmp/nds-node26-wave25-tag-deno-event1`
- `/private/tmp/nds-node26-wave25-tag-deno-parallel-js1`
- `/private/tmp/nds-node26-wave25-tag-deno-url-inspect1`
- `/private/tmp/nds-node26-wave25-tag-deno-url-inspect2`
- `/private/tmp/nds-node26-wave25-promoted-whatwg1`
- `/private/tmp/nds-node26-wave25-promoted-whatwg2`
- `/private/tmp/nds-node26-wave25-promoted-event1`
- `/private/tmp/nds-node26-wave25-promoted-parallel-js1`
- `/private/tmp/nds-node26-wave25-promoted-core1`

## Next Action

Continue the Node26 required-surface burndown from the regenerated posture and
classification catalog. The remaining Node26 `v8_isolate_required` count is
`99`; prioritize broad coherent clusters over singleton cycles, especially the
remaining async lifecycle, module loader, crypto/networking, web platform, and
fs/stream groups.
