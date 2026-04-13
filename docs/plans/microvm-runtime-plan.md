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

- **Status:** `done`
- **Primary owner:** this plan
- **Activation gate:** met on 2026-04-12 after
  `vmm-infrastructure-plan.md` reached V3 closeout on a real Linux host and
  `docs/plans/archive/runtime-sandbox-architecture-plan.md` had already landed
  the canonical `neovex-sandbox` seam
- **Related plans:**
  - `docs/plans/archive/runtime-sandbox-architecture-plan.md` — completed
    baseline that owns the canonical sandbox crate naming and the server-facing
    seam this plan must consume
  - `docs/plans/service-control-plane-plan.md` — completed companion plan for
    the Compose-backed service control plane: project identity, control-root
    layout, backend-owned lifecycle state, and `neovex service ...` command
    semantics
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
- `port_manager.rs` now owns the first backend-local host-port auto-assignment
  seam. When an image exposes TCP ports and the generic `SandboxSpec` does not
  bind them explicitly, the krun backend now allocates host ports from a
  backend-owned range, materializes generic `SandboxPortBinding`s, rewrites the
  bundle `krun.port_map`, and publishes those endpoints through the generic
  sandbox handle. Linux-host proof now covers distinct-port allocation,
  end-to-end TSI reachability, port release on stop, and released-port reuse.
- `SandboxSpec` now also carries generic `SandboxResourceLimits`
  (`cpu_count`, `memory_limit_bytes`). The krun backend lowers
  `memory_limit_bytes` into OCI `linux.resources.memory.limit`, and when an
  explicit whole-vCPU count is requested it materializes backend-owned
  `/.krun_vm.json` data (`cpus`, `ram_mib`) so crun's krun handler can call
  `krun_set_vm_config()`. Both direct-rootfs and image-backed resource-limit
  paths are now Linux-verified on Debian 13: `/.krun_vm.json` contains
  `{"cpus":2,"ram_mib":256}` and `linux.resources.memory.limit = 268435456`,
  with TSI HTTP connectivity confirmed under the resource-limited VM.
- `vm.rs` no longer treats OCI runtime state `"running"` as automatically
  `Ready` in execute mode. It now derives a backend-local startup probe from
  published endpoints, prefers HTTP endpoints over raw TCP when both exist,
  keeps the sandbox in `Starting` until the probe succeeds, and hides
  execute-mode published endpoints until readiness passes. Local unit coverage
  proves HTTP probe success, probe-target selection, failure-to-starting
  fallback, and endpoint gating. Linux-host verification (2026-04-13) confirmed
  a delayed-start BusyBox httpd sandbox correctly reports `Starting` with empty
  `published_endpoints` during boot, transitions to `Ready` with endpoints
  published only after the guest answers on TSI port 18085.
- The M3 liveness slice is now Linux-verified: `SandboxStatus` includes a
  generic `NotReady` state, execute-mode krun sandboxes regress from `Ready` to
  `NotReady` when their readiness/liveness probe starts failing after startup,
  and recover back to `Ready` once the probe succeeds again. Execute-mode
  `published_endpoints` stay withdrawn for both `Starting` and `NotReady`.
  Linux proof: BusyBox httpd killed by PID → sandbox degrades to `NotReady`
  with empty endpoints and unreachable port → httpd restarts → sandbox
  recovers to `Ready` with endpoints and HTTP connectivity restored.
- The companion `service-control-plane-plan.md` is now complete. Project-
  scoped control roots, backend-owned persisted-state discovery, explicit
  `neovex service ...` commands, and the compose-backed main serve path are
  all implemented and Linux-verified, so M5 is closed rather than awaiting
  further lifecycle wiring.
- The restart-policy slice is now Linux-verified. `SandboxSpec` carries a
  generic `SandboxLifecycleSpec` with `SandboxRestartPolicy`, krun manifests
  persist `restart_count`, and execute-mode `inspect()` performs inspect-driven
  restart for crashed sandboxes when policy allows it. Linux proof (2026-04-13):
  a BusyBox sandbox with `OnFailure { max_restarts: 1 }` exits with code 42
  on first boot, is automatically restarted by the backend, reaches `Ready`
  on port 18087 on second boot, with `restart_count: 1` and
  `last_exit_code: 42` recorded in the manifest. `Never`, `OnFailure`, and
  `Always` are supported with bounded restart counts; exponential backoff and
  background workers are deferred.
- Restart backoff is now Linux-verified: repeated restarts no longer relaunch
  immediately. krun persists `next_restart_at_millis` in the manifest, computes
  a capped exponential delay (1s, 2s, 4s, ... up to 60s), and keeps the sandbox
  in `Starting` until the backoff deadline passes. Linux proof (2026-04-13): a
  sandbox with `OnFailure { max_restarts: 2 }` that exits 42 twice before
  starting httpd takes ~10s total (visible backoff), reaches `Ready` on port
  18088, with `restart_count: 2` and 3 boots confirmed in rootfs marker.
- Guest-side user switching is now Linux-verified. The krun backend rewrites
  execute args to launch a statically-linked `neovex-guest-user-switch` helper
  (built via musl) only when image metadata carries a resolved numeric `USER`.
  It injects `NEOVEX_GUEST_UID` / `NEOVEX_GUEST_GID` into the guest env, and
  bind-mounts a backend-owned helper root into `/.neovex` so the host-side VMM
  stays root while the guest workload drops to the image user. Linux proof
  (2026-04-13): a BusyBox image with `USER www-data` → guest `id -u` reports
  `33`, `id -g` reports `33` (via ctr.log), HTTP on port 18089 confirmed.
  Key finding: virtiofs in rootless krun VMs maps guest uid through the host
  user namespace, so non-root guests cannot write to the rootfs overlay;
  proof was captured via stderr/ctr.log instead of rootfs files.
- `neovex-server` now owns the M4 service-registry seam. `SandboxCatalog`
  lists tenant sandboxes, `service_registry.rs` projects ready sandboxes'
  published endpoints into serializable `InvocationServices`, and runtime
  invocations carry that snapshot into V8 as `ctx.services.*`. The same server
  seam now also handles lazy lookup for missing service names through an
  internal `RuntimeServiceRegistry` plus a sync host op, caching successful
  resolutions for the life of the invocation while keeping nested runtime calls
  and runtime-subscription re-evaluation on the same composition root.
- The M4 registry seam now has an explicit activation-capable blocking hook:
  `RuntimeServiceRegistry::ensure_service_binding(...)`. `ctx.services`
  property access still uses a sync host op, but that op now routes through the
  blocking `ensure` boundary rather than the immediate `resolve` path. The
  default sandbox-catalog implementation still returns only already-ready
  bindings, while tests now prove a registry can block once for readiness and
  still benefit from per-invocation `ctx.services` caching.
- The sandbox seam now exposes generic source-specific launch nouns alongside
  `SandboxSpec`: `SandboxImageLaunchSpec`, `SandboxBuildLaunchSpec`, and
  `SandboxImageProcessOverrides` are public `neovex-sandbox` types, and
  `SandboxBackend` now has matching `start_from_image(...)` /
  `start_from_build(...)` entrypoints. `SandboxSpec` remains the resolved
  filesystem/process/resources/port intent instead of being overloaded with
  image/build-source concerns, so krun-specific image launch is no longer
  trapped behind backend-local public types.
- M4 now has a real server-owned manager implementation, not just a registry
  seam. `neovex-server` exports `SandboxServiceManager` plus
  `SandboxServiceCatalog` / `SandboxServiceLaunch` nouns, and the manager owns
  declared sandbox-backed services, starts them through the generic
  `SandboxBackend` entrypoints on first `ctx.services.<name>` access, polls for
  readiness behind `ensure_service_binding(...)`, and then reuses the active
  handle for later snapshots/lookups. Local router proof (2026-04-13): a
  declared `db` service starts exactly once through a fake backend, waits for
  readiness, returns port `15432`, remains visible in later snapshots, and is
  stopped when the tenant is deleted through the HTTP API.
- M5 is now in progress under the active service-control-plane plan. In
  `neovex-bin`, the `service/compose.rs` adapter parses `compose.yaml`,
  resolves image/build source, env + env_file, ports, restart policy, and
  CPU/memory limits into a typed service plan, validates lowerable
  `command`/`entrypoint`/`working_dir`/`user` process overrides, validates
  lowerable lifecycle config (`restart` plus `stop_grace_period`) against the
  generic `SandboxLifecycleSpec` seam, validates the declared-service catalog
  handoff into the server-owned manager seam, and now has an explicit
  `neovex --compose-file ...` serve-path hook that builds a
  `SandboxServiceManager` with the default krun backend config. The service
  CLI now exposes `config`, `up`, `down`, `list`, `inspect`, `logs`, and `ps`
  through the same project/control-root derivation plus the same
  `SandboxServiceCatalog` lowering bridge used by `--compose-file`.
  Local proof (2026-04-13): the bin crate tests validate image- and
  build-backed services, env-file merging, ignored-field warnings,
  invalid-memory errors, lowerable process override translation,
  lifecycle-duration validation, catalog lowering, CLI parsing, helper
  lifecycle behavior, and manifest-history dedupe for `service down`.

## Current Review Findings

- Image `USER` and `STOPSIGNAL` handling is now verified on Linux. The key
  architectural finding: krun containers cannot apply the image USER via OCI
  `process.user` because the VMM needs `/dev/kvm` access (root). And
  `krun_setuid()`/`krun_setgid()` don't work in rootless mode because the
  host user namespace can't switch to arbitrary UIDs. The correct path is
  guest-side user switching via an explicit guest helper/wrapper seam. The
  image USER is resolved, stored in manifest metadata, and lowered into
  guest helper env (`NEOVEX_GUEST_UID` / `NEOVEX_GUEST_GID`) plus a mounted
  static helper binary. Linux proof complete: www-data (33:33) confirmed via
  ctr.log capture. M3 is complete.
