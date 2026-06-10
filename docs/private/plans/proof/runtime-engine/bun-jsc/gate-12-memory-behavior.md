# Bun/JSC Gate 12: Memory Behavior

Date: 2026-05-23

Nimbus prior proof revision: `ccdc0135` (`Record Bun permission inventory proof`)

Bun worktree: `/Users/jack/src/github.com/oven-sh/bun`

Bun prior proof commit: `9e20ac28a2` (`Add Bun embed permission inventory proof`)

Bun proof commit: `f6c87be47e` (`Add Bun embed memory behavior proof`)

Bun upstream base in local worktree: `f161e0311d`
(`shell: wrap only component-leading ! when neutralizing glob metachars (#31272)`)

Bun patch status: committed locally on Bun `main`, not upstreamed.

## Question

When the non-CLI Bun/JSC embed target runs generated Nimbus invocations under
retained allocation load, does Nimbus have a per-VM heap limit, a pressure
signal, or only a safe discard policy?

## Scope

This gate measures memory behavior. It does not make Bun/JSC a selectable
Nimbus backend, does not add a production route, and does not claim tenant
memory isolation inside a retained in-process VM.

## Patch Shape

The Bun proof commit adds a seventh C ABI probe to the non-CLI native smoke
target:

```text
nimbus_bun_embed_probe_memory_behavior()
```

`scripts/build/bun.ts` wires that probe into `check-bun-embed-probe` after the
permission inventory probe.

`src/embed_probe/lib.rs` now:

1. creates a fresh non-CLI `VirtualMachine` with `InitOptions::is_main_thread = false`,
2. installs the proof `__nimbusHostCall` and `__nimbusAsyncHostCall` JSC host
   functions,
3. installs a generated-wrapper-compatible `__nimbusCreateContext`,
4. loads the generated Nimbus program wrapper,
5. records a baseline heap sample after sync GC,
6. runs 16 generated `messages:sendAndSchedule` invocations while retaining a
   JS allocation graph,
7. records heap samples before load, after load, after retained sync GC, after
   releasing the retained graph, and after `shrink_footprint()`, and
8. prints the safe first policy.

## Probe Source Snippets

Generated invocation pressure:

```js
globalThis.__nimbusMemoryRetained = [];
globalThis.__nimbusMemoryProbe = async () => {
  for (let i = 0; i < 16; i += 1) {
    const payload = "x".repeat(64 * 1024) + ":" + i;
    const cells = Array.from({ length: 2048 }, (_value, j) => ({
      i,
      j,
      payload,
      marker: `cell-${i}-${j}`,
    }));
    globalThis.__nimbusMemoryRetained.push({ payload, cells });
    const response = await globalThis.__nimbusInvoke({
      kind: "mutation",
      function_name: "messages:sendAndSchedule",
      args: { body: payload.slice(0, 64) },
    });
    if (response.status !== "ok") {
      return -1;
    }
  }
  return globalThis.__nimbusMemoryRetained.length;
};
globalThis.__nimbusMemoryProbe()
```

Retained graph release:

```js
globalThis.__nimbusMemoryRetained = null;
globalThis.__nimbusMemoryProbe = undefined;
1
```

## Memory Result

Final native proof output:

| Sample | Value |
| --- | ---: |
| invocation count | 16 |
| heap after setup GC bytes | 221431 |
| heap before load bytes | 221431 |
| heap after load bytes | 512532 |
| heap retained after GC bytes | 5499474 |
| heap after release GC bytes | 285224 |
| heap after shrink bytes | 195847 |
| observed load growth bytes | 291101 |
| observed GC retained growth bytes | 5278043 |
| observed release drop bytes | 5214250 |

Classification:

| Capability | Result |
| --- | --- |
| hard per-VM heap limit | `not_observed` |
| pressure signal | `vm.heap_size_and_sync_gc` |
| safe first policy | `fresh_vm_or_discard_on_pressure` |

## Source Ownership

| Surface | Bun source owner observed | Nimbus implication |
| --- | --- | --- |
| Heap size sample | `src/jsc/VM.rs` exposes `VM::heap_size()`, backed by `JSC__VM__heapSize` in `src/jsc/bindings/bindings.cpp`, which returns `JSC::VM::heap.size()`. | Nimbus can sample pressure from the VM, but this is observability, not enforcement. |
| Sync GC sample | `src/jsc/VirtualMachine.rs` exposes `VirtualMachine::garbage_collect(sync)`, which calls mimalloc cleanup and `VM::run_gc(true)`; `src/jsc/bindings/bindings.cpp` runs full JSC GC and returns `sizeAfterLastFullCollection()`. | Nimbus can use sync GC as a discard/pressure decision point for a trusted proof VM. |
| Footprint shrink | `src/jsc/VM.rs` exposes `VM::shrink_footprint()`, backed by `JSC__VM__shrinkFootprint`; the first proof run hit a JSC assertion until the call was made under the API lock. | Lifecycle code must respect JSC API-lock ownership around shrink/teardown operations. |
| Small heap mode | `src/jsc/VirtualMachine.rs` carries `InitOptions::smol`; `packages/bun-types/bun.d.ts` documents worker `smol` as selecting JSC's small heap. | `smol` is a lower-memory mode, not a per-tenant hard heap cap in this embed proof. |

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
nimbus bun embed memory behavior:
  invocation_count: 16
  heap_after_setup_gc_bytes: 221431
  heap_before_load_bytes: 221431
  heap_after_load_bytes: 512532
  heap_retained_after_gc_bytes: 5499474
  heap_after_release_gc_bytes: 285224
  heap_after_shrink_bytes: 195847
  observed_load_growth_bytes: 291101
  observed_gc_retained_growth_bytes: 5278043
  observed_release_drop_bytes: 5214250
  hard_heap_limit: not_observed
  pressure_signal: vm.heap_size_and_sync_gc
  safe_first_policy: fresh_vm_or_discard_on_pressure
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

Status: memory behavior measured, hard per-VM heap limit not observed.

The current embed lane gives Nimbus a useful pressure signal:
`VM::heap_size()` and sync GC can show retained heap growth and release after
discarding the retained graph. This is not enough for untrusted multi-tenant
in-process isolation. A product Bun/JSC backend would need an outer hard limit
at the process, cgroup, or microVM boundary, and the first safe in-process
lifecycle policy is fresh VM or discard-on-pressure.

Bun/JSC therefore remains `in_process_trusted_only` and proof-only. The next
gate should prove package/module loading and resolver policy without adding any
production Bun selector, runtime route, or codegen target.
