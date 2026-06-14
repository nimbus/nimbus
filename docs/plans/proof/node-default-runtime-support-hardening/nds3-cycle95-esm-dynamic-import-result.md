# NDS3 Cycle 95 - no-referrer ESM dynamic import rejection

Date: 2026-06-14

## Summary

Cycle 95 promotes `test/es-module/test-esm-dynamic-import.js` in both required lanes.

Fork fix:

- Deno fork: `/Users/jack/src/github.com/nimbus/deno`
- Branch/tag: `nimbus/v2.8.3` / `v2.8.3-nimbus.42`
- Commit: `3e55fee636 core: reject no-referrer dynamic imports without callback`
- Files changed in the fork tag:
  - `libs/core/runtime/bindings.rs`
  - `libs/core/modules/tests.rs`
  - `libs/core/runtime/tests/snapshot.rs`

Nimbus is repinned from `v2.8.3-nimbus.41` to `v2.8.3-nimbus.42` in `Cargo.toml` and `Cargo.lock`.
No local Cargo `paths = [...]` override is present in the final Nimbus state.

## Root Cause

The fixture expects normal direct dynamic imports to work, but indirect eval of
`import("node:fs")` to reject with Node's
`ERR_VM_DYNAMIC_IMPORT_CALLBACK_MISSING`:

```js
expectModuleError(Promise.resolve('import("node:fs")').then(eval),
                  'ERR_VM_DYNAMIC_IMPORT_CALLBACK_MISSING');
```

V8 reports that indirect-eval import without host-defined script/module options
and without a concrete resource name. The Nimbus Deno fork already handled this
case when a `node:vm` context-level dynamic-import option was present, but with
no context option it fell through to the default module loader and resolved
`node:fs`. The fix makes the no-referrer/no-callback case reject immediately
with the same Node-style missing-callback error unless a context-level VM
callback was explicitly registered.

## Dynamic Proof

Specifier-local scratch probe before the fix showed only the indirect-eval case
missing:

```text
NDS_DYNAMIC_IMPORT_REJECT node_unknown ERR_UNKNOWN_BUILTIN_MODULE
NDS_DYNAMIC_IMPORT_REJECT node_internal_test_binding ERR_UNKNOWN_BUILTIN_MODULE
NDS_DYNAMIC_IMPORT_REJECT http_url ERR_UNSUPPORTED_ESM_URL_SCHEME
NDS_DYNAMIC_IMPORT_REJECT missing_relative ERR_MODULE_NOT_FOUND
Expected dynamicImportError to be called exactly 1, actual 0.
```

After the fix, the same scratch source showed the missing case rejecting:

```text
NDS_DYNAMIC_IMPORT_REJECT indirect_eval ERR_VM_DYNAMIC_IMPORT_CALLBACK_MISSING
NDS_DYNAMIC_IMPORT_REJECT node_unknown ERR_UNKNOWN_BUILTIN_MODULE
NDS_DYNAMIC_IMPORT_REJECT node_internal_test_binding ERR_UNKNOWN_BUILTIN_MODULE
NDS_DYNAMIC_IMPORT_REJECT http_url ERR_UNSUPPORTED_ESM_URL_SCHEME
NDS_DYNAMIC_IMPORT_REJECT missing_relative ERR_MODULE_NOT_FOUND
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 910 filtered out; finished in 2.12s
```

Local fork proof with temporary Cargo path override to `/Users/jack/src/github.com/nimbus/deno/libs/core`:

node24 local proof:

```text
node_compat nds-probe node24 summary: selected=1, passed=1, skipped=0, failed=0
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 910 filtered out; finished in 2.00s
```

node22 local proof:

```text
node_compat nds-probe node22 summary: selected=1, passed=1, skipped=0, failed=0
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 910 filtered out; finished in 2.04s
```

Fork hygiene before publishing:

```bash
cargo fmt --all --check
git diff --check
CARGO_ENCODED_RUSTFLAGS= cargo test -p deno_core dyn_import_without_referrer_rejects_missing_callback
```

Focused deno_core regression result:

```text
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 424 filtered out; finished in 0.03s
```

Immutable tag proof after repinning Nimbus to `v2.8.3-nimbus.42`:

node24 tag proof:

```text
node_compat nds-probe node24 summary: selected=1, passed=1, skipped=0, failed=0
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 910 filtered out; finished in 2.15s
```

node22 tag proof:

```text
node_compat nds-probe node22 summary: selected=1, passed=1, skipped=0, failed=0
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 910 filtered out; finished in 2.15s
```

## Promotion Guards

Permanent promotion file:

- `crates/nimbus-runtime/src/runtime/tests/node/cases/nds3_cycle95_esm_dynamic_import.rs`

Promotion guard:

```text
node_compat node22-default-lane-executes-cycle95-esm-dynamic-import node22 summary: selected=1, passed=1, skipped=0, failed=0
node_compat node24-default-lane-executes-cycle95-esm-dynamic-import node24 summary: selected=1, passed=1, skipped=0, failed=0
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 910 filtered out; finished in 4.08s
```

The scratch `nds_probe.rs` include/file and scratch inline JS source were removed before checkpointing.

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
node22 = 3
node24 = 4
```

Remaining generated gap list:

```text
node22:
  test/es-module/test-esm-loader-mock.mjs
  test/es-module/test-esm-virtual-json.mjs
  test/parallel/test-vm-module-import-meta.js

node24:
  test/es-module/test-esm-loader-mock.mjs
  test/es-module/test-esm-virtual-json.mjs
  test/parallel/test-vm-module-hastoplevelawait.js
  test/parallel/test-vm-module-import-meta.js
```
