# Plan: VMM Infrastructure — Patched crun + System Dependencies

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
  - `docs/plans/archive/runtime-sandbox-architecture-plan.md` — completed
    baseline that owns the canonical `neovex-sandbox` crate naming and the
    server-facing sandbox seam this plan must consume for any Rust
    implementation work
  - `microvm-runtime-plan.md` — builds OCI management, lifecycle, engine
    integration on top of this plan's VMM layer
  - `distribution-plan.md` — packages neovex + dependencies for each channel

## Control Plan Rules

Source of truth:
1. the current git worktree
2. this plan's `Phase Status Ledger` and `Execution Log`
3. `docs/research/libkrun-evaluation.md`
4. `docs/research/vm-lifecycle-probes.md`
5. `docs/research/gvisor-isolation-tier.md`

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
  └── conmon -r /usr/libexec/neovex/crun (per VM, long-lived, survives neovex restart)
        │
        ├── stdout/stderr → log files (persistent)
        ├── exit status → exit file
        ├── attach socket for interactive access
        │
        └── /usr/libexec/neovex/crun run --bundle path id
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

conmon's `-r` flag accepts an arbitrary path to any OCI runtime binary. neovex
passes `-r /usr/libexec/neovex/crun` so the forked crun is used without
replacing the system crun. System Podman continues to use the distro crun
undisturbed.

### Dependency comparison with Podman

```
Podman:                             neovex:
  conmon               ✓             conmon
  crun | runc          ✓             neovex-crun (patched crun, +10 lines, at /usr/libexec/neovex/crun)
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

### Evaluated alternatives (see research docs)

- **conmon-rs** — Rust rewrite (v0.8.0). Deferred: not production-default, per-pod
  model doesn't fit per-VM use, Podman integration incomplete
  (containers/conmon-rs#1127 open since 2023). Revisit if it becomes the
  Podman/CRI-O default.
- **gVisor** — User-space kernel, no KVM needed. Deferred: syscall compat gaps,
  I/O overhead, no hardware isolation boundary. See
  `docs/research/gvisor-isolation-tier.md`.
- **CRIU for snapshot/restore** — Cannot checkpoint KVM-based VMM processes (no
  KVM fd support). Every VMM with snapshot/restore implements it natively. See
  `docs/research/libkrun-evaluation.md` § "CRIU Cannot Solve the
  Snapshot/Restore Gap".
- **Warm pool mitigation** — Pre-boot idle VMs, aggressive rootfs caching,
  optimized guest kernel. Documented in `docs/research/libkrun-evaluation.md`
  § "Warm pool as a practical mitigation".

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

### Architectural evolution path

The Podman subprocess model (conmon → crun → buildah) is correct for v1's
long-running service VMs. CRI-O, containerd shims, and Podman all use this
pattern in production. Every microVM-at-scale system (Fly.io 2M+ VMs,
AWS Lambda, CodeSandbox, Gitpod Flex) eventually moved to direct VMM API
integration — but they all started with subprocess orchestration and
graduated when density/latency demands required it.

If neovex evolves toward high-density ephemeral VMs, the migration path is:
subprocess model → helper binary calling libkrun API directly (see
`docs/research/libkrun-evaluation.md`). The helper binary pattern is
architecturally compatible with the conmon process model — conmon monitors
the helper the same way it monitors crun.

### Rust crate target for Phase V3

When this plan graduates from manual infrastructure work into Rust integration,
the canonical crate target is `neovex-sandbox`, not `neovex-vmm`.

The public seam should stay generic:

- `SandboxBackend`
- `SandboxSpec`
- `SandboxHandle`
- published-endpoint / port-projection types

The first backend-specific implementation path should live under an internal
module such as `crates/neovex-sandbox/src/backends/krun/`, which may own the
current OCI/buildah + conmon + patched-crun + libkrun stack without turning
those implementation details into public product nouns.

---

## crun Patch: TSI Port Mapping

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
const char *port_map = find_annotation(def, "krun.port_map");
if (port_map) {
    krun_set_port_map(ctx_id, port_map);
}
```

That's it. ~10 lines of C.

The annotation name `krun.port_map` follows the convention established by
crun PR #1950 (Jan 2026), which added `krun.cpus`, `krun.ram_mib`, and
`krun.variant`. Using the same `krun.*` namespace keeps the OCI annotations
consistent with upstream conventions.

### Build-time patch, not a fork

A full GitHub fork is overkill for ~10 lines of C. neovex uses the standard
distro pattern: store a patch file, apply it to the upstream source at build
time. This is how Debian, Fedora, Homebrew, and Alpine handle minimal
customizations to upstream packages. No separate fork repo exists.

### End-to-end: patch → build → distribute → install

**Step 1: Patch file lives in this repo**

```
agentstation/neovex (this repo):
  patches/
    crun/
      0001-krun-add-tsi-port-mapping-via-oci-annotation.patch
```

The patch file is a standard unified diff generated from the upstream PR or
via `git format-patch`. It is checked into the neovex repo alongside the
Rust source code.

**Step 2: CI builds the patched crun binary**