- The `STOPSIGNAL` path is fully verified: the backend sends the
  image-configured signal first, waits the stop timeout, then falls back to
  SIGKILL. This was proven with a custom BusyBox image configured with
  `STOPSIGNAL SIGQUIT`.
- Auto-port assignment from image `EXPOSE` is now Linux-verified: the
  `PortManager` allocates distinct host ports from the backend-owned range,
  stopped sandboxes release their ports for reuse, and the auto-assigned ports
  are reachable via TSI.
- Resource limits are now Linux-verified: both direct-rootfs and image-backed
  sandboxes confirmed `/.krun_vm.json` materialization (`cpus:2, ram_mib:256`),
  OCI `linux.resources.memory.limit = 268435456`, and TSI HTTP connectivity
  under the resource-limited VM on Debian 13. M2 is complete.
- The first M3 startup-readiness gate is now Linux-verified: execute-mode
  sandboxes remain `Starting` with hidden `published_endpoints` until a real
  endpoint probe succeeds, then transition to `Ready` with endpoints published.
  Proven on Debian 13 with a delayed-start BusyBox httpd (2s sleep before
  service bind).
- The M3 liveness state slice is now Linux-verified: the generic sandbox API
  has a `NotReady` state, and krun uses it when a previously-ready sandbox
  keeps running but stops answering its published probe target. Linux proof
  (2026-04-13): a BusyBox httpd sandbox that kills httpd by PID, then restarts
  it, correctly transitions `Ready → NotReady → Ready` without a VM restart.
  During `NotReady`, published endpoints are withdrawn and the host port becomes
  unreachable. After recovery, endpoints reappear and HTTP connectivity returns.
  Key fix: BusyBox `killall httpd` does not work inside krun VMs because the
  process name is `busybox` not `httpd`; the test now uses PID-based killing
  (`httpd -f & HTTPD_PID=$!; ... kill $HTTPD_PID`). The port allocator treats
  `NotReady` sandboxes as active so degraded-but-running VMs do not leak their
  host-port reservations.
- The restart-policy slice is now Linux-verified: inspect-driven restart
  for crashed sandboxes works end-to-end on Debian 13. A sandbox with
  `OnFailure { max_restarts: 1 }` that exits 42 on first boot is automatically
  restarted and reaches `Ready` on port 18087. Manifest records
  `restart_count: 1` and `last_exit_code: 42`. M3 is complete.
- The exponential-backoff refinement is now implemented locally: the backend no
  longer retries repeated crash loops immediately. Pending restarts remain in
  `Starting` until the manifest-backed backoff deadline expires, then relaunch
  through the existing inspect-driven restart path. Linux-verified (2026-04-13):
  a sandbox with `OnFailure { max_restarts: 2 }` that exits 42 twice takes
  ~10s total (visible backoff), reaches `Ready` on port 18088 on third boot.
- The first two M4 slices are now implemented and locally verified: V8 gets a
  `ctx.services` view sourced from the server-owned runtime service registry,
  nested runtime calls preserve the same invocation snapshot, and missing
  service names now resolve lazily on first property access through a sync host
  op backed by `RuntimeServiceRegistry`. Successful lookups are cached for the
  remainder of the invocation. The next local M4 slice now proves that the
  same sync host op can wait on a registry-provided `ensure_service_binding`
  implementation, so the blocking boundary for future service activation is
  explicit. The follow-on M4 slice now lands a real server-owned
  `SandboxServiceManager`, and that path is now Linux-verified end to end with
  the real krun-backed manager smoke.
- M4 now has a concrete sync/async design constraint: `ctx.services.<name>` is
  synchronous property access inside V8, while sandbox launch is currently
  asynchronous (`SandboxBackend::start(...) -> Future`). That means the new
  lazy lookup seam is suitable for resolution, but true "start on first
  reference" cannot be added casually without choosing where blocking is
  allowed or introducing a higher-level activation boundary. The current local
  design chooses the runtime service registry as that blocking boundary via
  `ensure_service_binding(...)`. `SandboxServiceManager` now implements that
  boundary, and the real Linux-host smoke plus tenant-teardown proof have both
  landed.
- Podman source review now resolves the main M5 control-plane ownership
  question. `cmd/podman/compose.go` is only a thin wrapper around an external
  Compose provider, while Podman's native lifecycle commands resolve durable
  objects through libpod runtime state (`libpod/runtime.go`,
  `libpod/sqlite_state.go`, `pkg/domain/infra/abi/containers.go`) and read
  inspect/log data from runtime-owned container records plus persistent
  `StaticDir` content (`container_config.go`, `runtime_ctr.go`,
  `container_inspect.go`, `container_log.go`). Neovex should mirror that split:
  Compose stays an input and translation layer, while `neovex service
  up/down/list/logs/inspect` should resolve against backend-owned sandbox
  manifests/logs under a project-scoped control root. Do not add a second
  CLI-owned project state file; if richer operator UX is needed, add a
  backend-owned summary/inspect seam over the persisted sandbox state.
- The M5 service-control architecture was split into its own companion plan:
  `service-control-plane-plan.md`. This microVM plan still owns the krun
  backend, server/runtime integration, and end-to-end Compose-backed runtime
  verification baseline, but the service CLI/control-root architecture should
  no longer be rediscovered from this plan alone.
- The generic sandbox API now has a cleaner multi-source launch boundary:
  `SandboxSpec` stays the common resolved runtime intent, while image/build
  source data lives in `SandboxImageLaunchSpec` / `SandboxBuildLaunchSpec`.
  This is preferable to stuffing image/build concerns into `SandboxSpec`
  because the service manager still needs image-default merging and process
  override semantics before a runnable sandbox spec exists. Local proof now
  covers both image-backed and build-backed launches through the public
  `SandboxBackend` trait surface.
- The service-manager design is now explicit in code: `neovex-server`
  separates declared services (`SandboxServiceCatalog` / `SandboxServiceLaunch`)
  from active handles (`SandboxCatalog`), validates tenant/service/backend
  alignment before start, and reuses active handles once a service has been
  activated. Local proof covers manager-level activation, router-level
  `ctx.services` startup, and tenant-delete stop/cleanup with a fake backend.
  Remaining M4 work is Linux-host end-to-end proof with a real krun-backed
  service under the manager.
- The first M5 slice is now implemented and locally verified: `neovex-bin`
  owns the Compose translation and CLI seam instead of pushing YAML parsing
  into `neovex-sandbox` or `neovex-server`. `neovex service config` validates
  the supported Compose subset and prints a resolved service plan, while
  `--services` lists service names only. That validation now reaches down into
  the actual launch seam for lowerable process overrides: Compose
  `command`, `entrypoint`, `working_dir`, and `user` are translated into
  `SandboxImageProcessOverrides`, and the sandbox layer now honors explicit
  `user` overrides over image USER metadata. Compose `restart` and
  `stop_grace_period` now validate against the generic
  `SandboxLifecycleSpec` seam, and krun stop now honors per-sandbox lifecycle
  timeout overrides before falling back to the backend default. The same
  adapter now lowers validated services into a typed declared-service catalog
  that implements `SandboxServiceCatalog`, so the server manager handoff is no
  longer hypothetical. `neovex-bin` now also has an explicit `--compose-file`
  startup hook that constructs a real `SandboxServiceManager`; with Convex
  enabled, the main serve path can now carry that manager into the runtime
  registry seam without additional glue. Fractional
  `deploy.resources.limits.cpus` currently round up to whole guest vCPU counts
  with an explicit warning because krun only accepts whole vCPU counts today.
  The follow-on service-control-plane plan now owns the full Compose-backed
  service control plane: typed persisted-state lookup plus `service up`,
  `service down`, `service list`, `service inspect`, `service logs`, and
  `service ps`. Remaining M5 work is Linux-host end-to-end verification of the
  compose-backed serve path plus recovery-drill/operator evidence.
- Upstream source review confirmed libkrun's built-in guest init does **not**
  parse or apply OCI `user`; it only consumes env, args/Cmd, WorkingDir/Cwd,
  and Entrypoint from `/.krun_config.json`. That means guest-side user
  switching needs an explicit helper/wrapper seam rather than another host-side
  crun patch. The local implementation now follows that architecture, and Linux
  verification is complete.
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
| `USER` | `User` | Manifest `image_metadata.user` for future guest-side init drop (bundle stays root for `/dev/kvm`) |
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
      lib.rs                  # Stable sandbox API, launch specs, handles
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
- Keep bundle `process.user` at root and store image `User` in backend-owned
  manifest metadata for future guest-side application
- Set `linux.resources.memory.limit` from generic `SandboxResourceLimits`
  memory settings
- Add annotation `run.oci.handler = "krun"` (selects krun handler in crun)
- Add annotation `krun.port_map` from ExposedPorts
  (auto-assigned host ports via PortManager)
- Set `root.path` to the buildah-mounted rootfs path
- Preserve the configured `StopSignal` in backend-owned launch metadata so
  graceful shutdown uses the image-configured signal
- Materialize backend-owned `/.krun_vm.json` when explicit whole-vCPU counts
  are requested so crun's krun handler can call `krun_set_vm_config()`

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
- Generic memory limits lower into OCI `linux.resources.memory.limit`
- Explicit whole-vCPU limits materialize `/.krun_vm.json` with `cpus` and
  `ram_mib`

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
- `neovex-sandbox` owns the generic backend and launch nouns
  (`SandboxSpec`, `SandboxImageLaunchSpec`, `SandboxBuildLaunchSpec`,
  `SandboxBackend`)
