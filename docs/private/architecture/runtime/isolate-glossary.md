# Nimbus Isolate Glossary

Status: **internal grounding** · LOCAL-ONLY (`docs/private/`) · Authored 2026-06-23

> **Canonical, user-facing version:** `docs/reference/glossary/isolate-runtime.md`
> (published under Reference on the docs site). This internal copy keeps the
> exemplar `file:line` provenance and naming-decision evidence the public page
> omits; the public page is the source of truth for the definitions.

The canonical vocabulary for Nimbus isolates and their implementation
architecture. Every term carries a one-line definition, an **origin** (the
canonical exemplar `file:line` under `~/src/github.com/*` or a primary
paper/spec), and **aliases/collisions**. Built from the canonical exemplars
(workerd, deno_core, isolated-vm, openworkers, blueboat, bun, Firecracker,
libkrun, convex-backend) plus primary sources (V8 blog, WHATWG, Firecracker
NSDI 2020, AWS SnapStart, CRIU, Orleans/Akka/EJB, ACPI), cross-checked against
the broader field with our exemplar domains blocked (so terms are corroborated,
not self-confirmed).

> **Read §3 + §7 first** if you touch residency/lifecycle code — that is where
> the contested naming was settled.

---

## 0. Canonical naming decisions (the settled calls)

| Concept | **Nimbus term** | Why (evidence) | Rejected / aliased |
|---|---|---|---|
| Warm in RAM, instant resume | **Resident** | OS "resident set" (RSS/working set) = pages held in RAM. Our coinage for the isolate layer (no exemplar uses a single word; workerd says "pinned to memory", runtimes say "warm"). | not "warm" (latency adjective, overloaded) |
| Dropped from RAM; only a small explicit serialized attachment survives; rebuilt on next use | **Hibernated** | workerd uses `hibernat*` for *exactly* this (`hibernation-manager.h`, `serializeAttachment`); our DO-compat target. **= passivation semantics.** | aliases: **passivated** (EJB/Akka), **deactivated** (Orleans), **evicted** (mechanism). ⚠ **NOT** ACPI-S4 hibernate (which *preserves* full state — see §7). |
| Full live state captured, RAM freed, everything restored exactly | **Checkpointed** | CRIU "checkpoint/restore"; the precise term. Only feasible at the process/microVM layer (the firecracker backend family), never in-process V8. | mechanism aliases: microVM **snapshot** (Firecracker), **SnapStart** (AWS). Named *Checkpointed* not *Snapshotted* to avoid colliding with the **V8 startup snapshot** used pervasively in the runtime. |
| Who picks the residency state | **`auto`** (policy default) | platform chooses; corroborated by Orleans auto-deactivation, Convex idle-timeout recreate. | — |
| **Rejected as residency names** | — | **`pinned`** = affinity (session / CPU / `mlock`) — collides *within one cluster* (OpenWorkers `PinnedPoolConfig`=affinity vs workerd "pinned to memory"=residency). **`frozen`** = cgroup freezer (`Pause()` literally `Freeze(cgroups.Frozen)` in runc/crun/moby), SIGSTOP, `Object.freeze`. | — |

The residency **states** (Resident/Hibernated/Checkpointed) are one axis; the
**policy** (`auto | resident | hibernated`) is a separate axis that selects them.
See `docs/private/plans/connection-broker-plan.md` §2/§13.

---

## 1. Isolate fundamentals

- **Isolate** — A V8 VM instance with its own heap, GC, and microtask queue;
  objects in one isolate cannot be referenced from another, and it executes JS
  on at most one thread at a time. The unit of memory/correctness isolation for
  per-tenant JS. *Origin:* V8 embedder guide (v8.dev/docs/embed); `ManagedIsolate`
  wraps `v8::Isolate` (`nimbus/deno/libs/core/runtime/managed_isolate.rs:11`).
  *Aliases:* "V8 isolate", isolated-vm "vm"; **not** a process or microVM.
- **Context** — An execution environment inside an isolate with its own global
  object + built-ins; one isolate can hold many. *Origin:* v8.dev/docs/embed.
  *Collisions:* overloaded with deno_core `OpCtx`; spec twin is **Realm**.
