# NDS3 Cycle 97 - VM module top-level await

Date: 2026-06-14

## Summary

Cycle 97 promotes this Node24 VM fixture:

- `test/parallel/test-vm-module-hastoplevelawait.js`

Fork fixes:

- rusty_v8 fork: `/Users/jack/src/github.com/nimbus/rusty_v8`
- Branch/tag: `nimbus/v149.4.0` / `v149.4.0-nimbus.2`
- Commit: `8f70a59 Expose Module::HasTopLevelAwait`
- Deno fork: `/Users/jack/src/github.com/nimbus/deno`
- Branch/tags: `nimbus/v2.8.3` / `v2.8.3-nimbus.44`, `v2.8.3-nimbus.45`
- Commits:
  - `0ea61c46b2 Expose vm module top-level await status`
  - `d23b4c5c47 Register vm module top-level await op`

Nimbus is repinned from Deno `v2.8.3-nimbus.43` / rusty_v8
`v149.4.0-nimbus.1` to Deno `v2.8.3-nimbus.45` / rusty_v8
`v149.4.0-nimbus.2` in `Cargo.toml`, `Cargo.lock`, and `.cargo/config.toml`.
No local Cargo path override is present in the final Nimbus state.

## Host Resource Answer

The previous "from-source V8 build OOM" concern was not merely a one-time disk
full event. This host has 32 GiB RAM and 12 logical CPUs, and rusty_v8's
`build.rs` passes Cargo `NUM_JOBS` to ninja unless `NINJA` is `autoninja`.
That makes an unconstrained local V8 source build capable of driving enough
parallel compile/link work to exhaust memory on this machine. Disk pressure is a
separate risk: this run started with `/System/Volumes/Data` around 50 GiB free
and 95% used.

Cycle 97 therefore did not build V8 from source locally. The source change was
published through the `nimbus/rusty_v8` release workflow, then Nimbus consumed
the prebuilt `v149.4.0-nimbus.2` archive.

## Root Cause

`vm.SourceTextModule.prototype.hasTopLevelAwait()` needs the V8 module-record
metadata exposed by `v8::Module::HasTopLevelAwait()`. The previous Nimbus fork
line had the neighboring graph-async API but not this binding, so Deno's
`node:vm` polyfill could not answer the Node fixture's top-level-await query.

The fix is deliberately narrow:

- rusty_v8 adds `v8__Module__HasTopLevelAwait` in `src/binding.cc` and
  `Module::has_top_level_await()` in `src/module.rs`.
- Deno adds `op_vm_module_has_top_level_await` in `ext/node/ops/vm.rs`,
  registers it in `ext/node/lib.rs`, and exposes
  `SourceTextModule.prototype.hasTopLevelAwait()` from
  `ext/node/polyfills/vm.js`.

## Fork Proof

rusty_v8 local checks before tagging:

```text
git diff --check
cargo fmt --check
RUSTY_V8_VERSION=149.4.0-nimbus.1 cargo check --locked
```

The last check intentionally forced the existing `.1` archive while validating
the Rust wrapper shape before the `.2` release asset existed.

rusty_v8 release proof:

```text
gh run watch 27510442251 -R nimbus/rusty_v8 --interval 60 --exit-status
```

Result:

```text
release aarch64-apple-darwin: success
release x86_64-unknown-linux-gnu: success
release x86_64-pc-windows-msvc: success
release aarch64-unknown-linux-gnu: success
publish release assets: success
```

Deno fork proof:

```text
git diff --check
cargo fmt --check
CARGO_ENCODED_RUSTFLAGS='' cargo check -p deno_node --locked
```

The final Deno check passed after the `.45` registration fix:

```text
Checking deno_node v0.189.0 (/Users/jack/src/github.com/nimbus/deno/ext/node)
Finished `dev` profile [unoptimized + debuginfo] target(s) in 6.14s
```

## Immutable-Tag Probe

First immutable Nimbus probe against Deno `v2.8.3-nimbus.44` failed because the
op existed but was not registered in the `deno_node` extension:

```text
node_compat nds-probe node24 summary: selected=1, passed=0, skipped=0, failed=1
TypeError: op_vm_module_has_top_level_await is not a function
```

Diagnostic artifacts:

- `target/node-compat/diagnostics/batch/node24__nds_probe__summary.json`
- `target/node-compat/diagnostics/vm/node24__test_parallel_test_vm_module_hastoplevelawait_js.json`