- `neovex-server` owns the service manager, service registry, and
  `ctx.services.<name>` projection so the sandbox crate does not become a
  second server layer
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
| M2: OCI bundle generation | `done` | M1 | All M2 components Linux-verified on Debian 13: image USER resolved and stored in manifest (bundle forces root for VMM /dev/kvm), image STOPSIGNAL honored during shutdown, auto-port-assignment from image EXPOSE proven with distinct allocation and reuse after stop, resource limits lowered into OCI `linux.resources.memory.limit` and `/.krun_vm.json` for both direct-rootfs and image-backed paths. Guest-side user switching deferred to M3 |
| M3: Lifecycle management | `done` | M2 | Startup-readiness, liveness (NotReady/Ready transitions), restart policy (OnFailure crash-then-recover), and exponential restart backoff are Linux-verified. Guest-side user switching is Linux-verified via a statically-linked guest helper that drops to image uid:gid (www-data 33:33 proven via ctr.log). Key finding: virtiofs in rootless krun VMs maps guest uid through host user namespace so non-root guests cannot write to the rootfs overlay |
| M4: Engine integration | `done` | M3 | local slices landed: server-owned service-registry projection to `ctx.services.*`, lazy per-name lookup/caching, an activation-capable blocking `ensure_service_binding(...)` seam, generic sandbox image/build launch entrypoints on `SandboxBackend`, and a server-owned `SandboxServiceManager` that starts declared services on first reference and stops them on tenant deletion in local tests. A checked-in ignored Linux-host smoke lane now exists for the real krun-backed manager path; Linux-verified on Debian 13 (2026-04-13): V8 function ctx.services.db.port triggered real krun service activation via SandboxServiceManager, HTTP connectivity confirmed on TSI port 18090, tenant deletion stopped service and released port. Sandbox db-01kp3ktd3gy7gjsbqwrxbaeant reached Ready then Stopped with exit code 137 after clean teardown |
| M5: Developer experience | `done` | M4 | local Compose/CLI seam is active in `neovex-bin`: `neovex service config` parses/validates Compose YAML, prints a resolved typed service plan, supports `--services`, warns on ignored fields, resolves env_file + resource limits locally, validates lowerable process overrides (`command`, `entrypoint`, `working_dir`, `user`) against the sandbox launch seam, validates lifecycle overrides (`restart`, `stop_grace_period`) against `SandboxLifecycleSpec` with per-sandbox krun stop-timeout support, lowers validated services into a typed `SandboxServiceCatalog` bridge for the server manager, exposes an explicit `--compose-file` startup hook in the main binary, and now has backend-owned persisted-state plus local `service up` / `service down` / `service list` / `service inspect` / `service logs` / `service ps` wiring under the service-control-plane plan. Linux-verified on Debian 13 (2026-04-13) using hardened helpers from `81cf133`: compose-serve passed (~6.9s), recovery drill all checks passed. Evidence: project_key=`smoke-app-cf079a18bd54`, sandbox=`db-01kp3yamn6rb2dtwbk0wn1t8tz`, status=stopped, shutdown_requested=True, exit_code=137, no leaked ports, no orphan processes, ctr+oci logs persist |

---

## Open Questions

1. **buildah mount persistence:** Does `buildah mount` survive neovex restart?
   If not, neovex must remount on startup.
2. **Volume persistence:** Compose `volumes:` maps to virtiofs additional
   mounts. Named volumes need host-side storage managed by neovex.
3. **conmon log rotation:** Does conmon rotate logs, or does neovex need to
   manage log file size?
4. **Compose fractional CPU values:** The current M5 config adapter rounds
   fractional values like `cpus: "0.5"` up to the next whole guest vCPU with a
   warning because krun only accepts whole vCPU counts today. The remaining
   question is whether a later quota abstraction should replace or refine that
   policy.
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
- **M2 Linux verification is complete** (2026-04-13). Resource limits verified
  via `scripts/verify-microvm-m2-resource-limits-helper.sh`:
  - direct-rootfs: `/.krun_vm.json` = `{"cpus":2,"ram_mib":256}`,
    `linux.resources.memory.limit = 268435456`, TSI HTTP OK on port 18083
  - image-backed: `/.krun_vm.json` = `{"cpus":2,"ram_mib":256}`,
    `linux.resources.memory.limit = 268435456`, TSI HTTP OK on port 18084
  - Logs at `${NEOVEX_KRUN_SMOKE_WORKDIR}/m2-resource-limit-verification/`
- **M3 is complete** (2026-04-13). Startup-readiness, liveness, restart
  policy, restart backoff, and guest-side user switching are all Linux-verified.
- **M3 startup-readiness gate Linux-verified** (2026-04-13): the
  `krun_backend_m3_readiness_probe_gates_ready_and_published_endpoints` smoke
  test passes on Debian 13. A delayed-start BusyBox httpd sandbox initially
  reports `Starting` with empty `published_endpoints`, then transitions to
  `Ready` with endpoints published only after the guest answers on TSI port
  18085. All 7 ignored smoke tests pass with no regressions (~60s total).
- **M3 liveness gate Linux-verified** (2026-04-13): the
  `krun_backend_m3_liveness_probe_degrades_and_recovers_without_vm_restart`
  smoke test passes on Debian 13. BusyBox httpd is killed by PID, the sandbox
  degrades `Ready -> NotReady` with empty endpoints and an unreachable host
  port, then recovers `NotReady -> Ready` without a VM restart once httpd comes
  back.
- **M3 restart-policy gate Linux-verified** (2026-04-13): the
  `krun_backend_m3_restart_policy_restarts_failed_vm` smoke test passes on
  Debian 13. A sandbox with `OnFailure { max_restarts: 1 }` exits 42 on first
  boot, is restarted by the backend, and reaches `Ready` on host port 18087
  with `restart_count == 1` and `last_exit_code == 42` recorded in the
  manifest.
- **M3 guest-user-switch Linux-verified** (2026-04-13): the
  `krun_backend_m3_guest_user_switch_applies_image_user_inside_guest` smoke
  test passes on Debian 13. Key evidence:
  - helper root: `/tmp/neovex-guest-user-switch-root`
  - bundle `process.user` stays root (`uid: 0`, `gid: 0`) for `/dev/kvm`
  - bundle `process.args[0] = "/.neovex/neovex-guest-user-switch"`
  - guest uid/gid proof comes from `ctr.log` (`NEOVEX_UID=33`, `NEOVEX_GID=33`)
    because virtiofs/rootless uid mapping prevents reliable non-root writes
    back into the overlay rootfs
  - sandbox reaches `Ready` and answers HTTP on port `18089`
  - manifest preserves `image_metadata.user = "33:33"`
- **M4 is complete** (2026-04-13). The sandbox seam now exposes generic
  image/build launch entrypoints, `neovex-server` now has a real
  `SandboxServiceManager`, and both the local M4 slices plus the Linux-host
  krun-backed manager smoke are recorded here:
  - `cargo test -p neovex-runtime runtime_exposes_service_bindings_from_invocation_request -- --nocapture`
  - `cargo test -p neovex-runtime runtime_lazily_looks_up_missing_service_bindings_and_caches_them -- --nocapture`
  - `cargo test -p neovex-server convex_runtime_query_exposes_service_bindings_and_preserves_them_for_nested_calls -- --nocapture`
  - `cargo test -p neovex-server convex_runtime_query_lazily_resolves_missing_service_bindings -- --nocapture`
  - `cargo test -p neovex-server convex_runtime_query_waits_for_activation_capable_service_lookup_once -- --nocapture`
  - `cargo test -p neovex-server ensure_service_binding_ -- --nocapture`
  - `cargo test -p neovex-server convex_runtime_query_starts_declared_service_on_first_reference -- --nocapture`
  - `cargo test -p neovex-server delete_tenant_stops_manager_owned_sandbox_services -- --nocapture`
  - `cargo test -p neovex-server convex_runtime_query_starts_real_krun_service_under_manager_and_tears_it_down -- --ignored --exact --nocapture`
  - `cargo test -p neovex-sandbox plan_only_backend_lowers_image_launch_through_generic_trait_surface -- --nocapture`
  - `cargo test -p neovex-sandbox plan_only_backend_lowers_build_launch_through_generic_trait_surface -- --nocapture`
  - What this proves locally:
    - invocation requests can carry server-projected `InvocationServices`
    - V8 exposes those bindings as `ctx.services.<name>`
    - nested runtime calls preserve the same service snapshot
    - missing `ctx.services.<name>` lookups now resolve through the server-owned
      `RuntimeServiceRegistry` and cache successful bindings within the
      invocation
    - sync `ctx.services.<name>` access can block once on a registry-provided
      `ensure_service_binding(...)` path and still preserve per-invocation
      caching
    - the generic sandbox trait now accepts image-backed and build-backed
      launch requests without exposing krun-only public types
    - a real server-owned `SandboxServiceManager` can start a declared service
      on first reference, poll until the handle becomes bindable, and reuse the
      active handle on later lookups
    - tenant deletion now routes through the same server-owned manager and
      stops manager-owned sandboxes before the tenant is removed from storage
    - the real Linux-host manager smoke path is checked in, uses the existing
      `NEOVEX_KRUN_SMOKE_*` environment contract plus
      `NEOVEX_KRUN_SMOKE_M4_HOST_PORT` / `NEOVEX_KRUN_SMOKE_M4_GUEST_PORT`,
      and now passes on Linux
  - **M4 Linux verification is complete** (2026-04-13). The ignored Linux-host
    smoke `tests::convex_functions::runtime_queries::execution::services::convex_runtime_query_starts_real_krun_service_under_manager_and_tears_it_down`
    passed in ~10s on Debian 13:
    - `ctx.services.db.port` triggered real krun service activation via
      `SandboxServiceManager`
    - BusyBox httpd responded on TSI host port 18090 (guest port 8090)
    - sandbox `db-01kp3ktd3gy7gjsbqwrxbaeant` reached Ready then Stopped
    - tenant deletion stopped the service (`shutdown_requested: true`,
      `last_exit_code: 137`) and released port 18090
    - state produced at `/tmp/neovex-sandbox-smoke/m4-manager-state/`
