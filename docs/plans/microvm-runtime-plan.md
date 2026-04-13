# Plan: MicroVM Runtime — OCI Image Execution in Hardware-Isolated VMs

Canonical plan for adding a microVM-based runtime to neovex that runs
OCI/Docker images in hardware-isolated microVMs, enabling V8 isolates to
interact with containerized services via TSI networking.

Builds on `vmm-infrastructure-plan.md` (patched crun, conmon, system deps).

**Platform scope: Linux.** On macOS, the neovex server runs inside a Linux
machine VM (see `distribution-plan.md` Channel 4). Services run as standard
containers (crun, no krun handler) — the same way Podman runs containers on
macOS. The API surface is identical. MicroVM isolation is a Linux production
feature. macOS should mirror Podman's one-machine-VM architecture, not add a
second host-side orchestration path or nested per-service microVMs for v1.
`containers/podman-machine-os` currently builds that guest with standard
container tooling (`crun`, `podman`, `netavark`, `aardvark-dns`), which
supports the same conclusion from source.

---

## Status

- **Status:** `in_progress`
- **Primary owner:** this plan
- **Activation gate:** met on 2026-04-12 after
  `vmm-infrastructure-plan.md` reached V3 closeout on a real Linux host and
  `docs/plans/archive/runtime-sandbox-architecture-plan.md` had already landed
  the canonical `neovex-sandbox` seam
- **Related plans:**
  - `docs/plans/archive/runtime-sandbox-architecture-plan.md` — completed
    baseline that owns the canonical sandbox crate naming and the server-facing
    seam this plan must consume
  - `vmm-infrastructure-plan.md` — completed VMM foundation (crun fork,
    conmon, deps, Linux validation evidence)
  - `distribution-plan.md` — packaging for all channels

## Current Assessed State

- `vmm-infrastructure-plan.md` V1 through V3 are complete, including real
  Debian 13 validation for patched `crun`, `conmon`, libkrun/libkrunfw,
  host-to-guest TSI connectivity, manifest-backed restart recovery, and log
  persistence.
- `crates/neovex-sandbox/src/backends/krun/` already owns the first concrete
  backend skeleton: `bundle.rs`, `command.rs`, `conmon.rs`, `buildah.rs`, and
  `vm.rs`.
- `buildah.rs` now owns the first typed `BuildahCli` wrapper for pull/build/
  mount/inspect/cleanup execution plus image-config translation into a backend-
  local `OciImageConfig`. It also resolves image defaults into
  `SandboxProcessSpec`, typed exposed-port records, and a combined
  `OciImageLaunchDefaults` handoff object, and it can now materialize that
  launch-default object directly from real buildah pull/build + mount +
  inspect command sequences.
- `vm.rs` now owns a backend-local launch-resolution seam that can merge sparse
  generic `SandboxSpec` inputs with `OciImageLaunchDefaults`, persist image
  metadata in the manifest, and write an OCI bundle from the resolved launch
  spec. It now also exposes backend-local `start_from_image()` /
  `start_from_build()` helpers that connect prepared buildah launches to real
  krun start/stop lifecycle paths.
- `bundle.rs` always sets OCI `process.user` to root (0:0) for krun bundles
  because the crun VMM process needs `/dev/kvm` access. Image `USER` values
  are resolved (including named-user lookup in the mounted rootfs `/etc/passwd`)
  and stored in the sandbox manifest's `image_metadata.user` field for future
  guest-side application. Linux verification proved that `krun_setuid()`/
  `krun_setgid()` from libkrun do not work in rootless mode (the host-side
  user namespace cannot switch to arbitrary UIDs), so guest-side user switching
  via the guest init is the correct architecture.
- `vm.rs` stop now honors image-configured `StopSignal` values instead of
  hard-coding `TERM`. Linux evidence: a `SIGQUIT`-configured image sandbox
  took ~5.4s to stop (SIGQUIT → 5s timeout → SIGKILL → exit code 137),
  proving the configured signal is sent first.
