# Plan: VMM Infrastructure — crun Fork + System Dependencies

Canonical plan for the VMM infrastructure that enables neovex to run OCI/Docker
images in hardware-isolated microVMs. Follows the Podman distribution model:
neovex is a single binary with system package dependencies.

This plan produces the VMM foundation that `microvm-runtime-plan.md` builds on.

---

## Status

- **Status:** `deferred`
- **Primary owner:** this plan
- **Activation gate:** promote when microVM runtime work begins
- **Related plans:**
  - `microvm-runtime-plan.md` — builds OCI management, lifecycle, engine
    integration on top of this plan's VMM layer
  - `distribution-plan.md` — packages neovex + dependencies for each channel

## Control Plan Rules

Source of truth:
1. the current git worktrees (neovex, agentstation/crun)
2. this plan's `Phase Status Ledger` and `Execution Log`
3. `docs/research/libkrun-evaluation.md`
4. `docs/research/vm-lifecycle-probes.md`

### Status model

- `todo` / `in_progress` / `blocked` / `done` / `deferred`

---

## Architecture: The Podman Model

neovex follows the same process model and dependency pattern as Podman.
neovex is a single binary. The VMM stack is system packages.

### Process model

```
neovex serve
  │
  └── conmon (per VM, long-lived, survives neovex restart)
        │
        ├── stdout/stderr → log files (persistent)
        ├── exit status → exit file
        ├── attach socket for interactive access
        │
        └── agentstation-crun run --bundle path id
              │
              ├── namespaces (PID, mount, user)
              ├── cgroups (memory, CPU limits)
              ├── seccomp (syscall filtering)
              │
              └── krun handler (with TSI port mapping patch)
                    ├── krun_set_root() — virtiofs rootfs
                    ├── krun_set_port_map() — TSI port mapping
                    └── krun_start_enter() → _exit()
                          └── Guest VM
                                ├── catatonit/tini (PID 1)
                                └── workload (postgres, etc.)
```

### Dependency comparison with Podman

```
Podman:                             neovex:
  conmon               ✓             conmon
  crun | runc          ✓             agentstation-crun (forked, +10 lines)
  containers-common    ✓             containers-common
  netavark             ✗ (TSI)       —
  catatonit|tini       ✓             catatonit | tini | dumb-init
  buildah              ✓             buildah
  passt                ✓             passt
  uidmap               ✓             uidmap
  fuse-overlayfs       ✓             fuse-overlayfs
  libkrun              ✓             libkrun
  libkrunfw            ✓             libkrunfw
```

