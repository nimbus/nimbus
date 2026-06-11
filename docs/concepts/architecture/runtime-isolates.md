---
title: Runtime and isolates
description: How Nimbus executes user JavaScript in-process — the standalone V8 runtime crate, the HostBridge inversion, bundle integrity, and the resource limits around every invocation.
sidebar:
  label: "Runtime & isolates"
  order: 6
---

Every Nimbus function — query, mutation, action, HTTP handler, cron job —
executes inside the server process in a V8 isolate. This page explains the
architecture of that execution layer: why it lives in a deliberately
isolated crate, how user code reaches the rest of the system through one
narrow trait, and what surrounds each invocation so misbehaving code cannot
take the server down with it.

For what functions are *allowed to touch*, see
[Runtime permissions](/concepts/runtime-permissions/). For what the Node
compatibility surface *means*, see
[How the Node runtime works](/concepts/nodejs-runtime/). This page is about
the machinery underneath both.

## A runtime crate with zero workspace dependencies

The execution layer is `crates/nimbus-runtime`, and it depends on no other
Nimbus crate. Its dependency list is the JavaScript engine stack and
general-purpose libraries — nothing that knows about Nimbus storage, the
engine, adapters, or transport.

That constraint is an architectural invariant, not an accident. It forces
the runtime to define execution in its own vocabulary:

- `RuntimeBundle` — a path to compiled user code plus an optional expected
  SHA-256 (`crates/nimbus-runtime/src/runtime/bundle.rs`).
- `RuntimePolicy` / `RuntimeLimits` — the full execution contract: backend,
  compatibility target, mode, grants, heap and timeout budgets, pooling and
  admission caps (`crates/nimbus-runtime/src/limits/`).
- `InvocationRequest` / `InvocationAuth` — what to run and on whose behalf,
  carried as plain data.
- `HostBridge` — the single trait through which running code can reach
  anything outside the isolate (`crates/nimbus-runtime/src/host.rs`).

Because the crate cannot import the engine, it is physically incapable of
"reaching around" its own boundary. Everything user code can do to the
outside world is enumerable by reading one trait and one operation enum.

## The HostBridge inversion

The runtime does not call the engine; the server hands the runtime an
implementation of the runtime's own trait. `HostBridge` is defined in
`crates/nimbus-runtime/src/host.rs` with one required method — `call`,
taking a typed `HostCallRequest` — plus cancellable and async variants.

Every operation user code can perform against the host is a variant of the
`HostCallOperation` enum: document reads and writes, query-builder steps,
pagination, scheduler calls (`run_after`, `run_at`, `cancel`), nested
function calls, service lookups. There is no second channel.

On the server side, each adapter's bridge implements the trait over the
engine. The Convex bridge lives at
`crates/nimbus-server/src/adapters/convex/host_bridge/` and holds an
`Arc<Engine>`; the Cloud Functions bridge is
`crates/nimbus-cloud-functions/src/host_bridge.rs`. Shared plumbing —
host-call dispatch, admission, capability checks, read tracking — lives in
`crates/nimbus-bridge`, which depends on `nimbus-engine`, not the other way
around.

The consequence is the property described in
[the engine and mutation path](/concepts/architecture/engine-mutation-path/):
a `ctx.db.insert(...)` from inside an isolate goes through exactly the same
engine code as a direct HTTP write. The runtime cannot bypass schema
validation, authorization, index maintenance, or the commit log, because
the only door out of the isolate leads straight to the engine.

```text
isolate (user code)
   │  ctx.db.*, ctx.scheduler.*, ctx.run*
   ▼
HostBridge trait            crates/nimbus-runtime  (defines)
   ▼
adapter host bridge         crates/nimbus-server, crates/nimbus-bridge  (implements)
   ▼
Engine                      crates/nimbus-engine  (authorizes + applies)
```

## V8 through a maintained Deno-family fork

Nimbus does not embed raw V8 directly. The runtime is built on the Deno
runtime stack — `deno_core` for the isolate and module machinery, plus the
extension crates (`deno_web`, `deno_fetch`, `deno_crypto`, `deno_node`, and
others) that provide web-standard and Node-compatible builtins — pinned to
Nimbus-maintained forks of that stack, including the matching V8 binding.
The forks exist so Nimbus can control bootstrap, capability wiring, and
compatibility behavior precisely rather than tracking upstream defaults
designed for a general-purpose CLI runtime.

## Runtime backends

Execution engines sit behind a small internal seam,
`crates/nimbus-runtime/src/backends/`:

- **V8** (`backends/v8/`) is the product backend. Every shipped
  configuration routes through it.
- **Bun/JSC** (`backends/bun_jsc/`) is an exploratory lane behind a
  non-default build feature (`bun-jsc-linked-adapter`). In a default build the
  adapter reports itself *not linked* and any invocation routed at it fails
  closed with a disabled-backend error — it never silently falls back to V8
  or runs with weaker enforcement. Even when linked, the backend's policy
  axes are validated as a set (`crates/nimbus-runtime/src/limits/axes.rs`):
  a Bun/JSC policy must declare outer-quota memory enforcement, and a linked
  Bun/JSC policy that asks for a wall-clock timeout it cannot enforce is
  rejected outright.

