# Plan: MicroVM Runtime (libkrun Backend)

Canonical design and execution plan for adding a microVM-based runtime to
Neovex that runs OCI/Docker images in hardware-isolated microVMs using libkrun,
enabling V8 isolates to interact with containerized services.

---

## Status

- **Status:** `deferred`
- **Primary owner:** this plan
- **Activation gate:** promote when ready to begin implementation
- **Related plan:** `docs/plans/krun-embedded-plan.md` — optional but
  recommended prerequisite. Creates `agentstation/krun-embedded`, a patched
  libkrun that can be embedded as a Cargo dependency (single-binary
  deployment, no system deps). If krun-embedded is available, Phase M2 uses
  it instead of system-installed libkrun + helper binary.

## How To Use This Plan

- Read this before starting any microVM, libkrun, or container runtime work.
- Treat it as the canonical control plane for the microVM workstream once
  promoted.
- When promoted, implement exactly one phase at a time and record verification
  in the Execution Log before marking a phase `done`.

## Control Plan Rules

This document is the durable control plane for the microVM runtime workstream.
The source of truth is:

1. the current git worktree
2. this plan's `Phase Status Ledger`, `Implementation Checkpoints`, and
   `Execution Log`
3. `ARCHITECTURE.md` for the landed runtime architecture
4. `docs/research/firecracker-container-runtime.md` for the evaluated
   approaches
5. `docs/research/libkrun-evaluation.md` for the libkrun deep evaluation
6. `docs/research/firecracker-implementation-sketches.md` for code sketches

Do not rely on prior chat transcripts as progress state.

### Status model

- `todo`: not started; eligible when hard dependencies and gate notes are
  satisfied
- `in_progress`: actively being implemented
- `blocked`: cannot proceed until the recorded blocker is resolved
- `done`: acceptance criteria are met and verification has been recorded
- `deferred`: intentionally parked behind a product or benchmarking gate

---

## Problem Statement

Neovex embeds V8 for running user-defined JavaScript functions and plans a
Wasmtime backend for WASM execution. A third runtime is needed: one that runs
**arbitrary Docker images as long-running services inside hardware-isolated
microVMs**, allowing V8 isolates to interact with those services.

**User-facing model:**

```
Developer provides:    Neovex does:                    Result:
  Dockerfile       →   builds image (via podman/docker) →  OCI image
  registry ref     →   pulls image (oci-client)         →  OCI image
  local image      →   imports (docker/podman save)     →  OCI image

  OCI image        →   unpacks layers to directory      →  rootfs dir
  rootfs dir       →   runs in microVM (libkrun)        →  running service
  running service  ←→  V8 isolates talk to it (vsock)   →  agent workload
```

**Example:** A developer writes a Dockerfile with PostgreSQL and a JS function
that queries it. Neovex boots the PostgreSQL Dockerfile in a microVM, and V8
isolates call `ctx.services.db.query("SELECT ...")` which routes over vsock to
the PostgreSQL service.

---

## Why libkrun Over Firecracker

See `docs/research/libkrun-evaluation.md` for the full evaluation. Summary:

| Dimension | libkrun | Firecracker |
|-----------|---------|-------------|
| Rootfs from OCI image | virtiofs from directory (no ext4) | Must create ext4 block device |
| Networking | TSI (zero config, transparent) | TAP + iptables (manual) |
| Guest init | Built-in (reads OCI config) | Must write custom init |
| Kernel | Bundled in libkrunfw | Must download + manage vmlinux |
| Helper binary | ~50 lines | ~200+ lines |
| Snapshots | No (not needed for long-running) | Yes (~5ms restore) |

**For long-running service VMs, libkrun eliminates three entire subsystems**
(ext4 pipeline, custom init, network configuration) at the cost of no
snapshot/restore — which doesn't matter when VMs boot once and run for the
session.

**Firecracker remains viable as a future alternative backend** if neovex later
needs ephemeral function-call-style VMs with sub-10ms boot via
snapshot/restore. The helper binary pattern is the same either way.

---

## What crun Does That Neovex Needs