- **Realm** — The ECMAScript-spec notion a Context implements (global + intrinsics).
  deno_core `JsRealm` is a non-owning *reference*; two realms on one isolate can
  never run concurrently (every op needs `&mut v8::Isolate`). *Origin:*
  `nimbus/deno/libs/core/runtime/jsrealm.rs:221`. *Aliases:* used interchangeably
  with Context.
- **Heap** — The per-isolate region where JS objects live; what a startup
  snapshot serializes. Reached via Handles (GC may relocate). Two isolates have
  disjoint heaps → values are **copied**, never shared. *Origin:* v8.dev/docs/embed.
  *Collisions:* distinct from the system heap and from V8 "external" memory.
- **Garbage collector (GC)** — V8's per-isolate generational memory manager;
  runs on the isolate's thread, so it **cannot** reclaim a live closure graph
  into a serializable form (the fact that bounds hibernation). *Origin:*
  v8.dev/docs/embed. *Aliases:* "Orinoco", "scavenger" (minor), "mark-compact".
- **Startup snapshot (V8)** — A pre-serialized image of an *initialized* isolate
  heap, deserialized to create contexts in <2 ms. **STARTUP-ONLY:** cannot capture
  API callbacks, typed-array backing stores, or live external state. *Origin:* V8
  blog "Custom startup snapshots"; `create_snapshot`
  (`nimbus/deno/libs/core/runtime/snapshot.rs:186`). *Collisions:* **NOT** a
  live-heap checkpoint — contrast Firecracker/CRIU (see Checkpointed, §3).
- **Code cache / bytecode cache** — Serialized compiled bytecode keyed by source
  hash, so a module need not be re-parsed on next load. *Origin:* deno_core
  `SourceCodeCacheInfo` (`nimbus/deno/libs/core/modules/mod.rs:444`); V8
  `ScriptCompiler::CreateCodeCache`. *Aliases:* "compile cache".
- **Microtask checkpoint** — Where V8 drains the microtask queue (promise
  reactions, `queueMicrotask`) to empty when the call stack empties. *Origin:*
  WHATWG HTML; `perform_microtask_checkpoint`
  (`nimbus/deno/libs/core/runtime/jsruntime.rs:2905`).
- **Event loop** — The cooperative scheduler alternating "run JS to stack-empty +
  microtask drain" with "poll host async sources (timers/I-O/op completions)".
  *Origin:* WHATWG HTML; deno_core `poll_event_loop` (`jsruntime.rs:2823`).
  *Aliases:* Node "libuv loop", "reactor"; **one loop per isolate-thread**.
- **Op (deno_core)** — A registered `#[op2]` function crossing the JS↔Rust
  boundary — the mechanism for any capability outside the ECMAScript spec.
  *Origin:* `OpCtx` (`nimbus/deno/libs/core/runtime/ops.rs:83`). *Collisions:* not
  a Nimbus `HostCallOperation`.
- **External reference** — A pointer to embedder code registered in V8's
  external-reference table so a snapshot can encode it as a stable index and
  rebind on restore — *why* snapshots can't auto-capture native callbacks.
  *Origin:* `OpCtx::external_references` (`ops.rs:149`).
- **Thread-affinity / `!Send` / Locker** — A V8 isolate is thread-affine
  (entered by one thread at a time; `OwnedIsolate` auto-entry or cooperative
  `v8::Locker` handoff), surfacing in Rust as `!Send`. *Origin:*
  `ManagedIsolate` (`managed_isolate.rs:11`); workerd `jsg::Lock`. ⚠ **Do not call
  this "pinned"** (reserved/rejected — see §0).
- **Module / ESM graph** — The import/export DAG materialized in deno_core's
  `ModuleMap`; instantiated then evaluated. *Origin:* `nimbus/deno/libs/core/modules/map.rs`.
- **HostBridge / host call** — The Nimbus seam by which sandboxed JS requests a
  privileged host op (DB/fetch/KV); a `Send + Sync` trait with
  `call`/`call_cancellable`/`call_async`. The single mediated capability boundary
  (keeps `nimbus-runtime` workspace-dep-free). *Origin:*
  `crates/nimbus-runtime/src/host.rs`. *Collisions:* sits *above* raw deno ops.
- **Embedder** — The native program hosting V8 (registers contexts/ops, drives
  the loop). Nimbus's runtime is the embedder. *Origin:* v8.dev/docs/embed.
  *Aliases:* "host", "runtime"; distinct from the **guest** (the JS workload) and
  the **operator** (who deploys Nimbus).