- `vm.rs` currently reports OCI runtime state `"running"` as
  `SandboxStatus::Ready`. Linux smoke evidence proved that is too early for
  service readiness: one initial TCP connection was refused before the guest
  service began answering through TSI.
- The sandbox seam is now generic and stable enough to continue iterating here:
  `SandboxSpec` carries filesystem, process, and port bindings without leaking
  krun nouns into the public API.

## Current Review Findings

- Image `USER` and `STOPSIGNAL` handling is now verified on Linux. The key
  architectural finding: krun containers cannot apply the image USER via OCI
  `process.user` because the VMM needs `/dev/kvm` access (root). And
  `krun_setuid()`/`krun_setgid()` don't work in rootless mode because the
  host user namespace can't switch to arbitrary UIDs. The correct path is
  guest-side user switching via the guest init process (deferred to M3).
  The image USER is resolved, stored in manifest metadata, and available for
  guest-side application.
- The `STOPSIGNAL` path is fully verified: the backend sends the
  image-configured signal first, waits the stop timeout, then falls back to
  SIGKILL. This was proven with a custom BusyBox image configured with
  `STOPSIGNAL SIGQUIT`.
- The remaining M2 work is resource limits and port-manager auto-assignment.
- Readiness, startup, and liveness probing remain required follow-on work for
  M3. The current V3 backend state mapping is acceptable as a bootstrap seam,
  but it is not a trustworthy published-endpoint readiness signal.
- macOS remains a packaging and development surface only: the active runtime
  plan should continue targeting Linux microVMs while keeping the API shape
  portable to the machine-VM delivery path described in `distribution-plan.md`.

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
| `EXPOSE` | `ExposedPorts` | Annotation `krun.port_map` → TSI |
| `VOLUME` | `Volumes` | virtiofs additional mounts (Phase M3) |
| `STOPSIGNAL` | `StopSignal` | conmon sends this signal (not always SIGTERM) |
| `HEALTHCHECK` | `Healthcheck` | Default probe config if no explicit probe set |
| `LABEL` | `Labels` | Service metadata |

**StopSignal handling:** the sandbox backend must preserve the image-configured
stop signal and use it for graceful shutdown instead of hard-coding SIGTERM. If
the image specifies `STOPSIGNAL SIGQUIT` (nginx), the backend should honor
SIGQUIT during shutdown.

---

## Architecture

### New crate

```text
crates/
  neovex-sandbox/             # NEW: isolation/orchestration crate
    src/
      lib.rs                  # SandboxManager, public API
      spec.rs                 # SandboxSpec / service-level launch config
      instance.rs             # SandboxHandle / published endpoints
      backends/
        mod.rs
        krun/
          mod.rs
          buildah.rs          # Shell out to buildah (pull, build, mount, inspect)
          bundle.rs           # OCI bundle generation (config.json + annotations)
          conmon.rs           # Spawn/monitor conmon, read logs/exit/attach
          vm.rs               # Backend-local VM lifecycle wrapper
          lifecycle.rs        # Probe engine (startup, readiness, liveness)
          port_manager.rs     # TSI port auto-assignment
          compose.rs          # Phase M5 translation layer, if landed
    Cargo.toml
```

### Crate dependency rules

- **`neovex-sandbox` depends on `neovex-core` only** — types and config
- **No OCI image crates needed** — buildah handles everything
- **No C dependencies** — crun/conmon/buildah are system binaries
- Server wires `neovex-sandbox` to the engine via dependency inversion

### What `neovex-sandbox`'s first `krun` backend does NOT implement

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
handles image management. `neovex-sandbox`'s first `krun` backend is a thin
orchestration layer that:
- Parses `compose.yaml` (serde_yaml)
- Shells out to buildah (image pull/build/mount)
- Spawns conmon → crun (VM lifecycle)
- TCP health checks (tokio::net::TcpStream)

