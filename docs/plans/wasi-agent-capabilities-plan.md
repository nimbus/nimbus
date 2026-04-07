# Plan: WASI Agent Capabilities

Canonical deferred design and execution plan for adding agent OS primitives
(virtual filesystem, sandboxed process execution, HTTP client) to Neovex via
WASI Component Model interfaces backed by swappable providers.

This document owns the durable forward-looking context for the `neovex:agent`
WIT package, the `AgentOsProvider` trait, capability-based tenant admission, and
the integration path with external systems like
[agent-os](https://github.com/rivet-dev/agent-os).

---

## Status

- **Status:** `deferred`
- **Primary owner:** this plan
- **Activation gate:** promote only after `docs/plans/wasmtime-backend-plan.md`
  Phase W3 (run-to-completion wasmtime backend) reaches `done` status, so the
  WIT linker surface and `neovex-function` world are stable before this plan
  extends them with agent capabilities

## How To Use This Plan

- Read this before starting any agent OS capability or agent-os integration
  work.
- Treat it as the canonical control plane for the agent capabilities workstream
  once promoted.
- Do not start implementation until the activation gate is met.
- When promoted, implement exactly one phase at a time and record verification
  in the Execution Log before marking a phase `done`.

## Control Plan Rules

This document is the durable control plane for the WASI agent capabilities
workstream. The source of truth is:

1. the current git worktree
2. this plan's `Phase Status Ledger`, `Implementation Checkpoints`, and
   `Execution Log`
3. `ARCHITECTURE.md` for the landed runtime architecture
4. `docs/plans/wasmtime-backend-plan.md` for the wasmtime backend surface this
   plan builds on

Do not rely on prior chat transcripts as progress state.

### Status model

- `todo`: not started; eligible when hard dependencies and gate notes are
  satisfied
- `in_progress`: actively being implemented; keep exactly one phase in this
  state per autonomous execution run
- `blocked`: cannot proceed until the recorded blocker is resolved
- `done`: acceptance criteria are met and verification has been recorded
- `deferred`: intentionally parked behind a product or benchmarking gate

### Recovery loop for every new session

1. Reread this `Control Plan Rules` section, `Phase Status Ledger`,
   `Implementation Checkpoints`, `Phase Order and Dependencies`, and
   `Execution Log`.
2. Inspect the current git worktree and reconcile it against this plan before
   picking new scope.
3. If any phase is already `in_progress`, resume that phase first.
4. If the worktree is dirty, identify which phase owns the changes and update
   that phase's checkpoint or log entry before starting new work.
5. Implement exactly one phase by default.
6. Record verification in `Execution Log` before marking a phase `done`.
7. If blocked, record the blocker here before stopping.

---

## Why Agent Capabilities Need A Separate Plan

The wasmtime backend plan (`docs/plans/wasmtime-backend-plan.md`) delivers the
execution substrate: engine, linker, scheduling, module cache, bundle format.
Its WIT interfaces cover the existing `HostBridge` surface (`neovex:host` —
database, scheduler, runtime, context).

Agent workloads need OS-level primitives that do not exist in the current
`HostBridge` contract:

| Capability | Why agents need it | Current Neovex surface |
|---|---|---|
| Virtual filesystem | Read/write working files, configuration, intermediate outputs | None |
| Process execution | Run shell commands, invoke tools, execute Python/Node scripts | None |
| HTTP client | Call LLM APIs, external services, webhooks | None (host calls go through Engine, not outbound HTTP) |

These are additive capabilities — standard `neovex-function` components never
see them. The WASI Component Model enforces this: a component that does not
import `neovex:agent/filesystem` cannot call filesystem operations, period. This
is the enterprise trust property.

## Architecture Boundary

### What this plan owns

- The `neovex:agent` WIT package (filesystem, process, http-client interfaces)
- The `ComponentWorld::NeovexAgent` variant and its admission gate
- The `AgentOsProvider` trait and its concrete implementations
- Integration with external agent-os systems
- Capability-based tenant admission enforcement

### What this plan does NOT own

- The wasmtime engine, WIT linker, module cache, or fuel scheduling — those
  belong to `docs/plans/wasmtime-backend-plan.md`
- The `neovex:host` WIT interfaces (database, scheduler, runtime, context) —
  those also belong to the wasmtime backend plan
- The V8/deno_core execution path — unchanged by this plan
- The `HostBridge` trait — this plan adds new backing providers, not new host
  call operations

## Reference Implementations

### agent-os (rivet-dev/agent-os)

**Key finding from review:** agent-os does NOT embed V8 directly. It is a
process-spawning OS virtualization layer. Its value is the **virtual kernel**
(VFS, process table, PTY, pipes, permissions, sandboxed networking), not its
compute layer (Node.js child processes).

| agent-os component | What it provides | How Neovex uses it |
|---|---|---|
| `crates/kernel` | Virtual filesystem, process table, PTY, pipes, permission model | Pattern reference for `AgentOsProvider` implementations; not a direct dependency |
| `crates/bridge` | JSON-RPC protocol between sidecar and child processes | IPC protocol reference for the sidecar provider |
| `crates/execution` | Node.js process spawning | **Not used** — Neovex's V8/wasmtime replaces this entirely |
| `crates/sidecar` | Long-lived Rust service managing the virtual kernel | Possible external sidecar for the `AgentOsSidecarProvider` implementation |

### WASI standard interfaces

The following WASI interfaces are relevant and may be adopted or extended rather
than reinvented:

| WASI interface | Relevance | Adoption stance |
|---|---|---|
| `wasi:filesystem` | Standard virtual filesystem | Evaluate as a base; extend with Neovex-specific scoping if needed |
| `wasi:sockets` | Network access | Evaluate for the HTTP client surface |
| `wasi:cli` | Process environment, stdin/stdout/stderr | Reference for process I/O model |
| `wasi:http` | Outbound HTTP | Strong candidate for the `http-client` interface |

Prefer standard WASI interfaces where they fit. Define `neovex:agent`-specific
interfaces only where the standard WASI interfaces are insufficient or where
Neovex needs per-tenant scoping that WASI does not provide.

### Other references

| System | What it contributes |
|---|---|
| Fermyon Spin | Shows how to compose WASI interfaces with application-specific WIT worlds |
| Wasmtime WASI implementation | Canonical Rust implementation of `wasi:filesystem`, `wasi:sockets`, etc. |
| Shopify Functions | Shows capability-scoped WASM execution in a multi-tenant commerce context |

## Proposed WIT Interfaces

### neovex:agent/filesystem

```wit
package neovex:agent@0.1.0;

interface filesystem {
    record file-info {
        path: string,
        size: u64,
        is-dir: bool,
        modified-ms: u64,
    }

    read: func(path: string) -> result<list<u8>, string>;
    write: func(path: string, data: list<u8>) -> result<_, string>;
    append: func(path: string, data: list<u8>) -> result<_, string>;
    list: func(path: string) -> result<list<file-info>, string>;
    mkdir: func(path: string) -> result<_, string>;
    remove: func(path: string) -> result<_, string>;
    exists: func(path: string) -> result<bool, string>;
}
```

All paths are relative to a per-tenant, per-session virtual root. No path
traversal above the root. The backing implementation enforces this regardless of
which provider is active.

### neovex:agent/process

```wit
interface process {
    record process-result {
        stdout: list<u8>,
        stderr: list<u8>,
        exit-code: s32,
    }

    /// Synchronous exec — blocks until completion, with timeout
    exec: func(
        cmd: string,
        args: list<string>,
        env: list<tuple<string, string>>,
    ) -> result<process-result, string>;

    /// Long-running process handle
    type process-handle = u64;

    spawn: func(
        cmd: string,
        args: list<string>,
        env: list<tuple<string, string>>,
    ) -> result<process-handle, string>;

    write-stdin: func(handle: process-handle, data: list<u8>) -> result<_, string>;
    read-stdout: func(handle: process-handle) -> result<list<u8>, string>;
    read-stderr: func(handle: process-handle) -> result<list<u8>, string>;
    wait: func(handle: process-handle) -> result<process-result, string>;
    kill: func(handle: process-handle) -> result<_, string>;
}
```

Process handles are scoped to the current invocation. They are invalidated when
the invocation completes. The `exec` function has an implicit timeout derived
from `RuntimeLimits::execution_timeout`. Allowed commands are governed by the
provider's permission model.

### neovex:agent/http-client

```wit
interface http-client {
    record http-request {
        method: string,
        url: string,
        headers: list<tuple<string, string>>,
        body: option<list<u8>>,
    }

    record http-response {
        status: u16,
        headers: list<tuple<string, string>>,
        body: list<u8>,
    }

    fetch: func(request: http-request) -> result<http-response, string>;
}
```

Outbound HTTP is subject to:
- URL allowlist/denylist per tenant (configurable)
- Request timeout derived from remaining invocation budget
- Response body size limits

### neovex-agent world

```wit
world neovex-agent {
    // All standard Neovex host capabilities
    import neovex:host/database;
    import neovex:host/scheduler;
    import neovex:host/runtime;
    import neovex:host/context;

    // Additive agent capabilities
    import filesystem;
    import process;
    import http-client;

    export handler: func(args: string) -> result<string, string>;
}
```

A `neovex-agent` world component has strictly more capabilities than a
`neovex-function` component. The type system enforces this: a component compiled
against `neovex-function` will not import agent interfaces and therefore cannot
call them.

## Proposed Internal Shape

### AgentOsProvider trait

```text
AgentOsProvider (trait, Send + Sync + 'static)
  - fs_read(tenant, session, path) -> Result<Vec<u8>>
  - fs_write(tenant, session, path, data) -> Result<()>
  - fs_append(tenant, session, path, data) -> Result<()>
  - fs_list(tenant, session, path) -> Result<Vec<FileInfo>>
  - fs_mkdir(tenant, session, path) -> Result<()>
  - fs_remove(tenant, session, path) -> Result<()>
  - fs_exists(tenant, session, path) -> Result<bool>
  - process_exec(tenant, session, cmd, args, env) -> Result<ProcessResult>
  - process_spawn(tenant, session, cmd, args, env) -> Result<ProcessHandle>
  - process_write_stdin(handle, data) -> Result<()>
  - process_read_stdout(handle) -> Result<Vec<u8>>
  - process_read_stderr(handle) -> Result<Vec<u8>>
  - process_wait(handle) -> Result<ProcessResult>
  - process_kill(handle) -> Result<()>
  - http_fetch(tenant, session, request) -> Result<HttpResponse>
```

Every method receives tenant and session identifiers for scoping. The trait is
`Send + Sync` so it can be shared across workers.

### Concrete providers

| Provider | Backing | When to use |
|---|---|---|
| `InMemoryVfsProvider` | In-process virtual filesystem, no real OS access | Development, testing, lightweight agent workloads |
| `AgentOsSidecarProvider` | IPC to external agent-os sidecar process | Production agent workloads needing full OS virtualization |
| `SandboxedOsProvider` | Real OS with chroot/namespace restriction | Trusted environments with real file/process access |

The provider is selected per-tenant or globally via configuration. The WIT
interface contract is identical regardless of which provider is active.

### Linker extension

The wasmtime backend plan's `component::Linker<InvocationHostState>` is extended
with agent interface bindings:

```text
InvocationHostState (extended)
  - bridge: Arc<dyn HostBridge>           (existing — neovex:host)
  - agent_provider: Option<Arc<dyn AgentOsProvider>>  (new — neovex:agent)
  - context: RuntimeInvocationContext
  - cancellation: Option<HostCallCancellation>
  - limiter: StoreLimiter

Linker bindings:
  neovex:host/* → HostBridge::call / call_async     (from wasmtime plan)
  neovex:agent/filesystem/* → agent_provider.fs_*   (new)
  neovex:agent/process/* → agent_provider.process_* (new)
  neovex:agent/http-client/* → agent_provider.http_*(new)
```

If `agent_provider` is `None` (tenant lacks agent capabilities), the linker
still contains the bindings but instantiation of a `neovex-agent` world
component will fail at link time because the imports resolve to an error
provider. This is intentional — capability denial is a link-time error, not a
runtime panic.

### ComponentWorld admission gate

```text
BundleContent::WasmComponent { target_world: ComponentWorld, .. }

ComponentWorld::NeovexFunction
  → always allowed
  → linker binds neovex:host only

ComponentWorld::NeovexAgent
  → requires tenant-level agent capability flag
  → linker binds neovex:host + neovex:agent
  → deploy rejected if tenant lacks the capability
```

The admission gate is in the bundle upload/deploy path, not in the invocation
path. Once deployed, a `NeovexAgent` component runs through the same scheduling
and invocation path as any other component.

### Async agent operations and cooperative scheduling

Agent operations (especially `process_exec`, `http_fetch`, and the `spawn`/
`wait` family) are inherently async and potentially long-running. In the
cooperative fuel model:

1. WASM component calls `neovex:agent/process/exec`
2. Linker binding starts the async operation on the `AgentOsProvider`
3. The fuel slot transitions to `Parked` (same as async host I/O in the
   wasmtime backend plan)
4. The cooperative scheduler picks the next runnable slot
5. When the agent operation completes, the activity signal wakes the scheduler
6. The scheduler resumes the parked slot on the next cycle
7. The linker binding resolves the result back to the WASM component

This is identical to how async `HostBridge::call_async` operations work in the
wasmtime backend plan. Agent operations do not require special scheduling
support.

## Agent-OS Sidecar Integration

### Near-term: AgentOsSidecarProvider

The `AgentOsSidecarProvider` communicates with an external agent-os sidecar
process over a local transport (Unix domain socket or stdio pipes).

```text
Neovex process
  ├── WasmtimeBackend worker 0
  │     └── Store → neovex:agent/filesystem/read("data.json")
  │           → AgentOsSidecarProvider
  │             → IPC to agent-os sidecar
  │               → virtual kernel VFS
  │             ← file contents
  │           ← result to WASM component
  └── WasmtimeBackend worker 1
        └── (same path, same or different sidecar)
```

Sidecar lifecycle options:
- **Per-process:** one shared agent-os sidecar for all tenants (simpler, lower
  memory)
- **Per-tenant:** dedicated sidecar per tenant (stronger isolation, higher
  memory)
- **Per-session:** sidecar lifecycle tied to an agent session (stateful agent
  conversations)

The provider selects the sidecar by `(tenant_id, session_id)`. Sidecar
creation/destruction is managed by the provider, not by the scheduling layer.

### Long-term: In-process virtual kernel

If agent-os abstracts its kernel over a capability-based interface (or if Neovex
builds its own), the kernel could run in-process as native Rust code (not
compiled to WASM). The `AgentOsProvider` trait allows this swap without changing
the WIT interfaces, the linker bindings, or the scheduling model.

This plan does **not** depend on agent-os ever making that abstraction. The
sidecar path is the viable near-term integration. The in-process path is a
future optimization.

### What Neovex replaces in agent-os

| agent-os component | Neovex replacement |
|---|---|
| `crates/execution` (Node.js process spawning) | Neovex V8 or wasmtime execution |
| `crates/sidecar` service loop | Neovex `RuntimeExecutor` scheduling |
| agent-os TypeScript SDK | Neovex WASM component + WIT imports |
| Node.js `--permission` sandboxing | WASI Component Model capability scoping |

What Neovex keeps from agent-os: the virtual kernel patterns (VFS, process
table, PTY, permission model) — either as a sidecar dependency or as extracted
design patterns.

## Required Invariants

- Agent capabilities must be strictly additive. `neovex-function` components
  must never gain access to agent interfaces.
- All agent file paths must be scoped to a per-tenant, per-session virtual root
  with no traversal above it.
- All agent process execution must be governed by an allowlist/denylist
  permission model.
- All outbound HTTP must be subject to URL allowlist/denylist per tenant.
- Agent operation timeouts must derive from the invocation's remaining execution
  budget, not from a separate timeout.
- The `AgentOsProvider` trait must be `Send + Sync` so it can be shared across
  workers.
- Sidecar lifecycle management must not block the scheduling layer.
- A tenant without agent capabilities must fail at deploy time (bundle
  rejection), not at invocation time.

## Promotion Criteria

Promote this plan only if all of the following are true:

1. `docs/plans/wasmtime-backend-plan.md` Phase W3 (run-to-completion wasmtime
   backend) has reached `done` status, including the `neovex:host` WIT linker.
2. The product direction confirms agent workloads as an intended execution
   surface.
3. At least one concrete `AgentOsProvider` implementation is scoped (even if
   it is the `InMemoryVfsProvider` for initial development).

## Phase Status Ledger

| Phase | Status | Summary | Hard Dependencies | Gate Note |
|-------|--------|---------|-------------------|-----------|
| A1 | `todo` | `neovex:agent` WIT interface definitions | Wasmtime plan W3 `done` | define `filesystem`, `process`, `http-client` interfaces and `neovex-agent` world; validate WIT compilation and component targeting |
| A2 | `todo` | `AgentOsProvider` trait and `InMemoryVfsProvider` | A1 | define the provider trait; implement an in-memory virtual filesystem provider for development and testing; wire into wasmtime linker as `neovex:agent/*` bindings |
| A3 | `todo` | `ComponentWorld::NeovexAgent` admission gate | A1, A2 | extend `BundleContent::WasmComponent` with `target_world`; enforce capability check at deploy time; reject `NeovexAgent` bundles for tenants without agent capability |
| A4 | `todo` | Agent-os sidecar provider | A2 | `AgentOsSidecarProvider` implementation; IPC protocol over Unix domain socket or stdio; sidecar lifecycle management; per-tenant or per-session sidecar routing |
| A5 | `todo` | `SandboxedOsProvider` for trusted environments | A2 | real filesystem + process execution with chroot/namespace sandboxing; configurable command and URL allowlists |
| A6 | `todo` | End-to-end agent workflow validation | A2, A3, and at least one of A4 or A5 | full agent scenario: WASM component imports agent capabilities, executes filesystem and process operations, calls LLM via HTTP, returns result through Neovex scheduling and admission |

## Phase Order and Dependencies

```text
Wasmtime plan W3 (done)
  └── A1 neovex:agent WIT definitions
        ├── A2 AgentOsProvider trait + InMemoryVfsProvider
        │     ├── A3 ComponentWorld admission gate
        │     ├── A4 agent-os sidecar provider
        │     └── A5 sandboxed OS provider
        └── A6 end-to-end validation (needs A2 + A3 + one of A4/A5)
```

Recommended delivery order: A1 → A2 → A3 → A4 → A5 → A6

A4 and A5 are independent and can run in parallel after A2.

## Implementation Checkpoints

| Phase | Checkpoint | Next Step |
|-------|------------|-----------|
| A1 | none yet | promote after wasmtime plan W3 completes |
| A2 | none yet | |
| A3 | none yet | |
| A4 | none yet | |
| A5 | none yet | |
| A6 | none yet | |

## Execution Log

| Date | Phase | Outcome | Summary | Verification | Next Step |
|------|-------|---------|---------|--------------|-----------|
| 2026-04-05 | meta | documented | Initial plan authored. Covers `neovex:agent` WIT interfaces (filesystem, process, http-client), `AgentOsProvider` trait with three concrete providers (in-memory, agent-os sidecar, sandboxed OS), `ComponentWorld` admission gate, and integration path with rivet-dev/agent-os. Includes agent-os architecture review finding: agent-os value is the virtual kernel, not the compute layer. | document review against `ARCHITECTURE.md`, `wasmtime-backend-plan.md`, and agent-os source analysis | keep deferred until wasmtime plan W3 reaches `done` |

## Verification Expectations

When promoted, the agent capabilities should not be considered viable without:

- WIT interface compilation and component targeting tests
- `InMemoryVfsProvider` unit tests (read, write, list, path scoping, traversal
  rejection)
- `AgentOsSidecarProvider` integration tests (IPC round-trip, sidecar lifecycle,
  tenant scoping)
- `ComponentWorld::NeovexAgent` admission gate tests (deploy accepted with
  capability, rejected without)
- process execution permission model tests (allowed commands, denied commands)
- HTTP client allowlist/denylist tests
- async agent operation park/resume correctness tests under cooperative
  scheduling
- end-to-end agent scenario test (file I/O + process execution + HTTP call
  through wasmtime cooperative scheduling)
- `neovex-function` world isolation test (verify agent imports are unreachable)
- V8 and standard wasmtime backend regression suites green after every phase

## Relationship To Other Plans

- **`wasmtime-backend-plan.md`**: hard prerequisite. This plan activates after
  W3 (run-to-completion wasmtime backend) completes. The WIT linker and Store
  lifecycle from the wasmtime plan are the substrate this plan builds on.
- **`v8-locker-fork-plan.md`**: no direct dependency, but the V8 backend must
  remain green. Agent capabilities are wasmtime-only; V8 functions do not gain
  agent interfaces.
- **`raw-v8-warm-backend-plan.md`**: no dependency. Agent capabilities are
  delivered through WASI Component Model, not through V8.
- **`ARCHITECTURE.md`**: update when each phase lands, documenting the agent
  capability surface and tenant admission model.
