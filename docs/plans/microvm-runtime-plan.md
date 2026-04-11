# Plan: MicroVM Runtime — OCI Image Execution in Hardware-Isolated VMs

Canonical plan for adding a microVM-based runtime to neovex that runs
OCI/Docker images in hardware-isolated microVMs, enabling V8 isolates to
interact with containerized services via TSI networking.

Builds on `vmm-infrastructure-plan.md` (crun fork, conmon, system deps).

---

## Status

- **Status:** `deferred`
- **Primary owner:** this plan
- **Activation gate:** promote when `vmm-infrastructure-plan.md` Phase V3 is
  complete (neovex can programmatically boot and manage VMs)
- **Related plans:**
  - `vmm-infrastructure-plan.md` — VMM foundation (crun fork, conmon, deps)
  - `distribution-plan.md` — packaging for all channels

## Control Plan Rules

Source of truth:
1. the current git worktree
2. this plan's `Phase Status Ledger` and `Execution Log`
3. `ARCHITECTURE.md`
4. `docs/research/vm-lifecycle-probes.md`
5. `docs/research/libkrun-evaluation.md`

---

## Problem Statement

Developers provide Dockerfiles, registry refs, or local images. neovex runs
them as long-running services in hardware-isolated microVMs. V8 isolates
interact with those services via TCP (through TSI).

```
Developer provides:              neovex does:
  Dockerfile                 →   buildah bud (build)
  registry ref (postgres:16) →   buildah from (pull)
  local image                →   buildah from (import)
                                      ↓
                                 buildah mount → merged rootfs
                                 generate OCI bundle (config.json)
                                 conmon → crun → krun → VM
                                      ↓
                             V8 isolates connect via TSI (TCP)
                             conmon manages lifecycle (logs, signals)
```

---

## OCI Image Config Compliance

All Dockerfile instructions that affect runtime behavior are handled.

| Dockerfile | OCI Field | Handled by |
|-----------|-----------|-----------|
| `CMD` | `Cmd` | OCI config.json `process.args` |
| `ENTRYPOINT` | `Entrypoint` | OCI config.json `process.args` |
| `ENV` | `Env` | OCI config.json `process.env` + `krun_set_env()` |
| `WORKDIR` | `WorkingDir` | OCI config.json `process.cwd` |
| `USER` | `User` | OCI config.json `process.user` (crun handles setuid) |
| `EXPOSE` | `ExposedPorts` | Annotation `krun.neovex.tsi.port_map` → TSI |
| `VOLUME` | `Volumes` | virtiofs additional mounts (Phase M3) |
| `STOPSIGNAL` | `StopSignal` | conmon sends this signal (not always SIGTERM) |
| `HEALTHCHECK` | `Healthcheck` | Default probe config if no explicit probe set |
| `LABEL` | `Labels` | Service metadata |

**StopSignal handling:** conmon reads the configured stop signal from the
OCI config and sends it instead of SIGTERM. If the image specifies
`STOPSIGNAL SIGQUIT` (nginx), conmon sends SIGQUIT. This works without any
custom init because conmon handles it on the host side.

---

## Architecture

### New crate

```
crates/
  neovex-vmm/                 # NEW: VM management
    src/
      lib.rs                  # VmServiceManager, public API
      buildah.rs              # Shell out to buildah (pull, build, mount, inspect)
      bundle.rs               # OCI bundle generation (config.json + annotations)
      conmon.rs               # Spawn/monitor conmon, read logs/exit/attach
      vm.rs                   # VmHandle — lifecycle wrapper
      lifecycle.rs            # Probe engine (startup, readiness, liveness)
      port_manager.rs         # TSI port auto-assignment
      config.rs               # Service config types (TOML)
    Cargo.toml
```

### Crate dependency rules

- **`neovex-vmm` depends on `neovex-core` only** — types and config
- **No OCI image crates needed** — buildah handles everything
- **No C dependencies** — crun/conmon/buildah are system binaries
- Server wires neovex-vmm to the engine via dependency inversion

### What neovex-vmm does NOT implement (handled by system deps)

| Capability | Handled by |
|-----------|-----------|
| OCI image pull, auth, mirrors | buildah + containers-common |
| Image layer storage, dedup, overlay | containers-storage (via buildah) |
| Container process monitoring, logs | conmon |
| Namespace/cgroup/seccomp isolation | crun (libcrun) |
| VMM (KVM, virtio devices, TSI) | libkrun |
| Guest kernel | libkrunfw |
| PID 1 init (signals, zombies) | catatonit/tini |
| Rootless networking | passt |
| Rootless overlay | fuse-overlayfs |