---

## 2. Execution & scheduling

- **Invocation** — One end-to-end execution of a workload's entry handler (one
  `fetch`/`scheduled`/RPC event); the atomic unit of scheduling, billing, and
  limits. *Origin:* workerd `LimitEnforcer` per-invocation
  (`io/limit-enforcer.h`); Vercel bills "Invocations". *Collisions:* distinct from
  "instance" (the reusable isolate serving many invocations).
- **Invocation-scoped / request-scoped** — State/capabilities/timers bound to one
  Invocation, torn down when it ends. *In V8 isolates a persistent socket/server
  is impossible* because the isolate is alive only for the request window.
  *Origin:* Nimbus established fact; workerd per-request `IoContext`. *Aliases:*
  "per-request", "ephemeral"; opposite of "instance-scoped"/"actor-scoped" (DO).
- **Run-to-completion** — Once JS starts it runs entirely with no preemption
  before any other JS; the next task is taken only after the stack empties and
  microtasks drain. *Origin:* WHATWG HTML; MDN execution model.
- **Cooperative scheduling** — Tasks yield voluntarily (at `await`/loop
  boundaries); the runtime cannot interrupt sync JS except by hard-terminating
  the isolate. *Origin:* contrasted in workerd's out-of-band watchdog;
  Wasmtime epoch-interruption is the Wasm analog.
- **Event-loop turn / tick** — One loop iteration: take one macrotask, run to
  completion, fully drain microtasks. *Origin:* openworkers `pump_and_checkpoint`
  (`execution_context.rs:751`). *Collisions:* Node `process.nextTick` is a distinct
  higher-priority queue.
- **Watchdog** — An out-of-band monitor (thread/timer) that force-terminates a
  workload stuck in sync code past its deadline; may watch a forward-progress
  counter instead of a flat timeout. *Origin:* workerd `Watchdog` +
  `ThreadProgressCounter`; blueboat `computation_watcher` (`exec.rs:202`).
- **Watchdog grace period** — A short wait after the deadline trips before the
  expensive `TerminateExecution` + context reset, in case the workload was about
  to finish. *Origin:* blueboat 100 ms grace + `abort_fence` (`exec.rs:205`).
- **Execution termination (`terminate_execution`)** — Forcibly cancel running JS
  via V8 `Isolate::TerminateExecution()` from another thread (uncatchable
  unwind); the context is poisoned afterward and must be reset, not reused.
  V8 builtins skip interrupt checks, so a flag is also polled. *Origin:* workerd
  `setup.h:107`; blueboat `exec.rs:231`. *Collisions:* not a catchable JS error.
- **CPU time vs wall-clock time** — CPU time counts on-processor cycles only;
  wall-clock counts elapsed real time incl. I/O waits. Time on `fetch`/KV does
  **not** accrue CPU but does accrue wall-clock. *Origin:* Cloudflare Workers
  limits (30 s CPU default within a longer duration).
- **Active CPU** — Vercel Fluid's billing metric: ms the code is on-CPU; **pauses
  during I/O and between requests**. *Origin:* vercel.com usage-and-pricing.
  *Collisions:* a billing line, **not** a scheduler/residency state — do not
  conflate "idle" here with Hibernated.
- **Fuel / gas metering** — Deterministic per-instruction budget that traps the
  guest at zero (Wasm). *Origin:* Wasmtime `consume_fuel`. ⚠ **Wasm-only** — V8
  isolates have **no** instruction metering; Nimbus bounds isolate compute with
  the wall-clock watchdog + `TerminateExecution`.
- **waitUntil / continuation** — Registers async work (logging/cache) that
  finishes *after* the response, extending the Invocation under a separate bounded
  limit. *Origin:* workerd `limitDrain` (`io/limit-enforcer.h:152`); openworkers
  `drain_waituntil`.
- **Drain** — The bounded post-response phase keeping an instance alive to finish
  waitUntil/streams/alarm tails. *Origin:* workerd `limitDrain`/`limitScheduled`.
  *Collisions:* not "connection draining" (graceful server shutdown) nor microtask
  drain (one turn).
- **Cold start** — Latency of serving with no warm instance: isolate-create +
  context + top-level eval, or snapshot-restore, or microVM/process spawn.
  Layer-overloaded (isolate ≪ microVM ≪ container). *Origin:* openworkers cold
  path (`pool_common.rs:41`).
