# Bun/JSC Gate 32: Memory, Cancellation, And Lifecycle Checkpoint

Date: 2026-05-23

Nimbus plan: `docs/plans/bun-jsc-embedder-api-and-pool-plan.md`

## Status

Status: macOS proof passed; Linux/minicloud proof still required before
`BEP7` can be marked complete.

Bun proof commit `44540674fc38a51fbea949ea7cf23decb1f4481c`
(`Clarify Bun embedder lifecycle proof coverage`) strengthens the lifecycle
proof record. It keeps cancellation state/ack-driven instead of sleep-driven,
records fresh teardown and retained trusted reuse, and explicitly marks
before-guest-entry cancellation as still requiring an embedder lifecycle hook.

## What Changed

The Bun proof target now records lifecycle cancellation as:

```text
nimbus bun embed cancellation policy:
  before_guest_entry: pending_embedder_lifecycle_hook
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

An attempted pre-entry check using the current termination APIs did not
interrupt evaluation before guest entry. That is now recorded as a source-level
product blocker instead of being hidden by the passing stress loop: Bun/JSC
needs an explicit embedder lifecycle hook for owner-side cancellation before
guest code starts.

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

## Linux Verification Status

Linux/minicloud verification has not yet been rerun for Bun proof commit
`44540674fc`. The existing minicloud worktrees are detached, and the Nimbus
worktree there has a local verification-script modification from the earlier
Gate 25 proof lane. To avoid overwriting remote state, the intended route is
an isolated minicloud proof worktree created from Git bundles.

The Nimbus bundle transfer completed. The Bun bundle transfer was blocked by
the approval reviewer because it exports local Bun source changes to another
machine. Since `minicloud` is a user-owned machine on the same network, this is
a reasonable verification step only after explicit user approval.

## Outcome

`BEP7` remains in progress. It can be marked complete only after the
before-guest-entry lifecycle hook is proven, the Linux/minicloud lane passes
with the same Bun proof head, and the plan records that evidence.
