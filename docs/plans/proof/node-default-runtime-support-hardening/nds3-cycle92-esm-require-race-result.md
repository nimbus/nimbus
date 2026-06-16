# NDS3 Cycle 92: ESM require race condition

Date: 2026-06-14

## Scope

Promoted one remaining required Node24 ESM fixture:

- `test/es-module/test-esm-require-race-condition.js`

This wave did not edit official Node fixtures or checkers. It did not touch
V8/rusty_v8. The earlier local `module.register()` async-hook exploration stayed
red with `resolve hook returned a pending promise`; those changes were reverted
and are not part of this cycle.

## Fork Change

Deno fork:

- Branch: `nimbus/v2.8.3`
- Commit: `8f7081b03b9614aca4f3c4cf71d2d4e991131db0`
- Tag: `v2.8.3-nimbus.39`

Changed files in `nimbus/deno`:

- `libs/core/ops_builtin.rs`: rejects sync `require()` of a non-evaluated ES
  module while a dynamic import graph is still pending.
- `ext/node/polyfills/internal/errors.ts`: adds
  `ERR_REQUIRE_ESM_RACE_CONDITION`.
- `ext/node/polyfills/01_require.js`: maps the deno_core "not yet fully loaded"
  error into Node's public race-condition error code.

Push proof:

```text
To github.com:nimbus/deno.git
   ced4fb1626..8f7081b03b  nimbus/v2.8.3 -> nimbus/v2.8.3
To github.com:nimbus/deno.git
 * [new tag]               v2.8.3-nimbus.39 -> v2.8.3-nimbus.39
```

Nimbus was repinned from `v2.8.3-nimbus.38#ced4fb1626` to
`v2.8.3-nimbus.39#8f7081b03b` in `Cargo.toml` and `Cargo.lock`. No local Cargo
path override remains.

## Proof

Local fork proof before publishing:

```text
gtimeout -s KILL 90 env \
  NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/Users/jack/src/github.com/nimbus/nimbus-worktrees/node-default-runtime-support-hardening/target/node-compat-diagnostics/nds3-cycle92-require-race-local-1 \
  NIMBUS_RECENSUS_FIXTURE='test/es-module/test-esm-require-race-condition.js' \
  NIMBUS_RECENSUS_LANE=node24 \
  NIMBUS_RECENSUS_EXTRA_DIRS='test/common:test/fixtures/import-require-cycle' \
  cargo test -p nimbus-runtime --lib nds_probe -- --ignored --nocapture

node_compat nds-probe node24 summary: selected=1, passed=1, skipped=0, failed=0
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 905 filtered out; finished in 2.03s
```

Published immutable-tag rebuild proof:

```text
Compiling deno_core v0.404.0 (https://github.com/nimbus/deno?tag=v2.8.3-nimbus.39#8f7081b0)
Compiling deno_node v0.189.0 (https://github.com/nimbus/deno?tag=v2.8.3-nimbus.39#8f7081b0)
Finished `test` profile [unoptimized + debuginfo] target(s) in 41.96s
```

Published immutable-tag focused proof:

```text
gtimeout -s KILL 90 env \
  NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/Users/jack/src/github.com/nimbus/nimbus-worktrees/node-default-runtime-support-hardening/target/node-compat-diagnostics/nds3-cycle92-require-race-tag39-1 \
  NIMBUS_RECENSUS_FIXTURE='test/es-module/test-esm-require-race-condition.js' \
  NIMBUS_RECENSUS_LANE=node24 \
  NIMBUS_RECENSUS_EXTRA_DIRS='test/common:test/fixtures/import-require-cycle' \
  cargo test -p nimbus-runtime --lib nds_probe -- --ignored --nocapture

node_compat nds-probe node24 summary: selected=1, passed=1, skipped=0, failed=0
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 905 filtered out; finished in 2.05s
```

Promoted watchpoint proof:

```text
gtimeout -s KILL 90 env \
  NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/Users/jack/src/github.com/nimbus/nimbus-worktrees/node-default-runtime-support-hardening/target/node-compat-diagnostics/nds3-cycle92-require-race-promotion-1 \
  cargo test -p nimbus-runtime --lib node24_default_lane_executes_cycle92_esm_require_race -- --nocapture

node_compat node24-default-lane-executes-cycle92-esm-require-race node24 summary: selected=1, passed=1, skipped=0, failed=0
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 905 filtered out; finished in 2.02s
```

Diagnostic roots retained:

- `/Users/jack/src/github.com/nimbus/nimbus-worktrees/node-default-runtime-support-hardening/target/node-compat-diagnostics/nds3-cycle92-module-hooks-js-microtasks-local-1`
- `/Users/jack/src/github.com/nimbus/nimbus-worktrees/node-default-runtime-support-hardening/target/node-compat-diagnostics/nds3-cycle92-module-hooks-js-microtasks-local-2`
- `/Users/jack/src/github.com/nimbus/nimbus-worktrees/node-default-runtime-support-hardening/target/node-compat-diagnostics/nds3-cycle92-module-hooks-rust-microtasks-local-1`
- `/Users/jack/src/github.com/nimbus/nimbus-worktrees/node-default-runtime-support-hardening/target/node-compat-diagnostics/nds3-cycle92-require-race-local-1`
- `/Users/jack/src/github.com/nimbus/nimbus-worktrees/node-default-runtime-support-hardening/target/node-compat-diagnostics/nds3-cycle92-require-race-tag39-1`
- `/Users/jack/src/github.com/nimbus/nimbus-worktrees/node-default-runtime-support-hardening/target/node-compat-diagnostics/nds3-cycle92-require-race-promotion-1`

## Result

Generated posture after regeneration:

- node22: `v8_isolate_required.gaps == 6`, `pass_rate_percent == 99.75`
- node24: `v8_isolate_required.gaps == 7`, `pass_rate_percent == 99.71`

The gate remains red and honest.
