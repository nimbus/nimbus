# NDS gate - current state after cycle 98

Date: 2026-06-14

**Branch/PR:** `codex/node-default-runtime-support-hardening` -> PR #10  
**Fork pins:** nimbus/deno `v2.8.3-nimbus.46` (`d3f650c2fa`); nimbus/rusty_v8 `v149.4.0-nimbus.2` (`8f70a59`)  
**Verifier target:** `bash scripts/verify-node-default-runtime-support-hardening.sh` step 9

## Gate Result

The literal NDS merge gate is green:

- `node22` `v8_isolate_required.gaps = 0`
- `node22` `v8_isolate_required.pass_rate_percent = 100.0`
- `node24` `v8_isolate_required.gaps = 0`
- `node24` `v8_isolate_required.pass_rate_percent = 100.0`

The cycle-98 checkpoint promoted the last remaining required-surface fixture in
both supported lanes:

- `test/parallel/test-vm-module-import-meta.js`

## Final Fix

The remaining blocker was real VM module import-meta initialization rather than
a structural non-isolate fixture. Deno-created VM modules do not live in
deno_core's normal module map, so the host import-meta callback previously
panicked while trying to resolve them through the normal module graph path.

nimbus/deno `v2.8.3-nimbus.46` adds a VM-module import-meta initializer side
table in `deno_core`, exposes registration helpers for `deno_node`, and wires
`vm.SourceTextModule`'s `initializeImportMeta` option so the callback receives
the Node-style `(meta, module)` arguments. The registration is scoped to module
evaluation and cleared after the evaluation promise settles.

## Proof

Cycle proof:

- `docs/private/plans/proof/node-default-runtime-support-hardening/nds3-cycle98-vm-module-import-meta-result.md`

Focused permanent guard:

```text
cargo test -p nimbus-runtime cycle98_vm_module_import_meta -- --nocapture
node_compat node22-supported-lane-executes-cycle98-vm-module-import-meta node22 summary: selected=1, passed=1, skipped=0, failed=0
node_compat node24-default-lane-executes-cycle98-vm-module-import-meta node24 summary: selected=1, passed=1, skipped=0, failed=0
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 915 filtered out
```

Generated required blocker inventory after regeneration:

```text
node22 required gaps: 0
node24 required gaps: 0
```

Verifier step 9:

```text
[9] Node22/Node24 V8-isolate-required green
  PASS  Node22 and Node24 V8-isolate-required fixtures are 100%
```

## Notes

The full verifier still reports unrelated private closeout/proof-corpus failures
in this checkout, but the named NDS merge gate condition is no longer blocked by
required-surface gaps.
