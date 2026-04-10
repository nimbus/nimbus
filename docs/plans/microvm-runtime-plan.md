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

**Goal:** Boot a microVM from a rootfs directory, managed by neovex.

**Two implementation paths depending on krun-embedded availability:**

#### Path A: With krun-embedded (recommended — single binary)

If `docs/plans/krun-embedded-plan.md` Phase K3 is complete:

- `neovex-vmm` depends on `krun` crate from `agentstation/krun-embedded`
- `neovex-vmm/src/vm.rs`: Uses `MicroVm::builder()` API to configure and
  start VMs on dedicated threads
- No helper binary needed — VMM runs in-process on `std::thread`
- No system dependencies — kernel embedded in binary

#### Path B: Without krun-embedded (fallback — helper binary)

If krun-embedded is not yet available:

- `neovex-vmm-helper/src/main.rs`: Read config from stdin, call libkrun API,
  enter VM (~50 lines)
- `neovex-vmm/src/helper.rs`: Spawn helper via `tokio::process::Command`
- `PR_SET_PDEATHSIG(SIGKILL)` for cleanup
- Requires system-installed libkrun + libkrunfw

**Dependencies:** krun-embedded (Path A) OR libkrun + libkrunfw system
packages (Path B)

**Acceptance criteria (both paths):**
- Can boot Alpine in a microVM from an unpacked OCI rootfs directory
- `echo "hello from VM"` works via the OCI entrypoint
- VM exits with the workload's exit code, neovex reads it
- Parent can monitor VM lifecycle (running, exited, crashed)

### Phase M3: Host ↔ Guest Communication (vsock)

**Goal:** V8 isolates can talk to services running in microVMs.

**Scope:**
- `neovex-vmm/src/vsock.rs`: Connect to guest vsock ports via libkrun's UDS
  proxy (if applicable) or TSI port mapping
- Define a protocol for V8 ↔ VM service calls (JSON-RPC over vsock, or
  HTTP proxy — TBD based on what services expect)
- `neovex-vmm/src/helper.rs`: Spawn helper with vsock port config, monitor
  lifecycle

**Key design decision:** Many Docker services (postgres, redis, HTTP APIs)
speak standard protocols (TCP). With TSI, the guest service binds a port and
the host can connect via `localhost:<mapped_port>`. This might mean vsock is
only needed for lifecycle management, not for service traffic.

**Acceptance criteria:**
- Can boot a postgres:16 image in a microVM
- Can connect to postgres from the host via TSI-mapped port
- Can connect via vsock for lifecycle management (health check, shutdown)

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
| M2: VMM Helper Binary | `todo` | libkrun installed | |
| M3: Host ↔ Guest Comms | `todo` | M2 | |
| M4: Engine Integration | `todo` | M1, M2, M3 | |
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
| **Fly.io init-snapshot** (`superfly/init-snapshot`) | Custom init design | vsock API, snapshot-ready lifecycle (if we add Firecracker later) |
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

1. **TSI port mapping vs vsock for service traffic:** Should V8 connect to
   services via TSI-mapped TCP ports (simpler, standard protocols) or via
   vsock (more control, no port conflicts)? Likely TSI for standard services,
   vsock for neovex-specific lifecycle.

2. **Multi-VM port conflicts:** If two VMs both run postgres on 5432, TSI
   port mapping needs unique host ports. How should this be managed?
   Auto-assign? User-specified?

3. **Image layer caching strategy:** Share layers across images (content-
   addressable by digest) or keep separate rootfs directories per image?
   Shared layers save disk; separate directories are simpler.

4. **VM restart policy:** If a service VM crashes, should neovex auto-restart
   it? With backoff? Configurable per service?

5. **Resource defaults:** What are sensible default vCPU/RAM for service VMs
   on constrained hardware (2 cores, 8GB)? Probably 1 vCPU, 256MB RAM.

6. **libkrun installation story:** Should neovex check for libkrun at startup
   and provide install instructions? Or bundle it?

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
