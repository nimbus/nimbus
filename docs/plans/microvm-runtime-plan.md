# Plan: MicroVM Runtime — OCI Image Execution in Hardware-Isolated VMs

Canonical design and execution plan for adding a microVM-based runtime to
neovex that runs OCI/Docker images in hardware-isolated microVMs, enabling
V8 isolates to interact with containerized services via TSI networking.

This plan builds on the VMM infrastructure delivered by
`vmm-infrastructure-plan.md`.

---

## Status

- **Status:** `deferred`
- **Primary owner:** this plan
- **Activation gate:** promote when `vmm-infrastructure-plan.md` Phase V4 is
  complete (neovex can boot a VM via `--internal-vmm`)
- **Hard dependency:** `docs/plans/vmm-infrastructure-plan.md` — provides
  libcrun/libkrun static linkage, kernel embedding, and `--internal-vmm`
  entry point

## Control Plan Rules

Source of truth:
1. the current git worktree
2. this plan's `Phase Status Ledger` and `Execution Log`
3. `ARCHITECTURE.md` for the landed runtime architecture
4. `docs/plans/vmm-infrastructure-plan.md` for the VMM foundation
5. `docs/research/vm-lifecycle-probes.md` for probe model design
6. `docs/research/libkrun-evaluation.md` for libkrun API reference

### Status model

- `todo` / `in_progress` / `blocked` / `done` / `deferred`

---

## Problem Statement

Neovex embeds V8 for JavaScript functions and plans Wasmtime for WASM. A third
runtime is needed: run **arbitrary Docker images as long-running services
inside hardware-isolated microVMs**, allowing V8 isolates to interact with
those services.

**User-facing model:**

```
Developer provides:              Neovex does:
  Dockerfile                 →   builds image (podman/docker build)
  registry ref (postgres:16) →   pulls image (oci-client crate)
  local image                →   imports (podman/docker save)
                                      ↓
                                 unpacks OCI layers to rootfs directory
                                 injects neovex-init into rootfs
                                 generates OCI bundle (config.json + rootfs)
                                 spawns --internal-vmm child (re-exec)
                                      ↓
                             libcrun sets up namespaces + cgroups + seccomp
                             libkrun boots VM with virtiofs rootfs
                             TSI maps guest ports to host
                                      ↓
                             V8 isolates talk to services via TCP (TSI)
                             neovex manages lifecycle via vsock
```

**Example:** Developer writes a Dockerfile with PostgreSQL. neovex boots it in
a microVM. V8 isolate calls `ctx.services.db.query("SELECT 1")` which connects
to postgres via TSI-mapped TCP port.

---

## OCI Image Config Compliance

neovex handles all OCI image config fields that affect workload execution.
This is distinct from OCI Runtime Spec compliance (handled by libcrun in
`vmm-infrastructure-plan.md`).

### Fields from OCI Image Config

| Dockerfile | OCI Field | How neovex handles it | Where |
|-----------|-----------|----------------------|-------|
| `CMD` | `Cmd` | Written to `.krun_config.json`, read by guest init | Phase M1 |
| `ENTRYPOINT` | `Entrypoint` | Written to `.krun_config.json` | Phase M1 |
| `ENV` | `Env` | Written to `.krun_config.json` + passed to `krun_set_env()` | Phase M1 |
| `WORKDIR` | `WorkingDir` | Written to `.krun_config.json` + `krun_set_workdir()` | Phase M1 |
| `USER` | `User` | Written to `.krun_config.json`, guest init calls setuid/setgid | Phase M1, M2 |
| `EXPOSE` | `ExposedPorts` | Auto-generates TSI port mappings via OCI annotation | Phase M1 |
| `VOLUME` | `Volumes` | Maps to virtiofs additional mounts (`krun_add_virtiofs()`) | Phase M3 |
| `STOPSIGNAL` | `StopSignal` | Passed to neovex-init, used instead of default SIGTERM | Phase M2 |
| `HEALTHCHECK` | `Healthcheck` | Used as default probe config when no explicit probe set | Phase M3 |
| `LABEL` | `Labels` | Stored as service metadata, queryable | Phase M5 |