- **Warm start** — Serving by reusing an already-Resident pooled instance
  (`reset()` between uses). *Origin:* openworkers warm-hit pool.
- **In-instance concurrency** — One instance handling many Invocations at once
  (amortizing memory/cold-starts), valuable for I/O-bound work. *Origin:* Vercel
  "optimized/in-function concurrency"; openworkers multiplexed pool
  (`pool.rs:66`). *Collisions:* Cloudflare's default is one-request-per-isolate.

---

## 3. Residency, lifecycle & memory

> The states are **Resident → Hibernated → Checkpointed** (decreasing idle
> memory, increasing resume cost / state-fidelity tradeoffs). See §0 and §7.

- **Resident** — Live state (heap/stack/external refs) warm in RAM, runnable with
  zero rebuild. Measured at the OS layer as **RSS** (Resident Set Size). *Origin:*
  Wikipedia "Resident set size". *Aliases:* working set (the actively-used subset),
  "warm" (latency framing).
- **Hibernated** — **Passivation:** the live heap is dropped from RAM; only a
  small explicit serialized **attachment** survives; rebuilt on next use. *Origin:*
  workerd WebSocket Hibernation (`io/hibernation-manager.h:71`,
  `api/web-socket.h:322` "if the object isn't serialized, it will not survive
  hibernation"). *Aliases:* Passivated, Deactivated, Evicted, Deflated. ⚠
  **Collision:** ACPI-S4 "hibernate" is the *inverse* (preserves full state) — §7.
- **Checkpointed** — Full live-state capture (memory + vCPU/process state)
  restored to resume exactly. **V8 cannot do this** for a running heap; it is a
  process/microVM-layer capability (Nimbus **firecracker backend
  family**, `firecracker-fast-invocation-backend-plan.md`). *Origin:* CRIU
  checkpoint/restore (criu.org); Firecracker Full snapshot
  (`firecracker/docs/snapshotting/snapshot-support.md:182`). *Aliases:* microVM
  **snapshot**, **SnapStart**. Named *Checkpointed* not *Snapshotted* to avoid
  the V8-startup-snapshot collision (§7).
- **Eviction** — Reclaiming a workload's RAM by removing it (with or without an
  attachment), typically under memory pressure. The *mechanism*; Hibernated is one
  resulting *state*. *Origin:* convex-backend `eviction.rs`; workerd actor-cache
  time-based eviction (`io/worker.h:210`).
- **Activation / Deactivation** — Orleans virtual-actor lifecycle: activate a
  grain on-demand when a message arrives, deactivate idle grains. Maps to
  Resident ↔ Hibernated. *Origin:* Orleans grain lifecycle. *Collisions:* Orleans
  "activation" is *also* the noun for the in-memory instance; EJB "activation" =
  the *restore* half of passivation.
- **Passivation** — The canonical name for Hibernated's semantics: serialize an
  idle stateful entity to secondary storage, remove from memory, restore on next
  access. *Origin:* EJB `ejbPassivate`/`ejbActivate`; Akka `ShardRegion.Passivate`.
- **Attachment / `serializeAttachment`** — The small explicitly-serialized blob
  that survives Hibernation; everything else is discarded. Hard cap **16 KiB**.
  *Origin:* workerd `web-socket.c++:766`, `MAX_ATTACHMENT_SIZE = 1024*16`
  (`web-socket.h:448`).
- **Warm pool / prewarming** — A reserve of initialized Resident workers so an
  incoming request skips cold-start. *Origin:* convex-backend "prewarmed V8
  context" (`isolate_worker.rs:302`). *Aliases:* hot pool; AWS "provisioned
  concurrency" is the managed analog.
- **Idle timeout (`hibernateAfter`)** — Inactivity duration after which an idle
  Resident workload transitions out of RAM. *Origin:* convex-backend
  `ISOLATE_IDLE_TIMEOUT` (`client.rs:1490`); Orleans/Akka
  `passivate-idle-entity-after`. *Collisions:* distinct from request/execution
  timeout (the 30 s `system_timeout` is per-invocation, not residency).
