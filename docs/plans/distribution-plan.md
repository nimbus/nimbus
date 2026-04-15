# Plan: Distribution — Packaging neovex for All Channels

Canonical plan for distributing neovex and its dependencies across all
target platforms and package channels.

---

## Status

- **Status:** `deferred`
- **Primary owner:** this plan
- **Activation gate:** met on 2026-04-13 when the microVM service baseline
  reached `done`; this plan remains deferred until packaging work is promoted
- **Related plans:**
  - `docs/reference/microvm-service-baseline.md` — current landed runtime and
    service-control baseline
  - `docs/plans/macos-machine-support-plan.md` — active execution plan for the
    macOS developer-machine architecture and implementation
  - `docs/plans/archive/vmm-infrastructure-plan.md` — historical VMM
    foundation execution record with Linux/macOS validation evidence
  - `docs/plans/install-script-plan.md` — execution plan for Channel 1
    install script (`curl | sh`)

## Control Plan Rules

Source of truth:
1. this plan's `Phase Status Ledger` and `Execution Log`
2. CI/CD pipeline configuration

---

## What We Ship

### Binaries

| Binary | Source | Size | Built by |
|--------|--------|------|----------|
| `neovex` | `agentstation/neovex` | ~60MB | Cargo (Rust + V8) |
| `neovex-crun` | upstream crun + build-time patch | ~2MB | autotools (C) |

### System dependencies — Linux (not shipped, installed from OS repos)

| Package | Debian/Ubuntu | Fedora/RHEL |
|---------|--------------|-------------|
| conmon | `apt install conmon` | `dnf install conmon` |
| buildah | `apt install buildah` | `dnf install buildah` |
| containers-common | Comes with buildah | Comes with buildah |
| libkrun | **Not in repos** — we package it | `dnf install libkrun` |
| libkrunfw | **Not in repos** — we package it | `dnf install libkrunfw` |
| catatonit | `apt install catatonit` | `dnf install catatonit` |
| passt | `apt install passt` | `dnf install passt` |
| uidmap | `apt install uidmap` | `dnf install shadow-utils` |
| fuse-overlayfs | `apt install fuse-overlayfs` | `dnf install fuse-overlayfs` |

### System dependencies — macOS (Homebrew)

On macOS, neovex runs inside a Linux machine VM (same model as Podman).
Only two host-side deps are needed — everything else runs inside the VM.

| Package | Install | What |
|---------|---------|------|
| krunkit | `brew tap slp/krunkit && brew install krunkit` | Machine VM (libkrun / Hypervisor.framework) |
| gvproxy | Bundled with neovex or from `containers/gvisor-tap-vsock` | Networking + port forwarding |

Do not assume Homebrew `podman` or the `podman-desktop` cask provide a
shell-visible `krunkit` binary. neovex should depend on `krunkit` directly so
`brew install neovex` produces a known-good macOS machine-VM dependency set.

Verified Homebrew packaging boundary on the current host:
- Homebrew `podman` `5.8.1` installs `podman-mac-helper`, `gvproxy`, and
  `vfkit`; the formula does not declare `krunkit`.
- Homebrew `podman-desktop` `1.26.2` installs the GUI app bundle; the cask does
  not declare `krunkit` as a Homebrew dependency.
- Therefore, if neovex chooses `krunkit` as its macOS machine provider, the
  neovex formula must depend on `krunkit` directly instead of inheriting that
  dependency from Podman packaging.
- This evidence is intentionally scoped to the Homebrew formula and cask,
  because Channel 4 is a Homebrew delivery plan. Do not treat it as proof
  about Podman's separate upstream macOS `.pkg` installer without checking
  that installer independently.

Verified upstream Podman installer boundary from source:
- `containers/podman` `v5.8.1` `contrib/pkginstaller/Makefile` downloads
  `gvproxy`, `vfkit`, and `krunkit` into the official macOS `.pkg` payload.
- `containers/podman` `v5.8.1` `pkg/machine/provider/platform_darwin.go`
  supports both `applehv` and `libkrun` on Apple Silicon, but falls back to
  `applehv` when no provider is configured.
- So the official Podman `.pkg` and the Homebrew formula have different
  packaging contracts. neovex should document the Homebrew contract we plan to
  ship, while still using Podman's upstream source as architecture guidance.

### Platform support

| Platform | How it runs | Service isolation | Supported |
|----------|------------|-------------------|-----------|
| Linux x86_64 (bare metal) | Native (KVM) | Hardware-isolated microVMs | **Yes** (primary) |
| Linux x86_64 (cloud VM) | Native (nested KVM) | Hardware-isolated microVMs | **Yes** |
| Linux aarch64 | Native (KVM) | Hardware-isolated microVMs | **Partial** (neovex-crun CI, machine-os CI) |
| macOS aarch64 (Apple Silicon, M1+) | Machine VM (krunkit) | Containers (same as Podman) | **Future** (dev) |
| macOS x86_64 (Intel) | Not supported | — | **No** |
| Windows | WSL2 | TBD | **Future** |

---

## Distribution Channels

### Channel 1: Install Script (Quick Start)

```bash
curl -fsSL https://neovex.dev/install.sh | sh
```

The script:
1. Detects OS (Debian/Ubuntu, Fedora/RHEL, macOS)
2. Detects architecture (x86_64, aarch64)
3. Adds neovex apt/dnf repo (or Homebrew tap)
4. Installs neovex + all dependencies via package manager
5. Checks `/dev/kvm` access
6. Prints getting-started instructions

