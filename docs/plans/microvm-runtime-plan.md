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
serde_yaml = "0.9"     # parse compose.yaml (Compose Spec)
```

No `oci-client`. No `oci-spec`. No `flate2`. No `tar`. No `sha2`. buildah
handles image management. neovex-vmm is a thin orchestration layer that:
- Parses `compose.yaml` (serde_yaml)
- Shells out to buildah (image pull/build/mount)
- Spawns conmon → crun (VM lifecycle)
- TCP health checks (tokio::net::TcpStream)

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

**Goal:** Configuration via Docker Compose files, CLI, error messages.

#### Configuration format: Docker Compose (`compose.yaml`)

**Decision:** Use the [Compose Spec](https://compose-spec.io/) as the service
definition format. Do not invent a custom format. Developers already know
Compose, tooling exists (VS Code, `docker compose config` validation), and
the same file works with `docker compose up` for local testing.

neovex-specific extensions use the Compose Spec's official `x-` extension
mechanism.

**Example `compose.yaml` (works with both Docker and neovex):**

```yaml
services:
  db:
    image: postgres:16
    environment:
      POSTGRES_PASSWORD: secret
    ports:
      - "5432:5432"
    volumes:
      - pgdata:/var/lib/postgresql/data
    healthcheck:
      test: ["CMD", "pg_isready", "-U", "postgres"]
      interval: 10s
      timeout: 5s
      retries: 3
      start_period: 30s
    deploy:
      resources:
        limits:
          cpus: "1.0"
          memory: 256M
    restart: on-failure
    stop_grace_period: 30s

  api:
    build:
      context: .
      dockerfile: Dockerfile
    ports:
      - "8080:8080"
    depends_on:
      db:
        condition: service_healthy
    environment:
      DATABASE_URL: postgres://postgres:secret@db:5432/postgres
    healthcheck:
      test: ["CMD", "curl", "-f", "http://localhost:8080/health"]
      interval: 15s
      start_period: 45s
    deploy:
      resources:
        limits:
          cpus: "0.5"
          memory: 128M

volumes:
  pgdata:
```

**Compose fields neovex supports:**

| Compose field | Maps to | Notes |
|---|---|---|
| `image` | `buildah from docker://...` | Registry pull |
| `build.dockerfile` | `buildah bud -f ...` | Dockerfile build |
| `build.context` | buildah build context | Directory path |
| `environment` | OCI config `process.env` | Map or list form |
| `env_file` | Loaded and merged into env | File path(s) |
| `ports` | TSI port mapping annotation | `"HOST:CONTAINER"` syntax |
| `volumes` | virtiofs additional mounts | Named volumes + bind mounts |
| `healthcheck.test` | `HealthCheck::Exec` or `Http` or `Tcp` | CMD, CMD-SHELL |
| `healthcheck.interval` | `ProbeConfig.interval` | Duration string |
| `healthcheck.timeout` | `ProbeConfig.timeout` | Duration string |
| `healthcheck.retries` | `ProbeConfig.failure_threshold` | Integer |
| `healthcheck.start_period` | `ProbeConfig.startup_grace` | Duration string |
| `deploy.resources.limits.cpus` | `krun_set_vm_config()` vCPUs | String, fractional |
| `deploy.resources.limits.memory` | `krun_set_vm_config()` RAM | `256M`, `1G`, etc. |
| `restart` | `RestartPolicy` | `no`, `always`, `on-failure`, `unless-stopped` |
| `depends_on` | Service startup ordering | `service_healthy` waits for health |
| `stop_grace_period` | conmon signal timeout | Duration string, default 30s |
| `command` | OCI config `process.args` | Override CMD |
| `entrypoint` | OCI config entrypoint | Override ENTRYPOINT |
| `user` | OCI config `process.user` | UID:GID |
| `working_dir` | OCI config `process.cwd` | Directory path |
| `labels` | Service metadata | Key-value map |

**Compose fields neovex ignores (with warnings):**

| Compose field | Why ignored |
|---|---|
| `networks` | TSI handles networking transparently |
| `configs` / `secrets` | Not applicable to VM model (yet) |
| `cap_add` / `cap_drop` | VM provides isolation |
| `privileged` | VM provides isolation |
| `logging.driver` | conmon handles logging |
| `deploy.replicas` | neovex handles scaling separately |
| `deploy.placement` | Single-node for now |

**neovex extensions (`x-neovex`):**

```yaml
services:
  db:
    image: postgres:16
    x-neovex:
      idle_timeout: 5m          # stop VM after idle (future)
      snapshot: true             # enable snapshot/restore (future)
```

**Parsing:** Use `serde_yaml` to parse `compose.yaml`. The Compose Spec is
well-defined YAML. Unknown fields are ignored (forward compatibility).
`x-neovex` fields are parsed into a neovex-specific struct.

```toml
# crates/neovex-vmm/Cargo.toml — add:
[dependencies]
serde_yaml = "0.9"
```