**Implementation reference:** crun's OCI config handling in
`containers/crun/src/libcrun/handlers/krun.c` (see `libkrun_configure_container`).
The `.krun_config.json` format is documented by examining
`containers/libkrun/init/init.c:730-850` (the `config_parse_file` function).

### `.krun_config.json` format (read by libkrun's init and neovex-init)

```json
{
  "Entrypoint": ["/docker-entrypoint.sh"],
  "Cmd": ["postgres"],
  "Env": [
    "POSTGRES_PASSWORD=secret",
    "PGDATA=/var/lib/postgresql/data"
  ],
  "WorkingDir": "/",
  "User": "postgres",
  "StopSignal": "SIGTERM",
  "ExposedPorts": {"5432/tcp": {}}
}
```

---

## Architecture

### Workspace changes

```
crates/
  neovex-vmm/              # NEW: VM management (host side)
    src/
      lib.rs               # VmServiceManager, public API
      oci.rs               # OCI image pull, build, unpack, layer cache
      bundle.rs            # OCI bundle generation (config.json + rootfs)
      vm.rs                # VmHandle — spawn, monitor, shutdown
      lifecycle.rs         # Probe engine — startup, readiness, liveness
      shutdown.rs          # Graceful shutdown via vsock
      port_manager.rs      # TSI port auto-assignment
      config.rs            # Service config types
    Cargo.toml

  neovex-init/             # NEW: Guest init (PID 1 inside VM)
    src/
      main.rs              # Mount, configure, exec, signal forward, vsock
    Cargo.toml             # musl static binary target
```

### Crate dependency rules

- **`neovex-vmm` depends on `neovex-core` only** — types and config, no
  engine dependency. The server wires it to the engine via dependency
  inversion (same pattern as neovex-runtime).
- **`neovex-init` has zero workspace dependencies** — standalone static
  binary compiled for `x86_64-unknown-linux-musl`. Injected into guest rootfs.
- **V8 ↔ VM routing goes through the engine** — V8 calls
  `ctx.services.<name>`, the server's bridge impl routes through
  VmServiceManager to the VM via TSI TCP.

---

## Phase Plan

### Phase M1: OCI Image Management

**Goal:** Pull, build, unpack, and cache OCI images. Generate OCI bundles
with correct config.json for libcrun.

**Scope:**

`crates/neovex-vmm/src/oci.rs`:
- Pull from registry via `oci-client` crate (v0.16.1)
- Flatten layers with OCI whiteout handling (`.wh.*` deletions, `.wh..wh..opq`)
- Unpack to content-addressable cache directory
- Layer deduplication by digest (shared across images)

`crates/neovex-vmm/src/bundle.rs`:
- Parse OCI image config via `oci-spec` crate (v0.9.0)
- Extract ALL fields: Entrypoint, Cmd, Env, WorkingDir, User, ExposedPorts,
  Volumes, StopSignal, Healthcheck
- Generate `.krun_config.json` (written into rootfs)
- Generate OCI `config.json` (runtime spec, for libcrun):
  - `process.args` = Entrypoint + Cmd
  - `process.env` = Env
  - `process.cwd` = WorkingDir
  - `process.user` = User (parsed to UID/GID)
  - `linux.resources.memory.limit` = from service config
  - `linux.resources.cpu` = from service config
  - `annotations["krun.neovex.vsock.ports"]` = "10000:stream"
  - `annotations["krun.neovex.tsi.port_map"]` = auto-generated from ExposedPorts
- Inject neovex-init binary into rootfs at `/sbin/neovex-init`
  (embedded via `include_bytes!` from build output)

Support three input types:
- **Registry ref** (`postgres:16`): `oci-client` pull
- **Dockerfile path**: shell out to `podman build` or `docker build`,
  export with `podman save --format oci-archive`, unpack
- **Local image**: `podman save --format oci-archive <image>` → unpack

**Rust crate dependencies (verified on crates.io 2026-04-09):**

| Crate | Version | Downloads | Purpose |
|-------|---------|-----------|---------|
| `oci-client` | 0.16.1 | 2.9M | Pull OCI images from registries |
| `oci-spec` | 0.9.0 | 12.3M | Parse OCI image/runtime configs |
| `flate2` | 1.x | — | Decompress gzipped layers |
| `tar` | 0.4.x | — | Extract layer tarballs |
| `tempfile` | 3.x | — | Temp directories for unpacking |
| `sha2` | 0.10 | — | Content-addressable layer cache |

