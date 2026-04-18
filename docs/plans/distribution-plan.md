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
  - `docs/reference/macos-machine-flow.md` — current macOS developer-machine
    contract reference
  - `docs/plans/archive/macos-machine-support-plan.md` — completed macOS
    execution record with exact closeout evidence for Channel 4
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
  neovex Homebrew package must depend on `krunkit` directly instead of inheriting that
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
- So the official Podman `.pkg` and the Homebrew cask/formula surfaces have different
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
   bundles `krunkit`, but the Homebrew Podman formula does not, so neovex can depend
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

**Current macOS v1 contract:** use Podman's published machine image directly,
by pinned immutable reference owned by the host `neovex` release:

- base image: `quay.io/podman/machine-os@sha256:...`
- selection rule: provider-specific OCI artifact selection (`disktype=applehv`
  on the current macOS krunkit path), not a floating tag and not the older
  generic `disktype=raw` assumption
- convergence owner: `neovex machine start`, which caches the machine image,
  caches the matching Linux guest `neovex` binary, boots or rebuilds from the
  pinned image, hash-syncs `/usr/local/bin/neovex`, repairs guest socket
  activation, and validates the forwarded machine API before reporting success
- provisioning scope: narrow Ignition only (SSH keys, guest units, virtiofs
  mounts, readiness wiring)

**Future supply-side track:** `agentstation/neovex-machine-os` remains the
later Neovex-owned image pipeline once it preserves the same Podman-aligned
FCOS/ignition/libkrun semantics. That repo split still mirrors Podman's
`containers/podman` + `containers/podman-machine-os` ownership model, but it
is not the current shipped macOS dependency contract.

The Podman machine-os source remains the canonical implementation reference for
the guest package shape: standard container tooling (`crun`, `conmon`,
`netavark`, `aardvark-dns`) rather than a guest-side `krun` runtime path.
Neovex's current macOS guest should stay aligned with that same
standard-container pattern.

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

#### Homebrew cask

Dependency contract:
- `neovex` owns `krunkit` as an explicit Homebrew dependency on macOS.
- `neovex` bundles `gvproxy` inside the macOS release archive under
  `libexec/gvproxy`, following Podman's pkg-installer pattern instead of
  treating Homebrew `podman` as a transitive dependency manager.
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
 cask "neovex" do
  name "neovex"
  desc "Reactive document database with microVM runtime"
  homepage "https://neovex.dev"
  version "0.1.0"

  binary "neovex"

  on_macos do
    depends_on arch: :arm64
    depends_on macos: ">= :sonoma"
    depends_on formula: "slp/krunkit/krunkit"

    on_arm do
      url "https://github.com/agentstation/neovex/releases/download/v#{version}/neovex_darwin_arm64.tar.gz"
      sha256 "..."
    end
  end
end
```

```bash
brew install agentstation/tap/neovex
# Installs: neovex CLI, krunkit, gvproxy