- **M5 is now in progress** (2026-04-13). The first local developer-experience
  slice is `neovex service config` in `neovex-bin`:
  - `cargo check -p neovex-bin`
  - `cargo test -p neovex-bin`
  - `cargo test -p neovex-sandbox configured_stop_timeout_prefers_sandbox_lifecycle_and_falls_back_to_backend_default -- --exact`
  - `cargo check -p neovex-sandbox -p neovex-server -p neovex-bin -p neovex`
  - `bash -n scripts/verify-microvm-m5-compose-serve-helper.sh`
  - What this proves locally:
    - `neovex service config` parses a supported Compose subset and prints a
      resolved YAML service plan
    - `neovex service config --services` lists service names only
    - image-backed and build-backed services both resolve
    - `env_file` merges before inline `environment`
    - lowerable Compose process overrides now validate against the real
      sandbox launch seam, including explicit `user` override support
    - lowerable Compose lifecycle overrides now validate against the real
      sandbox lifecycle seam, including `stop_grace_period`
    - the resolved Compose plan lowers into a typed `SandboxServiceCatalog`
      bridge for the server-owned manager seam
    - the main binary accepts `--compose-file` and can construct the
      compose-backed manager/catalog serve path at compile time
    - a checked-in Linux helper now exists for the exact compose-backed serve
      smoke lane
    - ignored fields produce warnings instead of hard errors
    - invalid memory values fail with actionable messages
    - fractional CPU limits round up with an explicit warning
    - explicit `service up` / `service down` now share the same Compose
      lowering, deterministic project identity, and backend-owned lifecycle
      state model as `--compose-file`
  - What remains before M5 can close:
    - run the checked-in Linux end-to-end helper for the compose-backed serve
      path and record the evidence
    - add Linux-host recovery-drill evidence for restart, orphan discovery,
      and log persistence under the service-control-plane plan

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

- 2026-04-13: Reviewed local Podman source to resolve the M5 control-plane
  ownership question before adding lifecycle commands. Relevant files:
  `cmd/podman/compose.go`, `libpod/runtime.go`, `libpod/sqlite_state.go`,
  `pkg/domain/infra/abi/containers.go`, `libpod/runtime_ctr.go`,
  `libpod/container_config.go`, `libpod/container_inspect.go`, and
  `libpod/container_log.go`, plus docs `podman-systemd.unit(5)` and
  `podman-quadlet(1)`. Conclusion: `podman compose` is only a compatibility
  shim around an external Compose provider; Podman's native lifecycle commands
  resolve names/IDs against libpod runtime state and read inspect/log data from
  runtime-owned state plus persistent per-container files under `StaticDir`.
  Quadlet/systemd is Podman's native declarative service layer on Linux, but it
  still targets the same runtime-owned state model. Neovex M5 should mirror
  that split: keep Compose as the input format, avoid a CLI-owned service state
  file, and implement `neovex service up/down/list/logs/inspect` on top of
  backend-owned sandbox manifests/logs under a deterministic project-scoped
  control root.
- 2026-04-13: Landed the first local M5 developer-experience slice in
  `neovex-bin`. Added a `service` CLI family with
  `neovex service config [--file compose.yaml] [--services]`, added
  `crates/neovex-bin/src/service/compose.rs`, and taught it to parse and render
  a supported Compose subset into a typed service plan. The current adapter
  resolves image/build source, env + env_file, ports, restart policy, and
  CPU/memory limits; validates lowerable process overrides
  (`command`, `entrypoint`, `working_dir`, `user`) against the actual sandbox
  launch seam; preserves `depends_on`, `healthcheck`, `volumes`, labels, and
  `x-neovex`; and warns on ignored fields such as `networks`, `privileged`,
  and `logging`. `SandboxImageProcessOverrides` now also carries an explicit
  `user` override, and the buildah-backed krun launch preparation path
  resolves that override ahead of image USER metadata so Compose `user:` can
  flow into future runtime launches. Fractional CPU limits currently round up
  to the next whole guest vCPU with a warning because krun only accepts whole
  guest CPU counts today. Verification evidence:
  `cargo fmt --all --check`,
  `cargo check -p neovex-bin`,
  `cargo test -p neovex-bin`,
  `cargo test -p neovex-sandbox prepare_image_launch_prefers_process_user_override_over_image_user -- --nocapture`.
  M5 is now `in_progress`; remaining work is lifecycle wiring
  (`up/down`) and manager-backed catalog integration.
- 2026-04-13: Landed the next local M5 lifecycle-lowering slice. The generic
  `SandboxLifecycleSpec` now carries an optional per-sandbox `stop_timeout`
  override, serialized durably in manifests as milliseconds. The krun backend
  stop path now honors `spec.lifecycle.stop_timeout` before falling back to the
  backend default timeout, and `neovex service config` now validates Compose
  `stop_grace_period` against that real lifecycle seam instead of treating it
  as inert metadata. Added local tests for lifecycle serialization,
  per-sandbox stop-timeout precedence, Compose stop-grace lowering
  (`1m30s` -> `Duration::from_secs(90)`), and invalid stop-grace errors.
  Verification evidence:
  `cargo fmt --all --check`,
  `cargo check -p neovex-sandbox -p neovex-bin`,
  `cargo test -p neovex-sandbox configured_stop_timeout_prefers_sandbox_lifecycle_and_falls_back_to_backend_default -- --exact`,
  `cargo test -p neovex-bin`.
- 2026-04-13: Landed the next local M5 catalog-bridge slice. The Compose
  adapter now lowers validated services into a typed `ComposeServiceCatalog`
  that implements `SandboxServiceCatalog` with tenant-parametric launch
  generation. Image-backed
  and build-backed services lower into `SandboxServiceLaunch` values with the
  generic `SandboxSpec` carrying ports, restart policy, stop timeout, and
  resource limits, while image/build launch specs carry the lowerable process
  overrides. `neovex service config` now validates this manager-handoff seam on
  every run by constructing the catalog through the same lowerer used by the
  main binary. Added local tests covering image + build catalog lowering and
  tenant-parametric launch generation.
  Verification evidence:
  `cargo fmt --all --check`,
  `cargo check -p neovex-sandbox -p neovex-bin`,
  `cargo test -p neovex-bin`.
- 2026-04-13: Landed the next local M5 serve-path slice. `neovex-bin` now
  accepts an explicit `--compose-file` flag on the main server path, loads the
  validated Compose catalog through the same M5 adapter, and constructs a real
  `SandboxServiceManager` backed by the default krun backend config. With
  `--convex-app-dir`, the binary now routes that manager into the runtime
  service-registry seam via
  `serve_with_convex_and_license_and_sandbox_service_manager(...)`; without
  Convex, the manager is currently attached as a plain sandbox catalog so
  operator-facing sandbox snapshots remain visible without inventing fake
  activation behavior. Added new server helper functions for the sandbox-aware
  serve path and a CLI parse test for `--compose-file`.
  Verification evidence:
  `cargo fmt --all --check`,
  `cargo check -p neovex-sandbox -p neovex-server -p neovex-bin -p neovex`,
  `cargo test -p neovex-bin`.
- 2026-04-13: Landed the next local M5 lifecycle-control slice under the
  service-control-plane plan. `neovex-bin` now exposes
  `neovex service up [service] [--tenant <tenant-id>]` and
  `neovex service down [service] [--tenant <tenant-id>]`. Both commands reparse
  Compose through the same `ComposeProjectContext`, derive the same
  deterministic project key / local tenant / project-scoped krun backend root
  as `--compose-file`, and consume the same `SandboxServiceCatalog` lowering
  bridge as the server path. `service up` inspects backend-owned persisted
  state first and returns `already_running` for the current active service
  identity instead of launching duplicates. `service down` resolves one current
  target per service identity from backend-owned manifests, deduping historical
  manifest history before stop or `already_stopped` reporting. Verification
  evidence:
  `cargo fmt --all --check`,
  `cargo check -p neovex-sandbox -p neovex-bin -p neovex`,
  `cargo test -p neovex-bin service:: -- --nocapture`.
- 2026-04-13: Added the checked-in Linux-host M5 compose-backed serve helper:
  `scripts/verify-microvm-m5-compose-serve-helper.sh`. The helper replays the
  full local verification lane (`cargo fmt`, focused `cargo check`,
  `cargo test -p neovex-bin`) and then runs the new ignored Linux smoke
  `tests::convex_runtime_query_starts_real_krun_service_from_compose_file_and_tears_it_down`
  with `--ignored --exact --nocapture --test-threads=1`. It writes durable
  artifacts under
  `${NEOVEX_KRUN_SMOKE_WORKDIR}/m5-compose-serve-verification/`, including
  `compose-serve.log` and `summary.txt`, and uses
  `NEOVEX_KRUN_SMOKE_M5_HOST_PORT` / `NEOVEX_KRUN_SMOKE_M5_GUEST_PORT`
  (defaults: 18091 / 8091). Local verification:
  `bash -n scripts/verify-microvm-m5-compose-serve-helper.sh`.
- 2026-04-13: Added the checked-in Linux-host M4 smoke lane for the real
  manager-backed krun path:
  `convex_runtime_query_starts_real_krun_service_under_manager_and_tears_it_down`.
  The new ignored server test spins up a real `KrunSandboxBackend` under
  `SandboxServiceManager`, lets a Convex runtime query trigger `ctx.services`
  activation for a BusyBox-backed service, proves HTTP reachability on the
  returned TSI host port, then deletes the tenant and waits for both the
  service snapshot and host port to disappear. The test compiles locally via
  `cargo test -p neovex-server convex_runtime_query_starts_real_krun_service_under_manager_and_tears_it_down -- --exact`;
  the remaining M4 blocker is now executing that exact ignored lane
  successfully on a Linux KVM host and recording the evidence here.
