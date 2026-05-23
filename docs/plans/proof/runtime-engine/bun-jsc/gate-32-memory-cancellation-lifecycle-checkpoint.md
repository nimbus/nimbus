# Bun/JSC Gate 32: Memory, Cancellation, And Lifecycle Checkpoint

Date: 2026-05-23

Nimbus plan: `docs/plans/bun-jsc-embedder-api-and-pool-plan.md`

## Status

Status: macOS and Linux/minicloud proof passed; `BEP7` is complete.

Bun proof commit `4b5de5ee5d173975485fd907abe7b6e1457a90c5`
(`Add Bun embedder pre-entry cancellation gate proof`) strengthens the
lifecycle proof record. It keeps cancellation state/ack-driven instead of
sleep-driven, records fresh teardown and retained trusted reuse, and proves
before-guest-entry cancellation through an owner-side entry gate before calling
into Bun/JSC.

## What Changed

The Bun proof target now records lifecycle cancellation as:

```text
nimbus bun embed cancellation policy:
  before_guest_entry: owner_entry_gate_denied_and_recovered
  after_guest_entry_sync_loop: spin_entered_ack
  recovery_after_deadline_cancel: ok
  recovery_after_external_cancel: ok
  cancellation_timing_policy: state_ack_not_sleep

nimbus bun embed lifecycle reuse stress:
  fresh_vm_create_invoke_destroy_iterations: 4
  retained_vm_invocations_before_cancel: 8
  external_cancel_recovery_iterations: 3
  external_cancel_trigger: spin_entered_ack
  cancellation_timing_policy: state_ack_not_sleep
  normal_completion_before_cancel: retained_invocations_ok
  promise_microtask_progress: async_host_bridge_ok
  teardown_loop: fresh_vm_create_invoke_destroy_ok
  retained_vm_post_cancel_invocation: ok
  retained_vm_reuse: trusted_generated_wrapper_ok
  product_first_policy: fresh_vm_or_discard_with_outer_quota_required
```

This removes the previous elapsed-time sleep as the proof's cancellation
trigger. The background canceller now spins until either the invocation
finishes or the guest reaches the generated spin handler and calls the
host-owned acknowledgement function.

An attempted pre-entry check using the current JSC termination APIs did not
interrupt evaluation before guest entry. The proof now models the correct
pool-owned product behavior instead: if cancellation is observed before entry,
the owner entry gate denies the invocation and never calls `Bun__REPL__evaluate`.

## Memory Policy Evidence

The macOS proof still does not observe a hard Bun/JSC per-VM heap limit:

```text
nimbus bun embed memory behavior:
  invocation_count: 16
  hard_heap_limit: not_observed
  pressure_signal: vm.heap_size_and_sync_gc
  safe_first_policy: fresh_vm_or_discard_on_pressure
```

That keeps the product policy conservative: untrusted Bun/JSC remains
fresh/discard with an outer quota requirement unless a hard per-VM memory
boundary lands.

## Local Verification

Passed in `/Users/jack/src/github.com/oven-sh/bun`:

```sh
cargo fmt --all --check
bun scripts/build.ts --profile=debug-no-asan \
  --build-dir=/private/tmp/nimbus-bun-embed-native \
  --cache-dir=/private/tmp/nimbus-bun-cache \
  --target=check-bun-embed-probe
git diff --check
```

Passed in `/Users/jack/src/github.com/nimbus/nimbus`:

```sh
bash scripts/verify-bun-jsc-in-process-lockdown.sh
```

The reusable gate passed all 10 steps locally, including:

- 10 Nimbus runtime policy tests
- 4 Bun/JSC pool scaffold tests
- 10 registry/runtime metadata rejection tests
- 2 runtime diagnostics tests
- 1 ignored Bun source proof test
- Bun `cargo fmt --all --check`
- Bun native `check-bun-embed-probe`
- Nimbus and Bun whitespace checks

## Linux Verification