---

## Phase Plan

### Phase M1: buildah Integration

**Goal:** neovex can pull, build, mount, inspect, and clean up OCI images via
buildah through a typed Rust wrapper instead of ad hoc command strings.

**Scope:**

`crates/neovex-sandbox/src/backends/krun/buildah.rs`:
```rust
pub struct BuildahCli { /* binary path + rootless wrapping policy */ }

pub struct OciImageConfig {
    pub entrypoint: Vec<String>,
    pub cmd: Vec<String>,
    pub env: Vec<String>,
    pub working_dir: Option<String>,
    pub user: Option<String>,
    pub exposed_ports: Vec<String>,
    pub volumes: Vec<String>,
    pub stop_signal: Option<String>,
    pub healthcheck: Option<ImageHealthcheck>,
    pub labels: BTreeMap<String, String>,
}

impl BuildahCli {
    pub fn pull(&self, container_name: &str, image_reference: &str)
        -> Result<BuildahContainer>;
    pub fn build(
        &self,
        image_name: &str,
        container_name: &str,
        dockerfile: &Path,
        context: &Path,
    ) -> Result<BuildahContainer>;
    pub fn mount_container(&self, container_name: &str) -> Result<PathBuf>;
    pub fn inspect_container(&self, container_name: &str) -> Result<OciImageConfig>;
    pub fn cleanup_container(&self, container_name: &str) -> Result<()>;
}
```

**Acceptance criteria:**
- Can pull `postgres:16` via buildah, mount it, read its config
- Can build a Dockerfile via buildah
- buildah inspect returns all 10 OCI image config fields correctly
- Cleanup removes containers and unmounts
- Unit tests cover command lowering, inspect JSON translation, and cleanup
  ordering without requiring a live buildah installation

### Phase M2: OCI Bundle Generation

**Goal:** Generate valid OCI runtime bundles for crun with krun handler.

**Scope:**

`crates/neovex-sandbox/src/backends/krun/bundle.rs`:
- Generate OCI `config.json` per the runtime spec
- Set `process.args` from image Entrypoint + Cmd
- Set `process.env` from image Env + service-level overrides
- Set `process.user` from image User
- Set `linux.resources` from service config (memory, CPU)
- Add annotation `run.oci.handler = "krun"` (selects krun handler in crun)
- Add annotation `krun.port_map` from ExposedPorts
  (auto-assigned host ports via PortManager)
- Set `root.path` to the buildah-mounted rootfs path
- Preserve the configured `StopSignal` in backend-owned launch metadata so
  graceful shutdown uses the image-configured signal

`crates/neovex-sandbox/src/backends/krun/port_manager.rs`:
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

`crates/neovex-sandbox/src/backends/krun/lifecycle.rs`:

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
    Exec { command: Vec<String> },  // Run guest-defined health command
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
- `SandboxManager` in `neovex-sandbox/src/lib.rs`: sandbox lifecycle and
  published-endpoint access
- `neovex-server` owns the service registry and `ctx.services.<name>` projection
  so the sandbox crate does not become a second server layer
- V8 adapter wiring exposes `ctx.services.<name>.port` from the server-managed
  service registry
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