- 2026-04-13: Landed the fifth local M4 engine-integration slice: tenant
  teardown now routes through the runtime-service seam. Added a default
  `RuntimeServiceRegistry::teardown_tenant(...)` hook, taught the HTTP
  tenant-delete route to call it before deleting tenant storage, and made
  `SandboxServiceManager` override it by stopping tracked sandboxes through the
  generic backend and clearing later snapshots. Verification evidence:
  `cargo check -p neovex-server -p neovex`,
  `cargo fmt --all --check`,
  `cargo test -p neovex-server teardown_tenant_ -- --nocapture`,
  `cargo test -p neovex-server convex_runtime_query_starts_declared_service_on_first_reference -- --nocapture`,
  `cargo test -p neovex-server delete_tenant_stops_manager_owned_sandbox_services -- --nocapture`.
  M4 remains `in_progress`: local start/stop lifecycle under the manager is now
  real, but Linux-host end-to-end proof with a real krun-backed service still
  remains.
- 2026-04-13: Landed the fourth local M4 engine-integration slice: a real
  server-owned `SandboxServiceManager`. Added public
  `SandboxServiceCatalog` / `SandboxServiceLaunch` nouns plus
  `SandboxServiceManager` in `neovex-server`, added router builders that accept
  the manager directly, and implemented first-reference activation through the
  existing blocking `RuntimeServiceRegistry::ensure_service_binding(...)`
  boundary. The manager validates tenant/service/backend alignment, starts
  declared services through the generic `SandboxBackend` image/build
  entrypoints, polls `inspect()` until the service becomes bindable, and
  reuses the active handle on later lookups. Verification evidence:
  `cargo check -p neovex-server -p neovex`,
  `cargo test -p neovex-server ensure_service_binding_ -- --nocapture`,
  `cargo test -p neovex-server convex_runtime_query_starts_declared_service_on_first_reference -- --nocapture`,
  `cargo test -p neovex-server convex_runtime_query_ -- --nocapture`.
  M4 remained `in_progress`: local start-on-first-reference was real at this
  point, but tenant-teardown stop/cleanup validation and Linux-host proof still
  remained.
- 2026-04-13: Promoted krun's image/build launch surface into the generic
  `neovex-sandbox` API. Added public
  `SandboxImageLaunchSpec` / `SandboxBuildLaunchSpec` /
  `SandboxImageProcessOverrides` nouns, extended `SandboxBackend` with generic
  `start_from_image(...)` and `start_from_build(...)` entrypoints, migrated the
  krun backend and smoke/tests off the old krun-local public override type, and
  added focused trait-surface coverage proving both image-backed and
  build-backed launches lower through `Box<dyn SandboxBackend>`. Verification
  evidence:
  `cargo check -p neovex-sandbox -p neovex`,
  `cargo test -p neovex-sandbox plan_only_backend_lowers_image_launch_through_generic_trait_surface -- --nocapture`,
  `cargo test -p neovex-sandbox plan_only_backend_lowers_build_launch_through_generic_trait_surface -- --nocapture`.
  M4 remains `in_progress`: the generic launch seam is now available to a
  future service manager, but no real activation/start-on-first-reference flow
  or Linux-host end-to-end proof has landed yet.
- 2026-04-13: Landed the third local M4 engine-integration slice. Added
  `RuntimeServiceRegistry::ensure_service_binding(...)` as the explicit
  blocking boundary for `ctx.services` lookups, changed the `CtxServiceLookup`
  host op to call that hook instead of the immediate resolve path, and added
  focused server coverage proving a registry can block once for a binding and
  still benefit from per-invocation `ctx.services` caching. The default
  `SandboxCatalogRuntimeServiceRegistry` remains resolve-only; this slice does
  not start real sandboxes yet, but it establishes the contract a real
  activation-capable service manager must implement. Verification evidence:
  `cargo fmt --all --check`,
  `cargo check -p neovex-server -p neovex-runtime -p neovex`,
  `cargo test -p neovex-server convex_runtime_query_waits_for_activation_capable_service_lookup_once -- --nocapture`.
  M4 remains `in_progress`: the blocking activation seam is now explicit, but
  a real sandbox-starting registry and Linux-host end-to-end proof still
  remain.
- 2026-04-13: Landed the second local M4 engine-integration slice. The V8
  bootstrap now exposes `ctx.services` through a lazy proxy instead of a pure
  frozen snapshot, successful missing-name lookups are resolved through the new
  `op_neovex_ctx_service_lookup` sync host op and cached for the rest of the
  invocation, and `neovex-server` now routes that host op through an internal
  `RuntimeServiceRegistry` trait instead of reaching directly into
  `SandboxCatalog`. Added a test-only router seam that accepts an explicit
  runtime service registry so server tests can verify lazy lookup without
  changing the public router API. Added focused coverage for runtime-side lazy
  lookup/caching and for server-side lazy resolution with an empty initial
  snapshot. Verification evidence:
  `cargo fmt --all --check`,
  `cargo check -p neovex-server -p neovex-runtime -p neovex`,
  `cargo test -p neovex-runtime runtime_exposes_service_bindings_from_invocation_request -- --nocapture`,
  `cargo test -p neovex-runtime runtime_lazily_looks_up_missing_service_bindings_and_caches_them -- --nocapture`,
  `cargo test -p neovex-server convex_runtime_query_exposes_service_bindings_and_preserves_them_for_nested_calls -- --nocapture`,
  `cargo test -p neovex-server convex_runtime_query_lazily_resolves_missing_service_bindings -- --nocapture`.
  M4 remains `in_progress`: lazy lookup is local-proof complete, but true
  service activation/start-on-first-reference and Linux-host end-to-end proof
  still remain.
- 2026-04-13: Landed the first local M4 engine-integration slice. Added
  `InvocationServices` / `InvocationServiceBinding` to the runtime invocation
  contract, taught the V8 bootstrap to expose a frozen `ctx.services` object,
  and introduced `crates/neovex-server/src/service_registry.rs` so the server
  can project ready sandbox handles into that runtime-facing shape while
  keeping `neovex-sandbox` generic. Top-level HTTP/runtime routes now snapshot
  those bindings from `SandboxCatalog`, `ConvexHostBridge` preserves the same
  snapshot for nested runtime calls, and runtime-subscription transforms now
  carry the snapshot forward for re-evaluation. Added focused coverage for the
  raw runtime projection and for server-side propagation through nested runtime
  calls. Verification evidence:
  `cargo fmt --all --check`,
  `cargo check -p neovex-server -p neovex-runtime -p neovex`,
  `cargo test -p neovex-runtime runtime_exposes_service_bindings_from_invocation_request -- --nocapture`,
  `cargo test -p neovex-server convex_runtime_query_exposes_service_bindings_and_preserves_them_for_nested_calls -- --nocapture`.
  M4 remains `in_progress`: the current slice is snapshot-only and still needs
  lazy activation / "start on first reference" plus Linux-host end-to-end
  validation against a real krun-backed service.
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
- 2026-04-13: Started the next M2 slice for host-port auto-assignment. Added
  backend-local `port_manager.rs` that scans active manifests under
  `state_root/containers/`, leases host ports from the default backend-owned
  range `15000..=16000`, skips guest ports that already have explicit generic
  bindings, ignores non-TCP `EXPOSE` metadata, and reuses ports after a sandbox
  is stopped. `vm.rs` now materializes missing generic `SandboxPortBinding`s
  from image `EXPOSE` metadata during start, updates the sandbox handle's
  published endpoints, and rewrites the bundle `krun.port_map` annotation to
  match the leased host ports. Added unit coverage for range allocation and
  stopped-manifest reuse in `port_manager.rs` plus a plan-only integration test
  proving image-backed starts auto-assign `5432/tcp`, allocate unique ports
  across two live sandboxes, and reuse a released port after stop.
  Verification: `cargo fmt --all --check`,
  `cargo check -p neovex-sandbox`,
  `cargo test -p neovex-sandbox` (32 pass).