Passed on Debian 13 `minicloud` as `nimbus@192.168.4.29` from isolated proof
worktrees created from local Git bundles:

```text
Nimbus proof worktree: /home/nimbus/src/github.com/nimbus/nimbus-bep7-proof-20260523180348
Nimbus proof commit:   84e6fb640127d5985b2920e1edc8be4b6dbc912f
Bun proof worktree:    /home/nimbus/src/github.com/oven-sh/bun-bep7-proof-20260523180348
Bun proof commit:      4b5de5ee5d173975485fd907abe7b6e1457a90c5
```

The Linux command was:

```sh
cd /home/nimbus/src/github.com/nimbus/nimbus-bep7-proof-20260523180348
. "$HOME/.cargo/env"
export NVM_DIR="$HOME/.nvm"
. "$NVM_DIR/nvm.sh"
nvm use --lts >/dev/null
export PROOF_ROOT="$HOME/.cache/nimbus-proof"
mkdir -p "$PROOF_ROOT/tmp" \
  "$PROOF_ROOT/bun-embed-native" \
  "$PROOF_ROOT/bun-cache" \
  "$PROOF_ROOT/bun-rust-only" \
  "$PROOF_ROOT/bun-cargo-target"
export TMPDIR="$PROOF_ROOT/tmp"
export PATH="$HOME/.local/toolchains/LLVM-21.1.8-Linux-X64/bin:$HOME/.bun/bin:$PATH"
NIMBUS_BUN_REPO=/home/nimbus/src/github.com/oven-sh/bun-bep7-proof-20260523180348 \
NIMBUS_BUN_BUILD_DIR="$PROOF_ROOT/bun-embed-native" \
NIMBUS_BUN_CACHE_DIR="$PROOF_ROOT/bun-cache" \
NIMBUS_BUN_RUST_ONLY_BUILD_DIR="$PROOF_ROOT/bun-rust-only" \
NIMBUS_BUN_CARGO_TARGET_DIR="$PROOF_ROOT/bun-cargo-target" \
bash scripts/verify-bun-jsc-in-process-lockdown.sh
```

The reusable gate passed all 10 Linux steps, including:

- 10 Nimbus runtime policy tests
- 4 Bun/JSC pool scaffold tests
- 10 registry/runtime metadata rejection tests
- 2 runtime diagnostics tests
- 1 ignored Bun source proof test
- Bun `cargo fmt --all --check`
- Bun native `check-bun-embed-probe`
- Nimbus and Bun whitespace checks

The Linux native probe produced the same BEP7 lifecycle result:

```text
nimbus bun embed cancellation policy:
  before_guest_entry: owner_entry_gate_denied_and_recovered
  after_guest_entry_sync_loop: spin_entered_ack
  recovery_after_deadline_cancel: ok
  recovery_after_external_cancel: ok
  cancellation_timing_policy: state_ack_not_sleep

nimbus bun embed lifecycle reuse stress:
  fresh_vm_create_invoke_destroy_iterations: 4
  retained_vm_invocations_before_cancel: 8
  external_cancel_recovery_iterations: 3
  external_cancel_trigger: spin_entered_ack
  cancellation_timing_policy: state_ack_not_sleep
  normal_completion_before_cancel: retained_invocations_ok
  promise_microtask_progress: async_host_bridge_ok
  teardown_loop: fresh_vm_create_invoke_destroy_ok
  retained_vm_post_cancel_invocation: ok
  retained_vm_reuse: trusted_generated_wrapper_ok
  product_first_policy: fresh_vm_or_discard_with_outer_quota_required
```

## Outcome

`BEP7` is complete. Bun/JSC memory, cancellation, teardown, retained trusted
reuse, and fresh/discard policy are proven locally on macOS and on Debian 13
`minicloud` for Bun proof head `4b5de5ee5d`. The product policy remains
conservative: untrusted Bun/JSC must start as fresh/discard with an outer quota
unless a hard per-VM heap boundary lands.