**Implementation references:**
- OCI image layer application (whiteout handling):
  `docs/research/firecracker-implementation-sketches.md` (oci.rs sketch)
- crun's `.krun_config.json` generation:
  `containers/crun/src/libcrun/handlers/krun.c` (`libkrun_configure_container`)
- OCI runtime config.json spec:
  https://github.com/opencontainers/runtime-spec/blob/main/config.md
- libkrun's init config parser:
  `containers/libkrun/init/init.c:730-850` (`config_parse_file`)

**Acceptance criteria:**
- Can pull `docker.io/library/alpine:latest` and unpack to a directory
- Can pull `docker.io/library/postgres:16` and generate correct
  `.krun_config.json` with User=postgres, ExposedPorts=5432
- Can build a Dockerfile and unpack the result
- Layer cache avoids re-downloading unchanged layers
- OCI `config.json` is valid per the runtime spec
- neovex-init is present at `/sbin/neovex-init` in the rootfs

### Phase M2: Custom Guest Init (neovex-init)

**Goal:** Build a custom guest init binary that supports graceful shutdown
via vsock, tini-style signal forwarding, and correct OCI User handling.

**Why needed:** libkrun's built-in `init.c` does NOT handle vsock shutdown
signals, does NOT forward SIGTERM to the workload, and uses `SIGTERM`
regardless of the image's `STOPSIGNAL`. Without neovex-init, the only way to
stop a VM is SIGKILL (instant death, potential data loss for databases).

**Scope:**

`crates/neovex-init/src/main.rs` (~200 lines Rust):

```rust
// Compiled as x86_64-unknown-linux-musl static binary (~2MB)
fn main() {
    mount_filesystems();
    let config = read_krun_config("/.krun_config.json");

    // Start vsock shutdown listener (background thread)
    let shutdown_rx = start_vsock_listener(SHUTDOWN_PORT);

    // Set user/group from OCI config (before exec)
    if let Some(user) = &config.user {
        set_user_group(user);  // setuid/setgid
    }

    // Fork and exec workload (tini pattern)
    let child_pid = fork_exec(&config);

    // Main loop: reap zombies + wait for shutdown signal
    reap_and_wait_loop(child_pid, shutdown_rx, &config.stop_signal);
}
```