- 2026-04-13: Ran M2 Linux-host verification for auto-port assignment from image
  EXPOSE metadata on Debian 13 x86_64. Created a custom BusyBox image with
  `EXPOSE 8080/tcp` via buildah config/commit. Launched three sandboxes via
  `start_from_image()` with no explicit `SandboxPortBinding` entries and a
  backend port range of `15100..=15105`:
  (1) sandbox A got auto-assigned host port 15100 (first in range), HTTP
  connectivity confirmed via TSI;
  (2) sandbox B got auto-assigned host port 15101 (next available, distinct
  from A), HTTP connectivity confirmed;
  (3) stopped sandbox A → port 15100 released;
  (4) sandbox C got auto-assigned host port 15100 (reused A's released port),
  HTTP connectivity confirmed.
  This proves: auto-assignment from the backend-owned range, distinct ports for
  concurrent sandboxes, port release on stop, and port reuse after release —
  all end-to-end on a real Linux host with real krun VMs and TSI networking.
  Verification: `cargo fmt --all --check`, `cargo check -p neovex-sandbox -p neovex`,
  `cargo test -p neovex-sandbox` (32 pass),
  `cargo test -p neovex-sandbox --test krun_linux_smoke -- --ignored --test-threads=1`
  (4 pass, ~41s total).
  Env: `NEOVEX_KRUN_SMOKE_ROOTFS=/tmp/neovex-sandbox-smoke-rootfs`,
  `NEOVEX_KRUN_SMOKE_WORKDIR=/tmp/neovex-sandbox-smoke`,
  `NEOVEX_KRUN_SMOKE_RUNTIME=/usr/libexec/neovex/crun`,
  `NEOVEX_KRUN_SMOKE_CONMON=/usr/bin/conmon`,
  `NEOVEX_KRUN_SMOKE_BUILDAH=/usr/bin/buildah`.
  Auto-port-assignment is now Linux-verified.
- 2026-04-12: Implemented the remaining M2 resource-limits seam locally on the
  macOS development workspace. `SandboxSpec` now carries generic
  `SandboxResourceLimits` (`cpu_count`, `memory_limit_bytes`) with public
  builders and facade re-exports. `bundle.rs` now validates those limits and
  lowers memory into OCI `linux.resources.memory.limit`. `vm.rs` now derives
  backend-owned `/.krun_vm.json` data (`cpus`, `ram_mib`) for explicit
  whole-vCPU requests, writes/removes that file directly for local rootfs
  starts, and injects the same materialization step into the buildah-unshare
  conmon create path for image-backed sandboxes. Added unit coverage for
  bundle memory lowering, cpu-without-memory validation, direct-rootfs vm-config
  write/remove behavior, image-backed unshare prelude generation, and the
  backend launch-plan seam. Verification:
  `cargo fmt --all --check`,
  `cargo check -p neovex-sandbox -p neovex`,
  `cargo test -p neovex-sandbox` (39 pass).
  M2 remains `in_progress` because the new resource-limits path is only
  unit/local-verified; Linux host promotion is still required before this
  phase can move to `done`.
- 2026-04-12: Added the Linux-host proof harness for the remaining M2
  promotion gate. `crates/neovex-sandbox/tests/krun_linux_smoke.rs` now
  includes:
  (1) `krun_backend_m2_direct_rootfs_resource_limits_lowering`, which boots a
  real rootfs-backed BusyBox service with `memory_limit_bytes=268435456` and
  `cpu_count=2`, then verifies `/.krun_vm.json` contains `{"cpus":2,"ram_mib":256}`,
  `config.json` records `linux.resources.memory.limit = 268435456`, and the
  guest responds over TSI;
  (2) `krun_backend_m2_image_backed_resource_limits_lowering`, which starts an
  image-backed BusyBox sandbox with the same limits, reads `/.krun_vm.json`
  back out of the mounted buildah container rootfs via `buildah unshare`,
  verifies the same bundle memory limit, and confirms TSI-backed HTTP reachability.
  Added `scripts/verify-microvm-m2-resource-limits-helper.sh` as the checked-in
  Linux execution entrypoint for those tests; it captures durable logs at
  `${NEOVEX_KRUN_SMOKE_WORKDIR}/m2-resource-limit-verification/`.
  Local verification after adding those host-only tests:
  `cargo fmt --all --check`,
  `cargo check -p neovex-sandbox -p neovex`,
  `cargo test -p neovex-sandbox` (39 pass),
  `bash -n scripts/verify-microvm-m2-resource-limits-helper.sh`.
- 2026-04-13: Ran M2 resource-limits Linux-host verification on Debian 13 x86_64
  via `bash scripts/verify-microvm-m2-resource-limits-helper.sh`. Both the
  direct-rootfs and image-backed lanes passed on the first attempt:
  (1) direct-rootfs (`krun_backend_m2_direct_rootfs_resource_limits_lowering`):
  `/.krun_vm.json` at rootfs `/tmp/neovex-sandbox-smoke-rootfs/.krun_vm.json`
  contained `{"cpus":2,"ram_mib":256}`, bundle `config.json` had
  `linux.resources.memory.limit = 268435456`, TSI HTTP on port 18083 confirmed.
  (2) image-backed (`krun_backend_m2_image_backed_resource_limits_lowering`):
  `.krun_vm.json` inside the buildah overlay rootfs contained
  `{"cpus":2,"ram_mib":256}` (read via `buildah unshare -- cat`), bundle
  `config.json` had `linux.resources.memory.limit = 268435456`, TSI HTTP on
  port 18084 confirmed. Verification logs at
  `/tmp/neovex-sandbox-smoke/m2-resource-limit-verification/direct-rootfs.log`,
  `/tmp/neovex-sandbox-smoke/m2-resource-limit-verification/image-backed.log`,
  `/tmp/neovex-sandbox-smoke/m2-resource-limit-verification/summary.txt`.
  Env: `NEOVEX_KRUN_SMOKE_ROOTFS=/tmp/neovex-sandbox-smoke-rootfs`,
  `NEOVEX_KRUN_SMOKE_WORKDIR=/tmp/neovex-sandbox-smoke`,
  `NEOVEX_KRUN_SMOKE_RUNTIME=/usr/libexec/neovex/crun`,
  `NEOVEX_KRUN_SMOKE_CONMON=/usr/bin/conmon`,
  `NEOVEX_KRUN_SMOKE_BUILDAH=/usr/bin/buildah`.
  Verification: `cargo fmt --all --check` pass, `cargo check -p neovex-sandbox -p neovex` pass,
  `cargo test -p neovex-sandbox` (39 pass), both resource-limit smoke tests pass.
  M2 is now `done`. M3 promoted to `in_progress`.
- 2026-04-12: Started the first concrete M3 implementation slice on the macOS
  development workspace. `crates/neovex-sandbox/src/backends/krun/vm.rs` no
  longer maps OCI runtime state `"running"` directly to `SandboxStatus::Ready`
  in execute mode. Instead it now derives a backend-local readiness probe from
  published endpoints, prefers HTTP endpoints over raw TCP when both exist,
  keeps execute-mode sandboxes in `Starting` until that probe succeeds, and
  hides execute-mode published endpoints until readiness passes. Added local
  unit coverage for HTTP probe success, probe-target selection, failure-to-
  `Starting` behavior, and endpoint gating while booting. Verification:
  `cargo fmt --all --check`,
  `cargo check -p neovex-sandbox -p neovex`,
  `cargo test -p neovex-sandbox` (43 pass).
  This closes the first local M3 readiness gap but still needs Linux-host smoke
  promotion before the plan can claim end-to-end proof for the new startup gate.
- 2026-04-13: Ran M3 startup-readiness gate Linux-host verification on Debian 13
  x86_64. The ignored smoke test
  `krun_backend_m3_readiness_probe_gates_ready_and_published_endpoints` passed
  on the first attempt in ~7.9s:
  (1) `start()` returned `SandboxStatus::Starting` with empty
  `published_endpoints` — confirmed the gate holds;
  (2) during the 2s delay (busybox `sleep 2` before httpd starts), `inspect()`
  reported `Starting` with hidden endpoints — confirmed the probe doesn't
  short-circuit;
  (3) after the guest httpd bound port 8085, `inspect()` transitioned to
  `SandboxStatus::Ready` with 1 published endpoint on host port 18085;
  (4) HTTP probe on 127.0.0.1:18085 returned BusyBox httpd response.
  Full regression run of all 7 ignored smoke tests passed in ~60s with no
  regressions. Verification:
  `cargo fmt --all --check` pass,
  `cargo check -p neovex-sandbox -p neovex` pass,
  `cargo test -p neovex-sandbox` (43 pass),
  `cargo test -p neovex-sandbox --test krun_linux_smoke krun_backend_m3_readiness_probe_gates_ready_and_published_endpoints -- --ignored --exact --test-threads=1` pass.
  Env: `NEOVEX_KRUN_SMOKE_ROOTFS=/tmp/neovex-sandbox-smoke-rootfs`,
  `NEOVEX_KRUN_SMOKE_WORKDIR=/tmp/neovex-sandbox-smoke`,
  `NEOVEX_KRUN_SMOKE_RUNTIME=/usr/libexec/neovex/crun`,
  `NEOVEX_KRUN_SMOKE_CONMON=/usr/bin/conmon`,
  `NEOVEX_KRUN_SMOKE_BUILDAH=/usr/bin/buildah`.
  M3 startup-readiness gate is now Linux-verified. Remaining M3 work: liveness
  probes, restart policy, guest-side user switching.
- 2026-04-12: Landed the next local M3 lifecycle slice on the macOS workspace.
  `crates/neovex-sandbox/src/instance.rs` now exposes a generic
  `SandboxStatus::NotReady` state, and
  `crates/neovex-sandbox/src/backends/krun/vm.rs` now uses that state when an
  execute-mode sandbox has already proven readiness once but later stops
  answering its endpoint-derived probe. This gives krun a clean
  `Ready -> NotReady -> Ready` lifecycle without conflating a degraded running
  sandbox with either `Starting` or `Failed`, and execute-mode
  `published_endpoints` remain withdrawn whenever status is not `Ready`.
  Added unit coverage for:
  - degrading a previously ready sandbox to `NotReady` on probe failure
  - recovering a `NotReady` sandbox back to `Ready` when the probe succeeds
  - keeping execute-mode endpoints hidden in the `NotReady` state
  - keeping `NotReady` sandboxes' host ports reserved in the port manager
  Added a new ignored Linux smoke test,
  `krun_backend_m3_liveness_probe_degrades_and_recovers_without_vm_restart`,
  that scripts a BusyBox guest through `Ready -> NotReady -> Ready` without
  killing the VM. Verification:
  `cargo fmt --all --check` pass,
  `cargo check -p neovex-sandbox -p neovex` pass,
  `cargo test -p neovex-sandbox` (46 pass).
  This local liveness slice is ready for Linux-host promotion; restart policy
  and guest-side user switching remain after that proof.
- 2026-04-13: Ran M3 liveness probe Linux-host verification on Debian 13 x86_64.
  Initial run: test timed out waiting for `NotReady` because BusyBox `killall httpd`
  does not work inside krun VMs — the process name is `busybox`, not `httpd`.
  Fix: updated the test script to use PID-based killing:
  `/bin/busybox httpd -f -p 8086 & HTTPD_PID=$!; sleep 2; kill $HTTPD_PID; sleep 3;
  /bin/busybox httpd -f -p 8086`. Manual verification confirmed the service-down
  window: HTTP responds at T=1-2s, fails at T=3-4s, recovers at T=5s+.
  After fix, the smoke test
  `krun_backend_m3_liveness_probe_degrades_and_recovers_without_vm_restart`
  passed in ~13.9s:
  (1) sandbox reached `Ready` with 1 published endpoint on port 18086;
  (2) HTTP confirmed on port 18086 during initial Ready window;
  (3) sandbox transitioned to `NotReady` after httpd was killed — published
  endpoints withdrawn, port 18086 unreachable;
  (4) sandbox recovered to `Ready` after httpd restarted — endpoints reappeared,
  HTTP connectivity restored on port 18086;
  (5) stop succeeded normally.
  All 46 unit tests pass. Verification:
  `cargo fmt --all --check` pass,
  `cargo check -p neovex-sandbox -p neovex` pass,
  `cargo test -p neovex-sandbox` (46 pass),
  exact test: `cargo test -p neovex-sandbox --test krun_linux_smoke
  krun_backend_m3_liveness_probe_degrades_and_recovers_without_vm_restart
  -- --ignored --exact --test-threads=1` pass.
  Env: `NEOVEX_KRUN_SMOKE_ROOTFS=/tmp/neovex-sandbox-smoke-rootfs`,
  `NEOVEX_KRUN_SMOKE_WORKDIR=/tmp/neovex-sandbox-smoke`,
  `NEOVEX_KRUN_SMOKE_RUNTIME=/usr/libexec/neovex/crun`,
  `NEOVEX_KRUN_SMOKE_CONMON=/usr/bin/conmon`,
  `NEOVEX_KRUN_SMOKE_BUILDAH=/usr/bin/buildah`.
  M3 liveness probe slice is now Linux-verified. Remaining M3 work: restart
  policy and guest-side user switching.
- 2026-04-13: Landed the next local M3 lifecycle slice on the macOS workspace:
  restart policy. `crates/neovex-sandbox/src/spec.rs` now exposes the generic
  `SandboxLifecycleSpec` and `SandboxRestartPolicy` surface, re-exported
  through `neovex-sandbox` and the `neovex` facade. The krun backend now
  persists `restart_count` in the manifest and performs an inspect-driven
  restart when a crashed execute-mode sandbox has a policy that allows
  relaunch. The current slice is intentionally bounded: it supports
  `Never`, `OnFailure { max_restarts }`, and `Always { max_restarts }`, and
  the relaunch path does a runtime delete before recreate so the same sandbox
  identity can come back cleanly. Added local unit coverage for:
  - restart policy decision shapes
  - manifest-compatible serde defaults for new lifecycle/restart fields
  - unchanged liveness and port-lease behavior after the new lifecycle nouns
  Added a new ignored Linux smoke test,
  `krun_backend_m3_restart_policy_restarts_failed_vm`, that boots a direct-
  rootfs guest with `OnFailure { max_restarts: 1 }`, forces the first boot to
  exit 42, and expects the restarted boot to reach `Ready` on host port 18087
  with `restart_count == 1` and `last_exit_code == 42` recorded in the
  manifest. Verification:
  `cargo fmt --all --check` pass,
  `cargo check -p neovex-sandbox -p neovex` pass,
  `cargo test -p neovex-sandbox` (48 pass).
  This restart slice is ready for Linux-host promotion. Exponential backoff
  and guest-side user switching remain after the basic restart path is proven.
- 2026-04-13: Ran M3 restart-policy Linux-host verification on Debian 13 x86_64.
  The smoke test `krun_backend_m3_restart_policy_restarts_failed_vm` passed on
  both runs (~6.6s each):
  (1) first boot: guest script increments a marker file to `1` and exits with
  code 42;
  (2) backend detects the crash via inspect-driven status check, restart policy
  `OnFailure { max_restarts: 1 }` permits one restart;
  (3) backend performs `crun delete` then relaunches via `conmon -> crun create
  -> crun start`;
  (4) second boot: guest script increments marker to `2` and starts httpd on
  port 8087;
  (5) sandbox reaches `Ready` with 1 published endpoint on host port 18087;
  (6) HTTP probe on 127.0.0.1:18087 returns BusyBox httpd response;
  (7) manifest records `restart_count: 1`, `last_exit_code: 42`,
  `status: "ready"`;
  (8) marker file in rootfs confirms 2 boots.
  All 48 unit tests pass. Verification:
  `cargo fmt --all --check` pass,
  `cargo check -p neovex-sandbox -p neovex` pass,
  `cargo test -p neovex-sandbox` (48 pass),
  exact test: `cargo test -p neovex-sandbox --test krun_linux_smoke
  krun_backend_m3_restart_policy_restarts_failed_vm
  -- --ignored --exact --test-threads=1` pass.
  Env: `NEOVEX_KRUN_SMOKE_ROOTFS=/tmp/neovex-sandbox-smoke-rootfs`,
  `NEOVEX_KRUN_SMOKE_WORKDIR=/tmp/neovex-sandbox-smoke`,
  `NEOVEX_KRUN_SMOKE_RUNTIME=/usr/libexec/neovex/crun`,
  `NEOVEX_KRUN_SMOKE_CONMON=/usr/bin/conmon`,
  `NEOVEX_KRUN_SMOKE_BUILDAH=/usr/bin/buildah`.
  M3 restart-policy slice is now Linux-verified. Remaining M3 work: guest-side
  user switching and exponential backoff refinement.
- 2026-04-13: Landed the next local M3 restart refinement on the macOS
  workspace: exponential backoff. `crates/neovex-sandbox/src/backends/krun/vm.rs`
  now persists `next_restart_at_millis` in the manifest, computes a capped
  restart delay of 1s, 2s, 4s, ... up to 60s, and keeps crashing sandboxes in
  `Starting` until the scheduled retry time is reached. The relaunch still
  flows through the existing inspect-driven restart path; this slice only
  removes immediate crash-loop retries. Added local unit coverage for capped
  delay growth and kept the legacy manifest compatibility path green for the new
  manifest field. Added a new ignored Linux smoke test,
  `krun_backend_m3_restart_backoff_delays_repeated_restarts`, that forces two
  failed boots before a third successful HTTP boot on port 18088 and verifies
  the restart marker plus manifest restart count. Verification:
  `cargo fmt --all --check` pass,
  `cargo check -p neovex-sandbox -p neovex` pass,
  `cargo test -p neovex-sandbox` (49 pass).
  This backoff slice is ready for Linux-host promotion. Guest-side user
  switching remains after that proof.
- 2026-04-13: Ran M3 restart-backoff Linux-host verification on Debian 13 x86_64.
  The smoke test `krun_backend_m3_restart_backoff_delays_repeated_restarts`
  passed on first attempt (~10.4s):
  (1) first boot: guest script increments marker to `1` and exits with code 42;
  (2) backend detects crash, `OnFailure { max_restarts: 2 }` permits restart,
  backoff delay of 1s applied before first restart;
  (3) second boot: marker increments to `2`, exits 42 again;
  (4) backend detects second crash, backoff delay of 2s applied;
  (5) third boot: marker increments to `3`, starts httpd on port 8088;
  (6) sandbox reaches `Ready` with 1 published endpoint on host port 18088;
  (7) total elapsed ~10.4s shows visible backoff (asserted ≥2.5s);
  (8) HTTP probe on 127.0.0.1:18088 returns BusyBox httpd response;
  (9) manifest records `restart_count: 2`, `last_exit_code: 42`,
  `status: "ready"`;
  (10) rootfs marker confirms 3 boots.
  Note: one transient unit test failure observed
  (`prepare_built_image_launch_uses_built_image_reference`), not reproducible on
  rerun — a pre-existing flake in the fake-buildah script harness, not related
  to the backoff changes.
  All 49 unit tests pass on clean rerun. Verification:
  `cargo fmt --all --check` pass,
  `cargo check -p neovex-sandbox -p neovex` pass,
  `cargo test -p neovex-sandbox` (49 pass),
  exact test: `cargo test -p neovex-sandbox --test krun_linux_smoke
  krun_backend_m3_restart_backoff_delays_repeated_restarts
  -- --ignored --exact --test-threads=1` pass.
  Env: `NEOVEX_KRUN_SMOKE_ROOTFS=/tmp/neovex-sandbox-smoke-rootfs`,
  `NEOVEX_KRUN_SMOKE_WORKDIR=/tmp/neovex-sandbox-smoke`,
  `NEOVEX_KRUN_SMOKE_RUNTIME=/usr/libexec/neovex/crun`,
  `NEOVEX_KRUN_SMOKE_CONMON=/usr/bin/conmon`,
  `NEOVEX_KRUN_SMOKE_BUILDAH=/usr/bin/buildah`.
  M3 restart backoff slice is now Linux-verified. Guest-side user switching
  remains the last M3 item.
- 2026-04-13: Landed the local M3 guest-user-switch slice on the macOS
  workspace. Key design finding: upstream `containers/libkrun` `init/init.c`
  does not parse or apply OCI `user`, so preserving `process.user` in
  `/.krun_config.json` is insufficient. The krun backend now rewrites guest
  process args to call `/.neovex/neovex-guest-user-switch` only when
  `image_metadata.user` is present, injects `NEOVEX_GUEST_UID` /
  `NEOVEX_GUEST_GID`, and bind-mounts `guest_user_helper_root` into `/.neovex`
  in the OCI bundle. Added the guest helper binary at
  `crates/neovex-sandbox/src/bin/neovex-guest-user-switch.rs`, a Linux helper
  builder script at `scripts/build-neovex-guest-user-switch.sh`, and a new
  ignored smoke `krun_backend_m3_guest_user_switch_applies_image_user_inside_guest`
  that expects a BusyBox image with `USER www-data` to write uid/gid marker
  files containing `33` before serving HTTP on port 18089. Local verification:
  `cargo fmt --all --check` pass,
  `cargo check -p neovex-sandbox -p neovex` pass,
  `cargo test -p neovex-sandbox` pass (`52` library tests + `2` helper-binary
  tests). Linux-host verification is now the remaining step before M3 can move
  to `done`.
- 2026-04-13: Ran M3 guest-side user switching Linux-host verification on
  Debian 13 x86_64.
  Build: `bash scripts/build-neovex-guest-user-switch.sh` — fixed two issues:
  (1) changed from glibc `+crt-static` to musl target
  (`x86_64-unknown-linux-musl`) for a truly static binary; (2) fixed ldd check
  to accept "statically linked" in addition to "not a dynamic executable";
  (3) redirected cargo output to stderr so `$()` capture only gets the output
  path. Helper built at `/tmp/neovex-guest-user-switch-root/neovex-guest-user-switch`
  (459KB, static-pie, musl-linked).
  First smoke attempt: test failed because the guest script wrote uid/gid to
  `/.neovex-m3-user-uid` and `/.neovex-m3-user-gid` inside the rootfs, but
  uid 33 (www-data) got "Permission denied". Root cause: the OCI config mounts
  the rootfs root `./` as writable, but the BusyBox rootfs root directory is
  `dr-xr-xr-x` (no write for non-root).
  Second attempt: changed to `/tmp/.neovex-m3-user-uid` — still failed with
  "Operation not permitted". Root cause: virtiofs maps guest uid through the
  host user namespace. Guest uid 33 maps to a host uid that lacks write access
  to the overlay.
  Fix: changed the test to capture uid/gid via stderr (ctr.log) instead of
  rootfs files. Guest script: `echo NEOVEX_UID=$(id -u) >&2; echo
  NEOVEX_GID=$(id -g) >&2; exec /bin/busybox httpd -f -p 8089`.
  After fix, the smoke test
  `krun_backend_m3_guest_user_switch_applies_image_user_inside_guest` passed
  in ~7.6s:
  (1) buildah fixture image created with `USER www-data`;
  (2) bundle `process.user` = `{uid:0, gid:0}` (root for VMM /dev/kvm);
  (3) bundle `process.args[0]` = `/.neovex/neovex-guest-user-switch`;
  (4) ctr.log reports `NEOVEX_UID=33` and `NEOVEX_GID=33`;
  (5) sandbox reached `Ready` with 1 published endpoint on host port 18089;
  (6) HTTP probe on 127.0.0.1:18089 returned BusyBox httpd response;
  (7) manifest `image_metadata.user` preserved as `33:33`.
  Unused variable `guest_port_str` also removed from the test.
  All 54 tests pass (52 unit + 2 integration). Verification:
  `cargo fmt --all --check` pass,
  `cargo check -p neovex-sandbox -p neovex` pass,
  `cargo test -p neovex-sandbox` (54 pass on clean rerun),
  exact test: `cargo test -p neovex-sandbox --test krun_linux_smoke
  krun_backend_m3_guest_user_switch_applies_image_user_inside_guest
  -- --ignored --exact --test-threads=1` pass.
  Env: `NEOVEX_KRUN_SMOKE_ROOTFS=/tmp/neovex-sandbox-smoke-rootfs`,
  `NEOVEX_KRUN_SMOKE_WORKDIR=/tmp/neovex-sandbox-smoke`,
  `NEOVEX_KRUN_SMOKE_RUNTIME=/usr/libexec/neovex/crun`,
  `NEOVEX_KRUN_SMOKE_CONMON=/usr/bin/conmon`,
  `NEOVEX_KRUN_SMOKE_BUILDAH=/usr/bin/buildah`,
  `NEOVEX_KRUN_GUEST_USER_HELPER_ROOT=/tmp/neovex-guest-user-switch-root`.
  M3 is now done. All five M3 slices are Linux-verified: startup readiness,
  liveness, restart policy, exponential backoff, and guest-side user switching.
  M4 promoted to in_progress.
- 2026-04-13: Ran M4 engine-integration Linux-host smoke on Debian 13 x86_64.
  The ignored test
  `tests::convex_functions::runtime_queries::execution::services::convex_runtime_query_starts_real_krun_service_under_manager_and_tears_it_down`
  passed on first attempt (~10.2s):
  (1) server started with a `SandboxServiceManager` owning a declared "db"
  service backed by BusyBox httpd on krun;
  (2) V8 function `services:activate` returned `ctx.services.db.port` = 18090,
  triggering real krun sandbox activation through the server-owned manager;
  (3) HTTP probe on 127.0.0.1:18090 returned BusyBox httpd response;
  (4) `sandbox_service_manager.snapshot_for_tenant(&tenant_id)` confirmed
  the "db" key was present (service bound and cached);
  (5) tenant deletion (`DELETE /api/v1/tenants/demo`) returned 204 No Content;
  (6) wait-for-condition confirmed port 18090 became unreachable and
  `snapshot_for_tenant` became empty after deletion;
  (7) post-teardown state: sandbox `db-01kp3ktd3gy7gjsbqwrxbaeant` manifest
  shows `status: stopped`, `shutdown_requested: true`, `last_exit_code: 137`;
  port 18090 released.
  Note: the exact test name requires the full module prefix
  `tests::convex_functions::runtime_queries::execution::services::` when using
  `--exact` with `cargo test -p neovex-server`.
  Verification:
  `cargo fmt --all --check` pass,
  `cargo check -p neovex-runtime -p neovex-sandbox -p neovex-server -p neovex` pass,
  exact command: `cargo test -p neovex-server
  'tests::convex_functions::runtime_queries::execution::services::convex_runtime_query_starts_real_krun_service_under_manager_and_tears_it_down'
  -- --ignored --exact --nocapture` pass.
  Env:
  `NEOVEX_KRUN_SMOKE_WORKDIR=/tmp/neovex-sandbox-smoke`,
  `NEOVEX_KRUN_SMOKE_RUNTIME=/usr/libexec/neovex/crun`,
  `NEOVEX_KRUN_SMOKE_CONMON=/usr/bin/conmon`,
  `NEOVEX_KRUN_SMOKE_BUILDAH=/usr/bin/buildah`,
  `NEOVEX_KRUN_SMOKE_ROOTFS=/tmp/neovex-sandbox-smoke-rootfs`,
  `NEOVEX_KRUN_SMOKE_M4_HOST_PORT=18090`,
  `NEOVEX_KRUN_SMOKE_M4_GUEST_PORT=8090`.
  State: `/tmp/neovex-sandbox-smoke/m4-manager-state/containers/db-01kp3ktd3gy7gjsbqwrxbaeant/manifest.json`.
  M4 is now done. M5 remains `todo`.
- 2026-04-13: Ran initial M5/SCP5 Linux-host verification on Debian 13 x86_64.
  Compose-serve smoke (`bash scripts/verify-microvm-m5-compose-serve-helper.sh`)
  passed (~9.5s): V8 `ctx.services.db.port = 18091`, HTTP on TSI port 18091,
  tenant deletion teardown. Recovery drill
  (`bash scripts/verify-microvm-m5-recovery-drill-helper.sh`) also reported
  success and wrote
  `/tmp/neovex-sandbox-smoke/m5-recovery-drill/summary.txt`.
- 2026-04-13: Post-review hardening found two durability gaps in the initial
  M5 closeout:
  - the bin smoke had widened public API surface by making
    `RuntimeServiceRegistry` public even though the test could assert through
    the already-public `SandboxCatalog` surface on `SandboxServiceManager`
  - the recovery helper could validate stale manifests/logs from the shared
    `${NEOVEX_KRUN_SMOKE_WORKDIR}` and could miss orphaned conmon/crun
    processes by grepping the host port rather than the sandbox identity
  The current code now keeps `RuntimeServiceRegistry` internal again, records
  the exact current-run project root/key in the compose-serve helper, clears
  the M5 control root before the smoke run, and makes the recovery helper
  validate exact manifest/log/sandbox-id paths from that run instead of using
  `find ... | head -1`. Because the hardened helper has not been rerun on
  Linux yet, M5 remains `in_progress` pending one more Linux-host verification
  pass before the closeout can be trusted.
- 2026-04-13: Re-verified M5 on Debian 13 x86_64 using hardened helpers from
  commit `81cf133`. Fixed one additional issue: compose-serve helper's grep for
  `M5_PROJECT_ROOT=` used a `^` anchor that failed because cargo's
  `--nocapture` test harness prefixes test output on the same line; changed to
  `grep -o 'M5_PROJECT_ROOT=[^ ]*'` to extract mid-line markers reliably.
  Commands run:
  1. `cargo fmt --all --check` — passed
  2. `cargo check -p neovex-sandbox -p neovex-server -p neovex-bin -p neovex` — passed
  3. `cargo test -p neovex-bin` — 39 passed, 1 ignored
  4. `bash scripts/verify-microvm-m5-compose-serve-helper.sh` — passed (~6.9s)
  5. `bash scripts/verify-microvm-m5-recovery-drill-helper.sh` — all checks passed
  Evidence:
  - project_key: `smoke-app-cf079a18bd54`
  - project_root: `/tmp/neovex-sandbox-smoke/m5-compose-control/services/projects/smoke-app-cf079a18bd54`
  - sandbox_id: `db-01kp3yamn6rb2dtwbk0wn1t8tz`
  - manifest.status: stopped, shutdown_requested: True, last_exit_code: 137
  - port.18091.released: ok, orphan.processes: ok (none)
  - logs.ctr.persists: ok (1/1), logs.oci.persists: ok (1/1)
  - project.layout: ok (key=smoke-app-cf079a18bd54)
  - Artifacts: compose-serve summary at
    `/tmp/neovex-sandbox-smoke/m5-compose-serve-verification/summary.txt`,
    recovery-drill summary at
    `/tmp/neovex-sandbox-smoke/m5-recovery-drill/summary.txt`
  M5 is now `done`.
- 2026-04-13: Post-closeout helper hardening removed the remaining cargo-output
  parsing seam from the M5 compose-backed smoke. The ignored Linux smoke in
  `crates/neovex-bin/src/main.rs` now writes machine-readable project metadata
  to `NEOVEX_KRUN_SMOKE_M5_METADATA_FILE` when requested, and
  `scripts/verify-microvm-m5-compose-serve-helper.sh` now reads
  `${log_root}/metadata.json` for `project_root` / `project_key` instead of
  scraping those values from test stdout. Local verification only:
  1. `cargo fmt --all --check` — passed
  2. `cargo check -p neovex-sandbox -p neovex-server -p neovex-bin -p neovex` — passed
  3. `cargo test -p neovex-bin service:: -- --nocapture` — passed
  4. `bash -n scripts/verify-microvm-m5-compose-serve-helper.sh` — passed
  5. `bash -n scripts/verify-microvm-m5-recovery-drill-helper.sh` — passed
  This did not change the recorded Debian 13 Linux evidence; it hardened the
  helper-to-test metadata handoff against cargo output formatting drift.