### Cargo.toml (minimal — no OCI crates!)

```toml
[dependencies]
# Existing workspace deps
tokio = { workspace = true }
serde = { workspace = true }
serde_json = { workspace = true }
tracing = { workspace = true }

# New deps
anyhow = "1"
tempfile = "3"
```

No `oci-client`. No `oci-spec`. No `flate2`. No `tar`. No `sha2`. buildah
handles all of that. neovex-vmm is a thin orchestration layer that shells
out to system tools.

---

## Phase Plan

### Phase M1: buildah Integration

**Goal:** neovex can pull, build, and mount OCI images via buildah.

**Scope:**

`crates/neovex-vmm/src/buildah.rs`:
```rust
/// Pull an image from a registry
pub async fn pull(image_ref: &str) -> Result<String> {
    // buildah from --name neovex-{ulid} docker://{image_ref}
    // Returns: container name
}

/// Build from a Dockerfile
pub async fn build(dockerfile: &Path, context: &Path) -> Result<String> {
    // buildah bud -t neovex-{ulid} -f {dockerfile} {context}
    // buildah from --name neovex-{ulid} localhost/neovex-{ulid}
    // Returns: container name
}

/// Mount a container's rootfs
pub async fn mount(container: &str) -> Result<PathBuf> {
    // buildah mount {container}
    // Returns: /var/lib/containers/storage/overlay/.../merged
}

/// Inspect image config (Entrypoint, Cmd, Env, User, ExposedPorts, etc.)
pub async fn inspect(container: &str) -> Result<OciImageConfig> {
    // buildah inspect --format json {container}
    // Parse: Entrypoint, Cmd, Env, WorkingDir, User, ExposedPorts,
    //        Volumes, StopSignal, Healthcheck, Labels
}

/// Clean up
pub async fn cleanup(container: &str) -> Result<()> {
    // buildah umount {container}
    // buildah rm {container}
}
```

**Acceptance criteria:**
- Can pull `postgres:16` via buildah, mount it, read its config
- Can build a Dockerfile via buildah
- buildah inspect returns all 10 OCI image config fields correctly
- Cleanup removes containers and unmounts

### Phase M2: OCI Bundle Generation

**Goal:** Generate valid OCI runtime bundles for crun with krun handler.

**Scope:**

`crates/neovex-vmm/src/bundle.rs`:
- Generate OCI `config.json` per the runtime spec
- Set `process.args` from image Entrypoint + Cmd
- Set `process.env` from image Env + service-level overrides
- Set `process.user` from image User
- Set `linux.resources` from service config (memory, CPU)
- Add annotation `run.oci.handler = "krun"` (selects krun handler in crun)
- Add annotation `krun.neovex.tsi.port_map` from ExposedPorts
  (auto-assigned host ports via PortManager)
- Set `root.path` to the buildah-mounted rootfs path
- Handle `StopSignal` → OCI process.signal field (conmon reads this)

`crates/neovex-vmm/src/port_manager.rs`:
```rust
pub struct PortManager {
    range: RangeInclusive<u16>,   // default: 15000..=16000
    assigned: HashMap<String, Vec<(u16, u16)>>,
}
```
Auto-assigns unique host ports per VM. Two postgres VMs on guest port 5432
get different host ports (e.g., 15000 and 15001).

**Acceptance criteria:**
- Generated config.json passes `crun spec --validate`
- `run.oci.handler` annotation selects krun handler
- TSI port mapping annotation is correctly formatted
- Multiple VMs get unique host port assignments

### Phase M3: Lifecycle Management

**Goal:** Health probes, shutdown, restart policy.

**Scope:**

`crates/neovex-vmm/src/lifecycle.rs`:

```rust
pub enum VmState {
    Starting,       // conmon spawned, VM booting
    Ready,          // health probe passing
    NotReady,       // health probe failing
    ShuttingDown,   // stop signal sent, waiting
    Exited(i32),    // clean exit
    Crashed(i32),   // killed by signal
}

pub struct ProbeConfig {
    pub check: HealthCheck,
    pub startup_grace: Duration,    // default: 10s
    pub interval: Duration,         // default: 10s
    pub timeout: Duration,          // default: 5s
    pub failure_threshold: u32,     // default: 3
    pub success_threshold: u32,     // default: 1
}

pub enum HealthCheck {
    Tcp { port: u16 },              // TCP connect to TSI-mapped port
    Http { port: u16, path: String }, // HTTP GET, expect 2xx
}

pub enum RestartPolicy {
    Never,
    OnFailure { max_restarts: u32, backoff: BackoffConfig },
    Always { max_restarts: u32, backoff: BackoffConfig },
}

pub struct BackoffConfig {
    pub initial: Duration,          // 1s
    pub max: Duration,              // 60s
    pub multiplier: f64,            // 2.0
    pub reset_after: Duration,      // 300s
}
```