This phase is a follow-on translation and UX layer. Do not start it until M1
through M4 are complete and the recovery drills in the verification contract
are green.

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
# crates/neovex-sandbox/Cargo.toml — add:
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
| M1: buildah integration | `done` | V3 from vmm-infrastructure-plan | `BuildahCli` with typed pull/build/mount/inspect/cleanup, image-backed `start_from_image()`/`start_from_build()` helpers, and Linux-host image-backed smoke test all passing on Debian 13. Three issues fixed during Linux verification: (1) `OciImageConfig` null-field deserialization (many OCI fields are `null` not absent), (2) empty `process.cwd` in bundle config when image has no `WorkingDir`, (3) buildah overlay mount not persisting across `buildah unshare` sessions (fixed by chaining mount inside the conmon create/state/start sessions) |
| M2: OCI bundle generation | `in_progress` | M1 | Linux-verified: image USER resolved to numeric uid:gid and stored in manifest (bundle always uses root for VMM /dev/kvm access), image STOPSIGNAL honored during shutdown (SIGQUIT→timeout→SIGKILL proved with custom image). Key finding: krun_setuid/krun_setgid don't work in rootless mode, guest-side user switching deferred to M3. Remaining M2 work: resource limits and port-manager auto-assignment |
| M3: Lifecycle management | `todo` | M2 | Probes, shutdown, restart |
| M4: Engine integration | `todo` | M3 | server-owned service registry + V8 access |
| M5: Developer experience | `todo` | M4 | follow-on translation/CLI layer after core runtime verification |

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

Before M5, keep verification split across four lanes:

- unit tests for bundle translation, image-config parsing, port allocation, and
  lifecycle state transitions
- integration tests for `buildah`, `conmon`, patched `crun`, and libkrun on a
  KVM-capable host
- recovery drills for neovex restart, orphan detection, log persistence, and
  port release after crash or forced stop
- distribution probes on supported packaging targets before calling the stack
  production-ready

### Linux Agent Handoff

- **M1 Linux verification is complete** (2026-04-12). Both the rootfs-only and
  image-backed smoke tests pass on Debian 13. The image-backed test
  complements the rootfs-only test (two lanes, not a replacement).
- For future Linux reruns, the ignored smoke suite is safe to run without
  `--test-threads=1` because the two tests use different default ports
  (`18080` rootfs-only, `18081` image-backed).
- Required environment for the ignored suite:
  - `NEOVEX_KRUN_SMOKE_ROOTFS` — extracted BusyBox rootfs directory
  - `NEOVEX_KRUN_SMOKE_WORKDIR` — scratch directory for bundle/state
  - `NEOVEX_KRUN_SMOKE_RUNTIME=/usr/libexec/neovex/crun`
  - `NEOVEX_KRUN_SMOKE_CONMON=$(command -v conmon)`
  - `NEOVEX_KRUN_SMOKE_BUILDAH=$(command -v buildah)`
- The next Linux promotion should cover a non-root image `USER` and a
  non-default image `STOPSIGNAL` on a real host so the new M2 lowering path is
  proven beyond unit tests.
- The next meaningful phase is M2/M3. The readiness gap (OCI `"running"` !=
  service-ready) is the highest-priority follow-on.

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

- 2026-04-12: Promoted this plan from `deferred` to `in_progress` after
  `vmm-infrastructure-plan.md` reached V3 closeout with real Linux evidence.
  Recorded the first M3 readiness finding from Linux smoke: OCI runtime state
  `"running"` is not yet sufficient to publish `Ready`, because one initial
  TSI TCP connection was refused before the guest service answered. Started M1
  implementation by replacing the low-level buildah command stubs with a typed
  CLI wrapper, inspect translation, and unit-level verification.
- 2026-04-12: Landed `BuildahCli` in
  `crates/neovex-sandbox/src/backends/krun/buildah.rs`, corrected the buildah
  inspect command shape to use default JSON output with `--type container`
  instead of template mode, and added script-backed unit tests for pull/build/
  mount/inspect/cleanup lowering. Verification evidence:
  `cargo fmt --all --check`,
  `cargo check -p neovex-sandbox`,
  and `cargo test -p neovex-sandbox` all passed on the current host.
- 2026-04-12: Extended `OciImageConfig` with backend-local launch lowering:
  `resolve_process_spec()` now merges image defaults plus overrides into a
  generic `SandboxProcessSpec`, and `exposed_port_bindings()` parses typed
  image port records for later port-manager wiring. Added unit coverage for
  default-command lowering, override precedence, invalid empty commands,
  exposed-port parsing, and invalid port-shape rejection. Verification
  evidence: `cargo fmt --all --check`,
  `cargo check -p neovex-sandbox`,
  and `cargo test -p neovex-sandbox` all passed on the current host.