crun is the production OCI runtime for Podman. Its krun handler
(`src/libcrun/handlers/krun.c`) is the canonical libkrun consumer. Analysis of
what crun provides and what neovex should cherry-pick:

### crun functionality neovex DOES need

| crun feature | How neovex should handle it |
|-------------|---------------------------|
| **OCI image config parsing** (entrypoint, cmd, env, workdir, user) | Use `oci-spec` crate to parse image config. Write `.krun_config.json` to rootfs (same as crun). libkrun's init reads it. |
| **VM resource config** (vCPUs, RAM) | `krun_set_vm_config()` — configurable per service definition |
| **GPU passthrough detection** | `krun_set_gpu_options()` — optional, for GPU-enabled agents |
| **Exit code propagation** | Automatic — libkrun's init uses virtiofs ioctl, helper exits with workload code |

### crun functionality neovex does NOT need

| crun feature | Why not needed |
|-------------|---------------|
| Linux namespaces (PID, net, mount, user, UTS, IPC) | VM provides isolation — namespaces are redundant |
| Cgroup resource limits | `krun_set_vm_config()` sets VM-level CPU/RAM directly |
| Seccomp syscall filtering | VM boundary is stronger than seccomp |
| Linux capabilities | Irrelevant inside a VM |
| rootfs pivot_root / bind mounts | virtiofs handles rootfs sharing |
| OCI hooks (prestart/poststart/poststop) | Neovex has its own lifecycle; may add later |
| Device node creation (/dev/kvm etc.) | Helper binary manages this |
| Conmon-style container monitoring | Neovex monitors via `tokio::process::Command::wait()` |

### What neovex needs that crun does NOT provide

