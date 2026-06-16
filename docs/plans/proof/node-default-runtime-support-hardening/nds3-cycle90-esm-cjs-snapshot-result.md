# NDS3 Cycle 90 - CommonJS ESM Snapshot Parity

Date: 2026-06-14

## Scope

Promoted `test/es-module/test-esm-snapshot.mjs` in both required lanes: Node22
and Node24.

Fork tag: `nimbus/deno` `v2.8.3-nimbus.38`
(`ced4fb1626 node: snapshot CommonJS exports for ESM wrappers`).

Nimbus pin: Deno-family crates in `Cargo.toml` / `Cargo.lock` repinned from
`v2.8.3-nimbus.37` to `v2.8.3-nimbus.38`.

`nimbus/rusty_v8` remained on `v149.4.0-nimbus.1`; no V8 or rusty_v8 code was
changed. No upstream Node fixture or checker was edited. No generated posture
JSON was hand-edited.

## Fork Fix

The Deno fork changed:

- `ext/node/polyfills/01_require.js`
- `libs/node_resolver/analyze.rs`

The failing fixture loads `fixtures/es-modules/esm-snapshot.js` through
CommonJS, mutates `require.cache[filename].exports++`, and then imports the same
CommonJS module through ESM. The previous generated wrapper exported the live
`require(filename)` result, so the ESM default saw `2` instead of Node's expected
first-load snapshot `1`.

The fork now:

- Records a CommonJS `module.exports` snapshot immediately after successful
  `module.load(filename)`.
- Exposes that value through `Module._getCjsEsmExportsSnapshot(filename,
  fallback)`.
- Generates CommonJS-to-ESM wrappers from the snapshot value for named exports,
  default, and `"module.exports"`.

## Dynamic Proof

Local fork override proof before publishing, with Nimbus temporarily pointed at
`/Users/jack/src/github.com/nimbus/deno/ext/node` and
`/Users/jack/src/github.com/nimbus/deno/libs/node_resolver`:

```text
$ CARGO_ENCODED_RUSTFLAGS='' cargo test -p node_resolver test_exports_to_wrapper_module
running 2 tests
test analyze::tests::test_exports_to_wrapper_module ... ok
test analyze::tests::test_exports_to_wrapper_module_without_module_exports ... ok
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 18 filtered out; finished in 0.00s

$ gtimeout -s KILL 90 env CARGO_NET_OFFLINE=true \
  NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=target/node-compat-diagnostics/nds3-cycle90-esm-snapshot-node22-local-1 \
  NIMBUS_RECENSUS_LANE=node22 \
  NIMBUS_RECENSUS_FIXTURE=test/es-module/test-esm-snapshot.mjs \
  cargo test -p nimbus-runtime --lib nds_esm_probe -- --ignored --nocapture

node_compat nds-esm-probe node22 summary: selected=1, passed=1, skipped=0, failed=0
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 901 filtered out; finished in 2.94s

$ gtimeout -s KILL 90 env CARGO_NET_OFFLINE=true \
  NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=target/node-compat-diagnostics/nds3-cycle90-esm-snapshot-node24-local-1 \
  NIMBUS_RECENSUS_LANE=node24 \
  NIMBUS_RECENSUS_FIXTURE=test/es-module/test-esm-snapshot.mjs \
  cargo test -p nimbus-runtime --lib nds_esm_probe -- --ignored --nocapture

node_compat nds-esm-probe node24 summary: selected=1, passed=1, skipped=0, failed=0
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 901 filtered out; finished in 2.87s
```

Published-tag proof after pushing `v2.8.3-nimbus.38`, removing the local Cargo
path override, repinning `Cargo.toml` / `Cargo.lock`, and rebuilding from the
immutable tag:

```text
$ CARGO_NET_OFFLINE=true cargo test -p nimbus-runtime --lib nds_esm_probe --no-run

Compiling deno_node v0.189.0 (https://github.com/nimbus/deno?tag=v2.8.3-nimbus.38#ced4fb16)
Compiling node_resolver v0.89.0 (https://github.com/nimbus/deno?tag=v2.8.3-nimbus.38#ced4fb16)
Compiling deno_node_crypto v0.21.0 (https://github.com/nimbus/deno?tag=v2.8.3-nimbus.38#ced4fb16)
Compiling deno_node_sqlite v0.21.0 (https://github.com/nimbus/deno?tag=v2.8.3-nimbus.38#ced4fb16)
Finished `test` profile [unoptimized + debuginfo] target(s) in 40.80s

$ gtimeout -s KILL 90 env CARGO_NET_OFFLINE=true \
  NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=target/node-compat-diagnostics/nds3-cycle90-esm-snapshot-node22-tag38-1 \
  NIMBUS_RECENSUS_LANE=node22 \
  NIMBUS_RECENSUS_FIXTURE=test/es-module/test-esm-snapshot.mjs \
  cargo test -p nimbus-runtime --lib nds_esm_probe -- --ignored --nocapture

node_compat nds-esm-probe node22 summary: selected=1, passed=1, skipped=0, failed=0
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 901 filtered out; finished in 2.99s

$ gtimeout -s KILL 90 env CARGO_NET_OFFLINE=true \
  NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=target/node-compat-diagnostics/nds3-cycle90-esm-snapshot-node24-tag38-1 \
  NIMBUS_RECENSUS_LANE=node24 \
  NIMBUS_RECENSUS_FIXTURE=test/es-module/test-esm-snapshot.mjs \
  cargo test -p nimbus-runtime --lib nds_esm_probe -- --ignored --nocapture

node_compat nds-esm-probe node24 summary: selected=1, passed=1, skipped=0, failed=0
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 901 filtered out; finished in 2.94s
```

Promotion guard after deleting the scratch `nds_esm_probe` file:

```text
$ gtimeout -s KILL 90 env CARGO_NET_OFFLINE=true \
  NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/Users/jack/src/github.com/nimbus/nimbus-worktrees/node-default-runtime-support-hardening/target/node-compat-diagnostics/nds3-cycle90-esm-cjs-snapshot-promotion-tag38-abs-1 \
  cargo test -p nimbus-runtime --lib cycle90_esm_cjs_snapshot -- --nocapture

node_compat node22-supported-lane-executes-cycle90-esm-cjs-snapshot-batch node22 summary: selected=1, passed=1, skipped=0, failed=0
node_compat node22-supported-lane-executes-cycle90-esm-cjs-snapshot-batch node22 summary artifact: /Users/jack/src/github.com/nimbus/nimbus-worktrees/node-default-runtime-support-hardening/target/node-compat-diagnostics/nds3-cycle90-esm-cjs-snapshot-promotion-tag38-abs-1/batch/node22__node22_supported_lane_executes_cycle90_esm_cjs_snapshot_batch__summary.json
node_compat node24-default-lane-executes-cycle90-esm-cjs-snapshot-batch node24 summary: selected=1, passed=1, skipped=0, failed=0
node_compat node24-default-lane-executes-cycle90-esm-cjs-snapshot-batch node24 summary artifact: /Users/jack/src/github.com/nimbus/nimbus-worktrees/node-default-runtime-support-hardening/target/node-compat-diagnostics/nds3-cycle90-esm-cjs-snapshot-promotion-tag38-abs-1/batch/node24__node24_default_lane_executes_cycle90_esm_cjs_snapshot_batch__summary.json
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 901 filtered out; finished in 5.28s
```

## Regeneration And Checks

The classification/posture pipeline was regenerated with:

```text
$ /opt/homebrew/bin/python3.12 scripts/runtime/node/classifications.py sync --lane all
$ for s in status dashboard trends publish_evidence default_support_posture required_surface_blockers; do /opt/homebrew/bin/python3.12 scripts/runtime/node/$s.py >/dev/null; done
```