The axis validation is the interesting part: backend kind, trust tier,
lockdown profile, lifecycle policy, and pool kind must form a coherent,
explicitly allowed combination, or policy construction panics. There is no
way to configure a backend with another backend's isolation assumptions.

## Compatibility targets are not permissions

`RuntimeLimits` carries two axes that are easy to conflate and deliberately
independent:

- **`RuntimeCompatibilityTarget`** — which API surface the module sees:
  `WebStandardIsolate` (the default web-platform surface) or a Node lane
  (`Node20`, `Node22`, `Node24`, `Node26`, driven by a checked-in LTS lane
  registry). This decides *what globals and builtins exist*.
- **`RuntimeGrants`** — fourteen named grant families (filesystem read and
  write, network connect and listen, environment read and write, secrets,
  identity, services, subprocesses, system info, FFI, workers, tools) that
  decide *what those APIs are allowed to reach*
  (`crates/nimbus-runtime/src/limits/grants.rs`).

Selecting `Node24` gives a module `node:fs` as an API; it grants no file
access. A Node action and a web-standard query with the same grants have
the same blast radius. Mode ceilings cap the combination — `Restricted`
mode requires all fourteen grant families to be empty, and `Standard` mode
forbids FFI grants entirely. The full model, including how risky workloads
are routed out of the shared engine, is covered in
[Runtime permissions](/concepts/runtime-permissions/) and
[How the Node runtime works](/concepts/nodejs-runtime/).

## Bundle integrity: hash before every invocation

A deployed function bundle is identified by content, not just by path.
`RuntimeBundle` records an expected SHA-256 at construction, and
`verify_integrity` re-reads and re-hashes the bundle file before the module
is evaluated — on every invocation, not once at deploy time
(`crates/nimbus-runtime/src/runtime/bundle.rs`). The check sits on each
execution path: the standard invocation driver, the cooperative warm-pool
path, and the feature-gated Bun/JSC lane all call it before running code.

A mismatch aborts the invocation with an integrity error. The reasoning is
that path-backed bundles are mutable files on disk; a stable bundle
identity is fine for pooling and metrics, but never a substitute for
proving that the bytes about to execute are the bytes that were deployed.

## Resource limits: heap, wall clock, cancellation

Every isolate runs inside three independent enforcement mechanisms, wired
in `crates/nimbus-runtime/src/runtime/driver/`:

- **Heap.** The V8 isolate is created with hard heap limits derived from
  the policy's `max_heap_mb`. A near-heap-limit callback additionally fires
  before exhaustion, cancels the invocation, and terminates execution — the
  isolate is killed rather than allowed to thrash at the ceiling.
- **Wall clock.** A dedicated watchdog thread
  (`crates/nimbus-runtime/src/watchdog.rs`) tracks every invocation's
  deadline from the policy's `execution_timeout`. On expiry it calls V8's
  thread-safe `terminate_execution` from outside the isolate, so a hot loop
  that never yields is still stopped.
- **External cancellation.** Host-side cancellation (a disconnected client,
  a shut-down deployment) registers with the same watchdog and terminates
  the isolate the same way.

All three converge on the same outcome — the isolate is terminated, not
asked politely — which is what makes the limits enforceable against
adversarial code.

## Executor admission: per-tenant fairness

Above individual isolates sits `RuntimeExecutor`
(`crates/nimbus-runtime/src/executor/`): a fixed set of worker threads
fed by an admission layer that is tenant-aware. Each tenant gets
independent caps from the policy — maximum active invocations, maximum
in-flight invocations, and a bounded per-tenant queue. When a tenant
exceeds its in-flight budget, its work parks in its own queue; queued
tenants are promoted in round-robin rotation, and a tenant whose queue is
full has new work rejected rather than absorbed
(`crates/nimbus-runtime/src/executor/admission/`). One tenant's burst
cannot starve another tenant's latency, and cannot grow server memory
without bound.

The executor also owns pooling strategy. The default pool kind reuses a
per-worker V8 startup snapshot but builds a fresh isolate runtime per
invocation — the freshest execution boundary. An opt-in warm-pool mode
retains evaluated runtimes across invocations for latency, with surgical
per-request state reset and routing affinity keys (per tenant, function, or
script) controlling who may reuse what. Module-level state persistence in
warm mode is an explicit, typed contract in the policy, never an ambient
side effect.

## Where this sits in the architecture

The runtime is one layer of the single binary described in
[How Nimbus works](/concepts/how-nimbus-works/). Requests reach it through
the [server transport](/concepts/architecture/server-transport/) and
[adapters](/concepts/architecture/adapters/); everything it does to data
flows through the
[engine mutation path](/concepts/architecture/engine-mutation-path/); and
workloads that need a real OS boundary instead of an isolate are routed to
[sandboxes and machines](/concepts/architecture/sandbox-machines/). How
caller identity reaches the isolate — and why it arrives as data rather
than authority — is covered in
[Auth and the trust boundary](/concepts/architecture/auth-trust/).