neovex drops only netavark (TSI replaces container networking) and adds
libkrun/libkrunfw (not needed by Podman's default runc mode).

### Why no vsock

vsock was evaluated and deferred. Reasons:

1. **Guest apps don't speak AF_VSOCK.** postgres, redis, nginx use TCP. vsock
   requires a bridge process inside the VM — more complexity, not less.
2. **TSI handles service traffic.** V8 connects to guest services via
   TCP through TSI-mapped ports. Standard, works with every application.
3. **No performance benefit for DB/API workloads.** Transport overhead
   (microseconds) is negligible vs query latency (milliseconds).
4. **Security proxying doesn't need vsock.** A TCP proxy in neovex (v2)
   provides tenant isolation, audit logging, and rate limiting over TSI.
5. **Graceful shutdown via conmon.** SIGTERM → grace → SIGKILL, same as
   Podman. No custom guest agent needed.

**Future:** vsock may be added for dedicated control channels (exec, live
debugging, filesystem access) as part of `wasi-agent-capabilities-plan.md`.
If added, neovex-init and vsock support in crun would be revisited then.

### Communication model

```
v1 (this plan):
  V8 → TCP localhost:mapped_port → TSI → guest service
  Shutdown: conmon → SIGTERM → grace → SIGKILL (same as Podman)
  Health: TCP connect to TSI-mapped port

v2 (future, microvm-runtime-plan Phase M4+):
  V8 → neovex proxy (policy, audit, rate limit) → TCP → TSI → guest
  Same transport, neovex is now in the data path for observability
```

---

## Fork: `agentstation/crun`

### What and why

The upstream crun krun handler does NOT call `krun_set_port_map()`. Without
TSI port mapping, V8 isolates cannot connect to guest services. This is the
only change needed.

**Upstream:** `containers/crun` (latest release)
**License:** GPL-2.0 (binary), LGPL-2.1 (libcrun library)
**Patch size:** ~10 lines in one file
**File:** `src/libcrun/handlers/krun.c`

### The patch

```c
// Add after krun_set_vm_config() call in libkrun_exec():

// Read TSI port map from OCI annotation
// Format: "5432:15432,6379:16379" (guest:host)
const char *port_map = find_annotation(def, "krun.neovex.tsi.port_map");
if (port_map) {
    krun_set_port_map(ctx_id, port_map);
}
```

That's it. ~10 lines of C.

### Upstream strategy

TSI port mapping via OCI annotations is a generally useful feature — not
neovex-specific. Submit a PR to `containers/crun` proposing it. If accepted,
the fork becomes unnecessary.

**PR title:** "krun: add TSI port mapping via OCI annotation"
**Rationale:** Currently krun_set_port_map() is never called. Users who want
TSI port forwarding (exposing guest services to the host) have no mechanism
to configure it through the OCI spec.

### Build and distribution

The forked crun is built the standard way (autotools) and packaged as
`agentstation-crun` (deb/rpm). See `distribution-plan.md` for details.

```bash
# Build
cd agentstation/crun
./autogen.sh
./configure
make
# Result: crun binary with krun handler + TSI port mapping
```

---

## System Dependencies

### Required

| Package | What | Available on Debian 13 | Available on Fedora 40+ |
|---------|------|----------------------|------------------------|
| `agentstation-crun` | Forked crun with TSI | We package it | We package it |
| `libkrun` | VMM library | **Not in repos** — we package it | `dnf install libkrun` ✓ |
| `libkrunfw` | Guest kernel | **Not in repos** — we package it | `dnf install libkrunfw` ✓ |
| `conmon` | Process monitor | `apt install conmon` ✓ | `dnf install conmon` ✓ |
| `buildah` | Image build/pull/mount | `apt install buildah` ✓ | `dnf install buildah` ✓ |
| `containers-common` | Registry auth/config | Comes with buildah ✓ | Comes with buildah ✓ |

### Recommended

| Package | What | Why |
|---------|------|-----|
| `catatonit` \| `tini` \| `dumb-init` | Guest PID 1 init | Signal forwarding, zombie reaping |
| `passt` | Rootless networking | Non-root neovex operation |
| `uidmap` | User namespace mapping | Non-root neovex operation |
| `fuse-overlayfs` | Rootless overlay storage | Layer dedup for buildah |

### Runtime requirements

| Requirement | How to enable |
|------------|---------------|
| `/dev/kvm` | Enable VT-x in BIOS (bare metal) or nested virt (cloud VM) |
| KVM group membership | `sudo usermod -aG kvm $USER` |

---

## How neovex uses buildah (instead of custom OCI code)

buildah replaces the entire OCI image management layer that was previously
planned as custom Rust code (oci-client, layer flattening, whiteout handling,
layer caching, overlay assembly).

### Image pull

```bash
# neovex shells out to buildah to pull and mount an image:
buildah from --name neovex-postgres docker://postgres:16
ROOTFS=$(buildah mount neovex-postgres)
# $ROOTFS is now the merged rootfs directory (all layers applied)
# Pass to crun: krun_set_root($ROOTFS) via virtiofs
```

### Dockerfile build

```bash
buildah bud -t neovex-myapp -f ./Dockerfile .
buildah from --name neovex-myapp localhost/neovex-myapp
ROOTFS=$(buildah mount neovex-myapp)
```

### Cleanup

```bash
buildah umount neovex-postgres
buildah rm neovex-postgres
```

### What this eliminates from neovex's Rust code

| Previously planned (custom Rust) | Now handled by buildah |
|----------------------------------|----------------------|
| `oci-client` crate for registry pull | `buildah from docker://...` |
| Layer flattening with whiteout handling | `containers-storage` (via buildah) |
| Content-addressable layer cache | `containers-storage` |
| Layer deduplication across images | `containers-storage` + overlayfs |
| Registry authentication | `containers-common` (registries.conf) |
| `.krun_config.json` generation | Still in neovex (reads OCI image config via `buildah inspect`) |
| OCI bundle `config.json` generation | Still in neovex |

---

## Phase Plan

### Phase V1: Fork crun

**Goal:** Create `agentstation/crun` with TSI port mapping in the krun handler.

**Scope:**
1. Fork `containers/crun` at latest release
2. Add `krun_set_port_map()` call driven by OCI annotation (~10 lines)
3. Build and test on Debian 13 and Fedora
4. Submit upstream PR to `containers/crun`

**Acceptance criteria:**
- `crun run` with a krun-configured OCI bundle boots a VM with TSI port mapping
- Guest service (e.g., `nc -l -p 8080`) is accessible from host via mapped port
- Upstream PR submitted

### Phase V2: System Integration

**Goal:** Verify neovex can spawn conmon → crun → VM using system packages.

**Scope:**
1. Install dependencies: conmon, buildah, libkrun, libkrunfw, catatonit
2. Write a test script that manually creates an OCI bundle, spawns conmon
   → crun, boots a VM, connects via TSI
3. Verify: log files, exit status, process tree, port mapping
4. Document the manual flow for implementation agents

**Implementation reference:**
- Podman's container creation flow:
  `containers/podman/pkg/specgen/` → `containers/podman/libpod/`
- conmon invocation:
  `containers/podman/libpod/oci_conmon_common_linux.go`
- crun invocation by conmon:
  `containers/conmon/src/runtime_args.c`

**Acceptance criteria:**
- Manual end-to-end: boot alpine in a krun VM via conmon → crun, connect
  via TSI, stop via conmon signal, verify logs and exit status
- Process tree matches expected model (neovex → conmon → crun is not running
  after VM boots — crun exits, conmon monitors the VM process)

**Note:** In the crun+krun model, crun does NOT exit after starting the VM.
`krun_start_enter()` blocks, so the crun process IS the VMM. conmon monitors
the crun process (which is the VMM). When the VM exits, `_exit()` kills the
crun process, conmon detects it and writes the exit file.

### Phase V3: neovex Wrapper

**Goal:** neovex can spawn and manage VMs programmatically.

**Scope:**
1. `crates/neovex-vmm/src/conmon.rs`: Spawn conmon as subprocess, read
   sync pipe, manage PID files, read exit files, connect to attach socket
2. `crates/neovex-vmm/src/bundle.rs`: Generate OCI bundle for crun
   (config.json with krun handler, TSI annotations)
3. `crates/neovex-vmm/src/buildah.rs`: Shell out to buildah for image
   pull/build/mount/inspect
4. `crates/neovex-vmm/src/vm.rs`: VmHandle wrapping conmon management

**Acceptance criteria:**
- `neovex` can programmatically boot a postgres:16 VM, connect via TSI,
  run a query, stop the VM, verify exit status
- VM survives `neovex` process restart (conmon keeps it alive)
- Logs are persisted to disk via conmon

---

## Phase Status Ledger

| Phase | Status | Hard deps | Notes |
|-------|--------|-----------|-------|
| V1: Fork crun | `todo` | none | ~10 lines of C |
| V2: System integration | `todo` | V1, system packages installed | Manual testing |
| V3: neovex wrapper | `todo` | V2 | Rust code in neovex-vmm crate |

---

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| libkrun not packaged for Debian/Ubuntu | High | Medium | Package it ourselves (see distribution-plan.md) |
| TSI port mapping PR rejected by crun upstream | Low | Low | Maintain fork (~10 lines, trivial rebase) |
| conmon API changes between versions | Low | Low | conmon has a stable interface (used by Podman, CRI-O) |
| buildah CLI output format changes | Medium | Medium | Pin buildah version, use --json output |
| Rootless operation issues with KVM | Medium | Medium | Document KVM permissions, test rootless flow |

---

## Source Code References

| File | Repo | What to study |
|------|------|---------------|
| `src/libcrun/handlers/krun.c` | containers/crun | krun handler — the only file to patch |
| `include/libkrun.h` | containers/libkrun | `krun_set_port_map()` signature |
| `src/runtime_args.c` | containers/conmon | How conmon invokes crun |
| `libpod/oci_conmon_common_linux.go` | containers/podman | How Podman invokes conmon |
| `src/tini.c` | krallin/tini | PID 1 reference (signal forwarding) |

---

## Execution Log

_Empty — no work started._