- **Memory pinning (`mlock`)** — Locking pages into physical RAM so the kernel
  can't swap them. *Origin:* `mlock(2)`. ⚠ **Rejected** as a residency-state name
  ("pinned" = affinity — §0/§7); included so the collision is documented.
- **Deflation / Inflation** — Reclaiming a warm container's RAM by swapping app
  pages to disk, re-faulting on request (init cost kept, RAM cut). The container-
  layer analog of Hibernated. *Origin:* "Hibernate Container" paper (arXiv
  2305.10963) — ~7–25% of warm memory.
- **Residency policy (`auto`)** — The knob deciding *when* to transition between
  states; Nimbus default `auto` (platform chooses by idle/pressure/eligibility).
  *Origin:* this taxonomy; Orleans auto-deactivation.

---

## 4. Isolation & multi-tenancy

- **Runtime owner ID** — The authority-bearing identity allowed to reuse
  guest-mutated runtime state: owner class, opaque stable subject, and positive
  incarnation. Display/audit labels do not participate in equality. Tenant
  incarnations come from Engine/storage, not the runtime or adapter. *Origin:*
  `crates/nimbus-runtime/src/retained_state.rs` and
  `crates/nimbus-compute/src/runtime_manager.rs`.
- **Runtime owner lease** — A revocable manager-issued proof that one runtime
  owner incarnation is still admitted. It is carried by queued, active, and
  returning work; revocation condemns retained state and rejects new guest
  entry. *Collisions:* not a tenant label, routing key, deployment generation,
  or `TenantIsolationDecisionId`.
- **Reuse authority** — The closed exact-match facts required after the runtime
  owner matches: deployment authority, bundle provenance, runtime lane/shape,
  permissions, effective capabilities and services, construction mode, and
  backend-specific observable state. Missing or changed facts deny reuse.
- **Routing affinity** — Best-effort worker locality for hit rate. It grants no
  reuse authority: `None`, tenant, function, and script routing choices all
  retain the same mandatory owner boundary. *Collisions:* affinity is not
  ownership and cannot make a runtime globally clean.
- **Isolate-as-security-boundary** — Using a V8 isolate as the *only* tenant
  separation. **Verdict: a strong memory/correctness boundary but NOT a hard
  security boundary** — it cannot stop CPU side channels. *Origin:* Cloudflare
  security-model blog ("V8 itself cannot defend against Spectre").
- **Process isolation** — Separate OS processes with disjoint address spaces —
  the accepted answer to Spectre at the language layer. *Origin:* V8 "A year with
  Spectre" ("move sensitive data out of the process's address space"); Chrome
  Site Isolation.
- **MicroVM** — A minimal hardware-virtualized guest (KVM/HVF) with a stripped
  device model, booting in tens of ms — VM-grade isolation at container density.
  The strongest practical tenant boundary in the stack. *Origin:* Firecracker
  NSDI 2020 (`firecracker/docs/design.md:7`); libkrun.
