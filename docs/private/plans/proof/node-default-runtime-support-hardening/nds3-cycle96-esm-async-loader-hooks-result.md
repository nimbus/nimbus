# NDS3 Cycle 96 - ESM async loader hooks

Date: 2026-06-14

## Summary

Cycle 96 promotes these ESM loader fixtures in both required lanes:

- `test/es-module/test-esm-loader-mock.mjs`
- `test/es-module/test-esm-virtual-json.mjs`

Fork fix:

- Deno fork: `/Users/jack/src/github.com/nimbus/deno`
- Branch/tag: `nimbus/v2.8.3` / `v2.8.3-nimbus.43`
- Commit: `782994513c node(module): support async register resolve hooks`
- Files changed in the fork tag:
  - `ext/node/lib.rs`
  - `ext/node/ops/module_hooks.rs`
  - `ext/node/polyfills/01_require.js`
  - `libs/core/ops_builtin.rs`

Nimbus is repinned from `v2.8.3-nimbus.42` to `v2.8.3-nimbus.43` in `Cargo.toml` and `Cargo.lock`.
No local Cargo `paths = [...]` override is present in the final Nimbus state.

## Root Cause

Both fixtures use `module.register()` loader modules with async `resolve()`
hooks. Before cycle 96, Nimbus' Deno hook bridge could call synchronous
`module.registerHooks()` resolve hooks and could service async load hooks through
the existing polling loop, but an async resolve hook returned a pending Promise
from V8's synchronous module-resolution callback:

```text
runtime JavaScript error: Error: resolve hook returned a pending promise
```

The fix adds a resolve-hook placeholder bridge: JS reserves a synthetic
`nimbus-async-resolve:*` URL when an async resolve hook returns a Promise, then
responds to Rust when the Promise settles. The Nimbus loader recognizes that
placeholder at `load()`, awaits the real resolved URL, and redirects the
`ModuleSource` back to the settled URL so the module map keeps coherent aliases.

`module.register()` also gets a register-only sync import path so the hook module
can be loaded while a dynamic import graph is already in flight, without
weakening the existing general `require(esm)` race-condition guard.

## Dynamic Proof

Initial focused proof before this cycle's bridge, with the correct fixture
directories, failed on pending async resolve:

```text
node_compat nds-probe node24 summary: selected=1, passed=0, skipped=0, failed=1
test/es-module/test-esm-loader-mock.mjs: upstream node_compat fixture `test/es-module/test-esm-loader-mock.mjs` should execute: runtime JavaScript error: Error: resolve hook returned a pending promise
test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 912 filtered out; finished in 2.06s
```

Local fork proof with temporary Cargo path override to
`/Users/jack/src/github.com/nimbus/deno/libs/core` and
`/Users/jack/src/github.com/nimbus/deno/ext/node`:

```text
node_compat nds-probe node24 summary: selected=1, passed=1, skipped=0, failed=0
node_compat nds-probe node22 summary: selected=1, passed=1, skipped=0, failed=0
node_compat nds-probe node24 summary: selected=1, passed=1, skipped=0, failed=0
node_compat nds-probe node22 summary: selected=1, passed=1, skipped=0, failed=0
```

The four local lines above are, in order:

- `test-esm-loader-mock.mjs` node24
- `test-esm-loader-mock.mjs` node22
- `test-esm-virtual-json.mjs` node24
- `test-esm-virtual-json.mjs` node22

Nearby ESM regression guards stayed green after the local fork fix:

```text
node_compat node24-default-lane-executes-cycle92-esm-require-race node24 summary: selected=1, passed=1, skipped=0, failed=0
node_compat node22-default-lane-executes-cycle93-esm-dynamic-import-commonjs node22 summary: selected=1, passed=1, skipped=0, failed=0
node_compat node24-default-lane-executes-cycle93-esm-dynamic-import-commonjs node24 summary: selected=1, passed=1, skipped=0, failed=0
node_compat node22-default-lane-executes-cycle94-esm-dynamic-import-commonjs-mjs node22 summary: selected=1, passed=1, skipped=0, failed=0
node_compat node24-default-lane-executes-cycle94-esm-dynamic-import-commonjs-mjs node24 summary: selected=1, passed=1, skipped=0, failed=0
node_compat node22-default-lane-executes-cycle95-esm-dynamic-import node22 summary: selected=1, passed=1, skipped=0, failed=0
node_compat node24-default-lane-executes-cycle95-esm-dynamic-import node24 summary: selected=1, passed=1, skipped=0, failed=0
```

Fork hygiene before publishing:

```bash
cargo fmt
git diff --check
```

Immutable tag rebuild after repinning Nimbus to `v2.8.3-nimbus.43` compiled the
fork crates from the published tag:

```text
Compiling deno_node v0.189.0 (https://github.com/nimbus/deno?tag=v2.8.3-nimbus.43#78299451)
Compiling deno_core v0.404.0 (https://github.com/nimbus/deno?tag=v2.8.3-nimbus.43#78299451)
Finished `test` profile [unoptimized + debuginfo] target(s) in 45.14s
```

Immutable tag proof:

```text
node_compat nds-probe node24 summary: selected=1, passed=1, skipped=0, failed=0
node_compat nds-probe node22 summary: selected=1, passed=1, skipped=0, failed=0
node_compat nds-probe node24 summary: selected=1, passed=1, skipped=0, failed=0
node_compat nds-probe node22 summary: selected=1, passed=1, skipped=0, failed=0
```

The four immutable-tag lines above are, in order:

- `test-esm-loader-mock.mjs` node24
- `test-esm-loader-mock.mjs` node22
- `test-esm-virtual-json.mjs` node24
- `test-esm-virtual-json.mjs` node22

## Promotion Guards

Permanent promotion file:

- `crates/nimbus-runtime/src/runtime/tests/node/cases/nds3_cycle96_esm_async_loader_hooks.rs`

Promotion guard:

```text
node_compat node22-default-lane-executes-cycle96-esm-async-loader-hooks node22 summary: selected=2, passed=2, skipped=0, failed=0
node_compat node24-default-lane-executes-cycle96-esm-async-loader-hooks node24 summary: selected=2, passed=2, skipped=0, failed=0
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 912 filtered out; finished in 7.81s
```

The scratch `nds_probe.rs` include/file was removed before checkpointing.

## Regeneration

Commands:

```bash
/opt/homebrew/bin/python3.12 scripts/runtime/node/classifications.py sync --lane all
for s in status dashboard trends publish_evidence default_support_posture required_surface_blockers; do
  /opt/homebrew/bin/python3.12 scripts/runtime/node/$s.py >/dev/null
done
```

Generated posture after regeneration:

```text
node22 = 1, pass_rate = 99.96
node24 = 2, pass_rate = 99.92
```

Remaining generated gap list:

```text
node22:
  test/parallel/test-vm-module-import-meta.js

node24:
  test/parallel/test-vm-module-hastoplevelawait.js
  test/parallel/test-vm-module-import-meta.js
```
