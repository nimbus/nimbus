# Bun/JSC Gate 14: Lifecycle Reuse Stress

Date: 2026-05-23

Nimbus prior proof revision: `7af4b624` (`Record Bun package module policy proof`)

Bun worktree: `/Users/jack/src/github.com/oven-sh/bun`

Bun prior proof commit: `f0cee692c0` (`Add Bun embed package module policy proof`)

Bun proof commit: `65cdc97796` (`Add Bun embed lifecycle reuse proof`)

Bun upstream base in local worktree: `f161e0311d`
(`shell: wrap only component-leading ! when neutralizing glob metachars (#31272)`)

Bun patch status: committed locally on Bun `main`, not upstreamed.

## Question

Can the non-CLI Bun/JSC embed target create and destroy VMs repeatedly, reuse a
retained VM for generated Nimbus invocations, recover after host-owned
cancellation, and continue invoking the same generated wrapper?

## Scope

This gate measures lifecycle behavior. It does not make Bun/JSC selectable,
does not add a production runtime route, and does not claim that retained VM
reuse is safe for tenant-controlled code. Permission containment and package
resolver containment are still unresolved.

## Patch Shape

The Bun proof commit adds a ninth C ABI probe to the non-CLI native smoke
target:

```text
nimbus_bun_embed_probe_lifecycle_reuse_stress()
```

`scripts/build/bun.ts` wires that probe into `check-bun-embed-probe` after the
package/module policy probe.

`src/embed_probe/lib.rs` now:

1. creates and destroys four fresh non-CLI VMs through the existing generated
   program-wrapper host-call proof,
2. creates one retained non-CLI VM,
3. installs sync and async JSC host functions,
4. installs a stateful generated-wrapper-compatible `__nimbusCreateContext`,
5. loads the generated Nimbus program wrapper,
6. invokes `messages:sendAndSchedule` eight times on the retained VM,
7. verifies sync and async host-call counters after retained reuse,
8. interrupts generated `messages:spinForever` three times through external
   host cancellation,
9. proves same-VM recovery after each cancellation, and
10. invokes `messages:sendAndSchedule` once more after cancellation recovery.

## Probe Source Snippets

Retained lifecycle context:

```js
globalThis.__nimbusLifecycleProbeState = {
  dbObserved: -1,
  insertCount: 0,
  scheduleCount: 0,
  scheduleHostResult: -1,
  lastBody: "",
};
globalThis.__nimbusCreateContext = () => ({
  db: {
    insert: async (_table, document) => {
      const observed = await globalThis.__nimbusAsyncHostCall(41);
      const state = globalThis.__nimbusLifecycleProbeState;
      state.dbObserved = observed;
      state.insertCount += 1;
      state.lastBody = document && document.body || "";
      return `message-id-${state.insertCount}`;
    },
  },
  scheduler: {
    runAfter: async () => {
      const state = globalThis.__nimbusLifecycleProbeState;
      state.scheduleCount += 1;
      state.scheduleHostResult = globalThis.__nimbusHostCall(41);
      return `job-id-${state.scheduleCount}`;
    },
  },
});
```

Retained invocation:

```js
globalThis.__nimbusInvoke({
  kind: "mutation",
  function_name: "messages:sendAndSchedule",
  args: { body: "reuse-N" },
}).then((response) => {
  return response.status === "ok" && response.value === "message-id-N"
    ? 42
    : -1;
})
```

Final post-cancel state check:

```js
(() => {
  const state = globalThis.__nimbusLifecycleProbeState;
  return state.insertCount === 9
    && state.scheduleCount === 9
    && state.dbObserved === 42
    && state.scheduleHostResult === 42
    && state.lastBody === "post-cancel"
      ? 42
      : -1;
})()
```

## Result

Final native proof output:

| Surface | Result |
| --- | --- |
| fresh VM create/invoke/destroy iterations | `4` |
| retained VM invocations before cancel | `8` |
| external cancel/recovery iterations | `3` |
| retained VM post-cancel invocation | `ok` |
| retained VM reuse | `trusted_generated_wrapper_ok` |
| product first policy | `fresh_vm_or_discard_until_containment` |

The retained VM stayed usable across generated-wrapper invocations and after
three host-owned cancellation/recovery cycles. That is useful lifecycle
evidence, but it is not enough to promote Bun/JSC to a tenant-selectable
backend because Gate 11 and Gate 13 still expose unmediated host-sensitive
surfaces.

## Source Ownership

| Surface | Bun or Nimbus source owner observed | Nimbus implication |
| --- | --- | --- |
| VM create/destroy | `src/jsc/VirtualMachine.rs` owns `VirtualMachine::init` and `destroy`; the proof calls them below `bun_bin`. | Non-CLI VM lifecycle is reproducible for proof targets. |
| Generated wrapper load | `src/embed_probe/nimbus_generated_program_bundle.js` is loaded through `Bun__REPL__evaluate`. | Retained reuse proof covers the real generated program-wrapper lane selected by Gate 13. |
| Promise/event-loop progress | `VirtualMachine::wait_for_promise` and `event_loop_mut().ensure_waker()` drive generated async host calls. | Retained invocations need explicit event-loop driving from the embedder. |
| Cancellation | Prior Gate 10 helper calls JSC termination through `notify_need_termination()` and clears termination state after joining the cancel thread. | Same-VM reuse after cancellation is possible in the proof lane when the owner thread primes termination and recovery is verified. |
| Product lifecycle policy | Nimbus docs and runtime seam own the promotion gate. | Retained reuse is only acceptable for trusted generated-wrapper proof code until permission and resolver containment are solved. |

## Verification

Formatting:

```sh
cd /Users/jack/src/github.com/oven-sh/bun
cargo fmt --all --check
```

Result: passed.

Native proof target:

```sh
cd /Users/jack/src/github.com/oven-sh/bun
bun scripts/build.ts --profile=debug-no-asan \
  --build-dir=/private/tmp/nimbus-bun-embed-native \
  --cache-dir=/private/tmp/nimbus-bun-cache \
  --target=check-bun-embed-probe
```

Result:

```text
nimbus bun embed lifecycle reuse stress:
  fresh_vm_create_invoke_destroy_iterations: 4
  retained_vm_invocations_before_cancel: 8
  external_cancel_recovery_iterations: 3
  retained_vm_post_cancel_invocation: ok
  retained_vm_reuse: trusted_generated_wrapper_ok
  product_first_policy: fresh_vm_or_discard_until_containment
[build] check-bun-embed-probe done
```

Observed upstream warnings remained unchanged from prior proof runs:

- `bun_crash_handler`: 3 unnecessary `unsafe` warnings
- `bun_spawn`: 1 unused-label warning
- `bun_install`: 1 unused-label warning
- `bun_runtime`: 2 unnecessary `unsafe` warnings

Whitespace check:

```sh
cd /Users/jack/src/github.com/oven-sh/bun
git diff --check
```

Result: passed.

## Decision

Status: lifecycle behavior measured, product reuse still blocked by
containment.

The proof shows retained Bun/JSC VM reuse can survive generated Nimbus
invocation loops and external cancellation in the trusted program-wrapper lane.
The product posture does not change: Bun/JSC remains proof-only and
`in_process_trusted_only`. If Bun/JSC is ever promoted, the first product-safe
policy is still fresh VM or discard-on-pressure/timeout/package-loader use
until permission containment and resolver policy are enforced.

The next gate should verify explicit runtime artifact metadata and server or
codegen rejection of unsupported Bun combinations before invocation, without
adding a production Bun route.