Key responsibilities:
1. **Mount filesystems:** `/proc`, `/sys`, `/dev`, `/dev/pts`, `/dev/shm`,
   `/tmp`, `/run` (same as libkrun's `init.c:mount_filesystems()`)
2. **Read `.krun_config.json`:** Parse Entrypoint, Cmd, Env, WorkingDir,
   User, StopSignal
3. **vsock shutdown listener:** Listen on port 10000 for SHUTDOWN message
   with grace period
4. **Set user/group:** Parse `User` field (format: `name`, `uid`, `uid:gid`,
   `name:group`), resolve via `/etc/passwd` if name, call `setgid`+`setuid`
5. **Fork and exec workload:** `setsid()` → `fork()` → child execs
   workload in its own session/process group
6. **Signal forwarding:** Register handlers for SIGTERM, SIGINT, SIGQUIT,
   SIGHUP, SIGUSR1, SIGUSR2. Forward to workload process group via
   `kill(-child_pid, sig)`. Respect `StopSignal` from OCI config.
7. **Zombie reaping:** `waitpid(-1, WNOHANG)` in main loop
8. **Exit code propagation:** `ioctl(fd, KRUN_EXIT_CODE_IOCTL, code)` on
   virtiofs root, then `exit(0)`

**vsock shutdown protocol:**

```
Host sends to guest vsock port 10000:
  { "action": "shutdown", "signal": "SIGTERM", "grace_period_secs": 30 }

Guest init:
  1. Send configured StopSignal to workload process group
  2. Wait grace_period_secs
  3. If still alive: SIGKILL to process group
  4. Report exit code via KRUN_EXIT_CODE_IOCTL
  5. exit(0)
```

**Build:**
```bash
rustup target add x86_64-unknown-linux-musl
cargo build --release --target x86_64-unknown-linux-musl -p neovex-init
# Result: target/x86_64-unknown-linux-musl/release/neovex-init (~2MB)
```

**Cargo.toml:**
```toml
[package]
name = "neovex-init"
edition = "2021"

[dependencies]
nix = { version = "0.29", features = ["mount", "signal", "process",
        "hostname", "user", "fs"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
libc = "0.2"

[profile.release]
opt-level = "z"
lto = true
strip = true
panic = "abort"
```

**Implementation references:**
- [`krallin/tini/src/tini.c`](https://github.com/krallin/tini/blob/master/src/tini.c) —
  Signal forwarding, zombie reaping (~300 lines C)
- [`Yelp/dumb-init/dumb-init.c`](https://github.com/Yelp/dumb-init/blob/master/dumb-init.c) —
  Session leader, process group signals (~200 lines C)
- [`containers/libkrun/init/init.c`](https://github.com/containers/libkrun/blob/main/init/init.c) —
  Mount setup, .krun_config.json parsing, exit code ioctl
- [`kata-containers/.../rpc.rs`](https://github.com/kata-containers/kata-containers/blob/main/src/agent/src/rpc.rs) —
  vsock shutdown protocol (ttrpc), SignalProcess RPC
- `docs/research/vm-lifecycle-probes.md` — neovex-init design section

**Acceptance criteria:**
- Static musl binary, zero runtime deps, ~2MB
- Boots in a libkrun VM, mounts filesystems, execs workload
- Respects `User` field (runs workload as non-root when specified)
- Respects `StopSignal` (sends SIGQUIT for nginx, SIGTERM default)
- SHUTDOWN over vsock → stop signal to workload → grace period → SIGKILL →
  exit code propagated to host
- Zombie processes reaped (verify: no defunct processes after child exits)
- Host receives correct exit code via `child.wait()`

### Phase M3: Lifecycle Management

**Goal:** Full lifecycle management with health probes, graceful shutdown,
restart policy, and TSI port management.

**Scope:**

`crates/neovex-vmm/src/lifecycle.rs` — Probe engine:

```rust
/// Probe model inspired by Kubernetes (three-probe) with Docker/Fly.io
/// configurability. See docs/research/vm-lifecycle-probes.md for the
/// cross-platform comparison that informed this design.

/// VM lifecycle states
pub enum VmState {
    Spawning,      // --internal-vmm child spawned, waiting for READY
    Starting,      // READY received, waiting for startup probe
    Ready,         // Startup + readiness probes passing
    NotReady,      // Was Ready, readiness probes failing
    ShuttingDown,  // Graceful shutdown in progress
    Exited(i32),   // Process exited with code
    Crashed(i32),  // Process killed by signal
}

/// Per-service probe configuration
pub struct ProbeConfig {
    pub check: HealthCheck,
    pub startup_grace: Duration,       // default: 10s
    pub interval: Duration,            // default: 10s
    pub timeout: Duration,             // default: 5s
    pub failure_threshold: u32,        // default: 3
    pub success_threshold: u32,        // default: 1
    pub shutdown_grace: Duration,      // default: 30s
}

pub enum HealthCheck {
    Tcp { port: u16 },
    Http { port: u16, path: String },
}

pub enum RestartPolicy {
    Never,
    OnFailure { max_restarts: u32, backoff: BackoffConfig },
    Always { max_restarts: u32, backoff: BackoffConfig },
}

pub struct BackoffConfig {
    pub initial: Duration,             // default: 1s
    pub max: Duration,                 // default: 60s
    pub multiplier: f64,               // default: 2.0
    pub reset_after: Duration,         // default: 300s
}
```

State machine:
```
Spawning → [READY on stdout] → Starting → [startup probe passes] → Ready
Ready ↔ NotReady [readiness probe fails/passes]
Ready/NotReady → [liveness fails N times] → restart (per policy)
Any → [shutdown requested] → ShuttingDown → [exited or killed] → Exited/Crashed
```

`crates/neovex-vmm/src/shutdown.rs` — Graceful shutdown:

```rust
/// Graceful shutdown sequence:
/// 1. Connect to guest vsock port 10000
/// 2. Send SHUTDOWN message with grace_period and stop_signal
/// 3. Wait for child.wait() with timeout = grace_period + 5s buffer
/// 4. If timeout: SIGKILL the --internal-vmm child process
pub async fn graceful_shutdown(handle: &VmHandle, grace: Duration) -> ExitStatus
```

`crates/neovex-vmm/src/port_manager.rs` — TSI port auto-assignment:

```rust
/// Manages unique host port assignments for TSI port mapping.
/// Each VM gets unique host ports to avoid conflicts when multiple
/// VMs expose the same guest port (e.g., two postgres instances on 5432).
pub struct PortManager {
    range: RangeInclusive<u16>,  // default: 15000..=16000
    assigned: HashMap<String, Vec<(u16, u16)>>,  // vm_id → [(guest, host)]
}
```

`crates/neovex-vmm/src/vm.rs` — VmHandle (updated):

```rust
pub struct VmHandle {
    child: tokio::process::Child,
    id: String,
    ports: Vec<(u16, u16)>,      // (guest_port, host_port) TSI mappings
    state: Arc<RwLock<VmState>>,
    probe_config: ProbeConfig,
}

impl VmHandle {
    pub async fn start(bundle: &OciBundle, config: &ServiceConfig) -> Result<Self>;
    pub async fn wait(&mut self) -> Result<ExitStatus>;
    pub async fn shutdown(&mut self, grace: Duration) -> Result<ExitStatus>;
    pub async fn kill(&mut self) -> Result<()>;
    pub async fn health(&self) -> VmHealth;
    pub async fn wait_for_service(&self, timeout: Duration) -> Result<()>;
    pub fn state(&self) -> VmState;
    pub fn tsi_port(&self, guest_port: u16) -> Option<u16>;
}
```

**Implementation references:**
- [`kubernetes/kubernetes/pkg/kubelet/prober/worker.go`](https://github.com/kubernetes/kubernetes/tree/master/pkg/kubelet/prober) —
  Probe state machine, threshold transitions
- [`moby/moby/daemon/health.go`](https://github.com/moby/moby/blob/master/daemon/health.go) —
  Docker health check state machine (starting/healthy/unhealthy)
- `docs/research/vm-lifecycle-probes.md` — Full probe model design

**Acceptance criteria:**
- Boot `postgres:16` in a microVM, wait_for_service detects when ready
- `handle.shutdown(30s)` → postgres gets SIGTERM → clean exit → correct code
- Health check detects postgres is responsive (TCP connect to TSI port)
- Health check detects if postgres hangs (timeout → liveness failure)
- Restart policy: VM restarts on crash with exponential backoff
- Multiple VMs with port 5432 get unique host port assignments
- State machine transitions match the documented diagram

### Phase M4: Engine Integration

**Goal:** Wire VM services into neovex's engine so V8 isolates can use them.

**Scope:**
- `VmServiceManager` in `neovex-vmm/src/lib.rs`: manages VM pool, service
  registry, lifecycle
- Integration with `neovex-server`: service configuration via API, VM
  lifecycle tied to tenant lifecycle
- V8 `HostBridge` extension: `ctx.services.<name>` surface for accessing
  VM services from JavaScript
- TCP connection pooling to TSI-mapped service ports

**The key abstraction:**
```javascript
// In a V8 function:
const result = await ctx.services.db.query("SELECT * FROM users");
// This connects to localhost:15432 (TSI-mapped port) under the hood
```

**Implementation reference:**
- neovex's existing `HostBridge` trait and server bridge implementation
  (`crates/neovex-server/src/adapters/convex/host_bridge/`)

**Acceptance criteria:**
- V8 function calls `ctx.services.db.query("SELECT 1")`, gets result from
  postgres VM
- VMs start when the service is first referenced and stop on tenant teardown
- VM crash is detected and reported (not silent)
- Connection pooling to TSI ports for performance

### Phase M5: Developer Experience

**Goal:** Clean configuration surface for defining VM-backed services.

**Scope:**
- Configuration format for specifying services:
  ```toml
  [services.db]
  image = "postgres:16"           # registry ref
  # OR: dockerfile = "./db/Dockerfile"  # build from Dockerfile
  memory = "256m"
  cpus = 1
  env = { POSTGRES_PASSWORD = "secret" }

  [services.db.health]
  check = "tcp"
  port = 5432
  interval = "10s"
  startup_grace = "15s"

  [services.db.restart]
  policy = "on-failure"
  max_restarts = 5
  ```
- CLI commands: `neovex service list`, `neovex service logs <name>`,
  `neovex service restart <name>`
- Error messages for common failures:
  - `/dev/kvm` not available
  - Image pull failures
  - Build failures
  - VM crash with exit code
  - Port conflicts

**Acceptance criteria:**
- Developer defines a service in neovex config referencing a Dockerfile
- `neovex service list` shows running VMs with state and port mappings
- Clear error when `/dev/kvm` is missing
- Documentation for all config fields

---

## Phase Status Ledger

| Phase | Status | Hard deps | Notes |
|-------|--------|-----------|-------|
| M1: OCI Image Management | `todo` | V4 from vmm-infrastructure-plan | |
| M2: Custom Guest Init | `todo` | M1 (injects init into rootfs) | ~200 lines Rust, musl static |
| M3: Lifecycle Management | `todo` | M1, M2 | Probes, shutdown, restart, ports |
| M4: Engine Integration | `todo` | M1, M2, M3 | HostBridge extension |
| M5: Developer Experience | `todo` | M4 | Config format, CLI, errors |

---

## Key Dependencies (verified on crates.io 2026-04-09)

| Crate | Version | Purpose |
|-------|---------|---------|
| `oci-client` | 0.16.1 | Pull OCI images from registries |
| `oci-spec` | 0.9.0 | Parse OCI image/runtime configs |
| `flate2` | 1.x | Decompress gzipped layers |
| `tar` | 0.4.x | Extract layer tarballs |
| `sha2` | 0.10 | Content-addressable layer cache |
| `nix` | 0.29 | (neovex-init) mount, signal, user/group |
| `vsock` | 0.5.4 | (neovex-init) vsock listener for shutdown |

---

## Open Questions

1. **Multi-VM port conflicts:** Auto-assign from pool (15000-16000)?
   User-specified? Both? Currently planned as auto-assign with override.

2. **Image layer caching strategy:** Content-addressable by digest (shared
   across images) is the plan. Need to decide on cache eviction policy.

3. **VM restart policy defaults:** `OnFailure` with max 5 restarts and
   exponential backoff (1s → 60s) is the current plan. Need user feedback.

4. **Volume persistence:** How should `VOLUME` paths be persisted across
   VM restarts? Host directory bind via virtiofs?

---

## Verification Contract

### Per-phase verification

Each phase must demonstrate:
1. Feature works end-to-end (manual test + automated test)
2. Error cases handled (missing deps, build failures, VM crashes)
3. No regressions in existing `make ci`

### End-to-end verification (after M4)

```bash
# 1. Write a Dockerfile
cat > /tmp/test/Dockerfile << 'EOF'
FROM postgres:16
ENV POSTGRES_PASSWORD=testpass
EXPOSE 5432
EOF

# 2. Configure as neovex service
# (in neovex.toml)
[services.db]
dockerfile = "/tmp/test/Dockerfile"
memory = "256m"

# 3. Start neovex
neovex serve

# 4. V8 function connects to postgres
# ctx.services.db.query("SELECT 1")  →  returns [{?column?: 1}]

# 5. Stop neovex
# Verify: no orphan VMM processes, no leaked ports
ps aux | grep internal-vmm  # should be empty
ss -tlnp | grep 15432       # should be empty
```

---

## Research References

| Document | Contents |
|----------|----------|
| `docs/research/libkrun-evaluation.md` | libkrun API, crun integration, consumer patterns |
| `docs/research/firecracker-container-runtime.md` | Approach comparison, Firecracker as alternative |
| `docs/research/firecracker-implementation-sketches.md` | OCI pipeline code sketches, vsock protocol |
| `docs/research/vm-lifecycle-probes.md` | K8s/Docker/Fly.io probe models, graceful shutdown |

---

## Execution Log

_Empty — no work started._