- 2026-04-12: Added `OciImageLaunchDefaults` so inspected image metadata,
  mounted rootfs, parsed ports, stop signal, healthcheck, labels, and lowered
  process defaults travel together as one backend-local handoff object.
  Added unit coverage for that combined launch-default resolution. Verification
  evidence: `cargo fmt --all --check`,
  `cargo check -p neovex-sandbox`,
  and `cargo test -p neovex-sandbox` all passed on the current host.
- 2026-04-12: Taught `vm.rs` to consume backend-local launch defaults during
  planning via a new resolved-launch seam. `plan_start_with_launch_defaults()`
  now materializes sparse generic specs from image defaults, preserves explicit
  operator overrides, stores image metadata in the manifest, and writes bundle
  config from the resolved launch spec. Added unit coverage for sparse-spec
  materialization and explicit-override preservation. Verification evidence:
  `cargo fmt --all --check`,
  `cargo check -p neovex-sandbox`,
  and `cargo test -p neovex-sandbox` all passed on the current host.
- 2026-04-12: Extended `BuildahCli` with `prepare_image_launch()` and
  `prepare_built_image_launch()` so the buildah wrapper can now produce a
  fully prepared `PreparedImageLaunch` from real pull/build + mount + inspect
  command sequences. Added script-backed unit coverage for both registry-image
  and Dockerfile-build materialization paths. Verification evidence:
  `cargo fmt --all --check`,
  `cargo check -p neovex-sandbox`,
  and `cargo test -p neovex-sandbox` all passed on the current host.
- 2026-04-12: Wired the prepared launch seam into `vm.rs` through backend-local
  `start_from_image()` / `start_from_build()` helpers. The krun backend now
  tracks buildah container metadata in the manifest, uses the resolved launch
  spec for image-backed start planning, and cleans up buildah mounts/containers
  on stop. Added a plan-only integration test that proves image-backed
  start-then-stop persists buildah metadata while running and clears it after
  cleanup. Verification evidence: `cargo fmt --all --check`,
  `cargo check -p neovex-sandbox`,
  and `cargo test -p neovex-sandbox` all passed on the current host.
- 2026-04-12: Ran both the rootfs-only and image-backed Linux smoke tests on
  Debian 13 x86_64 and fixed four issues. (1) `OciImageConfig` field
  deserialization: OCI images frequently have `"Entrypoint": null` rather than
  omitting the field; added `null_as_default` serde deserializer for `Vec` and
  `BTreeMap` fields. (2) Empty `process.cwd` in bundle config: BusyBox image
  has no `WorkingDir`, and the sparse spec path left cwd as `""` instead of
  `/`; added `process_cwd()` fallback in `bundle.rs`. (3) Buildah overlay mount
  lifetime: `buildah mount` inside one `buildah unshare` session creates a
  mount that disappears when the session exits; the conmon create/state/start
  commands ran in separate sessions, so the rootfs was gone before crun could
  access it. Fixed by adding `wrap_unshare_with_mount()` to `BuildahCli` and
  `maybe_wrap_with_mount()` to the conmon launch plan builder, so the mount
  command chains inside the same user-namespace session as the wrapped command.
  (4) Added the new `krun_backend_image_backed_smoke_pulls_and_boots_busybox`
  ignored test alongside the existing rootfs-only test; both pass sequentially
  on this host in ~13s total.
  Decision: the image-backed smoke test complements (does not replace) the
  rootfs-only smoke. The rootfs-only lane tests core krun/TSI/conmon
  lifecycle without buildah image management; the image-backed lane tests the
  full M1 integration path.
  Verification evidence:
  `cargo fmt --all --check`,
  `cargo check -p neovex-sandbox -p neovex`,
  `cargo test -p neovex-sandbox` (25 pass),
  `cargo test -p neovex-sandbox --test krun_linux_smoke -- --ignored --test-threads=1` (2 pass).
  Env:
  `NEOVEX_KRUN_SMOKE_ROOTFS=/tmp/neovex-sandbox-smoke-rootfs`,
  `NEOVEX_KRUN_SMOKE_WORKDIR=/tmp/neovex-sandbox-smoke`,
  `NEOVEX_KRUN_SMOKE_RUNTIME=/usr/libexec/neovex/crun`,
  `NEOVEX_KRUN_SMOKE_CONMON=/usr/bin/conmon`,
  `NEOVEX_KRUN_SMOKE_BUILDAH=/usr/bin/buildah`,
  `NEOVEX_KRUN_SMOKE_HOST_PORT=18080`,
  `NEOVEX_KRUN_SMOKE_GUEST_PORT=8080`.
  Remaining readiness gap: OCI runtime state `"running"` still maps to
  `SandboxStatus::Ready` before the guest service binds its TSI port (one
  initial TCP connection refused observed in both smoke tests). This must be
  addressed in M3 with a proper startup probe.