| Need | Solution |
|------|----------|
| **V8 ↔ VM communication (vsock)** | `krun_add_vsock_port()` — direct libkrun API |
| **OCI image pull from registry** | `oci-client` crate (crun doesn't pull images) |
| **Dockerfile build** | Shell out to `podman build` / `docker build` |
| **Service discovery / routing** | Neovex engine routes V8 calls to VM vsock ports |
| **Multi-VM orchestration** | VM pool in neovex engine |
| **Image layer caching** | Neovex manages OCI layer cache |

---

## Proposed Architecture

```
neovex process (tokio + V8 + engine)
  │
  ├── neovex-engine (Service)
  │     ├── V8 isolates call ctx.services.<name>.<method>(...)
  │     └── VmServiceManager routes to the right VM
  │
  ├── VmServiceManager
  │     ├── OCI image management (pull, build, unpack, cache)
  │     ├── VM lifecycle (spawn helper, monitor, restart)
  │     └── vsock broker (connect V8 calls to VM ports)
  │
  └── tokio::process::Command("neovex-vmm-helper")  [per VM]
        │
        └── krun_start_enter()
              └── Guest VM (libkrun + libkrunfw kernel)
                    ├── libkrun init (PID 1)
                    │     └── reads .krun_config.json
                    │     └── mounts, networking (TSI), exec entrypoint
                    ├── User service (e.g. postgres, redis, custom API)
                    └── vsock listener (for V8 communication)
```

### Workspace changes

```
crates/
  neovex-vmm/             # NEW: VM management (host side)
    src/
      lib.rs              # VmServiceManager, VmHandle
      oci.rs              # OCI image pull, unpack, layer cache
      helper.rs           # Spawn/monitor neovex-vmm-helper
      vsock.rs            # V8 ↔ VM vsock communication
      config.rs           # VM config types, .krun_config.json generation

  neovex-vmm-helper/      # NEW: Tiny binary, calls libkrun
    src/
      main.rs             # ~50 lines: read config, krun_* calls, start_enter
    Cargo.toml            # depends on libkrun (git dep or krun-sys)
```

### Crate dependency rules (extending existing invariants)

- **`neovex-vmm` depends on `neovex-core` only** — types and config, no
  engine dependency. The server wires it to the engine, same as neovex-runtime.
- **`neovex-vmm-helper` has zero workspace dependencies** — it is a standalone
  binary that talks to libkrun via FFI. It reads config from stdin (JSON).
- **V8 ↔ VM routing goes through the engine** — V8 calls
  `ctx.services.<name>`, the server's bridge impl routes to VmServiceManager,
  which connects to the VM via vsock. Same HostBridge pattern as V8→engine.

---

## Phase Plan

### Phase M1: OCI Image Management

**Goal:** Pull, build, unpack, and cache OCI images.

**Scope:**
- `neovex-vmm/src/oci.rs`: Pull from registry via `oci-client`, flatten
  layers with whiteout handling, unpack to cache directory
- `neovex-vmm/src/config.rs`: Parse OCI image config (entrypoint, cmd, env,
  workdir, exposed ports), generate `.krun_config.json`
- Support three input types:
  - Registry ref: `oci-client` pull
  - Dockerfile: shell out to `podman build` or `docker build`, capture image,
    then pull from local store
  - Local image: `podman save --format oci-archive` → unpack

**Dependencies:** None (standalone)

**Acceptance criteria:**
- Can pull `docker.io/library/alpine:latest` and unpack to a directory
- Can build a Dockerfile and unpack the result
- Layer cache avoids re-downloading unchanged layers
- `.krun_config.json` correctly captures entrypoint/cmd/env/workdir

### Phase M2: VMM Integration

**Goal:** Boot a microVM from a rootfs directory, managed by neovex, with
full lifecycle observability.

**Two implementation paths depending on krun-embedded availability:**

#### Path A: With krun-embedded (recommended — single binary)

If `docs/plans/krun-embedded-plan.md` Phase K3 is complete:

- `neovex-vmm` depends on `krun` crate from `agentstation/krun-embedded`
- `neovex-vmm/src/vm.rs`: Uses `MicroVm::builder()` API to configure and
  start VMs via the re-exec self pattern (child process per VM, zero leaks)
- `neovex` main.rs adds `--internal-vmm` check (~10 lines) for the re-exec
  entry point
- No system dependencies — libkrun + kernel embedded in binary

#### Path B: Without krun-embedded (fallback — separate helper binary)

If krun-embedded is not yet available:

- `neovex-vmm-helper/src/main.rs`: Read config from stdin, call libkrun API,
  enter VM (~50 lines)
- `neovex-vmm/src/helper.rs`: Spawn helper via `tokio::process::Command`
- `PR_SET_PDEATHSIG(SIGKILL)` for cleanup
- Requires system-installed libkrun + libkrunfw

**Both paths use the same process model:** a child process per VM, monitored
by the parent via `waitpid()`. The only difference is whether the child is a
re-exec of neovex itself (Path A) or a separate helper binary (Path B).

**Observability (both paths):**

| Layer | Signal | Implementation |
|-------|--------|----------------|
| Process liveness | Running / exited / crashed | `child.try_wait()` |
| Boot readiness | VMM configured, guest booting | Child writes `READY` to stdout |
| Service readiness | TCP service accepting connections | `TcpStream::connect()` via TSI port map |
| Hang detection | Service stopped responding | Timeout on TCP health check |

See `docs/plans/krun-embedded-plan.md` Observability Model section for details.

**Dependencies:** krun-embedded (Path A) OR libkrun + libkrunfw system
packages (Path B)

**Acceptance criteria (both paths):**
- Can boot Alpine in a microVM from an unpacked OCI rootfs directory
- `echo "hello from VM"` works via the OCI entrypoint
- VM exits with the workload's exit code, neovex reads it
- Parent detects boot readiness (READY signal)
- Parent detects VM exit, crash, and can force-kill

### Phase M2b: Custom Guest Init (neovex-init)

**Goal:** Build a custom guest init binary that supports graceful shutdown
via vsock and tini-style signal forwarding.

**Why:** libkrun's built-in `init.c` does NOT handle signals, does NOT
listen on vsock, and has no grace period logic. Without a custom init, the
only way to stop a VM is SIGKILL (instant death, potential data loss).
See `docs/research/vm-lifecycle-probes.md` for the full analysis.

**Scope:**
- `neovex-init/src/main.rs`: ~200 lines Rust, compiled as
  `x86_64-unknown-linux-musl` static binary
- Mount filesystems (`/proc`, `/sys`, `/dev`, `/dev/pts`, `/dev/shm`, `/tmp`)
- Read OCI config from `.krun_config.json` (same format as crun)
- **vsock shutdown listener** on port 10000: receives SHUTDOWN message with
  grace period, sends SIGTERM to workload process group, waits, SIGKILL
- **Signal forwarding:** tini/dumb-init pattern — register handlers, forward
  to workload process group via `kill(-child_pid, sig)`
- **Zombie reaping:** `waitpid(-1, WNOHANG)` in main loop (PID 1 obligation)
- Exit code propagation via `ioctl(KRUN_EXIT_CODE_IOCTL)`
- Inject into rootfs at `/sbin/neovex-init` during OCI unpack (Phase M1)

**Modeled on:**
- [`krallin/tini`](https://github.com/krallin/tini/blob/master/src/tini.c) —
  signal forwarding, zombie reaping (~300 lines C)
- [`Yelp/dumb-init`](https://github.com/Yelp/dumb-init/blob/master/dumb-init.c) —
  session leader, process group signals (~200 lines C)
- Kata Containers agent — vsock shutdown protocol
  ([`src/agent/src/rpc.rs`](https://github.com/kata-containers/kata-containers/blob/main/src/agent/src/rpc.rs))
- libkrun AWS Nitro signal proxy — signal forwarding over vsock (4-byte int)

**Dependencies:** Phase M1 (OCI unpack provides the rootfs to inject init into)

**Acceptance criteria:**
- Static musl binary, zero runtime deps, ~1-2MB
- Boots in a libkrun VM, mounts filesystems, execs workload
- SHUTDOWN over vsock → SIGTERM to workload → grace period → SIGKILL →
  exit code propagated to host
- Zombie processes reaped (no defunct processes accumulate)
- Host receives correct exit code via `child.wait()`

### Phase M3: Host ↔ Guest Communication

**Goal:** V8 isolates can talk to services running in microVMs, with full
lifecycle management including graceful shutdown.

**Two communication channels, serving different purposes:**

#### TSI for service traffic (primary)

Most Docker services speak standard TCP protocols (postgres on 5432, redis on
6379, HTTP on 80/443). TSI transparently maps guest ports to host ports. V8
connects using standard TCP — no custom protocol needed.

```
V8 isolate → TcpStream::connect("127.0.0.1:15432") → TSI → guest postgres:5432
```

#### vsock for lifecycle management (required)

vsock is **required** for graceful shutdown (not optional). Also used for:
- Shutdown signaling (SHUTDOWN message → neovex-init)
- Guest-level health introspection (future)
- Log streaming from guest (future)

**Scope:**
- `neovex-vmm/src/service.rs`: TSI port mapping, auto-assign unique host
  ports per VM, connection pooling
- `neovex-vmm/src/vsock.rs`: vsock connection for shutdown signaling
- `neovex-vmm/src/lifecycle.rs`: Probe engine — startup, readiness, liveness
  checks with configurable thresholds (see Probe Model below)
- `neovex-vmm/src/shutdown.rs`: Graceful shutdown sequence

**Probe model** (modeled on Kubernetes, see `docs/research/vm-lifecycle-probes.md`):

| Probe | Purpose | Mechanism | On Failure |
|-------|---------|-----------|------------|
| **Startup** | Service finished booting | TCP connect or HTTP GET via TSI | Wait (don't kill during startup_grace) |
| **Readiness** | Service can serve traffic | TCP connect or HTTP GET via TSI | Stop routing V8 calls (don't kill) |
| **Liveness** | Service is not hung | Timeout on readiness check | Restart VM (per restart_policy) |

```rust
struct ProbeConfig {
    check: HealthCheck,           // Tcp { port } or Http { port, path }
    startup_grace: Duration,      // default: 10s — don't probe before this
    interval: Duration,           // default: 10s
    timeout: Duration,            // default: 5s
    failure_threshold: u32,       // default: 3
    success_threshold: u32,       // default: 1
    shutdown_grace: Duration,     // default: 30s
}

enum HealthCheck {
    Tcp { port: u16 },
    Http { port: u16, path: String },
}
```

**Graceful shutdown sequence:**

```
handle.shutdown(grace: 30s)
  1. Connect to guest vsock port 10000
  2. Send SHUTDOWN { grace_period: 30s }
  3. neovex-init: SIGTERM → workload, wait grace, SIGKILL remaining
  4. VM exits, child.wait() returns exit code
  5. [Fallback: if no exit within grace + 5s, SIGKILL helper process]
```

**Restart policy:**

```rust
enum RestartPolicy {
    Never,
    OnFailure { max_restarts: u32, backoff: BackoffConfig },
    Always { max_restarts: u32, backoff: BackoffConfig },
}
struct BackoffConfig {
    initial: Duration,    // 1s
    max: Duration,        // 60s
    multiplier: f64,      // 2.0
    reset_after: Duration, // 300s — reset backoff after stable
}
```

**Acceptance criteria:**
- Can boot `postgres:16` in a microVM, connect via TSI, run queries
- Graceful shutdown: `handle.shutdown()` → postgres gets SIGTERM → clean exit
- Health check detects when postgres is ready to accept connections
- Health check detects when postgres hangs (timeout → liveness failure)
- Restart policy: VM restarts on crash with exponential backoff
- Multiple VMs with same guest port get unique host port mappings

### Phase M4: Engine Integration

**Goal:** Wire VM services into neovex's engine so V8 isolates can use them.

**Scope:**
- `VmServiceManager` struct in `neovex-vmm/src/lib.rs`: manages VM pool,
  routes service calls
- Integration with `neovex-server`: service configuration via API, VM
  lifecycle tied to tenant lifecycle
- V8 `HostBridge` extension: `ctx.services.<name>` surface for calling VM
  services from JavaScript

**Dependencies:** Phases M1-M3

**Acceptance criteria:**
- A V8 function can call `ctx.services.db.query("SELECT 1")` and get a result
  from a postgres VM
- VMs start when the service is first referenced and stop on tenant teardown
- VM crash is detected and reported (not silent)

### Phase M5: Developer Experience

**Goal:** Clean configuration surface for defining VM-backed services.

**Scope:**
- Configuration format for specifying services (Dockerfile, registry ref,
  resource limits, port mappings, env vars)
- CLI commands: `neovex service add`, `neovex service status`, `neovex
  service logs`
- Error messages for common failures (libkrun not installed, KVM not
  available, build failures)

**Dependencies:** Phases M1-M4

**Acceptance criteria:**
- Developer can define a service in neovex config referencing a Dockerfile
- `neovex service status` shows running VMs with resource usage
- Clear error when `/dev/kvm` is missing or libkrun is not installed

---

## Phase Status Ledger

| Phase | Status | Hard deps | Notes |
|-------|--------|-----------|-------|
| M1: OCI Image Management | `todo` | none | |
| M2: VMM Integration | `todo` | krun-embedded (Path A) or libkrun system pkg (Path B) | |
| M2b: Custom Guest Init | `todo` | M1 (injects into rootfs) | ~200 lines Rust, musl static |
| M3: Host ↔ Guest Comms | `todo` | M2, M2b | Probes, graceful shutdown, TSI |
| M4: Engine Integration | `todo` | M1, M2, M2b, M3 | |
| M5: Developer Experience | `todo` | M4 | |

---

## Key Dependencies

### System dependencies

| Dependency | Required | Purpose |
|------------|----------|---------|
| `/dev/kvm` | Yes | Hardware virtualization |
| `libkrun.so` | Yes | VMM library |
| `libkrunfw.so` | Yes | Bundled guest kernel |
| `podman` or `docker` | For Dockerfile builds only | Image building |

### Rust crate dependencies (verified on crates.io 2026-04-09)

| Crate | Version | Purpose |
|-------|---------|---------|
| `oci-client` | 0.16.1 | Pull OCI images from registries |
| `oci-spec` | 0.9.0 | Parse OCI image config |
| `flate2` | 1.x | Decompress gzipped layers |
| `tar` | 0.4.x | Extract layer tarballs |
| `krun-sys` | 1.10.1 | libkrun FFI bindings (for helper binary) |

### Reference implementations

| Project | Relevance | What to study |
|---------|-----------|---------------|
| **crun krun handler** (`containers/crun/src/libcrun/handlers/krun.c`) | Canonical libkrun consumer | `.krun_config.json` format, API call order, exit code propagation |
| **Fly.io init-snapshot** ([`superfly/init-snapshot`](https://github.com/superfly/init-snapshot)) | Custom init design | vsock API, snapshot-ready lifecycle |
| **tini** ([`krallin/tini`](https://github.com/krallin/tini/blob/master/src/tini.c)) | PID 1 signal forwarding | Signal handling, zombie reaping (~300 lines C) |
| **dumb-init** ([`Yelp/dumb-init`](https://github.com/Yelp/dumb-init/blob/master/dumb-init.c)) | PID 1 signal forwarding | Session leader, process group signals (~200 lines C) |
| **Kata agent** ([`kata-containers`](https://github.com/kata-containers/kata-containers/blob/main/src/agent/src/rpc.rs)) | vsock shutdown protocol | ttrpc over vsock, SignalProcess/DestroySandbox RPCs |
| **K8s kubelet prober** ([`pkg/kubelet/prober/`](https://github.com/kubernetes/kubernetes/tree/master/pkg/kubelet/prober)) | Probe state machine | Three-probe model, threshold transitions |
| **Docker health** ([`daemon/health.go`](https://github.com/moby/moby/blob/master/daemon/health.go)) | Health check state machine | starting/healthy/unhealthy states |
| **smolvm** (`smol-machines/smolvm`) | Helper binary pattern | fork/subprocess model for libkrun |
| **muvm** (`AsahiLinux/muvm`) | Singleton VM pattern | RPC to running VM, vsock port management |

---

## TSI Networking Model

TSI (Transparent Socket Impersonation) means services inside the VM
transparently use the host's network stack. A postgres inside the VM binds
`0.0.0.0:5432` and TSI makes it accessible from the host.

**For V8 ↔ VM service communication, TSI may eliminate the need for a custom
vsock protocol entirely.** If the service speaks HTTP/TCP (which most Docker
services do), V8 can connect to `localhost:<port>` via TSI port mapping.

vsock would still be used for:
- VM lifecycle management (health checks, graceful shutdown)
- Services that don't speak standard TCP protocols
- Secure channels that shouldn't traverse the network stack

---

## Open Questions

1. **Multi-VM port conflicts:** If two VMs both run postgres on 5432, TSI
   port mapping needs unique host ports. Auto-assign from a pool? User-
   specified? Likely auto-assign with a configurable range (e.g., 15000-16000).

2. **Image layer caching strategy:** Share layers across images (content-
   addressable by digest) or keep separate rootfs directories per image?
   Shared layers save disk; separate directories are simpler.

3. **VM restart policy:** If a service VM crashes, should neovex auto-restart
   it? With backoff? Configurable per service?

4. **Resource defaults:** What are sensible default vCPU/RAM for service VMs
   on constrained hardware (2 cores, 8GB)? Probably 1 vCPU, 256MB RAM.

### Resolved questions

- **TSI vs vsock for service traffic:** Resolved — TSI for standard TCP
  services (primary), vsock for lifecycle management only (secondary).
- **libkrun installation:** Resolved — krun-embedded bundles everything.
  Fallback to system-installed libkrun if krun-embedded is not used.
- **`_exit()` and resource leaks:** Resolved — re-exec self pattern gives
  process isolation with zero leaks. No `mem::forget()` needed.

---

## Verification Contract

### Per-phase verification

Each phase must demonstrate:
1. The feature works end-to-end (manual test)
2. Error cases are handled (missing deps, build failures, VM crashes)
3. No regressions in existing `make ci`

### End-to-end verification (after M4)

1. Write a Dockerfile with a simple HTTP server
2. Configure it as a neovex service
3. Write a V8 function that calls the HTTP server
4. Verify the function returns the expected response
5. Stop neovex, verify the VM is cleaned up (no orphan processes)

---

## Execution Log

_Empty — no work started._