**Reference implementations:**
- [Compose Spec](https://github.com/compose-spec/compose-spec/blob/main/spec.md)
  — canonical specification
- [Compose Go library](https://github.com/compose-spec/compose-go) — Go
  reference parser (for field names and validation rules)
- [Docker Compose validation](https://docs.docker.com/reference/compose-file/)
  — official documentation for all fields

#### CLI commands

```bash
# Service management
neovex service up                    # start all services from compose.yaml
neovex service up db                 # start specific service
neovex service down                  # stop all services
neovex service down db               # stop specific service
neovex service list                  # show running VMs with state and ports
neovex service logs db               # tail conmon log files
neovex service logs db --follow      # stream logs
neovex service restart db            # stop + start
neovex service ps                    # show VM process tree

# Compose file management
neovex service config                # validate and print resolved config
neovex service config --services     # list service names

# Diagnostics
neovex service inspect db            # show VM details (ports, state, resources)
neovex service health db             # show health check status
```

**CLI naming convention:** `neovex service <verb>` mirrors `docker compose <verb>`.
Developers can use muscle memory.

| neovex command | Docker equivalent |
|---|---|
| `neovex service up` | `docker compose up` |
| `neovex service down` | `docker compose down` |
| `neovex service logs` | `docker compose logs` |
| `neovex service ps` | `docker compose ps` |
| `neovex service config` | `docker compose config` |

#### Error messages

| Error | Message |
|---|---|
| `/dev/kvm` missing | `Error: /dev/kvm not found. Enable VT-x in BIOS (bare metal) or nested virtualization (cloud VM). See https://neovex.dev/docs/kvm` |
| crun not installed | `Error: crun not found. Install with: apt install agentstation-crun (Debian) or dnf install agentstation-crun (Fedora). See https://neovex.dev/install` |
| conmon not installed | `Error: conmon not found. Install with: apt install conmon` |
| buildah not installed | `Error: buildah not found. Install with: apt install buildah` |
| libkrun not installed | `Error: libkrun.so not found. See https://neovex.dev/install` |
| Image pull failed | `Error: failed to pull postgres:16 — auth required. Run: buildah login docker.io` |
| Dockerfile build failed | `Error: buildah build failed (exit code 1). See build output above.` |
| VM crashed | `Error: service 'db' crashed (exit code 137, signal SIGKILL). Check logs: neovex service logs db` |
| Health check failed | `Warning: service 'db' health check failing (3/3 retries). State: NotReady` |
| Port conflict | `Error: host port 15432 already in use. Choose a different port mapping.` |
| compose.yaml invalid | `Error: compose.yaml: services.db.deploy.resources.limits.memory: invalid value "abc". Expected format: 256M, 1G, etc.` |
| Unsupported Compose field | `Warning: compose.yaml: services.db.networks: ignored (neovex uses TSI networking)` |

**Acceptance criteria:**
- `compose.yaml` with postgres + custom API service works end-to-end
- Same `compose.yaml` also works with `docker compose up` for local testing
- `neovex service up/down/logs/ps/config` commands work
- Clear errors for every failure mode
- Unknown Compose fields produce warnings, not errors
- `x-neovex` extensions are parsed and applied

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
   If not, neovex must remount on startup.
2. **Volume persistence:** Compose `volumes:` maps to virtiofs additional
   mounts. Named volumes need host-side storage managed by neovex.
3. **conmon log rotation:** Does conmon rotate logs, or does neovex need to
   manage log file size?
4. **TSI port auto-assignment:** When ports are not explicitly mapped (only
   `EXPOSE` in Dockerfile), should neovex auto-assign host ports from a pool?
5. **`depends_on: condition: service_healthy`:** neovex must start services
   in dependency order and wait for health checks. How to handle circular deps?
6. **Inter-service networking:** In Compose, `db` resolves to the db
   service's IP. With TSI, services connect via `localhost:port`. How do we
   handle service names in connection strings (e.g., `DATABASE_URL=postgres://db:5432`)?
   Options: rewrite env vars, inject /etc/hosts, or require explicit ports.

---

## Verification Contract

### End-to-end (after M4)

```yaml
# compose.yaml
services:
  db:
    image: postgres:16
    environment:
      POSTGRES_PASSWORD: secret
    ports:
      - "5432:5432"
    healthcheck:
      test: ["CMD", "pg_isready", "-U", "postgres"]
      interval: 10s
      start_period: 30s
    deploy:
      resources:
        limits:
          memory: 256M
    restart: on-failure
```

```bash
# Start neovex with the compose file
neovex service up

# Verify service is running
neovex service list
# NAME  IMAGE         STATE  PORTS              HEALTH
# db    postgres:16   Ready  5432→15432/tcp     healthy

# V8 function queries postgres:
# const client = new Client({ host: "127.0.0.1", port: ctx.services.db.port })
# await client.query("SELECT 1") → [{?column?: 1}]

# Same compose file works with Docker for local testing:
docker compose up -d
docker compose exec db psql -U postgres -c "SELECT 1"
docker compose down

# Stop neovex services
neovex service down

# Verify cleanup: no orphan processes, no leaked ports
ps aux | grep -E "conmon|crun|krun" | grep -v grep  # should be empty
ss -tlnp | grep 15432                                 # should be empty
```

---

## Execution Log

_Empty — no work started._
