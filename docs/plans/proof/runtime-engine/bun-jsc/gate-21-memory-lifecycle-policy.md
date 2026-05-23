# Bun/JSC Gate 21: Memory And Lifecycle Policy

Date: 2026-05-23

Nimbus plan: `docs/plans/bun-jsc-in-process-lockdown-plan.md`

Inputs:

- `docs/plans/proof/runtime-engine/bun-jsc/gate-12-memory-behavior.md`
- `docs/plans/proof/runtime-engine/bun-jsc/gate-14-lifecycle-reuse-stress.md`
- `docs/plans/proof/runtime-engine/bun-jsc/gate-20-permission-lockdown-decision.md`

Bun worktree: `/Users/jack/src/github.com/oven-sh/bun`

Bun local proof head: `65cdc97796` (`Add Bun embed lifecycle reuse proof`)

Bun upstream base in local worktree: `f161e0311d`
(`shell: wrap only component-leading ! when neutralizing glob metachars (#31272)`)

## Decision

Status: policy encoded; untrusted promotion remains blocked.

Nimbus now treats lifecycle as a first-class runtime backend axis, not an
implicit property of a cache or pool:

| Lifecycle policy | Meaning | Product state |
| --- | --- | --- |
| `v8_deno_core_pool` | Current V8/Deno runtime pool semantics, including the existing warm-pool and reset behavior. | Selectable for V8 only. |
| `bun_jsc_trusted_retained_pool` | Retained Bun/JSC VM reuse proven only for Nimbus-generated trusted proof wrappers. | Rejected as a product route. |
| `bun_jsc_fresh_discard_pool_outer_quota_required` | Candidate untrusted Bun/JSC shape: fresh/discard VM lifecycle plus a hard outer memory quota until Bun/JSC exposes a hard per-VM heap boundary. | Rejected until permission, resolver, memory, and teardown hooks exist. |

The first tenant-selectable Bun/JSC runtime, if it ever lands, must use
fresh VM per invocation or discard-after-invocation behavior and a hard memory
boundary outside the JSC heap sample. It should still be implemented as a
dedicated Bun/JSC runtime pool beside the existing V8/Deno/Node pool; the
fresh/discard rule describes what the Bun pool may retain for untrusted
tenants, not whether a Bun pool exists. Retained in-process Bun/JSC VMs remain
trusted-only until the pool can prove resolver, permission, memory,
cancellation, and teardown isolation.

## Why Retained Bun/JSC Is Trusted-Only

Gate 14 proved a valuable lifecycle property:

- create/invoke/destroy fresh VMs works
- a retained VM can run multiple generated Nimbus invocations
- host-owned cancellation can interrupt generated code
- the same retained VM can recover after cancellation in the proof lane

That does not make retained reuse tenant-safe because Gates 11, 13, 18, 19,
and 20 still show unmediated host-sensitive surfaces and resolver authority.

For untrusted tenants, retained reuse would need all of these before promotion:

- permission hooks for host-sensitive builtins
- resolver/package hooks for dynamic import and `Bun.resolve*`
- proof that timers, workers, sockets, subprocess handles, promises, and
  native resources cannot survive across invocations
- a hard memory boundary or an outer quota that kills/discards the whole Bun/JSC
  runtime context

## Memory Policy

Gate 12 observed:

| Memory capability | Result |
| --- | --- |
| hard per-VM heap limit | `not_observed` |
| pressure signal | `vm.heap_size_and_sync_gc` |
| release signal after dropping retained graph | observed |
| first safe policy | `fresh_vm_or_discard_on_pressure` |

`VM::heap_size()`, synchronous GC, and `shrink_footprint()` are useful pressure
signals. They are not isolation controls. A tenant-safe Bun/JSC backend needs
one of:

- a real JSC/Bun hard per-VM heap limit with deterministic failure behavior, or
- an outer process/cgroup/microVM quota where exceeding memory kills or
  discards the runtime boundary, not another tenant's retained state.

Until then, `RuntimeBackendLifecyclePolicy::BunJscFreshDiscardPoolOuterQuotaRequired`
is the only acceptable untrusted candidate policy, and Nimbus still rejects it
because the required outer quota and permission hooks are not implemented.

## Cancellation And Teardown Policy

The lifecycle proof makes host-owned cancellation plausible but limited:

- the owner thread primes JSC termination state before external cancellation
- cancellation requests interrupt generated spin loops
- recovery is verified with a follow-up script and post-cancel invocation
- teardown uses `VirtualMachine::destroy()` for fresh VM paths

For untrusted promotion, the teardown proof must additionally verify:

- all timers are cleared or bound to the invocation deadline
- all workers are denied or joined/terminated with the same runtime identity
- pending promises cannot call HostBridge after invocation close
- open sockets/fetch/WebSocket/server handles are absent or closed
- subprocesses and IPC are absent or killed/reaped
- FFI/native addon state is absent
- event-loop work cannot escape into the next tenant invocation
- cancellation and teardown are recorded in runtime diagnostics/audit evidence

## Nimbus Code Changes

`RuntimeBackendLifecyclePolicy` was added next to the backend trust and
lockdown axes:

```rust
RuntimeBackendLifecyclePolicy::V8DenoCorePool
RuntimeBackendLifecyclePolicy::BunJscTrustedRetainedPool
RuntimeBackendLifecyclePolicy::BunJscFreshDiscardPoolOuterQuotaRequired
```

The lifecycle policy is included in:

- `RuntimeLimits`
- runtime bundle engine cache keys
- runtime diagnostics responses
- tenant runtime policy decisions
- public runtime/facade exports

Current admission behavior:

- V8 requires `v8_deno_core_pool`
- Bun/JSC proof/trusted profiles require `bun_jsc_trusted_retained_pool` and
  still panic as non-selectable
- Bun/JSC untrusted profile requires `bun_jsc_fresh_discard_pool_outer_quota_required`
  and still panics as not implemented
- mismatched Bun trust/profile/lifecycle combinations fail closed before any
  product route can form

## Verification

Nimbus:

```sh
cargo check -p nimbus-runtime -p nimbus-server
cargo test -p nimbus-runtime limits::tests --lib
```

Result:

```text
cargo check -p nimbus-runtime -p nimbus-server
Finished `dev` profile [unoptimized + debuginfo] target(s) in 10.26s

cargo test -p nimbus-runtime limits::tests --lib
running 9 tests
test result: ok. 9 passed; 0 failed; 0 ignored; 0 measured; 490 filtered out
```

Bun:

No Bun files were modified for this gate. Gate 12 and Gate 14 remain the source
of the memory and lifecycle proof output.

## Outcome

`BIL5` is complete. Lifecycle is now explicit in Nimbus policy, diagnostics,
and cache identity. Bun/JSC remains non-selectable: retained VM reuse is
trusted proof-only, and untrusted promotion requires fresh/discard lifecycle
plus an outer hard memory quota and the permission/resolver hooks recorded in
Gates 19 and 20.