Regenerated counts:

```text
node22 gaps = 7, pass_rate_percent = 99.7
node24 gaps = 9, pass_rate_percent = 99.62
```

Generator and formatting checks:

```text
$ /opt/homebrew/bin/python3.12 scripts/runtime/node/classifications.py sync --preserve-existing --check
node20.json is up to date
node22.json is up to date
node24.json is up to date
node26.json is up to date

$ /opt/homebrew/bin/python3.12 scripts/runtime/node/default_support_posture.py --check
node default support posture: pass

$ /opt/homebrew/bin/python3.12 scripts/runtime/node/required_surface_blockers.py --check
node required-surface blocker inventory: pass

$ cargo fmt --all --check
pass

$ git diff --check
pass
```

## Diagnostics

Diagnostic roots retained:

- `/Users/jack/src/github.com/nimbus/nimbus-worktrees/node-default-runtime-support-hardening/target/node-compat-diagnostics/nds3-cycle90-esm-broad-node22-1`
- `/Users/jack/src/github.com/nimbus/nimbus-worktrees/node-default-runtime-support-hardening/target/node-compat-diagnostics/nds3-cycle90-esm-broad-node24-1`
- `/Users/jack/src/github.com/nimbus/nimbus-worktrees/node-default-runtime-support-hardening/crates/nimbus-runtime/target/node-compat-diagnostics/nds3-cycle90-esm-snapshot-node22-local-1`
- `/Users/jack/src/github.com/nimbus/nimbus-worktrees/node-default-runtime-support-hardening/crates/nimbus-runtime/target/node-compat-diagnostics/nds3-cycle90-esm-snapshot-node24-local-1`
- `/Users/jack/src/github.com/nimbus/nimbus-worktrees/node-default-runtime-support-hardening/crates/nimbus-runtime/target/node-compat-diagnostics/nds3-cycle90-esm-snapshot-node22-tag38-1`
- `/Users/jack/src/github.com/nimbus/nimbus-worktrees/node-default-runtime-support-hardening/crates/nimbus-runtime/target/node-compat-diagnostics/nds3-cycle90-esm-snapshot-node24-tag38-1`
- `/Users/jack/src/github.com/nimbus/nimbus-worktrees/node-default-runtime-support-hardening/target/node-compat-diagnostics/nds3-cycle90-esm-cjs-snapshot-promotion-tag38-abs-1`

The initial broad ESM batch roots captured the root-cause grouping:

- Node22 broad ESM batch: `selected=6, passed=0, skipped=0, failed=6`.
- Node24 broad ESM batch: `selected=7, passed=0, skipped=0, failed=7`.
- `test-esm-snapshot.mjs` failed as `2 !== 1`, proving the live CommonJS export
  mutation was leaking into ESM default import.

## Remaining Gate

Remaining required gaps after this cycle:

```text
node22 (7):
test/es-module/test-esm-dynamic-import-commonjs.js
test/es-module/test-esm-dynamic-import-commonjs.mjs
test/es-module/test-esm-dynamic-import.js
test/es-module/test-esm-loader-mock.mjs
test/es-module/test-esm-virtual-json.mjs
test/parallel/test-vm-module-import-meta.js
test/parallel/test-webcrypto-sign-verify.js

node24 (9):
test/es-module/test-esm-dynamic-import-commonjs.js
test/es-module/test-esm-dynamic-import-commonjs.mjs
test/es-module/test-esm-dynamic-import.js
test/es-module/test-esm-loader-mock.mjs
test/es-module/test-esm-require-race-condition.js
test/es-module/test-esm-virtual-json.mjs
test/parallel/test-vm-module-hastoplevelawait.js
test/parallel/test-vm-module-import-meta.js
test/parallel/test-webcrypto-sign-verify.js
```

The gate remains red and honest; this cycle removes one Node22 and one Node24
V8-isolate-required gap.
