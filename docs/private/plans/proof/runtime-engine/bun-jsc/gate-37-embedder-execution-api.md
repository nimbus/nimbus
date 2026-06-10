# Gate 37: Embedder Execution API Proof

Date: 2026-05-24

## Purpose

`BJA2` proves the Bun-side execution surface that a future linked Nimbus
adapter can target. This gate does not link Bun into Nimbus yet. It proves that
the local Bun proof branch can construct a Bun/JSC VM outside the CLI path,
apply the lockdown profile, evaluate a generated self-contained program
wrapper, and report the containment/lifecycle evidence Nimbus needs before
`BJA3` starts the optional linked build path.

## Bun Source

Proof head:

```text
/Users/jack/src/github.com/oven-sh/bun
2f09ba33b1 Configure Bun embed probe stack checks
```

The proof branch is clean locally. It is ahead of upstream `origin/main` and
is still proof source, not product source. `BJA8` must resolve source ownership
through an upstream Bun release/tag or a Nimbus-owned Bun fork/tag.

This gate added one required entry-root fix to the proof target:

```text
bun_core::StackCheck::configure_thread()
```

Without that call, direct native execution of `bun-embed-probe` reached
`WTF::StackBounds::end()` with an unbound stack assertion during
`Bun__StackCheck__getMaxStack`. The Bun CLI entry path already configures the
thread stack bounds; an embedder entry root must do the same before constructing
the VM.

## Proven API Shape

The native `check-bun-embed-probe` target proves these adapter-facing behaviors:

- constructs a Bun/JSC VM from a non-CLI native entry root
- evaluates the generated Nimbus self-contained program wrapper
- returns JSON-compatible results through the proof wrapper
- keeps `require`, Node builtins, raw `process.env`, `Bun.env`, timers,
  network primitives, workers, dynamic code, and unsafe Bun APIs denied or
  absent by default
- routes dynamic imports and `Bun.resolve*` through a deny-all resolver policy
  hook
- records package/module policy evidence for the program-wrapper lane
- acknowledges before-entry cancellation without entering guest code
- acknowledges after-entry cancellation from a guest sync loop before deadline
  recovery
- proves retained VM reuse only for the trusted generated wrapper profile
- records fresh/discard as the product-first policy for untrusted code
- exposes memory pressure through `vm.heap_size` plus synchronous GC evidence
  rather than claiming a hard JSC heap limit

## Local Verification

Ran in `/Users/jack/src/github.com/oven-sh/bun`:

```sh
cargo fmt --all --check
bun scripts/build.ts --profile=debug-no-asan \
  --build-dir=/private/tmp/nimbus-bun-embed-native \
  --cache-dir=/private/tmp/nimbus-bun-cache \
  --target=check-bun-embed-probe
git diff --check
```

Result: all passed.

The proof output included:

```text
before_guest_entry: owner_entry_gate_denied_and_recovered
after_guest_entry_sync_loop: spin_entered_ack
recovery_after_deadline_cancel: ok
recovery_after_external_cancel: ok
cancellation_timing_policy: state_ack_not_sleep
resolver_policy_hook: native_embedder_deny_all
required_resolver_api: nimbus_owned_bun_package_resolver
hard_heap_limit: not_observed
pressure_signal: vm.heap_size_and_sync_gc
safe_first_policy: fresh_vm_or_discard_on_pressure
retained_vm_reuse: trusted_generated_wrapper_ok
product_first_policy: fresh_vm_or_discard_with_outer_quota_required
```

## Linux Verification

Ran on Debian 13 `minicloud` as `nimbus` in
`/home/nimbus/src/github.com/oven-sh/bun`.

The proof branch was transferred with a Git bundle and checked out as
`nimbus-bja2-proof` at:

```text
2f09ba33b184a541e2ade24bf6e46bebc971a262
```

Toolchain:

```text
Debian clang version 21.1.8 (++20251105083457+0a6acd39fe6a-1~exp1~20251105195953.139)
Debian LLD 21.1.8 (compatible with GNU linkers)
```

Command:

```sh
export PATH="$HOME/.bun/bin:$PATH"
cd ~/src/github.com/oven-sh/bun
mkdir -p \
  "$HOME/.cache/nimbus-bun-proof/tmp" \
  "$HOME/.cache/nimbus-bun-proof/embed-native" \
  "$HOME/.cache/nimbus-bun-proof/cache" \
  "$HOME/.cache/nimbus-bun-proof/cargo-target"
export TMPDIR="$HOME/.cache/nimbus-bun-proof/tmp"
export CARGO_TARGET_DIR="$HOME/.cache/nimbus-bun-proof/cargo-target"
cargo fmt --all --check
bun scripts/build.ts --profile=debug-no-asan \
  --build-dir="$HOME/.cache/nimbus-bun-proof/embed-native" \
  --cache-dir="$HOME/.cache/nimbus-bun-proof/cache" \
  --target=check-bun-embed-probe
git diff --check
```

Result: all passed with exit code 0.

The Linux proof output included the same key contract evidence:

```text
before_guest_entry: owner_entry_gate_denied_and_recovered
after_guest_entry_sync_loop: spin_entered_ack
recovery_after_deadline_cancel: ok
recovery_after_external_cancel: ok
cancellation_timing_policy: state_ack_not_sleep
artifact_shape: self_contained_program_wrapper
evaluation_format: program_via_Bun__REPL__evaluate
static_esm_import_in_program: rejected
dynamic_import_node_fs: denied_by_resolver_policy
dynamic_import_package_root: denied_by_resolver_policy
plugin_virtual_module_import: denied_by_resolver_policy
Bun.resolve: denied_by_resolver_policy
Bun.resolveSync: denied_by_resolver_policy
native_addon_resolveSync: denied_by_resolver_policy
hard_heap_limit: not_observed
pressure_signal: vm.heap_size_and_sync_gc
fresh_vm_create_invoke_destroy_iterations: 4
retained_vm_invocations_before_cancel: 8
external_cancel_recovery_iterations: 3
promise_microtask_progress: async_host_bridge_ok
retained_vm_reuse: trusted_generated_wrapper_ok
product_first_policy: fresh_vm_or_discard_with_outer_quota_required
```

## Result

`BJA2` is complete. The next gate, `BJA3`, should wire an optional linked
adapter build path in Nimbus while keeping default builds no-link,
fail-closed, and free of Bun/JSC embedder symbols.