State machine:
```
Starting → [health probe passes] → Ready
Ready ↔ NotReady [probe fails/passes, threshold-based]
Ready/NotReady → [probe fails N times] → restart (per policy)
Any → [shutdown] → ShuttingDown → Exited/Crashed
```

Shutdown (same as Podman):
```
neovex requests stop
  → conmon sends StopSignal to crun/VMM process
  → wait shutdown_grace (default 30s)
  → conmon sends SIGKILL
  → VM dies, conmon writes exit file
  → neovex reads exit status
```

**Implementation references:**
- K8s prober: [`pkg/kubelet/prober/worker.go`](https://github.com/kubernetes/kubernetes/tree/master/pkg/kubelet/prober)
- Docker health: [`daemon/health.go`](https://github.com/moby/moby/blob/master/daemon/health.go)
- `docs/research/vm-lifecycle-probes.md`

**Acceptance criteria:**
- Health probe detects when postgres:16 is ready (TCP connect to TSI port)
- Health probe detects hang (timeout → liveness failure)
- Shutdown: conmon sends StopSignal, waits, SIGKILL
- Restart: VM restarts on crash with exponential backoff
- State transitions match the documented state machine

### Phase M4: Engine Integration

**Goal:** V8 isolates can access VM services.

**Scope:**
- `VmServiceManager` in `neovex-vmm/src/lib.rs`: service registry, VM pool
- V8 `HostBridge` extension: `ctx.services.<name>.port` returns TSI port
- V8 connects to services via standard TCP (through TSI)
- neovex does NOT implement protocol-specific clients — V8 uses JS driver
  libraries (pg, ioredis, fetch)

```javascript
// V8 function:
import { Client } from "pg";
const client = new Client({
  host: "127.0.0.1",
  port: ctx.services.db.port,  // TSI-mapped host port
});
await client.connect();
const result = await client.query("SELECT 1");
```

**Future (v2):** Add a TCP proxy in neovex for tenant isolation, audit
logging, rate limiting. Same TSI transport, neovex in the data path.

**Acceptance criteria:**
- V8 function connects to postgres VM via TSI port, runs query
- `ctx.services.db.port` returns the correct TSI-mapped host port
- VMs start on first reference, stop on tenant teardown
- VM crash is reported (not silent)

### Phase M5: Developer Experience

**Goal:** Configuration, CLI, error messages.

**Scope:**
```toml
# neovex.toml
[services.db]
image = "postgres:16"
# OR: dockerfile = "./db/Dockerfile"
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

CLI commands:
- `neovex service list` — show running VMs with state and ports
- `neovex service logs <name>` — tail conmon log files
- `neovex service restart <name>` — stop + start

Error messages for:
- `/dev/kvm` not available
- buildah/crun/conmon not installed
- Image pull failures (auth, network)
- VM crash with exit code

**Acceptance criteria:**
- Service config in neovex.toml works
- CLI commands produce useful output
- Clear error when deps are missing

---

## Phase Status Ledger

| Phase | Status | Hard deps | Notes |
|-------|--------|-----------|-------|
| M1: buildah integration | `todo` | V3 from vmm-infrastructure-plan | |
| M2: OCI bundle generation | `todo` | M1 | |
| M3: Lifecycle management | `todo` | M2 | Probes, shutdown, restart |
| M4: Engine integration | `todo` | M3 | HostBridge, V8 access |
| M5: Developer experience | `todo` | M4 | Config, CLI, errors |

---

## Open Questions

1. **buildah mount persistence:** Does `buildah mount` survive neovex restart?
   Need to verify. If not, neovex must remount on startup.
2. **Volume persistence:** How to persist `VOLUME` paths across VM restarts.
   virtiofs additional mounts to host directories?
3. **conmon log rotation:** Does conmon rotate logs, or does neovex need to
   manage log file size?
4. **TSI port range:** Default 15000-16000 (1000 ports). Sufficient?

---

## Verification Contract

### End-to-end (after M4)

```bash
# neovex.toml
[services.db]
image = "postgres:16"
env = { POSTGRES_PASSWORD = "secret" }

# Start neovex
neovex serve

# V8 function queries postgres:
# const client = new Client({ port: ctx.services.db.port })
# await client.query("SELECT 1") → [{?column?: 1}]

# Stop neovex
# Verify: conmon + VM processes cleaned up
# Verify: no orphan processes, no leaked ports
```

---

## Execution Log

_Empty — no work started._
