# NDS3 Node26 Cycle 16: diagnostics_channel bounded scopes

## Scope

This checkpoint promotes the remaining Node26 Current
`process-and-timing/diagnostics-channel` required fixtures that were still red
after the foundation bump. The work is a coherent fork-owner wave: Deno's
`node:diagnostics_channel` polyfill now carries the Node26 bounded-channel,
run-store scope, and tracing promise behavior; Deno's `node:http2` diagnostics
ordering now matches the observable Node fixture order for pushed client
streams; Nimbus maps the three transform-error fixtures to the existing
single-emit `beforeExit` postlude they require.

No V8 or rusty_v8 changes were made. No official upstream fixture or checker
was edited. Nimbus was temporarily pinned to the canonical local Deno worktree
only for local proof, then restored to the immutable published tag
`v2.8.3-nimbus.50`.

Before this wave, Node26 `v8_isolate_required` posture was `333` gaps /
`84.18%`.

## Fork Change

Deno fork:

- worktree: `/Users/jack/src/github.com/nimbus/deno`
- branch: `nimbus/v2.8.3`
- commit: `7cbeb088e81301829844a252bd8be0edea943ebd`
- tag: `v2.8.3-nimbus.50`

Changed fork files:

- `ext/node/polyfills/async_hooks.ts`
- `ext/node/polyfills/diagnostics_channel.js`
- `ext/node/polyfills/http2.ts`

The tag was pushed to `git@github.com:nimbus/deno.git`, and Nimbus was repinned
from the temporary local path patch back to `https://github.com/nimbus/deno`,
tag `v2.8.3-nimbus.50`.

## Local Proof

Local Deno pin broad batch:

```bash
NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nds-node26-diagnostics-channel-wave1-local3 \
  cargo test -p nimbus-runtime --lib node26_current_lane_process_diagnostics_channel_watchpoint -- --ignored --nocapture
```

Result:

- selected: `14`
- passed: `14`
- skipped: `0`
- failed: `0`
- summary:
  `/private/tmp/nds-node26-diagnostics-channel-wave1-local3/batch/node26__node26_current_lane_process_diagnostics_channel_watchpoint__summary.json`

Local Deno pin promoted batch:

```bash
NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nds-node26-diagnostics-channel-wave1-local-promoted1 \
  cargo test -p nimbus-runtime --lib node26_current_lane_executes_process_diagnostics_channel_promoted_batch_fixture -- --nocapture
```

Result:

- selected: `55`
- passed: `55`
- skipped: `0`
- failed: `0`
- summary:
  `/private/tmp/nds-node26-diagnostics-channel-wave1-local-promoted1/batch/node26__node26_current_lane_executes_process_diagnostics_channel_promoted_batch__summary.json`

## Immutable Tag Proof

After committing, tagging, pushing, and repinning Nimbus to
`v2.8.3-nimbus.50#7cbeb088e81301829844a252bd8be0edea943ebd`, the same broad
and promoted batches were rerun on the immutable tag.

Tag broad batch:

```bash
NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nds-node26-diagnostics-channel-wave1-tag50 \
  cargo test -p nimbus-runtime --lib node26_current_lane_process_diagnostics_channel_watchpoint -- --ignored --nocapture
```

Result:

- selected: `14`
- passed: `14`
- skipped: `0`
- failed: `0`
- summary:
  `/private/tmp/nds-node26-diagnostics-channel-wave1-tag50/batch/node26__node26_current_lane_process_diagnostics_channel_watchpoint__summary.json`

Tag promoted batch:

```bash
NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nds-node26-diagnostics-channel-wave1-tag50-promoted \
  cargo test -p nimbus-runtime --lib node26_current_lane_executes_process_diagnostics_channel_promoted_batch_fixture -- --nocapture
```

Result:

- selected: `55`
- passed: `55`
- skipped: `0`
- failed: `0`
- summary:
  `/private/tmp/nds-node26-diagnostics-channel-wave1-tag50-promoted/batch/node26__node26_current_lane_executes_process_diagnostics_channel_promoted_batch__summary.json`

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
  `validated node-compat watchpoint catalog: 147 entries`
- `scripts/runtime/node/required_surface_blockers.py`:
  `node22 required gaps: 0`, `node24 required gaps: 0`
- `docs/private/architecture/runtime/node-default-support-posture.json`:
  Node26 `v8_isolate_required` is `319` gaps / `84.85%`

Posture after regeneration:

- Node22 `v8_isolate_required`: `0` gaps, `100.0%`
- Node24 `v8_isolate_required`: `0` gaps, `100.0%`
- Node26 `v8_isolate_required`: `319` gaps, `84.85%`
- Node26 required passed: `1786 / 2105`

This wave moves Node26 from `333` gaps / `84.18%` to `319` gaps / `84.85%`,
burning 14 required-surface gaps.

## Verifier

Command:

```bash
bash scripts/verify-node-default-runtime-support-hardening.sh
```

Result:

- Summary: `14 passed, 20 failed`.
- Step 9 passed: Node22 and Node24 V8-isolate-required fixtures are `100%`.
- Step 11 remains failed because Node26 Current evidence is still incomplete:
  Node26 is `1786` official passes and `319` required gaps, not `0` gaps /
  `100.0%`.
- The remaining verifier failures are honest red closeout/proof gaps in this
  checkout; this cycle does not claim full NDS completion.

## Integrity Checks

Commands:

```bash
cargo check -p nimbus-runtime
cargo test -p nimbus-runtime --lib node_compat_named_preludes_catalog_matches_default_behavior_registry -- --nocapture
cargo fmt --all --check
git diff --check
```

Results:

- `cargo check -p nimbus-runtime`: passed after resolving Deno-family crates to
  `v2.8.3-nimbus.50#7cbeb088e81301829844a252bd8be0edea943ebd`.
- `cargo test -p nimbus-runtime --lib node_compat_named_preludes_catalog_matches_default_behavior_registry -- --nocapture`:
  `1 passed; 0 failed; 942 filtered out`.
- `cargo fmt --all --check`: passed.
- `git diff --check`: passed.

## Remaining Node26 Required Buckets

After this wave:

- `149` `node-compat/unpromoted-surface`
- `34` `node-compat/current-lane`
- `33` `process-and-timing/process-host`
- `23` `loader-context/vm`
- `20` `loader-context/module`
- `18` `loader-context/domain`
- `15` `streams-local-io/fs-host-io`
- `10` `process-and-timing/perf-hooks`
- `7` `runtime/v8`
- `4` `core-semantics/console`
- `3` `loader-context/util`
- `2` `core-semantics/assert`
- `1` `core-semantics/url`

Continue with the broadest high-yield Node26 cluster, not singleton cleanup,
unless a singleton is the last member of its cluster.
