# NDS3 Node26 Cycle 13: module loader promotion

## Scope

This checkpoint burns Node26 Current required-surface gaps in the ESM/CJS
module-loader cluster. It starts from the regenerated Node26 posture after cycle
12, runs the broad ignored module-loader blocker batch, and promotes only the
fixtures that were dynamically green in that broad batch and then green again in
an enforced non-ignored promoted batch.

No V8 or rusty_v8 changes, fixture edits, checker edits, Deno fork edits, local
Deno pins, or generated false-green JSON hand edits were made in this cycle.
Nimbus remained pinned to the published immutable Deno tag
`v2.8.3-nimbus.49`.

Before this wave, Node26 `v8_isolate_required` posture was `558` gaps /
`74.62%`.

## Broad Pre-Run

Immutable `v2.8.3-nimbus.49` broad module-loader pre-run:

```bash
NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nds-node26-module-loader-wave1-tag49 \
  cargo test -p nimbus-runtime --lib node26_current_lane_module_loader_required_surface_blocker_watchpoint -- --ignored --nocapture
```

Result:

- Rust test result: failed, as expected for the broad diagnostic batch.
- Fixture summary: `selected=148`, `passed=100`, `skipped=1`, `failed=47`.
- Skipped fixture: `test/parallel/test-util-styletext.js`
- Summary artifact:
  `/private/tmp/nds-node26-module-loader-wave1-tag49/batch/node26__node26_current_lane_module_loader_required_surface_blocker_watchpoint__summary.json`
- Diagnostic root:
  `/private/tmp/nds-node26-module-loader-wave1-tag49`

The 100 broad passes became the only promotion candidates. The 47 failures and
1 skip remain required-surface red paths until a focused fix or honest
source-confirmed reclassification proves otherwise.

## Promotion Proof

Immutable `v2.8.3-nimbus.49` enforced promoted batch:

```bash
NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nds-node26-module-loader-wave1-tag49-promote1 \
  cargo test -p nimbus-runtime --lib node26_current_lane_executes_esm_module_loader_promoted_batch_fixture -- --nocapture
```

Result:

- Rust test result: `1 passed; 0 failed; 0 ignored`.
- Fixture summary: `selected=100`, `passed=100`, `skipped=0`, `failed=0`.
- Summary artifact:
  `/private/tmp/nds-node26-module-loader-wave1-tag49-promote1/batch/node26__node26_current_lane_executes_esm_module_loader_promoted_batch__summary.json`

## Promoted Fixtures

The 100 promoted paths are the exact entries added to
`ESM_MODULE_LOADER_PROMOTED_NODE26_PATHS` in
`crates/nimbus-runtime/src/runtime/tests/node/cases/watchpoints_extended.rs`.
They cover stable Node26 ESM basics, import attributes, import-meta,
JSON/WASM module loading, symlink and package-type handling, selected
`require(esm)` TLA cases, and two module-related `test/parallel` fixtures.

The new non-ignored Rust test
`node26_current_lane_executes_esm_module_loader_promoted_batch_fixture` now
enforces those 100 paths against the Node26 lane with the existing ESM module
loader support files and extra fixture directories.

## Remaining Module-Loader Failures

The broad run preserved diagnostics for the 47 failed paths under
`/private/tmp/nds-node26-module-loader-wave1-tag49/general`. Representative
root-cause buckets from those diagnostics:

- ESM/CJS require interop race and cache semantics:
  `ERR_REQUIRE_ESM_RACE_CONDITION` in fixtures such as
  `test/es-module/test-require-module.js`.
- Module-hook callback contract mismatches: for example
  `test/module-hooks/test-module-hooks-resolve-load-builtin-redirect.js`
  reported `resolve` called twice where Node expected exactly one call.
- Native addon / FFI boundary: `test/parallel/test-module-loading-error.js`
  reaches `fixtures/module-loading-error.node` and fails with Deno's
  `NotCapable: Requires ffi access`. This must not be greened by widening the
  multi-tenant isolate sandbox.
- Path/topology normalization: `test/parallel/test-util-callbackify.js`
  compares `/private/var/.../T/.tmp*` against `/private/tmp/nvx-*`.
- Color formatting residual: `test/parallel/test-util-inspect-regexp.js`
  expects Node26 tokenized regexp color output rather than the current Deno
  red-whole-regexp formatting.
- Symlink/package-type and subprocess/preload residuals remain in fixtures such
  as `test/es-module/test-esm-symlink-type.js` and
  `test/es-module/test-require-module-preload.js`.
- `test/parallel/test-util-styletext.js` was skipped in the broad run and was
  not promoted.

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

- `scripts/runtime/node/watchpoints.py validate`:
  `validated node-compat watchpoint catalog: 146 entries`
- `scripts/runtime/node/required_surface_blockers.py`:
  `node22 required gaps: 0`, `node24 required gaps: 0`
- `tests/runtime/node/compat/node-compat-evidence/latest/status-summary.json`:
  Node26 official manifested green count is `1741 / 5578`.

Posture after regeneration:

- Node22 `v8_isolate_required`: `0` gaps, `100.0%`
- Node24 `v8_isolate_required`: `0` gaps, `100.0%`
- Node26 `v8_isolate_required`: `458` gaps, `79.17%`

The Node26 count moved from `558` gaps / `74.62%` to `458` gaps /
`79.17%`, burning 100 required-surface gaps in this wave. The official fixture
evidence count for Node26 moved from `1641 / 5578` to `1741 / 5578`.

## Verifier

Command:

```bash
bash scripts/verify-node-default-runtime-support-hardening.sh
```

Result:

- Summary: `14 passed, 20 failed`.
- Step 9 passed: Node22 and Node24 V8-isolate-required fixtures are `100%`.
- Step 11 remains failed because Node26 Current evidence is still incomplete:
  Node26 is `1741` official passes and `458` required gaps, not `0` gaps /
  `100.0%`.
- The remaining verifier failures are honest red closeout/proof gaps in this
  checkout; this cycle does not claim full NDS completion.

## Next Node26 Work

Node26 remains at `458` required gaps. The next high-yield pass should start
from the regenerated posture and pick the broadest remaining coherent cluster by
expected gaps burned per hour. For module-loader specifically, useful follow-up
work is the ESM/CJS require race/cache semantics and module-hook callback
contract bucket; keep native `.node`/FFI cases structurally bounded unless
source-confirmed evidence shows they can run inside the multi-tenant isolate
without widening host authority.