**Implementation reference:**
- [rustup install script](https://github.com/rust-lang/rustup/blob/master/rustup-init.sh)
- [Docker install script](https://github.com/docker/docker-install/blob/master/install.sh)

### Channel 2: Debian/Ubuntu (.deb)

**Package: `neovex`**

```
Package: neovex
Version: 0.1.0
Architecture: amd64
Depends: neovex-crun, conmon, buildah, containers-common
Recommends: catatonit, passt, uidmap, fuse-overlayfs
Description: Reactive document database with microVM runtime
```

**Package: `neovex-crun`**

```
Package: neovex-crun
Version: 1.27+neovex1
Architecture: amd64
Depends: libkrun (>= 1.17), libkrunfw, libcap2, libseccomp2, libyajl2
Description: crun OCI runtime with krun TSI port mapping (patched for neovex)
```

Built from upstream crun release tarball + build-time patch (see
`docs/plans/archive/vmm-infrastructure-plan.md`). Installs to
`/usr/libexec/neovex/crun`. Does
NOT conflict with or replace the system `crun` — neovex invokes it via
`conmon -r /usr/libexec/neovex/crun`. System Podman/CRI-O continue using
the distro `crun` undisturbed.

Version format: `{upstream_version}+neovex{patch_revision}`. The `+` separator
follows Debian convention for local modifications. When upstream merges the
port mapping PR, `neovex-crun` is dropped and replaced by a dependency on the
system `crun` (>= the version that includes the patch).

**Package: `libkrun` (for Debian/Ubuntu where it's not in repos)**

```
Package: libkrun
Version: 1.17.4
Architecture: amd64
Depends: libc6
Description: Dynamic library for KVM-based process isolation
```

**Apt repository:**
```
deb [signed-by=/usr/share/keyrings/neovex.gpg] https://apt.neovex.dev stable main
```

**Build system:** GitHub Actions → build .deb → upload to apt repo (hosted
on GitHub Pages, Cloudflare R2, or Packagecloud).

**Implementation reference:**
- [goreleaser nfpm](https://github.com/goreleaser/nfpm) — build deb/rpm from
  YAML config, Go binary
- [cargo-deb](https://crates.io/crates/cargo-deb) — build .deb from Cargo
  metadata

### Channel 3: Fedora/RHEL (.rpm)

**Package: `neovex`**

```
Name: neovex
Version: 0.1.0
Requires: neovex-crun conmon buildah containers-common
Recommends: catatonit passt shadow-utils fuse-overlayfs
```

On Fedora, libkrun and libkrunfw are already in the repos. The
`neovex-crun` package installs to `/usr/libexec/neovex/crun` alongside
the system crun (does not replace it).

**COPR or custom repo:**
```
dnf copr enable agentstation/neovex
dnf install neovex
```

**Implementation reference:**
- [Fedora COPR](https://copr.fedorainfracloud.org/) — free RPM build service

### Channel 4: Homebrew + Machine VM (macOS)

On macOS, neovex runs inside a Linux VM ("neovex machine"), following the
same model as Podman. macOS does not have Linux namespaces, cgroups,
seccomp, or KVM — every major container tool solves this with a machine VM.

#### Architecture

```
macOS (Apple Silicon, M1+, macOS 14+)
  │
  └── neovex (macOS binary — thin CLI client)
        │
        ├── neovex machine init / start / stop
        │     └── krunkit (libkrun / Hypervisor.framework)
        │           ├── virtiofs (host ↔ guest file sharing)
        │           ├── virtio-net (guest networking via gvproxy)
        │           └── vsock devices (ready signal + first-boot ignition)
        │
        ├── gvproxy
        │     ├── guest networking + published localhost ports
        │     └── forwarded guest API/control socket
        │
        └── neovex serve (proxied to Linux guest via a host-local control channel)
              │
              └── Linux guest VM (Fedora CoreOS + neovex deps)
                    │
                    └── neovex serve (same Linux binary as production)
                          │
                          └── services run as containers (crun, same as Podman on macOS)
```

#### Architecture comparison

Rejected architecture for macOS:

```text
macOS host
  └── neovex CLI
        └── krunkit machine VM
              └── Linux guest
                    └── neovex
                          └── conmon -> crun(krun handler) -> microVM per service
```

Accepted architecture for macOS:

```text
macOS host
  └── neovex CLI
        └── krunkit machine VM
              └── Linux guest
                    └── neovex
                          └── conmon -> crun -> container per service
```

The difference is intentional:
- on macOS, the machine VM is the isolation boundary
- on Linux production, the service microVM is the isolation boundary
- `--nested` on a Podman-managed `krunkit` process is only a machine capability
  hint; it is not the architecture neovex should require on macOS

Inside the machine VM, services run as **standard Linux containers** — the
same way Podman runs containers on macOS today. The hardware-isolated
microVM layer (libkrun/KVM) is a Linux production feature, not a macOS dev
feature. The machine VM itself provides the isolation boundary from macOS.

The neovex server inside the VM is the **same binary** as on Linux
production. The only difference is that services use crun's standard
container mode (namespaces + cgroups) instead of the krun handler
(microVMs). The API surface is identical — `ctx.services.db.port` works
the same way.

#### Podman parity

neovex should mirror Podman's macOS architecture strictly:
- host-side binary stays thin and manages the machine VM
- the real Linux container toolchain lives inside the guest VM
- services run as standard Linux containers inside that guest
- per-service microVM isolation stays Linux-only

Implementation-reference split for Channel 4:
- use Podman core source as the canonical machine/runtime reference on macOS
- use Podman Desktop as a secondary reference for installer UX, dependency
  checks, and operator flows
- do not treat Podman Desktop's UI state as the authoritative machine-health
  signal; the underlying Podman machine backend remains the source of truth

This distinction matters because Podman's macOS docs and README describe
`podman machine` as a Linux VM where containers are run, while Podman Desktop
is a frontend that uses the `podman machine` backend on non-Linux operating
systems. Even when Podman enables nested virtualization for some `libkrun`
machines, that is a machine capability, not the normal container-execution
model we should target for neovex on macOS.

Source-backed guest-container note:
- `containers/podman-machine-os` `build.sh` builds the guest from
  `podman-image/Containerfile.COREOS`.
- `podman-image/build_common.sh` installs `crun`, `crun-wasm`, `podman`,
  `containers-common`, `containers-common-extra`, `netavark`, and
  `aardvark-dns`, and removes `runc`.
- That source strongly indicates Podman's macOS guest is configured for
  standard Linux container execution via `crun`, not for per-container `krun`
  microVM execution inside the guest.

#### Runtime directory policy

On macOS, the neovex machine manager should own a short runtime directory such
as `/tmp/neovex` for sockets, pid files, and transient logs.

Why:
- Darwin unix sockets have a 104-byte `sockaddr_un.sun_path` budget including
  the trailing NUL, which leaves a practical 103-character path-string limit
- the current Podman/libkrun repro on this host produced a derived
  `...-gvproxy.sock-krun.sock` path of 104 characters under the default
  `/var/folders/.../T/podman` root and failed with `InvalidAddress(ENAMETOOLONG)`
- the same path shape dropped to 60 characters under `/tmp/podman`, and the
  machine reaches the next boot stage without the socket-path panic

Important scope note:
- this short runtime dir policy fixes the socket-path startup blocker
- reusing a stale machine can still fail later in guest boot on this host
- a brand-new short-root machine does boot cleanly here, so reset/recreate
  semantics matter alongside the runtime-dir choice
- the repo now has a checked-in recreate helper for that stale-state case, and
  it repaired `neovex-libkrun-users-only` on this host under `/tmp/podman`

So Channel 4 should not inherit Darwin's default long `TMPDIR` subtree for the
machine runtime directory.

#### CLI taxonomy

Target command taxonomy for this channel:
- `neovex serve` starts or attaches to the neovex server process
- `neovex machine ...` owns machine-VM lifecycle on macOS
- reserve `neovex services ...` for future workload-management nouns if we ever
  need them, such as listing, inspecting, restarting, or tailing managed
  workloads
- do not use `neovex service` as the daemon-start command

Why this split:
- `serve` is a verb, which matches "start the server" and avoids overloading
  the word "service"
- `machine` is a managed resource, so a noun namespace is idiomatic and aligns
  with Podman and Docker Desktop concepts
- `service` would be ambiguous in neovex because the codebase already uses
  "service" for the core engine type and for tenant-facing workloads
- `services` is a better future namespace than `service` if we want commands
  like `neovex services list`, `neovex services inspect <name>`, or
  `neovex services logs <name>`
- `serve` is not redundant with `services`: one starts neovex itself, the other
  would manage workloads running under neovex

Current implementation note:
- today's binary is still a flag-driven server entrypoint rather than a
  shipped subcommand CLI
- this section defines the intended command surface when Channel 4 activates;
  do not read the examples here as proof that the subcommands already exist

#### Why krunkit

1. **Rust.** Same language as neovex. No Go dependency (unlike vfkit).
2. **libkrun.** Already in neovex's dependency chain for microVMs on Linux.
3. **Podman-aligned.** Podman's machine code supports both `applehv` and
   `libkrun` on Apple Silicon. Podman's upstream macOS `.pkg` installer
   bundles `krunkit`, but the Homebrew formula does not, so neovex can depend
   on `krunkit` directly instead of inheriting the Homebrew Podman formula's
   bundled provider choice.
4. **Full device support.** virtiofs, vsock, virtio-net, virtio-blk,
   RESTful lifecycle API.
5. **Same containers org.** Maintained alongside crun, buildah, Podman,
   libkrun. Apache-2.0.
6. **All Apple Silicon.** Works on M1, M2, M3, M4. Requires macOS 14+.

Provider-selection note:
- `krunkit` is the deliberate neovex provider choice for Channel 4.
- Podman's Darwin provider code still falls back to `applehv` when no provider
  is configured.
- So neovex is mirroring Podman's one-machine-VM architecture, not copying
  Podman's exact default-provider behavior.

#### Guest VM image

**Base:** Fedora bootc 42 (aarch64).
**Custom layer:** neovex + all Linux deps (neovex, neovex-crun, conmon,
buildah, libkrun, libkrunfw, catatonit, passt, fuse-overlayfs,
containers-common).
**Build:** `podman build` → `podman save --format oci-archive` →
`bootc-image-builder --type raw --rootfs ext4` → gzip.
**Distribution:** raw-disk OCI artifact at
`ghcr.io/agentstation/neovex-machine-os`, with `disktype=raw` manifest
selection for the bootable guest disk.
Provisioned via Ignition (SSH keys, neovex systemd unit, virtiofs mounts).

**Repo split:** The guest image source now lives in
`agentstation/neovex-machine-os`, mirroring Podman's
`containers/podman` + `containers/podman-machine-os` model. The host repo
(`agentstation/neovex`) owns the binary/CLI/server release and uses the
machine-image repo in two reusable-workflow phases on `v*` releases: a
publish-free contract build before the host release, then a publish/release
call after the host release succeeds. That keeps the GitHub Actions shape
modern by removing the extra cross-repo `workflow_dispatch` hop while still
preserving machine-image release ownership in `agentstation/neovex-machine-os`.
Because reusable workflows still execute in the caller's context, the
publish/release call must pass GitHub App credentials (`release_app_id` plus
`MACHINE_OS_RELEASE_APP_PRIVATE_KEY`) so the called workflow can mint its own
installation token, publish GHCR artifacts, and create a GitHub Release in
the separate machine-image repo. This keeps attestation ownership split cleanly:
`agentstation/neovex` attests the host binary/CLI/server, while
`agentstation/neovex-machine-os` attests and releases the guest image in its
own repo. Consumers still reference the image only by GHCR reference, so the
split stays transparent to operators.

The Podman machine-os source is also a useful negative reference here: its
guest image build script installs plain container tooling (`crun`, `podman`,
`netavark`, `aardvark-dns`) rather than a guest-side `krun` runtime path. Our
macOS guest should follow that same standard-container pattern.

#### Communication

- **API/control channel:** host-local forwarded socket — the macOS host
  should talk to the guest Neovex API through a host-local control socket or
  equivalent forwarded channel. Podman's current source uses `gvproxy` plus
  SSH-backed guest-socket forwarding as the reference model; do not describe
  the default API path as raw `vsock` forwarding.
- **File sharing:** virtiofs — developer project directories shared into
  the VM (default: home directory, same as Podman).
- **Port forwarding:** gvproxy forwards ports from macOS localhost to the
  guest VM. Same as Podman's port forwarding model on macOS.

#### Homebrew formula

Dependency contract:
- `neovex` depends on `krunkit` directly.
- Do not rely on a preexisting Homebrew `podman` or `podman-desktop`
  installation to make `krunkit` available on `PATH`.
- `podman-desktop` may still be useful as a GUI, but it is not neovex's
  dependency manager for the machine provider.
- `podman-mac-helper` stays optional. It only binds `/var/run/docker.sock`
  to a Podman-managed socket for Docker-compatible clients such as Compose,
  Testcontainers, or the Docker CLI.
- neovex should talk to its own machine socket or vsock proxy directly. Do
  not make the machine lifecycle or API path depend on `podman-mac-helper`.
- Installing `podman-mac-helper` can take over the global Docker socket path,
  so treat it as an explicit compatibility mode instead of a default neovex
  requirement.

```ruby
class Neovex < Formula
  desc "Reactive document database with microVM runtime"
  homepage "https://neovex.dev"
  url "https://github.com/agentstation/neovex/releases/download/v0.1.0/neovex-darwin-arm64.tar.gz"
  sha256 "..."

  depends_on "krunkit"  # Machine VM (libkrun / Hypervisor.framework)
  depends_on :macos
  depends_on arch: :arm64  # Apple Silicon only

  def install
    bin.install "neovex"
    libexec.install "gvproxy"  # Bundled networking
  end
end
```

```bash
brew tap agentstation/neovex
brew install neovex
# Installs: neovex CLI, krunkit, gvproxy

neovex machine init   # Downloads guest image (~800MB, one-time)
neovex serve          # Auto-starts machine if needed, proxies to VM
```

#### Developer experience

```bash
neovex machine init     # one-time: download image, configure
neovex machine start    # boot the VM (~3-5s)
neovex machine stop     # graceful shutdown (via krunkit REST API)
neovex machine rm       # delete VM and disk image
neovex machine ssh      # debug: SSH into the VM
neovex machine status   # show VM state, resource usage
```

`neovex serve` on macOS auto-starts the machine if not running.

#### Optional Docker compatibility

If a developer wants third-party Docker clients on macOS to talk to the
machine VM through the default `/var/run/docker.sock` path, `podman-mac-helper`
or an equivalent `DOCKER_HOST` export can provide that compatibility layer.
This is optional for neovex itself. The neovex CLI should work without taking
ownership of the system Docker socket.

#### Evaluated alternatives

- **vfkit (Virtualization.framework)** — Go binary, bundled with Podman
  Homebrew formula. Has Rosetta 2 for x86_64 containers which krunkit
  lacks. Consider if x86_64 image compat becomes important.
- **Apple Containerization (`apple/container`, WWDC 2025)** — Apple's
  open-source container runtime. Each container gets its own VM. Sub-second
  starts. Requires macOS 26+. Too new for v1, track for long-term.

**Implementation reference:**
- [containers/krunkit](https://github.com/containers/krunkit)
- [containers/gvisor-tap-vsock](https://github.com/containers/gvisor-tap-vsock)
- [containers/podman/pkg/machine/](https://github.com/containers/podman/tree/main/pkg/machine)
- [Homebrew formulae docs](https://docs.brew.sh/Formula-Cookbook)

### Channel 5: Binary Tarball (Manual Install)

```bash
# Download
curl -L -o neovex.tar.gz \
  https://github.com/agentstation/neovex/releases/download/v0.1.0/neovex-linux-amd64.tar.gz

# Extract
tar xzf neovex.tar.gz
sudo mv neovex /usr/local/bin/
sudo mkdir -p /usr/libexec/neovex
sudo mv neovex-crun /usr/libexec/neovex/crun

# Install deps manually
sudo apt install conmon buildah catatonit passt
# For Debian: also install libkrun and libkrunfw from neovex apt repo
```

The tarball includes: `neovex` + `neovex-crun` + install instructions.

### Channel 6: Container Image (for CI/CD tooling)

```dockerfile
FROM debian:13-slim
RUN apt-get update && apt-get install -y \
    conmon buildah catatonit passt uidmap fuse-overlayfs
COPY neovex /usr/local/bin/
COPY neovex-crun /usr/libexec/neovex/crun
# Note: This container must run with --privileged and /dev/kvm access
```

**Use case:** CI/CD pipelines that need to run neovex. The container
provides all dependencies. Must run with `--privileged` and
`--device /dev/kvm` for KVM access.

```bash
docker run --privileged --device /dev/kvm \
  ghcr.io/agentstation/neovex:latest serve
```

### Channel 7: Cloud VM Images (Production)

Pre-baked VM images with everything installed.

**AWS AMI:**
- Based on Debian 13 or Amazon Linux 2023
- neovex + all deps pre-installed
- KVM enabled (use `.metal` or nested-virt-capable instance types)
- Published to AWS Marketplace or as community AMI

**GCP Image:**
- Based on Debian 13
- Nested virtualization enabled
- Published to GCP Compute Image library

**Build system:** Packer (HashiCorp) for reproducible image builds.

```hcl
# packer.hcl
source "amazon-ebs" "neovex" {
  ami_name      = "neovex-{{timestamp}}"
  instance_type = "c5.metal"
  source_ami    = "ami-debian-13-..."
}

build {
  sources = ["source.amazon-ebs.neovex"]
  provisioner "shell" {
    inline = [
      "curl -fsSL https://neovex.dev/install.sh | sh",
    ]
  }
}
```

**Implementation reference:**
- [Packer](https://www.packer.io/) — VM image builder

---

## Phase Plan

### Phase D1: CI Build Pipeline

**Goal:** Automated builds of neovex and neovex-crun for Linux x86_64 and
aarch64.

**Scope:**
- GitHub Actions workflow: build neovex (cargo build --release)
- GitHub Actions workflow: build neovex-crun (clone upstream crun at pinned
  tag inside Fedora 43 container with `libkrun-devel` from repos, apply
  patch, autotools `--with-libkrun`)
- Matrix: amd64 (`ubuntu-latest`) + arm64 (`ubuntu-24.04-arm`)
- GitHub Releases: upload binaries as release assets with attestation
- Tarball (Channel 5): neovex + crun + README

**neovex-crun CI status:** `done` — `.github/workflows/neovex-crun.yml`
implements verify → build (matrix amd64+arm64) → publish (on `crun/v*` tags
with `actions/attest@v4` provenance). Build runs inside `fedora:43`
containers where `libkrun-devel` is available from repos (no source build
needed). Triggered on push to main (path-filtered), `crun/v*` tags, and
`workflow_dispatch`.

**neovex binary CI status:** `todo`

**Acceptance criteria:**
- `git tag crun/v1.27 && git push --tags` triggers neovex-crun build and
  publishes `neovex-crun-linux-amd64` + `neovex-crun-linux-arm64` with
  checksums and attestation
- `git tag v0.1.0 && git push --tags` triggers neovex build
- Release assets include: `neovex-linux-amd64.tar.gz`
- Tarball includes both binaries + install instructions

### Phase D2: Apt Repository (Debian/Ubuntu)

**Goal:** `apt install neovex` works on Debian 13 and Ubuntu 24.04+.

**Scope:**
- Build .deb packages: neovex, neovex-crun, libkrun, libkrunfw
- Host apt repository (GitHub Pages or Cloudflare R2)
- GPG-sign packages
- Install script (Channel 1) adds the repo and installs

**Acceptance criteria:**
- Fresh Debian 13 VM: `curl ... | sh && neovex serve` works
- Dependencies automatically pulled (conmon, buildah, etc.)

### Phase D3: Fedora/COPR (Fedora/RHEL)

**Goal:** `dnf install neovex` works on Fedora 40+.

**Scope:**
- Build .rpm packages: neovex, neovex-crun
- libkrun/libkrunfw already in Fedora repos — just depend on them
- Publish via COPR (free RPM build service)

**Acceptance criteria:**
- Fresh Fedora 40 VM: `dnf copr enable ... && dnf install neovex` works

### Phase D4: Homebrew + Machine VM (macOS)

macOS is a development environment, not production. The neovex server runs
inside a Linux machine VM (same model as Podman). See Channel 4 above.

#### Phase D4a: Homebrew formula + krunkit integration

**Goal:** `brew install neovex` works. `neovex machine start` boots a VM.

**Scope:**
- Build neovex macOS CLI for `aarch64-apple-darwin`
- Create Homebrew formula depending on krunkit; bundle gvproxy
- `neovex machine init/start/stop`: spawn krunkit with virtiofs,
  virtio-net/gvproxy, and any required machine-level ready/bootstrap devices
- Graceful shutdown via krunkit REST API

**Acceptance criteria:**
- `brew install neovex` installs CLI + krunkit + gvproxy on M1+ Mac
- `neovex machine start` boots a Fedora CoreOS VM
- SSH into the VM works; virtiofs mounts work

#### Phase D4b: Guest VM image

**Goal:** Custom machine image with neovex + all deps pre-installed.

**Scope:**
- Build Fedora bootc 42 image with neovex + all Linux deps
- Publish as raw-disk OCI artifact at `ghcr.io/agentstation/neovex-machine-os`
- Publish immutable version tags first, then attach moving aliases such as
  `stable` and optionally `latest`
- `neovex machine init` pulls and caches by digest
- Ignition provisioning: SSH keys, neovex systemd unit, virtiofs mounts
- Dedicated Linux ARM64 GitHub Actions lane in
  `agentstation/neovex-machine-os`; macOS consumes the published artifact, it
  does not build the guest image locally
- **Cross-repo release contract:** neovex `v*` releases call the reusable
  machine-os workflow with the same `v*` tag so the default host image
  reference `ghcr.io/agentstation/neovex-machine-os:v{CARGO_PKG_VERSION}`
  always resolves to a matching guest image. Standalone machine-os `v*` tags
  must embed the same neovex version they publish.

**Acceptance criteria:**
- `neovex machine init` downloads the custom image
- the published machine image has a versioned GHCR reference plus recorded
  digest/provenance
- `neovex serve` runs inside the VM with all deps available
- a neovex `v0.1.0` release triggers a matching
  `ghcr.io/agentstation/neovex-machine-os:v0.1.0` guest-image publish

#### Phase D4c: API forwarding + port forwarding

**Goal:** `neovex serve` on macOS is transparent — same as Linux.

**Scope:**
- host-local control socket/channel for the guest Neovex API
- `neovex serve` on macOS auto-starts the machine and proxies through that
  control channel
- gvproxy port forwarding: services accessible from macOS localhost
- machine-level readiness, guest Neovex readiness, and guest service readiness
  remain distinct probe stages

**Acceptance criteria:**
- `neovex serve` on macOS starts machine and proxies transparently
- WebSocket subscriptions work through the macOS guest-control proxy
- postgres:16 service accessible at `localhost:5432` from macOS

### Phase D5: Cloud VM Images

**Goal:** Pre-baked VM images for AWS and GCP.

**Scope:**
- Packer templates for AWS AMI and GCP Image
- Based on Debian 13
- All deps pre-installed
- KVM verified working

**Acceptance criteria:**
- Launch AMI on c5.metal → `neovex serve` works immediately
- Launch GCP VM with nested virt → `neovex serve` works immediately

---

## Phase Status Ledger

| Phase | Status | Hard deps | Notes |
|-------|--------|-----------|-------|
| D1: CI build pipeline | `in_progress` | Neovex compiles | neovex-crun done (amd64+arm64), neovex binary todo |
| D2: Apt repo (Debian/Ubuntu) | `todo` | D1 | cargo-deb or nfpm |
| D3: COPR (Fedora) | `todo` | D1 | COPR build service |
| D4a: Homebrew + krunkit | `todo` | D1 | Apple Silicon, macOS 14+ |
| D4b: Guest VM image | `in_progress` | D4a | split to `agentstation/neovex-machine-os`; host `v*` release now calls the reusable image workflow |
| D4c: API + port forwarding | `todo` | D4b | host-local control channel, gvproxy, layered probes |
| D5: Cloud VM images | `todo` | D2 or D3 | Packer |

---

## Execution Log

| Date | Phase | Status | Notes | Verification | Next |
|------|-------|--------|-------|--------------|------|
| 2026-04-11 | D4a prep | `documented` | Tightened the macOS installer and architecture claims to match Podman's published source, upstream packaging source, and the locally installed Homebrew artifacts. Podman's own docs and README say `podman machine` is a Linux VM where containers are run and that Podman Desktop uses the `podman machine` backend on non-Linux systems. Upstream `containers/podman` `v5.8.1` `contrib/pkginstaller/Makefile` downloads `gvproxy`, `vfkit`, and `krunkit` for the official macOS `.pkg`, while local Homebrew inspection shows a different packaging reality: the `podman` formula installs `podman-mac-helper`, `gvproxy`, and `vfkit`, and the `podman-desktop` cask installs the app bundle only. Channel 4 therefore stays scoped to the Homebrew contract we plan to ship, while still using Podman's upstream machine architecture as the reference model. | Podman docs: `https://docs.podman.io/en/latest/markdown/podman-machine-start.1.html`; Podman README: `https://raw.githubusercontent.com/containers/podman/main/README.md`; Podman release notes: `https://github.com/containers/podman/releases`; upstream source: `https://github.com/containers/podman/blob/v5.8.1/contrib/pkginstaller/Makefile`; local packaging inspection via `brew cat podman`; `brew info podman`; `ls -l /opt/homebrew/Cellar/podman/5.8.1/bin /opt/homebrew/Cellar/podman/5.8.1/libexec/podman`; `brew cat --cask podman-desktop`; `brew info --cask podman-desktop`; `find /Applications/Podman\\ Desktop.app -iname '*krunkit*' -o -iname '*vfkit*' -o -iname '*gvproxy*'`; `cargo fmt --all --check` | Keep the macOS plan Podman-aligned: thin host control shim, Linux guest with standard containers, and explicit `krunkit` ownership in the neovex Homebrew formula |
| 2026-04-11 | D4a prep | `documented` | Added direct guest-image evidence from Podman's machine-os sources. `containers/podman-machine-os` `build.sh` builds the guest from `podman-image/Containerfile.COREOS`, and `podman-image/build_common.sh` installs `crun`, `crun-wasm`, `podman`, `containers-common`, `containers-common-extra`, `netavark`, and `aardvark-dns` while removing `runc`. That source-backed package set supports our Channel 4 stance: on macOS, the guest should run standard Linux containers via `crun`, not nested per-service `krun` microVMs. | upstream source: `https://github.com/containers/podman-machine-os/blob/main/build.sh`; `https://github.com/containers/podman-machine-os/blob/main/podman-image/Containerfile.COREOS`; `https://github.com/containers/podman-machine-os/blob/main/podman-image/build_common.sh`; `cargo fmt --all --check` | Keep the macOS guest model aligned with Podman's machine-os packaging, then continue D4a validation on provider behavior and guest readiness rather than inventing a guest-side `krun` path |
| 2026-04-11 | D4a prep | `documented` | Recorded the macOS Docker-compatibility boundary explicitly. On the current host, `/var/run/docker.sock` points to `/Users/jack/.docker/run/docker.sock`, not to a Podman machine socket, which matches Podman's own docs: `podman-mac-helper` is the optional system helper that binds the default Docker socket path for Docker-compatible clients. That helper is useful for Compose/Testcontainers-style tooling, but it is not part of neovex's machine-VM boot contract and should not be a hard D4a dependency. | local socket inspection via `ls -l /var/run/docker.sock` and `ls -l /Users/jack/.docker/run/docker.sock`; Podman docs at `podman-machine-start(1)` and Podman Desktop's Docker compatibility docs; `cargo fmt --all --check` | Keep `podman-mac-helper` documented as optional compatibility tooling only, and continue D4a validation on the machine-provider and guest-readiness path |
| 2026-04-11 | D4a prep | `documented` | Clarified the macOS machine-provider dependency contract before D4a activation. Corrected the Homebrew tap name to `slp/krunkit`, and recorded that on the current macOS host neither Homebrew `podman` `5.8.1` nor the `podman-desktop` cask yielded a shell-visible `krunkit` binary. `krunkit` had to be installed explicitly, which is why the neovex Homebrew channel should own `depends_on "krunkit"` instead of assuming Podman/Desktop satisfies it. | local host verification: `which krunkit` before install failed; `brew info --cask podman-desktop`; `brew cat --cask podman-desktop`; `find /Applications/Podman\\ Desktop.app -iname '*krunkit*'`; `brew tap slp/krunkit`; `brew install krunkit`; `which krunkit`; `krunkit --version`; `brew info krunkit` | Keep `krunkit` as an explicit D4a dependency, then validate `neovex machine init/start` against the installed binary when D4a activates |
| 2026-04-11 | D4a prep | `documented` | Validated the first real Podman-managed libkrun machine attempt on the current Apple Silicon host. Homebrew Podman `5.8.1` still needed `CONTAINERS_MACHINE_PROVIDER=libkrun` to target the libkrun provider from the CLI, and the stale `applehv` machine had to be removed before the new libkrun machine could start. The fresh `neovex-libkrun-validation` machine launched both `gvproxy` and `/opt/homebrew/bin/krunkit`, but the Fedora CoreOS guest never reached SSH or Podman API readiness on this `Mac14,5` / `M2 Max` / macOS `15.7.2` host; the serial log showed repeated `(udev-worker)` soft lockups while the generated `krunkit` command was running with `--nested`. This means the D4a dependency contract is correct, but the current Podman-managed libkrun guest path on this host is still a blocker rather than a ready validation lane. | `CONTAINERS_MACHINE_PROVIDER=libkrun podman info --debug`; `podman machine list --all-providers --format json`; `CONTAINERS_MACHINE_PROVIDER=libkrun podman machine init --cpus 4 --memory 4096 --disk-size 60 neovex-libkrun-validation`; `CONTAINERS_MACHINE_PROVIDER=libkrun podman machine start neovex-libkrun-validation`; `CONTAINERS_MACHINE_PROVIDER=libkrun podman machine inspect neovex-libkrun-validation`; `ps -ax | rg 'krunkit|gvproxy|neovex-libkrun-validation'`; `CONTAINERS_MACHINE_PROVIDER=libkrun podman info`; `CONTAINERS_MACHINE_PROVIDER=libkrun podman machine ssh neovex-libkrun-validation 'uname -a ...'`; `tail -n 120 /var/folders/kw/d608x5pn4cq73rz78ztl92cw0000gn/T/podman/neovex-libkrun-validation.log`; `podman machine rm -f neovex-vmm-validation`; `podman machine list --all-providers --format json` | Keep `krunkit` as the explicit D4a dependency, but do not treat the current Podman-managed libkrun guest on this host as a proven machine recipe until the guest reaches stable SSH and API readiness |
| 2026-04-12 | D4a prep | `documented` | Added an explicit architecture comparison and CLI taxonomy so Channel 4 no longer leaves room for "nested microVMs on macOS" ambiguity. The plan now shows the rejected layout ("machine VM plus nested microVM per service") beside the accepted Podman-aligned layout ("machine VM plus standard containers per service"), and it records the intended command split: `neovex serve` remains the server-start verb, `neovex machine ...` owns machine lifecycle, and `service` stays reserved for future workload nouns instead of daemon startup. It also notes that the current binary is still flag-driven, so this is target command design rather than shipped subcommand behavior. | docs review of Channel 4 architecture and CLI sections; `cargo fmt --all --check` | Keep D4a Podman-aligned and implement any future subcommand CLI work against this recorded taxonomy rather than ad hoc naming |
| 2026-04-12 | D4a prep | `documented` | Tightened the CLI taxonomy further so the plan answers the startup-versus-workload question directly. Channel 4 now says `neovex serve` starts neovex itself, `neovex machine ...` manages the macOS Linux VM, and a future workload-management surface should prefer plural `neovex services ...` commands such as `list`, `inspect`, or `logs`. That keeps `serve` and `services` semantically different instead of treating `service` as both a daemon-start verb and a workload noun. | docs review of Channel 4 CLI taxonomy; `cargo fmt --all --check` | Keep future macOS CLI work aligned with the recorded split: `serve` for neovex startup, `machine` for VM lifecycle, `services` for any later workload inventory surface |
| 2026-04-12 | D4a prep | `documented` | Added two more source-backed macOS implementation rules to Channel 4. First, the plan now records the source-reference split explicitly: Podman core machine code is the canonical reference for helper resolution, socket wiring, and ready-state behavior, while Podman Desktop is a secondary reference for installer UX and operator flow. Second, Channel 4 now owns a short-runtime-dir policy for macOS instead of inheriting Darwin's long default `TMPDIR` subtree. The current Podman/libkrun repro on this host produced a derived `...-gvproxy.sock-krun.sock` path of 104 characters under `/var/folders/.../T/podman`, which matches the `InvalidAddress(ENAMETOOLONG)` failure we captured, while the same layout under `/tmp/podman` dropped to 60 characters and cleared that socket-path startup blocker. A later live readiness bundle showed that this is necessary but not sufficient on the current host: the short-root machine still failed with SSH handshake resets and guest Ignition/emergency-mode failure. | upstream source: `https://github.com/containers/podman/blob/main/pkg/machine/libkrun/stubber.go`; `https://github.com/containers/podman/blob/main/pkg/machine/apple/apple.go`; `https://github.com/podman-desktop/podman-desktop/blob/main/extensions/podman/packages/extension/src/helpers/krunkit-helper.ts`; `https://github.com/podman-desktop/podman-desktop/blob/main/website/docs/podman/creating-a-podman-machine.md`; local helper evidence via `bash scripts/check-podman-machine-socket-paths.sh --machine neovex-libkrun-users-only --tmp-root /var/folders/kw/d608x5pn4cq73rz78ztl92cw0000gn/T/podman`; `bash scripts/check-podman-machine-socket-paths.sh --machine neovex-libkrun-users-only --tmp-root /tmp/podman`; `TMPDIR=/tmp bash scripts/validate-podman-machine-readiness.sh --machine neovex-libkrun-users-only --connection neovex-libkrun-users-only --provider libkrun --tmp-root /tmp/podman --output-dir /tmp/neovex-libkrun-users-only-readiness`; `cargo fmt --all --check` | Keep D4a Podman-aligned, require a short runtime dir by default, and move the next macOS investigation toward guest-image / Ignition readiness rather than more socket-path tuning |
| 2026-04-12 | D4a prep | `documented` | Ran the comparison experiment that narrows the remaining macOS risk considerably. A brand-new disposable short-root machine, `neovex-libkrun-sr-fresh`, created with the same `libkrun` provider and the same `/Users` virtiofs mount, reached full readiness on this host: `podman machine start` exited successfully, the readiness bundle reported both connection-targeted `podman info --debug` and `podman machine ssh` as `ok`, and the guest log reached `sshd.service`, `ready.service`, and successful Ignition application. That means Channel 4's short-runtime-dir rule is not just theoretical, and it also means the older `neovex-libkrun-users-only` failure is most likely stale/corrupted machine state rather than a universal short-root libkrun problem on this Mac. | local host validation via `TMPDIR=/tmp CONTAINERS_MACHINE_PROVIDER=libkrun podman machine init --cpus 2 --memory 2048 --disk-size 20 -v /Users:/Users neovex-libkrun-sr-fresh`; `TMPDIR=/tmp CONTAINERS_MACHINE_PROVIDER=libkrun podman machine start neovex-libkrun-sr-fresh`; `TMPDIR=/tmp bash scripts/validate-podman-machine-readiness.sh --machine neovex-libkrun-sr-fresh --connection neovex-libkrun-sr-fresh --provider libkrun --tmp-root /tmp/podman --output-dir /tmp/neovex-libkrun-sr-fresh-readiness`; `tail -n 60 /tmp/podman/neovex-libkrun-sr-fresh.log`; `TMPDIR=/tmp CONTAINERS_MACHINE_PROVIDER=libkrun podman machine stop neovex-libkrun-sr-fresh`; `TMPDIR=/tmp CONTAINERS_MACHINE_PROVIDER=libkrun podman machine rm -f neovex-libkrun-sr-fresh`; `cargo fmt --all --check` | Keep D4a aligned with the proven fresh-machine recipe: short runtime dir by default, and a clean recreate/reset path when an existing machine shows stale-state boot corruption |
| 2026-04-12 | D4a prep | `documented` | Promoted that recreate/reset guidance from narrative advice into a checked-in operator path and validated it against the real long-lived machine on this Mac. The repo now owns `scripts/recreate-podman-machine.sh`, `scripts/verify-podman-machine-recreate-helper.sh`, `make recreate-podman-machine`, and `make verify-podman-machine-recreate-helper`, so Channel 4 has a durable Podman-aligned repair entrypoint instead of ad hoc shell history. A live run at `/tmp/neovex-libkrun-users-only-recreate` first preserved the stale-state failure under `pre-diagnostics/summary.txt` (`podman info --debug` failed and the API/gvproxy sockets were missing), then removed and recreated `neovex-libkrun-users-only` under the same `/tmp/podman` short-root contract and returned `result ready info=ok ssh=ok` in `readiness/summary.txt`. | `bash -n scripts/recreate-podman-machine.sh`; `bash -n scripts/verify-podman-machine-recreate-helper.sh`; `bash scripts/verify-podman-machine-recreate-helper.sh`; `make verify-podman-machine-recreate-helper`; `bash scripts/recreate-podman-machine.sh --machine neovex-libkrun-users-only --connection neovex-libkrun-users-only --provider libkrun --tmp-root /tmp/podman --output-dir /tmp/neovex-libkrun-users-only-recreate --cpus 2 --memory 2048 --disk-size 20 --volume /Users:/Users`; `sed -n '1,220p' /tmp/neovex-libkrun-users-only-recreate/summary.txt`; `sed -n '1,220p' /tmp/neovex-libkrun-users-only-recreate/pre-diagnostics/summary.txt`; `sed -n '1,220p' /tmp/neovex-libkrun-users-only-recreate/readiness/summary.txt`; `cargo fmt --all --check` | Keep Channel 4 aligned with the proven short-root recreate path: prefer the checked-in helper when a macOS machine wedges, and treat the recreated `users-only` result as stronger local evidence than the earlier stale-state failure |
| 2026-04-14 | D4b | `done` | Machine-os CI workflow (`.github/workflows/neovex-machine-os.yml`) migrated from self-hosted ARM64 runners to GitHub-hosted `ubuntu-24.04-arm`. Pipeline switched from rpm-ostree + custom-coreos-disk-images to `podman save --format oci-archive` + `bootc-image-builder`. Base image changed from Fedora CoreOS to `fedora-bootc:42`. Publishes raw-disk OCI artifact to GHCR on `machine-os/v*` tags with `actions/attest@v4` provenance. Consumer-side attestation verification added to `manager.rs`. | CI run green on `ubuntu-24.04-arm`; `actions/attest@v4` provenance attached; machine manager queries GitHub Attestations API after SHA256 verification | D4b acceptance criteria met: versioned GHCR reference, digest/provenance, dedicated ARM64 build lane |
| 2026-04-14 | D1 | `in_progress` | neovex-crun CI workflow (`.github/workflows/neovex-crun.yml`) implemented and verified green. Three jobs: verify (patch syntax + help entrypoints on `ubuntu-latest`), build (matrix amd64 on `ubuntu-latest` + arm64 on `ubuntu-24.04-arm`, inside `fedora:43` containers with `libkrun-devel` from repos), publish (on `crun/v*` tags with `actions/attest@v4` provenance + GitHub Release). Fixed existing `verify-neovex-crun-patch.yml` CRUN_VERSION from 1.22 to 1.27. Linux aarch64 platform support partially unlocked via both machine-os and neovex-crun CI. | CI run `24417536553` green: verify success, build amd64 success, build arm64 success, publish skipped (no tag) | neovex binary CI workflow still needed to complete D1; then `crun/v*` tag push to validate publish job end-to-end |
| 2026-04-15 | D4b | `documented` | The machine-image repo split has now landed. The guest image source and workflow moved out of the neovex monorepo into `agentstation/neovex-machine-os`, and the host `v*` release workflow now calls the external reusable build workflow with the same version tag. Follow-on hardening then converted the repo boundary into an explicit artifact contract: standalone machine-os `v*` tags now resolve the matching Neovex release tag instead of `latest`, the packaged OCI artifact carries source/attestation/version annotations, and the host machine manager reads those annotations before falling back to the older dual-repo attestation lookup. Durable conclusion: the host repo should treat machine-image production as an external dependency with a versioned, machine-readable cross-repo release contract, not as a future monorepo refactor. | repo review of `agentstation/neovex/.github/workflows/release.yml`; repo review of `agentstation/neovex-machine-os/.github/workflows/build.yml`; repo review of `agentstation/neovex-machine-os/scripts/package-oci.sh`; focused `cargo check -p neovex-bin`; `bash /Users/jack/src/github.com/agentstation/neovex-machine-os/scripts/verify-oci-layout-helper.sh`; `cargo fmt --all --check` | Keep host docs version-pinned (`v{CARGO_PKG_VERSION}`), keep publishing explicit OCI metadata, and continue removing host-side fallbacks once all live machine images carry the new annotations |