A GitHub Actions workflow (defined in `distribution-plan.md` Phase D1) runs
on each neovex release tag:

```bash
# .github/workflows/build-neovex-crun.yml (conceptual)
CRUN_VERSION=1.22
curl -L -o crun-$CRUN_VERSION.tar.gz \
  https://github.com/containers/crun/archive/refs/tags/$CRUN_VERSION.tar.gz
tar xzf crun-$CRUN_VERSION.tar.gz
cd crun-$CRUN_VERSION

# Apply the neovex patch
patch -p1 < $GITHUB_WORKSPACE/patches/crun/0001-krun-add-tsi-port-mapping-via-oci-annotation.patch

# Build (requires: autoconf, automake, libkrun-dev, libseccomp-dev, etc.)
./autogen.sh
./configure --with-libkrun
make

# Output: crun binary with krun handler + TSI port mapping
```

The CI runner needs C build tools and libkrun-dev headers. The workflow pins
`CRUN_VERSION` to a known-good upstream release.

**Step 3: CI packages the binary per distribution channel**

The same CI workflow (or downstream jobs) produces packages:

| Channel | What CI produces | How patch is applied |
|---------|-----------------|---------------------|
| Binary tarball | `neovex-linux-amd64.tar.gz` containing `neovex` + `crun` | CI applies patch, ships binary |
| Debian (deb) | `neovex-crun_1.22+neovex1_amd64.deb` | `debian/patches/series` + quilt (standard) |
| Fedora (rpm) | `neovex-crun-1.22-1.neovex1.x86_64.rpm` | `Patch0:` in spec, `%autosetup -p1` |
| Homebrew | Formula with `patch :DATA` block | Homebrew applies patch before `make` |
| Container image | `ghcr.io/agentstation/neovex:latest` | `RUN patch -p1 < ...` in Dockerfile |

Each format has a native mechanism for applying patches — the patch file is
the same, only the build harness differs.

**Step 4: User installs neovex, gets patched crun automatically**

```bash
# Debian/Ubuntu
curl -fsSL https://neovex.dev/install.sh | sh
# install.sh adds apt repo, then:
# apt install neovex  (depends on neovex-crun, conmon, buildah, ...)
# neovex-crun installs to /usr/libexec/neovex/crun

# Fedora
dnf copr enable agentstation/neovex
dnf install neovex
# neovex-crun installs to /usr/libexec/neovex/crun

# Manual tarball
tar xzf neovex-linux-amd64.tar.gz
sudo mv neovex /usr/local/bin/
sudo mkdir -p /usr/libexec/neovex
sudo mv crun /usr/libexec/neovex/crun
```

The user never interacts with the patch. They install `neovex`, which depends
on `neovex-crun`, which is a pre-built binary at `/usr/libexec/neovex/crun`.
neovex invokes it via `conmon -r /usr/libexec/neovex/crun`.

**If upstream independently adds port mapping support:** Delete the patch
file, stop building `neovex-crun`, change the `neovex` package to depend on
system `crun` (>= the version with port mapping). The
`/usr/libexec/neovex/` path is no longer needed — neovex uses system crun
directly.

### Why not a full GitHub fork

| | Full fork | Build-time patch |
|---|-----------|-----------------|
| Maintenance | Rebase entire repo on each upstream release | Verify 10-line patch applies cleanly |
| Signal | "Major divergence" — misleading for 10 lines | "Minimal delta" — accurate |
| Staleness | Fork drifts, looks abandoned if not synced | Patch is obviously temporary |
| GPL compliance | Source = fork repo | Source = upstream tarball + patch file |
| Drop when upstream merges | Delete fork repo, update packaging | Delete patch file, update packaging |

### Upstream exit path

We are not submitting an upstream PR — the change is too small to justify the
overhead of upstream engagement (review cycles, API discussions, maintenance
commitments). The build-time patch is the plan, not a fallback.