neovex machine init   # One-time: record the default machine contract
neovex serve          # Auto-starts that initialized machine if needed
```

#### Developer experience

```bash
neovex machine init     # one-time: record image/resources/SSH contract
neovex machine start    # optional explicit boot (~3-5s)
neovex machine stop     # graceful shutdown (via krunkit REST API)
neovex machine rm       # delete VM and disk image
neovex machine ssh      # debug: SSH into the VM
neovex machine status   # show VM state, resource usage
```

`neovex serve` on macOS auto-starts the initialized machine if not running.

#### Optional Docker compatibility

If a developer wants third-party Docker clients on macOS to talk to the
machine VM through the default `/var/run/docker.sock` path, `podman-mac-helper`
or an equivalent `DOCKER_HOST` export can provide that compatibility layer.
This is optional for neovex itself. The neovex CLI should work without taking
ownership of the system Docker socket.

#### Evaluated alternatives

- **vfkit (Virtualization.framework)** — Go binary, bundled with Podman
  Homebrew formula and pkg installer. Has Rosetta 2 for x86_64 containers which krunkit
  lacks. Consider if x86_64 image compat becomes important.
- **Apple Containerization (`apple/container`, WWDC 2025)** — Apple's
  open-source container runtime. Each container gets its own VM. Sub-second
  starts. Requires macOS 26+. Too new for v1, track for long-term.

**Implementation reference:**
- [containers/krunkit](https://github.com/containers/krunkit)
- [containers/gvisor-tap-vsock](https://github.com/containers/gvisor-tap-vsock)
- [containers/podman/pkg/machine/](https://github.com/containers/podman/tree/main/pkg/machine)
- [Homebrew cask docs](https://docs.brew.sh/Cask-Cookbook)

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

**neovex binary CI status:** `done` — `.github/workflows/release.yml`
verifies the tag/version contract, builds and publishes Neovex release assets
for Linux `x86_64` + `arm64`, macOS `arm64`, and Windows `x86_64`, attaches
provenance/checksums, dispatches the matching machine-os publish workflow, and
updates the Homebrew cask on tagged releases.

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
- Shared package-build foundation now exists in-repo:
  `scripts/build-linux-release-packages.sh`,
  `scripts/verify-build-linux-release-packages-helper.sh`, and
  `.github/workflows/linux-packages.yml` render and build candidate `.deb`
  artifacts for `neovex` and `neovex-crun` from released binaries
- Shared static apt-repo builder now exists in-repo:
  `scripts/build-apt-repository.sh`,
  `scripts/verify-build-apt-repository-helper.sh`, and
  `.github/workflows/apt-repo.yml` build a multi-arch apt repository tree
  with `Packages`, `Release`, `InRelease`, detached signatures, and exported
  public keyring material from those `.deb` artifacts; the same manual
  workflow can optionally upload and deploy that static bundle through GitHub
  Pages
- Shared Linux distribution release contract now exists in-repo:
  `packaging/linux-distribution-contract.env` plus
  `.github/workflows/linux-distribution-release.yml` mirror each published
  Neovex GitHub release into the Linux package/repo lanes using that single
  checked-in `neovex-crun`/channel contract instead of requiring ad hoc
  operator inputs
- Final Debian/Ubuntu channel still needs the hosted apt repository layer:
  final custom-domain publication for that signed static repo bundle
- Resolve Debian/Ubuntu ownership for `libkrun` / `libkrunfw` before claiming
  `apt install neovex` as a supported path
- Host apt repository (GitHub Pages or Cloudflare R2)
- GPG-sign packages
- Install script (Channel 1) adds the repo and installs

**Acceptance criteria:**
- Fresh Debian 13 VM: `curl ... | sh && neovex serve` works
- Dependencies automatically pulled (conmon, buildah, etc.)

### Phase D3: Fedora/COPR (Fedora/RHEL)

**Goal:** `dnf install neovex` works on Fedora 40+.

**Scope:**
- Shared package-build foundation now exists in-repo:
  `scripts/build-linux-release-packages.sh`,
  `scripts/verify-build-linux-release-packages-helper.sh`, and
  `.github/workflows/linux-packages.yml` render and build candidate `.rpm`
  artifacts for `neovex` and `neovex-crun` from released binaries
- Shared Fedora/COPR source-package bridge now exists in-repo:
  `scripts/build-fedora-release-srpms.sh`,
  `scripts/verify-build-fedora-release-srpms-helper.sh`, and
  `.github/workflows/copr-srpms.yml` wrap those same released binaries into
  deterministic source bundles and `.src.rpm` artifacts suitable for direct
  `copr-cli build ... <path-to-srpm>` submission
- Shared Linux distribution release contract now exists in-repo:
  `packaging/linux-distribution-contract.env` plus
  `.github/workflows/linux-distribution-release.yml` mirror each published
  Neovex GitHub release into the Debian/Fedora packaging workflows from the
  same released assets instead of maintaining a separate distro-build stack
- libkrun/libkrunfw already in Fedora repos — just depend on them
- Final Fedora channel still needs the live COPR project/publication contract,
  `dnf copr enable ...` install docs, and first real repo proof
- Publish via COPR (free RPM build service)

**Acceptance criteria:**
- Fresh Fedora 40 VM: `dnf copr enable ... && dnf install neovex` works

### Phase D4: Homebrew + Machine VM (macOS)

macOS is a development environment, not production. Neovex follows Podman's
one-machine-VM model for service execution, but the authoritative Neovex
server/runtime/storage loop stays on the macOS host. See Channel 4 above.

#### Phase D4a: Homebrew cask + krunkit integration

**Goal:** `brew install agentstation/tap/neovex` works. `neovex machine start`
boots a VM.

**Scope:**
- Build neovex macOS CLI for `aarch64-apple-darwin`
- Create Homebrew cask for Apple Silicon depending on `slp/krunkit/krunkit`;
  bundle `gvproxy` in the macOS release archive under `libexec/gvproxy`
- `neovex machine init/start/stop`: spawn krunkit with virtiofs,
  virtio-net/gvproxy, and any required machine-level ready/bootstrap devices
- Graceful shutdown via krunkit REST API

**Acceptance criteria:**
- `brew install agentstation/tap/neovex` installs the CLI on Apple Silicon
  macOS, owns `slp/krunkit/krunkit` explicitly, and ships bundled
  `libexec/gvproxy`
- `neovex machine start` boots a Fedora CoreOS VM
- SSH into the VM works; virtiofs mounts work

#### Phase D4b: Current machine-image contract

**Goal:** Ship the current macOS machine-image contract intentionally and keep
future image ownership separate.

**Scope:**
- Current macOS v1 contract uses Podman's published machine image directly at
  an immutable `quay.io/podman/machine-os@sha256:...` reference owned by the
  host `neovex` release
- `neovex machine start` is the primary convergence path:
  cache missing machine-image and guest-binary artifacts, rebuild boot
  artifacts when the recorded base image drifts, hash-sync the guest
  `/usr/local/bin/neovex`, and validate the forwarded machine API before
  reporting success
- Ignition stays machine-specific and version-agnostic: SSH keys, writable
  Neovex dirs, guest units, virtiofs mounts, readiness wiring
- explicit `neovex machine os apply` / `neovex machine os upgrade` surfaces
  remain host-managed rollout controls rather than ad hoc guest mutation
- a Neovex-owned FCOS-derived image in `agentstation/neovex-machine-os`
  remains the later ownership/supply-side track once it preserves the same
  Podman-aligned FCOS/ignition/libkrun semantics

**Acceptance criteria:**
- `neovex machine init` records the pinned Podman digest instead of a floating
  tag
- `neovex machine start` can repopulate a clean machine root from the pinned
  image and a matching guest Linux `neovex` asset
- the macOS recovery drill is documented against the supported default
  contract, not a bespoke local raw-disk workflow
- future Neovex-owned image work stays explicitly separated from the current
  shipped macOS v1 contract

#### Phase D4c: API forwarding + port forwarding

**Goal:** `neovex serve` on macOS feels transparent while remaining a
host-resident server.

**Scope:**
- host-local control socket/channel for the guest Neovex API
- `neovex serve` on macOS auto-starts the machine and proxies through that
  control channel
- gvproxy port forwarding: services accessible from macOS localhost
- machine-level readiness, guest Neovex readiness, and guest service readiness
  remain distinct probe stages

**Acceptance criteria:**
- `neovex serve` on macOS starts the initialized machine, stays host-resident,
  and proxies
  transparently to the guest machine API
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
| D1: CI build pipeline | `done` | Neovex compiles | release workflow now publishes Neovex binary assets plus checksums/provenance on `v*` tags; neovex-crun is already green on amd64+arm64 |
| D2: Apt repo (Debian/Ubuntu) | `in_progress` | D1 | shared `nfpm` package builder, signed static apt-repo builder, and release-driven mirror workflow landed; GitHub Pages deploy path exists, but final `apt.neovex.dev` cutover and Debian `libkrun` ownership remain |
| D3: COPR (Fedora) | `in_progress` | D1 | shared `nfpm`-based package builder, deterministic Fedora/COPR SRPM bridge, and release-driven mirror workflow landed; live COPR publication and first `dnf copr enable ...` proof still remain |
| D4a: Homebrew + krunkit | `done` | D1 | Apple Silicon, macOS 14+ cask ships bundled `gvproxy`, owns `krunkit`, and has real local install/start/ssh proof |
| D4b: Guest VM image | `done` | D4a | current macOS v1 contract is the pinned Podman machine image plus host-managed guest-binary sync; `agentstation/neovex-machine-os` remains the future Neovex-owned supply-side track |
| D4c: API + port forwarding | `done` | D4b | `neovex serve` now auto-starts an initialized macOS machine for container-backed Compose projects, then proves host `/health`, forwarded machine API, `ctx.services` activation, localhost service reachability, native `/ws` push, and tenant teardown on the real host |
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
| 2026-04-17 | D1 | `done` | Closed the stale Neovex binary-release gap. The main release workflow succeeded for `v0.1.10` after the Windows type-gating and cache-failure fixes, and the published release now carries the expected asset set: `neovex_linux_x86_64.tar.gz`, `neovex_linux_arm64.tar.gz`, `neovex_darwin_arm64.tar.gz`, `neovex_windows_x86_64.zip`, plus `checksums-sha256.txt`. The same workflow also attaches build provenance, dispatches the matching `neovex-machine-os` publish workflow, and updates the Homebrew cask, so the general binary CI/publish lane is no longer a plan gap. | `gh run list --workflow release.yml --limit 10 --json databaseId,displayTitle,headBranch,status,conclusion,url`; successful release run `24578780644` (`https://github.com/agentstation/neovex/actions/runs/24578780644`) on tag `v0.1.10`; `gh release view v0.1.10 --json tagName,isPrerelease,isDraft,assets,url`; published release `https://github.com/agentstation/neovex/releases/tag/v0.1.10` with uploaded Linux/macOS/Windows assets plus checksums | Resume the remaining distribution backlog at D2/D3/D5, or keep tightening release ergonomics and packaging evidence where the new landed pipeline exposed rough edges |
| 2026-04-18 | D1 | `documented` | Hardened the binary-release lane so the shipped archive contract is enforced in CI instead of living only in docs and post-release spot checks. The repo now owns `scripts/verify-release-archive-layout.sh`, `scripts/verify-release-archive-layout-helper.sh`, and `make verify-release-archive-layout-helper`; `.github/workflows/release.yml` runs that layout check immediately after artifact download, before checksums, GitHub Release creation, or Homebrew cask updates. The guard now fails the release if the macOS tarball ever drops the bundled `libexec/gvproxy`, if the unix archives lose `README.md` or `LICENSE`, or if the Windows zip drifts from the expected `neovex.exe` layout. This mirrors the same packaging discipline Podman uses in its macOS pkginstaller flow: helper binaries are part of the shipped payload, and packaging correctness is something the release pipeline should verify, not something operators have to rediscover after install. A real download of the already-published `v0.1.10` release assets then confirmed the value of the new guard: the current public `neovex_darwin_arm64.tar.gz` still contains only `neovex`, `README.md`, and `LICENSE`, so it predates the bundled-`gvproxy` fix and the next tagged release must republish the darwin asset before the public Homebrew cask can be considered aligned with the checked-in macOS contract. | `bash -n scripts/verify-release-archive-layout.sh`; `bash -n scripts/verify-release-archive-layout-helper.sh`; `bash scripts/verify-release-archive-layout-helper.sh`; focused review against `/Users/jack/src/github.com/containers/podman/contrib/pkginstaller/Makefile` and `/Users/jack/src/github.com/containers/podman/contrib/pkginstaller/package.sh`; real-release check: `gh release download v0.1.10 --repo agentstation/neovex --pattern 'neovex_*' --dir /tmp/neovex-release-assets.9PrBZQ`; `bash scripts/verify-release-archive-layout.sh --artifacts-dir /tmp/neovex-release-assets.9PrBZQ` failed with missing `libexec/gvproxy` in the darwin archive as expected for the pre-fix tag | Cut the next Neovex release from the fixed workflow so the public darwin asset and Homebrew cask finally match the documented macOS helper contract; after that, resume the higher-leverage D2/D3 live publication work |
| 2026-04-17 | D4a | `done` | Closed the macOS Homebrew packaging contract in code and on the live host. `crates/neovex-bin/src/machine/manager.rs` now prefers a packaged `libexec/gvproxy` beside the running `neovex` binary before falling back to Podman helper paths, and `.github/workflows/release.yml` now bundles Podman-aligned `gvproxy` `v0.8.8` (matching `containers/podman`'s current `go.mod`), emits an Apple-Silicon-only cask with explicit `slp/krunkit/krunkit` dependency ownership, and clears quarantine recursively across the staged cask payload instead of just the top-level `neovex` binary. Real host proof then used a temporary local tap/cask (`local/neovex-proof/neovex-dev`) at `/tmp/neovex-d4a-proof.ZGW6fC` so the packaged layout could be exercised without touching the user's real Homebrew `neovex`: `neovex-version.txt` showed `neovex 0.1.10`, `cask-symlink.txt` resolved to `/opt/homebrew/Caskroom/neovex-dev/0.1.10/neovex`, `path-gvproxy.txt` stayed empty under `PATH=/opt/homebrew/bin:/usr/bin:/bin:/usr/sbin:/sbin`, `machine-status-running.txt` recorded `runtime.helper_binaries.gvproxy: /opt/homebrew/Caskroom/neovex-dev/0.1.10/libexec/gvproxy`, and `machine ssh` reached the guest and showed the `/Users` virtiofs mount. Because this was an unsigned local proof cask rather than a published release, macOS Gatekeeper still required explicit operator allow actions for both `neovex` and `gvproxy`; the shipped cask template now clears quarantine on the whole staged payload so that helper-specific prompt does not remain in the generated release contract. | `cargo fmt --all --check`; `cargo check -p neovex-bin`; `cargo test -p neovex-bin bundled_helper_candidates_cover_root_and_bin_layouts -- --nocapture`; `cargo test -p neovex-bin helper_resolution_prefers_packaged_candidates_before_fallbacks -- --nocapture`; local packaging prep at `/tmp/neovex-d4a-proof.ZGW6fC`: tarball contents `neovex`, `libexec/gvproxy`, `README.md`, `LICENSE`; `brew tap local/neovex-proof /tmp/neovex-d4a-proof.ZGW6fC/homebrew-neovex-proof`; `brew install --cask local/neovex-proof/neovex-dev`; `readlink /opt/homebrew/bin/neovex-dev`; `HOME=/tmp/neovex-mac-closeout.FNcv0I/home NEOVEX_MACHINE_RUNTIME_ROOT=/tmp/neovex-mac-closeout.FNcv0I/runtime PATH=/opt/homebrew/bin:/usr/bin:/bin:/usr/sbin:/sbin /opt/homebrew/bin/neovex-dev --version`; `... neovex-dev machine start`; `... neovex-dev machine ssh -- /bin/sh -lc 'uname -a; mount | grep virtiofs'`; `... neovex-dev machine stop`; `brew uninstall --cask neovex-dev`; `brew untap local/neovex-proof` | Resume D1 and reconcile whether the now-landed release workflow evidence is enough to mark the general neovex binary CI pipeline done, or close the next remaining distribution item with a similar proof-first pass |
| 2026-04-18 | D4a | `documented` | Turned the earlier one-off local cask validation into a checked-in operator collector and re-ran it on the live host. The repo now owns `scripts/collect-neovex-homebrew-cask-proof.sh` plus `make collect-neovex-homebrew-cask-proof`, which package the local release `neovex` plus bundled `libexec/gvproxy` into a temporary proof cask, install it under an isolated Homebrew tap/token, and then capture the packaged `neovex --version`, symlink target, bundled-helper discovery, `machine init`, `machine start`, `machine status`, guest `neovex --version`, guest SSH `/Users` virtiofs proof, and `machine stop` against isolated machine roots without touching the user's shipped `neovex` cask token or default machine state. The fresh checked-in proof bundle at `/tmp/neovex-d4a-proof-checkedin` recorded `host-neovex-version.txt` and `guest-neovex-version.txt` as `neovex 0.1.10`, `cask-symlink.txt` resolving to `/opt/homebrew/Caskroom/neovex-dev/0.1.10/neovex`, `path-gvproxy.txt` staying empty under `PATH=/opt/homebrew/bin:/usr/bin:/bin:/usr/sbin:/sbin`, `machine-status-running.txt` recording `runtime.helper_binaries.gvproxy: /opt/homebrew/Caskroom/neovex-dev/0.1.10/libexec/gvproxy`, and `machine-ssh-mounts.txt` proving `/Users` virtiofs reachability before the helper cleaned up the temporary tap/cask again. | `bash -n scripts/collect-neovex-homebrew-cask-proof.sh`; `cargo check -p neovex-bin`; real-host proof via `make collect-neovex-homebrew-cask-proof OUTPUT_DIR=/tmp/neovex-d4a-proof-checkedin` | Keep the checked-in cask collector as the durable D4a packaging/install evidence path so future macOS release changes can be revalidated without reconstructing the proof from shell history |
| 2026-04-18 | D4a | `documented` | Tightened the machine-helper trust boundary to match Podman's darwin `helper_binaries_dir` model more closely. The macOS machine manager now resolves `krunkit` and `gvproxy` only from explicit per-binary overrides, `NEOVEX_MACHINE_HELPER_BINARY_DIR`, packaged `libexec` helpers, or the same class of named Podman/Homebrew helper directories Podman searches on darwin; ambient `PATH` is no longer treated as a valid machine-helper source. That keeps the shipped cask path canonical, preserves explicit escape hatches for local proof/development work, and removes a surprising fallback that could let an unrelated shell-installed helper shadow the intended packaged or Homebrew-managed binary. The hardening first passed the no-ambient-`PATH` rerun at `/tmp/neovex-d4a-proof-no-path`, then passed the Podman-default-directory rerun at `/tmp/neovex-d4a-proof-podman-dirs`; both bundles again recorded `runtime.helper_binaries.gvproxy: /opt/homebrew/Caskroom/neovex-dev/0.1.10/libexec/gvproxy`, guest `neovex 0.1.10`, machine API readiness, and `/Users` virtiofs reachability before the temporary tap/cask cleanup. | `cargo fmt --all --check`; `cargo check -p neovex-bin`; `cargo test -p neovex-bin helper -- --nocapture`; new regressions: `cargo test -p neovex-bin helper_resolution_does_not_fall_back_to_path -- --nocapture` and `cargo test -p neovex-bin known_helper_candidates_mirror_podman_darwin_defaults -- --nocapture`; Podman source review of `vendor/go.podman.io/common/pkg/config/config_darwin.go`; real-host proof via `make collect-neovex-homebrew-cask-proof OUTPUT_DIR=/tmp/neovex-d4a-proof-podman-dirs` | Keep machine-helper resolution explicit and predictable; if future non-Homebrew packaging needs another location, add it as a named supported directory rather than reopening ambient `PATH` fallback |
| 2026-04-18 | D4a | `documented` | Closed the remaining gap between the checked-in Homebrew/cask collector and the actual macOS shipping contract. `scripts/collect-neovex-homebrew-cask-proof.sh` no longer hardwires `NEOVEX_MACHINE_GUEST_BINARY`; it now defaults to the same tagged guest release-asset path used by normal packaged installs, with `--guest-binary` retained only as an explicit debug override. The collector also now nests `scripts/collect-neovex-machine-guest-proof.sh`, so the cask lane captures guest machine-API `/healthz` and `/capabilities` evidence instead of stopping at host-side `machine status`. Real-host proof at `/tmp/neovex-d4a-proof-release-asset` then packaged `target/release/neovex` plus bundled `libexec/gvproxy` into the temporary `local/neovex-proof/neovex-dev` cask, installed it under Homebrew, and recorded `guest.binary.override <none>`, host `neovex 0.1.11`, guest `neovex 0.1.11`, packaged helper resolution at `/opt/homebrew/Caskroom/neovex-dev/0.1.11/libexec/gvproxy`, `machine-status-running.txt` with `lifecycle: running` and `reachable: true`, nested guest proof with `HTTP/1.1 200 OK` on `/healthz` and `protocol_version: v1alpha2` in `/capabilities`, plus `/Users` virtiofs reachability before the script cleaned up both the temporary cask and tap. | `bash -n scripts/collect-neovex-homebrew-cask-proof.sh`; `cargo build --release -p neovex-bin`; real-host proof via `bash scripts/collect-neovex-homebrew-cask-proof.sh --output-dir /tmp/neovex-d4a-proof-release-asset` | Keep the packaged/Homebrew proof lane centered on the real release-asset contract; only use `--guest-binary` when intentionally debugging a local guest build rather than validating the shipped path |
| 2026-04-18 | D4a | `documented` | Added a CI-safe automation lane for the packaged macOS proof harness itself without pretending GitHub-hosted macOS runners are a trustworthy `krunkit` VM validation surface. `scripts/collect-neovex-homebrew-cask-proof.sh` now accepts a configurable `--brew-prefix`, the repo now owns `scripts/verify-neovex-homebrew-cask-proof-helper.sh` plus `make verify-neovex-homebrew-cask-proof-helper`, and `.github/workflows/ci.yml` now runs deterministic guest-proof, service-proof, and Homebrew/cask-proof helper verifiers on every non-scheduled CI run. The new helper stands up a fake Homebrew prefix, fake cask install, fake packaged `neovex`, and nested guest-proof responses so the packaged macOS harness stays regression-tested even though the real `krunkit` guest boot remains a checked-in local proof lane. | `bash -n scripts/collect-neovex-homebrew-cask-proof.sh`; `bash -n scripts/verify-neovex-homebrew-cask-proof-helper.sh`; `bash scripts/verify-neovex-homebrew-cask-proof-helper.sh`; `make verify-neovex-homebrew-cask-proof-helper`; `actionlint .github/workflows/ci.yml`; GitHub-hosted macOS runner reference showing arm64 nested virtualization unsupported: `https://docs.github.com/en/actions/reference/runners/github-hosted-runners` | Keep the full macOS guest boot as explicit local proof on real Apple Silicon hosts, and treat the helper-verifier job as the honest CI contract for the packaging/proof harness logic between those live reruns |
| 2026-04-18 | D4a | `documented` | Tightened the helper-discovery contract to match Podman's macOS packaging model more closely. The machine manager now keeps per-binary overrides (`NEOVEX_MACHINE_GVPROXY`, `NEOVEX_MACHINE_KRUNKIT`) and honors a Podman-style helper-directory override (`NEOVEX_MACHINE_HELPER_BINARY_DIR`) before it searches packaged `libexec` helpers and known Podman/Homebrew helper locations. This was the intermediate step that moved ambient `PATH` from implicit behavior into an explicit last-resort escape hatch; the later D4a hardening row on the same date then removed `PATH` fallback entirely. | `cargo fmt --all --check`; `cargo check -p neovex-bin`; `cargo test -p neovex-bin helper -- --nocapture` | Keep the shipped macOS contract centered on bundled helpers plus explicit overrides; if future packaging needs another location, add it as a named supported directory instead of relying on shell `PATH` |
| 2026-04-18 | D4a | `documented` | Corrected the remaining manual macOS install guidance so it no longer strands the bundled helper. `README.md`, `docs/reference/cli.md`, and `docs/reference/macos-machine-flow.md` now all say the same thing: on macOS, a direct tarball install must preserve the relative `prefix/bin/neovex` plus `prefix/libexec/gvproxy` layout, or set `NEOVEX_MACHINE_HELPER_BINARY_DIR` explicitly. The old one-line example that moved only `neovex` into `/usr/local/bin` was removed because it created a subtly broken machine-helper layout that no longer matches the supported Homebrew/cask contract or the Podman-aligned helper-discovery rules. | docs review plus focused diff of `README.md`, `docs/reference/cli.md`, and `docs/reference/macos-machine-flow.md` against the landed helper-discovery contract | Keep all user-facing macOS install guidance centered on preserving the shipped helper layout; if a future standalone installer is added, it should create the same relative binary/libexec shape automatically |
| 2026-04-18 | D2/D3 | `in_progress` | Landed the shared Linux package-build foundation instead of splitting Debian and Fedora packaging into two unrelated paths. The repo now owns `scripts/build-linux-release-packages.sh`, `scripts/verify-build-linux-release-packages-helper.sh`, `make build-linux-release-packages`, `make verify-build-linux-release-packages-helper`, and manual workflow `.github/workflows/linux-packages.yml`. That foundation stages release payloads for `neovex` and `neovex-crun`, renders deterministic `nfpm` manifests for both `deb` and `rpm`, builds real candidate packages from released binaries for `amd64` / `arm64`, and emits package-level SHA-256 checksums beside the generated artifacts. This materially advances both distro channels, but it does not yet publish a signed apt repository or a COPR-backed Fedora install channel, so both phases stay `in_progress` rather than `done`. | `bash -n scripts/build-linux-release-packages.sh`; `bash -n scripts/verify-build-linux-release-packages-helper.sh`; `PATH=/tmp/neovex-nfpm-bin:$PATH bash scripts/verify-build-linux-release-packages-helper.sh`; `actionlint .github/workflows/linux-packages.yml`; `cargo fmt --all --check`; direct real-package proof with temporary stubs under `/tmp/neovex-linux-packages-debug.Z2zWOq/out`: `PATH=/tmp/neovex-nfpm-bin:$PATH bash scripts/build-linux-release-packages.sh --output-dir /tmp/neovex-linux-packages-debug.Z2zWOq/out --neovex-binary /tmp/neovex-linux-packages-debug.Z2zWOq/neovex --neovex-crun-binary /tmp/neovex-linux-packages-debug.Z2zWOq/neovex-crun --version 0.1.10 --crun-version 0.1.4 --arch amd64` produced `.deb`, `.rpm`, and `checksums-sha256.txt` successfully | Push D2 next by deciding the Debian/Ubuntu repo/signing contract and `libkrun` / `libkrunfw` ownership; then mirror the same release artifacts into COPR for D3 instead of inventing a second packaging stack |
| 2026-04-18 | D2 | `in_progress` | Landed the signed static apt-repo bundle path on top of the earlier `.deb` package builder. The repo now owns `scripts/build-apt-repository.sh`, `scripts/verify-build-apt-repository-helper.sh`, `make build-apt-repository`, `make verify-build-apt-repository-helper`, and manual workflow `.github/workflows/apt-repo.yml`. That D2 slice turns prebuilt `.deb` artifacts into a multi-arch repository tree with `pool/`, `dists/`, `Packages`, `Packages.gz`, `Release`, `InRelease`, detached `Release.gpg`, and exported public keyring material; the workflow can also optionally upload and deploy the static repo bundle through GitHub Pages, with `APT_REPOSITORY_CNAME` available for the later custom-domain handoff. Real verification from the current macOS host ran the helper through Docker-backed Ubuntu so Debian's `apt-ftparchive` and `gnupg` could build and verify the signed metadata path end to end. D2 still remains `in_progress` because the repo is not yet cut over at `apt.neovex.dev`, and Debian/Ubuntu ownership of `libkrun` / `libkrunfw` is still unresolved. | `bash -n scripts/build-apt-repository.sh`; `bash -n scripts/verify-build-apt-repository-helper.sh`; `bash scripts/verify-build-apt-repository-helper.sh` (Docker-backed Ubuntu path on the current macOS host; produced `verified: apt repository builder produced signed metadata via docker`); `actionlint .github/workflows/apt-repo.yml`; `cargo fmt --all --check` | Cut the repo over behind `apt.neovex.dev` next by enabling the Pages deploy path plus the custom-domain/DNS side, and decide whether Debian `libkrun` / `libkrunfw` ship as Neovex-owned `.deb` packages or stay outside the supported apt path until that supply-side gap is closed |
| 2026-04-18 | D3 | `in_progress` | Added the Fedora/COPR bridge on top of the shared Linux release-artifact contract instead of creating a second Fedora-specific compile pipeline. The repo now owns `scripts/build-fedora-release-srpms.sh`, `scripts/verify-build-fedora-release-srpms-helper.sh`, `make build-fedora-release-srpms`, `make verify-build-fedora-release-srpms-helper`, and manual workflow `.github/workflows/copr-srpms.yml`. That path wraps the released `neovex_linux_x86_64.tar.gz`, `neovex_linux_arm64.tar.gz`, `neovex-crun-linux-amd64`, and `neovex-crun-linux-arm64` artifacts into deterministic source bundles plus `neovex` / `neovex-crun` `.src.rpm` files suitable for direct `copr-cli build` submission. The docker-backed helper also rebuilds installable x86_64 and aarch64 RPMs inside Fedora 42 userspace, verifies the expected dependency metadata, and proves the installed stubs execute after `dnf install` from the rebuilt local RPMs. The live COPR project, credentials, and first published `dnf copr enable ... && dnf install neovex` proof remain open, so D3 stays `in_progress`. | `bash -n scripts/build-fedora-release-srpms.sh`; `bash -n scripts/verify-build-fedora-release-srpms-helper.sh`; `bash scripts/verify-build-fedora-release-srpms-helper.sh`; `actionlint .github/workflows/copr-srpms.yml`; `cargo fmt --all --check` | Use the new workflow to submit the SRPMs to the real `agentstation/neovex` COPR project, then capture a fresh-Fedora install proof and document the final `dnf copr enable ...` operator path |
| 2026-04-18 | D2/D3 release mirror | `in_progress` | Promoted the Linux packaging lanes from manual-only helpers to a release-driven mirror pipeline. The repo now owns the checked-in contract at `packaging/linux-distribution-contract.env`, reusable-call support in `.github/workflows/linux-packages.yml`, `.github/workflows/apt-repo.yml`, and `.github/workflows/copr-srpms.yml`, plus the new tag/release-triggered orchestrator `.github/workflows/linux-distribution-release.yml`. That mirror workflow resolves the pinned `neovex-crun` version and default channel targets once, then reuses the already-published Neovex GitHub release assets to build Linux packages, the apt repository bundle, and Fedora/COPR SRPMs without asking the operator to restate those downstream inputs. Publication still stays explicit: GitHub Pages deploy and COPR submission remain gated behind repo variables/secrets, so the next closeout step is to run the mirror lane against a real release with those publication switches enabled and then capture fresh operator install proof from the public channels. | `actionlint .github/workflows/linux-packages.yml`; `actionlint .github/workflows/apt-repo.yml`; `actionlint .github/workflows/copr-srpms.yml`; `actionlint .github/workflows/linux-distribution-release.yml`; `cargo fmt --all --check` | Run the release-driven mirror lane against `v0.1.10` or the next tag with the publication toggles enabled, then capture `apt.neovex.dev` and `dnf copr enable ...` proof from fresh Linux VMs |
| 2026-04-15 | D4b | `documented` | The machine-image repo split has now landed. The guest image source and workflow moved out of the neovex monorepo into `agentstation/neovex-machine-os`, and the host `v*` release workflow now calls the external reusable build workflow with the same version tag. Follow-on hardening then converted the repo boundary into an explicit artifact contract: standalone machine-os `v*` tags now resolve the matching Neovex release tag instead of `latest`, the packaged OCI artifact carries source/attestation/version annotations, and the host machine manager reads those annotations before falling back to the older dual-repo attestation lookup. Durable conclusion: the host repo should treat machine-image production as an external dependency with a versioned, machine-readable cross-repo release contract, not as a future monorepo refactor. | repo review of `agentstation/neovex/.github/workflows/release.yml`; repo review of `agentstation/neovex-machine-os/.github/workflows/build.yml`; repo review of `agentstation/neovex-machine-os/scripts/package-oci.sh`; focused `cargo check -p neovex-bin`; `bash /Users/jack/src/github.com/agentstation/neovex-machine-os/scripts/verify-oci-layout-helper.sh`; `cargo fmt --all --check` | Keep host docs version-pinned (`v{CARGO_PKG_VERSION}`), keep publishing explicit OCI metadata, and continue removing host-side fallbacks once all live machine images carry the new annotations |
| 2026-04-17 | D4c | `done` | Closed the host-resident macOS API-forwarding and port-forwarding gap in `crates/neovex-bin/src/machine/mod.rs` and `crates/neovex-bin/src/service/mod.rs`. The default-machine path now has an explicit `ensure_default_machine_api_client_started()` helper that reuses the existing per-machine lock and `machine start` convergence path, and the host-backed `serve` loader now uses it only for macOS container-backed Compose projects instead of failing with "run `neovex machine start` first". Real-host proof on the existing isolated root at `/tmp/neovex-mac-closeout.FNcv0I/serve-proof-d4c-autostart` then started from a stopped machine, launched `neovex serve` directly, captured `serve-health.txt` (`GET /health -> 200 {"ok":true}`), `machine-status-after-serve.txt` (`lifecycle: running`, `machine_api.reachable: true`, `service_execution_ready: true`), `activate-query.txt` (`POST /convex/demo/query {"name":"services:activate","args":{}} -> 200 18080`), `service-health-via-port.txt` (`GET http://127.0.0.1:18080/healthz -> 200 ok`), `websocket-messages.jsonl` (initial empty `subscription_result` plus a pushed `subscription_result` after `websocket-insert.txt`), and `delete-tenant*.txt` plus `service-after-delete.txt` to prove tenant teardown withdraws the localhost service again. | `cargo fmt --all --check`; `cargo test -p neovex-bin macos_host_loader_auto_starts_default_machine_only_for_container_projects -- --nocapture`; `cargo test -p neovex-bin host_loader_accepts_default_projects_with_ready_forwarded_machine_api_on_macos -- --nocapture`; `cargo test -p neovex-bin macos_service_commands_use_forwarded_machine_api_for_container_projects -- --nocapture`; `cargo check -p neovex-bin`; real-host commands under `HOME=/tmp/neovex-mac-closeout.FNcv0I/home` and `NEOVEX_MACHINE_RUNTIME_ROOT=/tmp/neovex-mac-closeout.FNcv0I/runtime`: `target/debug/neovex machine status`; `target/debug/neovex serve --compose-file /tmp/neovex-mac-closeout.FNcv0I/ctx-services-app/compose.yaml --convex-app-dir /tmp/neovex-mac-closeout.FNcv0I/ctx-services-app --data-dir /tmp/neovex-mac-closeout.FNcv0I/serve-data-d4c --control-data-dir /tmp/neovex-mac-closeout.FNcv0I/serve-control-d4c --port 18084`; `curl -i -sS http://127.0.0.1:18084/health`; `curl -i -sS -X POST http://127.0.0.1:18084/api/tenants --data '{"id":"demo"}'`; `curl -i -sS -X POST http://127.0.0.1:18084/convex/demo/query --data '{"name":"services:activate","args":{}}'`; `curl -i -sS http://127.0.0.1:18080/healthz`; `curl -i -sS -X POST http://127.0.0.1:18084/api/tenants --data '{"id":"demo-ws"}'`; `node /tmp/neovex-mac-closeout.FNcv0I/serve-proof-d4c-autostart/websocket-proof.mjs ...`; `curl -i -sS -X DELETE http://127.0.0.1:18084/api/tenants/demo`; `curl -i -sS -X DELETE http://127.0.0.1:18084/api/tenants/demo-ws`; `target/debug/neovex machine stop` | Resume D4a packaging/install closeout and D1 binary-release automation against the now fully proved macOS runtime contract |