After Deno `v2.8.3-nimbus.45`, the same scratch probe passed against immutable
tags:

```text
Compiling v8 v149.4.0 (https://github.com/nimbus/rusty_v8?tag=v149.4.0-nimbus.2#8f70a59d)
Compiling deno_node v0.189.0 (https://github.com/nimbus/deno?tag=v2.8.3-nimbus.45#d23b4c5c)
node_compat nds-probe node24 summary: selected=1, passed=1, skipped=0, failed=0
test result: ok. 1 passed; 0 failed; 0 ignored; 914 filtered out; finished in 2.36s
```

The local worktree initially still linked the stale `.1` prebuilt archive
because `.cargo/config.toml` pinned `RUSTY_V8_VERSION = "149.4.0-nimbus.1"`.
Evidence before cleanup:

```text
nm -gU target/debug/gn_out/obj/librusty_v8.a | rg "v8__Module__HasTopLevelAwait|v8__Module__IsGraphAsync"
00000000000027e0 T _v8__Module__IsGraphAsync
```

Cleanup performed:

```text
cargo clean -p v8
Removed 406 files, 1.8GiB total
rm -f target/debug/gn_out/obj/librusty_v8.a target/debug/gn_out/obj/librusty_v8.tmp
find target/debug/build -maxdepth 1 -type d -name 'v8-*' -exec rm -rf {} +
find target/debug/.fingerprint -maxdepth 1 -type d -name 'v8-*' -exec rm -rf {} +
find target/debug/deps -maxdepth 1 \( -name 'libv8-*' -o -name 'v8-*' \) -delete
```

After updating `.cargo/config.toml` to `.2`, Cargo downloaded the expected
archive:

```text
static lib URL: https://github.com/nimbus/rusty_v8/releases/download/v149.4.0-nimbus.2/librusty_v8_simdutf_release_aarch64-apple-darwin.a.gz
Downloading https://github.com/nimbus/rusty_v8/releases/download/v149.4.0-nimbus.2/librusty_v8_simdutf_release_aarch64-apple-darwin.a.gz... Done.
```

The fresh archive contains the new symbol:

```text
00000000000027e4 T _v8__Module__HasTopLevelAwait
00000000000027e0 T _v8__Module__IsGraphAsync
```

## Promotion Guard

Permanent promotion file:

- `crates/nimbus-runtime/src/runtime/tests/node/cases/nds3_cycle97_vm_module_tla.rs`

Promotion guard:

```text
cargo test -p nimbus-runtime --lib node24_default_lane_executes_cycle97_vm_module_tla -- --nocapture
```

Result:

```text
node_compat node24-default-lane-executes-cycle97-vm-module-tla node24 summary: selected=1, passed=1, skipped=0, failed=0
test result: ok. 1 passed; 0 failed; 0 ignored; 914 filtered out; finished in 2.13s
```

Node22 does not vendor this fixture in the local Node22 corpus. A temporary
attempted cross-lane guard failed with "No such file or directory" for the
Node22 fixture path, so the committed cycle97 guard is Node24-only.

## Regeneration

Commands:

```bash
/opt/homebrew/bin/python3.12 scripts/runtime/node/classifications.py sync --lane all
for s in status dashboard trends publish_evidence default_support_posture required_surface_blockers; do
  /opt/homebrew/bin/python3.12 scripts/runtime/node/$s.py >/dev/null
done
```

The first non-escalated regeneration attempt hit sandbox write denials under the
worktree; rerunning the same commands with approval succeeded.

Generated required posture after regeneration:

```text
node22 = 1, pass_rate = 99.96
node24 = 1, pass_rate = 99.96
```

Remaining generated required gap list:

```text
node22:
  test/parallel/test-vm-module-import-meta.js

node24:
  test/parallel/test-vm-module-import-meta.js
```

## Verifier

Command:

```text
bash scripts/verify-node-default-runtime-support-hardening.sh
```

Result:

```text
Summary: 13 passed, 21 failed
```

Step 9 still fails because the generated posture is not literal 100% green:

```text
Node22/Node24 V8-isolate-required green: FAIL
Expected generated posture metrics
```

The remaining step-9 blocker is only
`test/parallel/test-vm-module-import-meta.js` in both node22 and node24. The PR
must remain draft and unmerged.

## Hygiene

Commands:

```text
cargo fmt --all --check
git diff --check
```

Both passed.