If upstream independently adds `krun_set_port_map()` support via OCI
annotations (likely, given PR #1950 established the pattern), we drop the
patch and depend on system crun directly.

### Patch update process

When upstream crun releases a new version:

1. Attempt `patch -p1 --dry-run` against the new release
2. If it applies cleanly → update `CRUN_VERSION`, rebuild, done
3. If it conflicts → manually resolve (~10 lines, usually trivial), regenerate
   patch file
4. If upstream added native port mapping support → delete the patch file,
   depend on system crun directly

### GPL-2.0 compliance

crun is GPL-2.0. Distributing a patched binary requires providing the
complete corresponding source. For each distribution channel:

- **deb/rpm packages:** The source package (`.dsc` + `.orig.tar.gz` +
  `.debian.tar.xz`, or SRPM) contains the upstream tarball + patch file +
  build instructions. Standard and sufficient.
- **Homebrew:** The formula references the upstream URL and includes the patch
  inline. Anyone who has the formula can rebuild from source.
- **Binary tarball:** Include a `SOURCE.md` pointing to the upstream release
  URL and the patch file location in the neovex repo.

### Package naming

The patched crun is packaged as `neovex-crun`:

- **Binary name:** `crun` (it IS crun, just patched)
- **Install path:** `/usr/libexec/neovex/crun` (private to neovex)
- **Package name:** `neovex-crun` (deb/rpm — makes the relationship clear)
- **No Conflicts/Replaces/Provides:** Does not touch the system `crun`.
  Podman, CRI-O, and any other container tools continue using the distro crun.

The `neovex-crun` name follows the convention of scoping a patched build to
the project that needs it, similar to how Fedora has `crun-krun` as a separate
build of crun with libkrun support.

---

## System Dependencies

### Required

| Package | What | Available on Debian 13 | Available on Fedora 40+ |
|---------|------|----------------------|------------------------|
| `neovex-crun` | Patched crun with TSI port mapping | We package it | We package it |
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

### Phase V1: Patch crun

**Goal:** Add TSI port mapping to crun's krun handler via build-time patch.

**Scope:**
1. Create patch file at
   `patches/crun/0001-krun-add-tsi-port-mapping-via-oci-annotation.patch`
2. Build patched crun and install to `/usr/libexec/neovex/crun`
3. Test on Debian 13 and Fedora

**Acceptance criteria:**
- `/usr/libexec/neovex/crun run` with a krun-configured OCI bundle boots a VM with TSI port mapping
- Guest service (e.g., `nc -l -p 8080`) is accessible from host via mapped port
- System crun is unaffected (Podman still works with distro crun)

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

### Phase V3: `neovex-sandbox` krun backend

**Goal:** neovex can spawn and manage VMs programmatically.

**Scope:**
1. `docs/plans/archive/runtime-sandbox-architecture-plan.md` `RS4` is complete so the Rust wrapper
   lands on the canonical sandbox seam rather than inventing a second public
   lifecycle surface.
2. `crates/neovex-sandbox/src/backends/krun/conmon.rs`: Spawn conmon with
   `-r /usr/libexec/neovex/crun` as subprocess, read sync pipe, manage
   PID files, read exit files, connect to attach socket
3. `crates/neovex-sandbox/src/backends/krun/bundle.rs`: Generate OCI bundle for crun
   (config.json with krun handler, `krun.port_map` annotation)
4. `crates/neovex-sandbox/src/backends/krun/buildah.rs`: Shell out to buildah for image
   pull/build/mount/inspect
5. `crates/neovex-sandbox/src/backends/krun/vm.rs`: backend-local VM handle
   wrapping conmon management
6. `crates/neovex-sandbox/src/lib.rs`: expose the first generic
   `SandboxBackend` / `SandboxHandle` seam needed by the server integration

**Acceptance criteria:**
- `neovex` can programmatically boot a postgres:16 VM, connect via TSI,
  run a query, stop the VM, verify exit status
- VM survives `neovex` process restart (conmon keeps it alive)
- Logs are persisted to disk via conmon
- System crun/Podman remain functional (neovex uses private crun path)

---

## Phase Status Ledger

| Phase | Status | Hard deps | Notes |
|-------|--------|-----------|-------|
| V1: Patch crun | `todo` | none | ~10 line build-time patch |
| V2: System integration | `todo` | V1, system packages installed | Manual testing |
| V3: `neovex-sandbox` krun backend | `todo` | V2, `docs/plans/archive/runtime-sandbox-architecture-plan.md` RS4 | Rust code in `neovex-sandbox` under a backend-owned `krun` module |

---

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| libkrun not packaged for Debian/Ubuntu | High | Medium | Package it ourselves (see distribution-plan.md) |
| crun krun.c churn causes patch conflicts | Medium | Low | krun.c has ~30 commits/year, but the patch is ~10 lines in one function. Verify `patch --dry-run` on each upstream release. Manual resolution is trivial when needed. |
| conmon API changes between versions | Low | Low | conmon has a stable interface (used by Podman, CRI-O) |
| buildah CLI output format changes | Medium | Medium | Pin buildah version, use --json output |
| Rootless operation issues with KVM | Medium | Medium | Document KVM permissions, test rootless flow |
| No snapshot/restore in libkrun | High | Low (for v1) | Long-running service VMs tolerate ~100ms cold boot. Warm pool and rootfs caching available if latency matters. See `docs/research/libkrun-evaluation.md` for full analysis. |
| Subprocess model limits future density | Low | Medium | Standard for v1 (CRI-O, Podman do the same). See "Architectural evolution path" above for the migration path to direct libkrun API. |

---

## Source Code References

| File | Repo | What to study |
|------|------|---------------|
| `src/libcrun/handlers/krun.c` | containers/crun | krun handler — the only file to patch |
| PR #1950 (Jan 2026) | containers/crun | Reference: `krun.cpus`, `krun.ram_mib` annotations use same `find_annotation()` pattern |
| `include/libkrun.h` | containers/libkrun | `krun_set_port_map()` signature |
| `src/runtime_args.c` | containers/conmon | How conmon invokes crun (via `-r` flag) |
| `libpod/oci_conmon_common_linux.go` | containers/podman | How Podman invokes conmon with `-r` runtime path |
| `src/tini.c` | krallin/tini | PID 1 reference (signal forwarding) |

---

## Execution Log

_Empty — no work started._