- 2026-04-12: Started M2 bundle-generation follow-on work. `bundle.rs` now
  resolves image `USER` into OCI `process.user` values during config
  generation, supporting numeric `uid[:gid]`, named-user lookup through the
  mounted rootfs `/etc/passwd`, and the numeric-only fallback to gid `0` when
  `/etc/passwd` is absent. `vm.rs` stop now uses the image-configured
  `StopSignal` when present instead of always sending `TERM`. Added unit
  coverage for numeric and named user lowering, missing-user rejection,
  numeric-user fallback without `/etc/passwd`, bundle rendering of lowered
  uid/gid, and stop-signal selection. Verification evidence:
  `cargo fmt --all --check`,
  `cargo check -p neovex-sandbox`,
  `cargo test -p neovex-sandbox` (30 pass).
- 2026-04-13: Ran M2 Linux-host verification for image USER and STOPSIGNAL on
  Debian 13 x86_64. Created a custom BusyBox image with `USER www-data` and
  `STOPSIGNAL SIGQUIT` via buildah config/commit. Key findings:
  (1) krun bundles must always use root for `process.user` because the crun VMM
  needs `/dev/kvm` access. Setting uid:33 in the OCI config crashes with
  `Error creating the Kvm object: Error(13)` — confirmed this also affects
  Podman (`podman --runtime /usr/libexec/neovex/crun run --rm --annotation
  run.oci.handler=krun localhost/user-test:latest /bin/busybox id` → same crash).
  (2) Attempted a crun patch using `krun_setuid()`/`krun_setgid()` libkrun APIs
  to defer user switching to after VMM init, but these fail in rootless mode with
  `Failed to set gid 33` because the host user namespace cannot switch to
  arbitrary UIDs. The correct architecture is guest-side user switching via the
  guest init process.
  (3) Updated `bundle.rs` to always emit `ProcessUser::ROOT` for krun bundles
  and removed the dead `resolve_process_user` host-side code. The image USER is
  still resolved by `buildah.rs` and stored in `image_metadata.user` in the
  manifest.
  (4) The STOPSIGNAL path works correctly: a sandbox with `SIGQUIT` configured
  took ~5.4s to stop (SIGQUIT sent first → 5s stop_timeout → SIGKILL fallback),
  exit code 137. Manifest records `stop_signal: SIGQUIT` and
  `shutdown_requested: true`.
  (5) Added `krun_backend_m2_user_and_stop_signal_lowering` ignored smoke test
  and updated `build-neovex-crun.sh` to apply all patches from `patches/crun/`
  in sorted order.
  Verification: `cargo fmt --all --check`, `cargo check -p neovex-sandbox -p neovex`,
  `cargo test -p neovex-sandbox` (29 pass),
  `cargo test -p neovex-sandbox --test krun_linux_smoke -- --ignored --test-threads=1`
  (3 pass, ~21s total).
