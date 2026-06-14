# NDS3 Cycle 98 - VM module import.meta

Date: 2026-06-14

## Summary

Cycle 98 promotes the last remaining V8-isolate-required fixture in both
required lanes:

- `test/parallel/test-vm-module-import-meta.js`

Fork fix:

- Deno fork: `/Users/jack/src/github.com/nimbus/deno`
- Branch/tag: `nimbus/v2.8.3` / `v2.8.3-nimbus.46`
- Commit: `d3f650c2fa Support vm module import.meta initialization`

Nimbus is repinned from Deno `v2.8.3-nimbus.45` to Deno
`v2.8.3-nimbus.46` in `Cargo.toml` and `Cargo.lock`. rusty_v8 remains pinned
to `v149.4.0-nimbus.2`. No local Cargo path override is present in the final
Nimbus state.

## Root Cause

`vm.SourceTextModule` creates V8 modules outside deno_core's normal module map.
When the fixture evaluated a VM module containing `import.meta`, deno_core's
global import-meta callback tried to look up the module in the normal module
map and panicked at `libs/core/runtime/bindings.rs`.

Node expects `SourceTextModule` to support an `initializeImportMeta(meta,
module)` option. The fixture checks three important semantics:

- `initializeImportMeta` is called exactly once.
- The second callback argument is the `SourceTextModule` instance.
- The created `import.meta` object has a null prototype and receives callback
  properties.

The fix adds a VM-module import-meta initializer registry to deno_core and wires
`deno_node`'s `vm.SourceTextModule` creation/evaluation path into that registry.
The host import-meta callback checks the VM registry before taking the normal
module-map path. Registrations are scoped to the linked module graph and are
cleared after the evaluation promise settles.

## Fork Proof

Deno fork local checks before tagging:

```text
cargo fmt
CARGO_ENCODED_RUSTFLAGS='' cargo check -p deno_node
```

Result:

```text
Finished `dev` profile [unoptimized + debuginfo] target(s) in 2.81s
```

The empty `CARGO_ENCODED_RUSTFLAGS` is the established macOS fork-check shape
for bypassing Nimbus' checked-in `-fuse-ld=lld` target flag during local Deno
verification.

Published fork state:

```text
git push origin nimbus/v2.8.3 v2.8.3-nimbus.46
d23b4c5c47..d3f650c2fa  nimbus/v2.8.3 -> nimbus/v2.8.3
[new tag]               v2.8.3-nimbus.46 -> v2.8.3-nimbus.46
```

## Local-Path Probe

Nimbus was temporarily pointed at the canonical local Deno worktree while the
fix was being proven. The scratch ignored probe selected only the target
fixture in Node22 and Node24:

```text
cargo test -p nimbus-runtime nds_probe_vm_module_import_meta -- --ignored --nocapture
```

Result:

```text
node_compat nds-probe node22 summary: selected=1, passed=1, skipped=0, failed=0
node_compat nds-probe node24 summary: selected=1, passed=1, skipped=0, failed=0
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 915 filtered out; finished in 4.74s
```

The temporary probe file was removed before checkpointing.

## Promotion Guard

Permanent promotion file:

- `crates/nimbus-runtime/src/runtime/tests/node/cases/nds3_cycle98_vm_module_import_meta.rs`

Local permanent guard while still pinned to the local Deno worktree:

```text
cargo test -p nimbus-runtime cycle98_vm_module_import_meta -- --nocapture
```

Result:

```text
node_compat node24-default-lane-executes-cycle98-vm-module-import-meta node24 summary: selected=1, passed=1, skipped=0, failed=0
node_compat node22-supported-lane-executes-cycle98-vm-module-import-meta node22 summary: selected=1, passed=1, skipped=0, failed=0
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 915 filtered out; finished in 4.22s
```

## Immutable-Tag Proof

After publishing `v2.8.3-nimbus.46`, Nimbus was repinned back to immutable git
tags and the same guard was rerun:

```text
cargo test -p nimbus-runtime cycle98_vm_module_import_meta -- --nocapture
```

Result:

```text
Compiling deno_node v0.189.0 (https://github.com/nimbus/deno?tag=v2.8.3-nimbus.46#d3f650c2)
Compiling v8 v149.4.0 (https://github.com/nimbus/rusty_v8?tag=v149.4.0-nimbus.2#8f70a59d)
Finished `test` profile [unoptimized + debuginfo] target(s) in 2m 17s
node_compat node22-supported-lane-executes-cycle98-vm-module-import-meta node22 summary: selected=1, passed=1, skipped=0, failed=0
node_compat node24-default-lane-executes-cycle98-vm-module-import-meta node24 summary: selected=1, passed=1, skipped=0, failed=0
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 915 filtered out; finished in 4.16s
```

## Regeneration

Commands:

```bash
/opt/homebrew/bin/python3.12 scripts/runtime/node/classifications.py sync --lane all
/opt/homebrew/bin/python3.12 scripts/runtime/node/status.py
/opt/homebrew/bin/python3.12 scripts/runtime/node/dashboard.py
/opt/homebrew/bin/python3.12 scripts/runtime/node/trends.py
/opt/homebrew/bin/python3.12 scripts/runtime/node/publish_evidence.py
/opt/homebrew/bin/python3.12 scripts/runtime/node/default_support_posture.py
/opt/homebrew/bin/python3.12 scripts/runtime/node/required_surface_blockers.py
```

The first non-escalated classifications sync hit sandbox write denials under the
worktree; rerunning the same generator commands with approval succeeded.

Generated posture after regeneration:

```text
node22 v8_isolate_required: gaps=0, pass_rate_percent=100.0, passed=2363, total=2363
node24 v8_isolate_required: gaps=0, pass_rate_percent=100.0, passed=2400, total=2400
```

Generated required blocker inventory after regeneration:

```text
node22 required gaps: 0
node24 required gaps: 0
```

## Verifier

Command:

```text
bash scripts/verify-node-default-runtime-support-hardening.sh
```

Target gate result:

```text
[9] Node22/Node24 V8-isolate-required green
  PASS  Node22 and Node24 V8-isolate-required fixtures are 100%
```

Full script summary in this checkout:

```text
Summary: 14 passed, 20 failed
```

The remaining failures are private closeout/proof-corpus checks that are not the
literal gap gate. The cycle-98 target gate is green at 0/0.

## Final Required-Surface State

The V8-isolate-required burndown is complete for the required supported lanes:

- Node22: `0` required gaps, `100.0%`
- Node24: `0` required gaps, `100.0%`

No fixture/checker edits were made, no generated JSON was hand-edited, and the
final Nimbus pin uses the published Deno tag rather than a local path.