- **Sandbox** — A mechanism-neutral confinement boundary (isolate, seccomp+ns,
  gVisor, or microVM). *Origin:* workerd README ("not a hardened sandbox … run it
  inside a VM"). ⚠ Always qualify the *tier* — "sandbox" alone says nothing about
  strength.
- **gVisor / userspace-kernel** — A memory-safe Go userspace "application kernel"
  (the Sentry) that intercepts and reimplements syscalls instead of forwarding
  them, shrinking host attack surface. *Origin:* gvisor.dev/docs.
- **Tenant isolation** — One customer's workload cannot observe/corrupt/starve
  another's (memory, CPU, I/O, side channels). *Origin:* Firecracker NSDI 2020.
  Inside Nimbus's shared in-process tier, guest-mutated V8 isolates and
  Wasmtime Stores are partitioned by exact runtime-owner incarnation and
  retired on deletion; this is still a language-level boundary, not protection
  from a V8/native/process-memory compromise. *Aliases:* "hard (VM-grade) vs
  soft (isolate-grade) multi-tenancy".
- **Side-channel / Spectre** — Speculative-execution attacks that transiently
  bypass safety checks and exfiltrate memory via a cache-timing channel — *why*
  an isolate is not a hard boundary (the leak is in shared silicon). *Origin:* V8
  "A year with Spectre".
- **Trusted vs untrusted workload** — The classification that picks the isolation
  tier. *Origin:* Firecracker design ("all vCPU threads … running malicious
  code"). *Collisions:* orthogonal to *authenticated*.
- **Capability / permission model (deno_permissions)** — Deny-by-default
  authority; each host op checked against a typed allowlist; default state is
  *Prompt* (fail-closed). *Origin:* `nimbus/deno/runtime/permissions/lib.rs:162`.
- **Defense-in-depth tiers (isolate < process < microVM)** — Layered containment,
  each a stronger fallback. Nimbus routes by trust onto this ladder. *Origin:*
  Cloudflare layered model; Firecracker seccomp + jailer.
- **Snapshot-fork / zygote** — Pre-init one warm template, spawn instances by CoW
  clone. *Origin:* Android Zygote; Firecracker CoW resume. ⚠ **Security caveat:**
  resuming one captured state more than once breaks RNG-seed/ID/token uniqueness.
- **Memory-safety boundary** — Enforced by language/runtime semantics (V8 heap
  separation, Go/Rust safety), strong vs memory-corruption but porous to side
  channels. *Origin:* gVisor (memory-safe Go); workerd README.

---

## 5. Concurrency, scaling & fast-start

- **Serialization lane** — Per-key write ordering; Nimbus keys it
  `(tenant, do_namespace, do_id)`. *Origin:* connection-broker-plan; the per-DO
  write lane (CFA7).
- **Single-activation / single-writer** — At most one in-memory instance per id
  (Orleans: one activation per grain). The DO correctness core. *Origin:* Orleans.
- **Fencing token / lease** — A monotonic token per write; storage rejects stale
  tokens (the safe substitute for "absolute" single-instance, which is impossible
  on commodity infra). *Origin:* Kleppmann "How to do distributed locking".
- **Placement / routing** — Resolving which node owns an id (grain directory);
  Nimbus's `ClusterTransport`-shaped lookup (HS5). *Origin:* Orleans directory.
- **Affinity / sticky session** — Routing a client to the same instance for a
  connection's life. **This is the correct meaning of "pinned"** — which is why
  Nimbus rejects "pinned" as a residency-state name. *Origin:* LB session
  affinity; k8s CPUManager "exclusively pinned".
- **Virtual actor** — An always-addressable entity the runtime activates
  on-demand and deactivates when idle. *Origin:* Orleans.
- **Durable Object** — A single-instance-per-id stateful actor with a private
  storage namespace. Nimbus builds it over its own primitives (not embedded
  workerd). *Origin:* Cloudflare DO; `cloudflare-adapters-plan.md`.
- **Firecracker snapshot** — Pause a microVM to `Paused`, capture memory + device
  + vCPU state (Full or Diff), resume later. The mechanism behind **Checkpointed**.
  *Origin:* `firecracker/docs/snapshotting/snapshot-support.md`.
- **CRIU (checkpoint/restore)** — Userspace checkpoint of a process tree (memory,
  fds, TCP, timers) to disk, restored exactly. *Origin:* criu.org.
- **AWS Lambda SnapStart / CRaC** — Managed snapshot-restore: run init, take a
  Full Firecracker snapshot, restore instead of re-initializing; coordinated by
  CRaC `beforeCheckpoint`/`afterRestore` hooks. The managed realization of
  Checkpointed. *Origin:* AWS Lambda docs.
- **Provisioned concurrency** — Pre-initialized warm envs kept ready (the managed
  warm-pool). *Origin:* AWS Lambda.
- **Fork / zygote** — Warm parent template forked CoW. *Origin:* Android/Chromium
  zygote. ⚠ V8 isolates cannot be forked; live capture is VM/process-layer only.
- **WebSocket hibernation** — Evict the JS object while the runtime keeps the
  socket open; re-instantiate on the next frame. *Origin:* workerd
  `hibernation-manager.h`; Cloudflare DO best-practices.
- **Two-object split** — A durable host-owned socket (`kj::WebSocket`) + a
  disposable JS wrapper (`api::WebSocket`); `acceptWebSocket` marks it
  hibernatable. *Origin:* workerd `actor-state.h:635`.
- **Auto-response** — `setWebSocketAutoResponse`: the runtime answers a configured
  ping/pong on the raw socket **without waking** the isolate. *Origin:* workerd;
  Nimbus caps: 2048-char request, 16 KiB attachment.
- **Connection broker** — The host-owned component holding every long-lived socket
  in Rust and invoking the isolate per-frame; unifies inbound hibernation with the
  egress PEP. *Origin:* `connection-broker-plan.md`.

---

## 6. Vendor term-map

One concept ↔ how each ecosystem names it. (⚠ marks a collision/inversion.)

| Concept | Nimbus | Cloudflare | Vercel | AWS Lambda | Orleans | Akka/EJB | OS / VM |
|---|---|---|---|---|---|---|---|
| Warm in RAM | **Resident** | "pinned to memory" / preventEviction | warm / provisioned memory | provisioned concurrency | activation (noun) | active | resident set (RSS); ACPI S3 ≈ RAM-kept |
| Drop from RAM, rebuild from explicit state | **Hibernated** | hibernation | — (no hibernation) | — | deactivation | passivation | cache **eviction**; k8s Job **suspend** ⚠ |
| Full live-state checkpoint + restore | **Checkpointed** | — | — | **SnapStart** (Firecracker snap + CRaC) | — | — | Firecracker **snapshot** / CRIU **checkpoint** / **ACPI S4 "hibernate"** ⚠ |
| Startup heap blob | (V8) **startup snapshot** | — | bytecode cache | — | — | — | — |
| Affinity / stickiness | **affinity** (not "pinned") | — | "pinned to one instance" | — | — | — | **"pinned"** = CPU affinity / `mlock` ⚠ |
| Cold-start avoidance | **warm pool** | — | — | provisioned concurrency | — | — | **zygote** (Android) / min-instances (GCP) |

---

## 7. Why these names (the contested calls, with evidence)

- **Resident** ✓ — RSS / "resident set" = pages in physical RAM (Wikipedia;
  bun uses "resident" for mapped memory). Our coinage for the isolate layer; no
  exemplar offers a cleaner single word. **Keep.**
- **Hibernated** ✓ (with a documented caveat) — **strongly attested**: workerd
  uses `hibernat*` for *exactly* our drop-heap/keep-attachment semantics
  (`HibernationManager::hibernateWebSockets`, `serializeAttachment`), and it is
  our DO-compat target. The precise field synonym is **passivation** (EJB/Akka)
  / **deactivation** (Orleans). ⚠ **The one caveat:** ACPI-S4 "hibernate"
  *preserves* the full memory image to disk — the **inverse** of ours. We keep
  "Hibernated" (serverless-domain term of art) but define it as *passivation,
  not S4*; reach for **passivated** when systems precision matters.
- **Checkpointed** ✓ (not *Snapshotted*) — the VM/process field uses **snapshot**
  (Firecracker) and **checkpoint** (CRIU) near-synonymously for full-live-state
  capture. We pick **Checkpointed** because **"snapshot" is already taken in
  Nimbus** for the **V8 startup snapshot** (deno_core, used pervasively); naming
  the residency state "Checkpointed" keeps `grep snapshot` unambiguous. The
  mechanism is still "microVM snapshot"; only the *state name* is Checkpointed.
- **Reject "pinned"** ✓ — overloaded *within a single cluster*: OpenWorkers
  `PinnedPoolConfig`/`execute_pinned` = thread/pool **affinity**, while workerd
  "pinned to memory"/`preventEviction` = residency; k8s CPUManager "exclusively
  pinned" + runc `cpuset` = CPU affinity; `mlock` "pinned pages" = page-locking.
  None cleanly means "warm-in-RAM residency".
- **Reject "frozen"** ✓ — container `Pause()` **literally is**
  `Freeze(cgroups.Frozen)` (runc/crun/moby/podman); also SIGSTOP "stopped" and
  `Object.freeze` (immutability). "Frozen" = suspended-but-resident, the opposite
  of a memory-freeing state.
- **`auto` policy default** ✓ — "the platform chooses" is the widely-understood
  reading; corroborated by Orleans auto-deactivation and Convex idle-timeout
  recreate. Kept distinct from the explicit forces `resident`/`hibernated`.

---

Related: `docs/private/plans/connection-broker-plan.md` (residency states +
policy, §2/§13), `docs/private/architecture/runtime/adapter-boundary.md`,
`docs/private/plans/firecracker-fast-invocation-backend-plan.md` (the
Checkpointed tier; formerly sandbox-plan Band S).
Memory: `[[project_connection_broker_and_egress_exemplars]]`.
