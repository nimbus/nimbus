# Plan: Distribution — Packaging nimbus for All Channels

Canonical plan for distributing nimbus and its dependencies across all
target platforms and package channels.

---

## Status

- **Status:** `in_progress`
- **Primary owner:** this plan
- **Activation gate:** met on 2026-04-13 when the microVM service baseline
  reached `done`; this plan is now active because the binary release,
  Homebrew/cask, and Linux package mirror lanes are all in flight
- **Related plans:**
  - `docs/architecture/sandbox/microvm-service-baseline.md` — current landed runtime and
    service-control baseline
  - `docs/architecture/sandbox/macos-machine-flow.md` — current macOS developer-machine
    contract reference
  - `docs/plans/archive/macos-machine-support-plan.md` — completed macOS
    execution record with exact closeout evidence for Channel 4
  - `docs/plans/archive/vmm-infrastructure-plan.md` — historical VMM
    foundation execution record with Linux/macOS validation evidence
  - `docs/plans/archive/install-script-plan.md` — completed execution
    record for Channel 1 install script (`curl | sh`); closed 2026-05-17
  - `docs/plans/archive/distribution-execution-log-early.md` — archived
    pre-completion investigation and intermediate documentation entries

## Control Plan Rules

Source of truth:
1. this plan's `Phase Status Ledger` and `Execution Log`
2. CI/CD pipeline configuration

---

## What We Ship

### Binaries

| Binary | Source | Size | Built by |
|--------|--------|------|----------|
| `nimbus` | `nimbus/nimbus` | ~60MB | Cargo (Rust + V8) |
| `nimbus-crun` | upstream crun + build-time patch | ~2MB | autotools (C) |
| `nimbus-desktop` | [`nimbus/desktop`](https://github.com/nimbus/desktop) | ~150-200MB | electron-builder (Electron 42) |

`nimbus-desktop` is an independently-released Electron shell wrapping
the operator console UI served at `/ui/` by `nimbus`. Its release
cadence, signing credentials, and packaging matrix are isolated from
the core server. See
[`docs/plans/archive/desktop-shell-plan.md`](./archive/desktop-shell-plan.md) for the
build, sign, and notarize pipeline, and the `nimbus/desktop`
repository for installers.

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

On macOS, nimbus runs inside a Linux machine VM (same model as Podman).
Only two host-side deps are needed — everything else runs inside the VM.

| Package | Install | What |
|---------|---------|------|
| krunkit | `brew tap slp/krunkit && brew install krunkit` | Machine VM (libkrun / Hypervisor.framework) |
| gvproxy | Bundled with the nimbus macOS archive/cask | Networking + port forwarding |

Do not assume Homebrew `podman` or the `podman-desktop` cask provide a
shell-visible `krunkit` binary. nimbus should depend on `krunkit` directly so
`brew install nimbus` produces a known-good macOS machine-VM dependency set.

Verified Homebrew packaging boundary on the current host:
- Homebrew `podman` `5.8.1` installs `podman-mac-helper`, `gvproxy`, and
  `vfkit`; the formula does not declare `krunkit`.
- Homebrew `podman-desktop` `1.26.2` installs the GUI app bundle; the cask does
  not declare `krunkit` as a Homebrew dependency.
- Therefore, if nimbus chooses `krunkit` as its macOS machine provider, the
  nimbus Homebrew package must depend on `krunkit` directly instead of inheriting that
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
  packaging contracts. nimbus should document the Homebrew contract we plan to
  ship, while still using Podman's upstream source as architecture guidance.

### Platform support

| Platform | How it runs | Service isolation | Supported |
|----------|------------|-------------------|-----------|
| Linux x86_64 (bare metal) | Native (KVM) | Hardware-isolated microVMs | **Yes** (primary) |
| Linux x86_64 (cloud VM) | Native (nested KVM) | Hardware-isolated microVMs | **Yes** |
| Linux aarch64 | Native (KVM) | Hardware-isolated microVMs | **Partial** (nimbus-crun CI, machine-os CI) |
| macOS aarch64 (Apple Silicon, M1+) | Machine VM (krunkit) | Containers (same as Podman) | **Yes** (developer surface) |
| macOS x86_64 (Intel) | Not supported | — | **No** |
| Windows | WSL2 | TBD | **Future** (binary built in CI for forward compatibility; no supported runtime path yet) |

---

## Distribution Channels

### Channel 1: Install Script (Quick Start)

```bash
curl -fsSL https://github.com/nimbus/nimbus/releases/latest/download/install.sh | sh
```

The script:
1. Detects OS (Debian/Ubuntu, Fedora/RHEL, macOS)
2. Detects architecture (x86_64, aarch64)
3. Chooses the supported install channel for that platform
4. On Linux today: installs distro dependencies via apt/dnf, then installs
   released `nimbus` + `nimbus-crun` artifacts directly from GitHub Releases
5. On macOS today: installs or upgrades `nimbus/tap/nimbus` via
   Homebrew cask, which owns `krunkit` and bundles `libexec/gvproxy`
6. Later, once D2/D3 are publicly proved, Linux can switch from direct
   release-artifact bootstrap to `apt` / `dnf copr` without changing the
   `curl | sh` user entrypoint
7. Prints getting-started instructions

**Implementation reference:**
- [rustup install script](https://github.com/rust-lang/rustup/blob/master/rustup-init.sh)
- [Docker install script](https://github.com/docker/docker-install/blob/master/install.sh)

### Channel 2: Debian/Ubuntu (.deb)

**Package: `nimbus`**

```
Package: nimbus
Version: ${NIMBUS_VERSION}
Architecture: amd64
Depends: nimbus-crun, conmon, buildah, containers-common
Recommends: catatonit, passt, uidmap, fuse-overlayfs
Description: Reactive document database with microVM runtime
```

(Version is illustrative — the actual version tracks the Nimbus release tag.)

**Package: `nimbus-crun`**

```
Package: nimbus-crun
Version: 1.27+nimbus1
Architecture: amd64
Depends: libkrun (>= 1.17), libkrunfw, libcap2, libseccomp2, libyajl2
Description: crun OCI runtime with krun TSI port mapping (patched for nimbus)
```

Built from upstream crun release tarball + build-time patch (see
`docs/plans/archive/vmm-infrastructure-plan.md`). Installs to
`/usr/libexec/nimbus/crun`. Does
NOT conflict with or replace the system `crun` — nimbus invokes it via
`conmon -r /usr/libexec/nimbus/crun`. System Podman/CRI-O continue using
the distro `crun` undisturbed.

Version format: `{upstream_version}+nimbus{patch_revision}`. The `+` separator
follows Debian convention for local modifications. When upstream merges the
port mapping PR, `nimbus-crun` is dropped and replaced by a dependency on the
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
deb [signed-by=/usr/share/keyrings/nimbus.gpg] https://nimbus.github.io/apt stable main
```

**Build system:** GitHub Actions → build .deb → upload to apt repo (hosted
on GitHub Pages, Cloudflare R2, or Packagecloud).

**Implementation reference:**
- [goreleaser nfpm](https://github.com/goreleaser/nfpm) — build deb/rpm from
  YAML config, Go binary
- [cargo-deb](https://crates.io/crates/cargo-deb) — build .deb from Cargo
  metadata

### Channel 3: Fedora/RHEL (.rpm)

**Package: `nimbus`**

```
Name: nimbus
Version: ${NIMBUS_VERSION}
Requires: nimbus-crun conmon buildah containers-common
Recommends: catatonit passt shadow-utils fuse-overlayfs
```

(Version is illustrative — the actual version tracks the Nimbus release tag.)

On Fedora, libkrun and libkrunfw are already in the repos. The
`nimbus-crun` package installs to `/usr/libexec/nimbus/crun` alongside
the system crun (does not replace it).

**COPR or custom repo:**
```
dnf copr enable nimbus/nimbus
dnf install nimbus
```

**Implementation reference:**
- [Fedora COPR](https://copr.fedorainfracloud.org/) — free RPM build service

### Channel 4: Homebrew + Machine VM (macOS)

On macOS, nimbus runs inside a Linux VM ("nimbus machine"), following the
same model as Podman. macOS does not have Linux namespaces, cgroups,
seccomp, or KVM — every major container tool solves this with a machine VM.

#### Architecture

```
macOS (Apple Silicon, M1+, macOS 14+)
  │
  └── nimbus (macOS binary — thin CLI client)
        │
        ├── nimbus machine init / start / stop
        │     └── krunkit (libkrun / Hypervisor.framework)
        │           ├── virtiofs (host ↔ guest file sharing)
        │           ├── virtio-net (guest networking via gvproxy)
        │           └── vsock devices (ready signal + first-boot ignition)
        │
        ├── gvproxy
        │     ├── guest networking + published localhost ports
        │     └── forwarded guest API/control socket
        │
        └── nimbus start (proxied to Linux guest via a host-local control channel)
              │
              └── Linux guest VM (Fedora CoreOS + nimbus deps)
                    │
                    └── nimbus start (same Linux binary as production)
                          │
                          └── services run as containers (crun, same as Podman on macOS)
```

#### Architecture comparison

Rejected architecture for macOS:

```text
macOS host
  └── nimbus CLI
        └── krunkit machine VM
              └── Linux guest
                    └── nimbus
                          └── conmon -> crun(krun handler) -> microVM per service
```

Accepted architecture for macOS:

```text
macOS host
  └── nimbus CLI
        └── krunkit machine VM
              └── Linux guest
                    └── nimbus
                          └── conmon -> crun -> container per service
```

The difference is intentional:
- on macOS, the machine VM is the isolation boundary
- on Linux production, the service microVM is the isolation boundary
- `--nested` on a Podman-managed `krunkit` process is only a machine capability
  hint; it is not the architecture nimbus should require on macOS

Inside the machine VM, services run as **standard Linux containers** — the
same way Podman runs containers on macOS today. The hardware-isolated
microVM layer (libkrun/KVM) is a Linux production feature, not a macOS dev
feature. The machine VM itself provides the isolation boundary from macOS.

The nimbus server inside the VM is the **same binary** as on Linux
production. The only difference is that services use crun's standard
container mode (namespaces + cgroups) instead of the krun handler
(microVMs). The API surface is identical — `ctx.services.db.port` works
the same way.

#### Podman parity

nimbus should mirror Podman's macOS architecture strictly:
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
model we should target for nimbus on macOS.

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

On macOS, the nimbus machine manager should own a short runtime directory such
as `/tmp/nimbus` for sockets, pid files, and transient logs.

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
  it repaired `nimbus-libkrun-users-only` on this host under `/tmp/podman`

So Channel 4 should not inherit Darwin's default long `TMPDIR` subtree for the
machine runtime directory.

#### CLI taxonomy

Target command taxonomy for this channel:
- `nimbus start` starts or attaches to the nimbus server process
- `nimbus machine ...` owns machine-VM lifecycle on macOS
- `nimbus compose ...` owns Compose-backed local service lifecycle commands
- do not use `nimbus service` as the daemon-start command

Why this split:
- `start` is a verb, which matches "start the server" and avoids overloading
  the word "service" with daemon semantics
- `machine` is a managed resource, so a noun namespace is idiomatic and aligns
  with Podman and Docker Desktop concepts
- `service` would be ambiguous in nimbus because the codebase already uses
  "service" for the core engine type and for tenant-facing workloads
- `compose` is not redundant with `start`: one manages declared local
  workloads, while the other starts Nimbus itself

Current implementation note:
- the shipped CLI now has explicit `start`, `machine`, and `compose`
  subcommands; treat examples in this section as the current distribution
  command vocabulary unless an execution-log row is explicitly labeled
  historical

#### Why krunkit

1. **Rust.** Same language as nimbus. No Go dependency (unlike vfkit).
2. **libkrun.** Already in nimbus's dependency chain for microVMs on Linux.
3. **Podman-aligned.** Podman's machine code supports both `applehv` and
   `libkrun` on Apple Silicon. Podman's upstream macOS `.pkg` installer
   bundles `krunkit`, but the Homebrew Podman formula does not, so nimbus can depend
   on `krunkit` directly instead of inheriting the Homebrew Podman formula's
   bundled provider choice.
4. **Full device support.** virtiofs, vsock, virtio-net, virtio-blk,
   RESTful lifecycle API.
5. **Same containers org.** Maintained alongside crun, buildah, Podman,
   libkrun. Apache-2.0.
6. **All Apple Silicon.** Works on M1, M2, M3, M4. Requires macOS 14+.

Provider-selection note:
- `krunkit` is the deliberate nimbus provider choice for Channel 4.
- Podman's Darwin provider code still falls back to `applehv` when no provider
  is configured.
- So nimbus is mirroring Podman's one-machine-VM architecture, not copying
  Podman's exact default-provider behavior.

#### Guest VM image

**Current macOS v1 contract:** use Podman's published machine image directly,
by pinned immutable reference owned by the host `nimbus` release:

- base image: `quay.io/podman/machine-os@sha256:...`
- selection rule: provider-specific OCI artifact selection (`disktype=applehv`
  on the current macOS krunkit path), not a floating tag and not the older
  generic `disktype=raw` assumption
- convergence owner: `nimbus machine start`, which caches the machine image,
  caches the matching Linux guest `nimbus` binary, boots or rebuilds from the
  pinned image, hash-syncs `/usr/local/bin/nimbus`, repairs guest socket
  activation, and validates the forwarded machine API before reporting success
- provisioning scope: narrow Ignition only (SSH keys, guest units, virtiofs
  mounts, readiness wiring)

**Future supply-side track:** `nimbus/machine-os` remains the Nimbus-owned
bootc image pipeline once the active bootc default plan proves parity and
promotion evidence. The repo split still mirrors Podman's
`containers/podman` + `containers/podman-machine-os` ownership model, while
the bootc implementation deliberately moves away from FCOS/Ignition as the
future default contract.

The Podman machine-os source remains the canonical implementation reference for
the guest package shape: standard container tooling (`crun`, `conmon`,
`netavark`, `aardvark-dns`) rather than a guest-side `krun` runtime path.
Nimbus's current macOS guest should stay aligned with that same
standard-container pattern.

#### Communication

- **API/control channel:** host-local forwarded socket — the macOS host
  should talk to the guest Nimbus API through a host-local control socket or
  equivalent forwarded channel. Podman's current source uses `gvproxy` plus
  SSH-backed guest-socket forwarding as the reference model; do not describe
  the default API path as raw `vsock` forwarding.
- **File sharing:** virtiofs — developer project directories shared into
  the VM (default: home directory, same as Podman).
- **Port forwarding:** gvproxy forwards ports from macOS localhost to the
  guest VM. Same as Podman's port forwarding model on macOS.

#### Homebrew cask

Dependency contract:
- `nimbus` owns `krunkit` as an explicit Homebrew dependency on macOS.
- `nimbus` bundles `gvproxy` inside the macOS release archive under
  `libexec/gvproxy`, following Podman's pkg-installer pattern instead of
  treating Homebrew `podman` as a transitive dependency manager.
- Do not rely on a preexisting Homebrew `podman` or `podman-desktop`
  installation to make `krunkit` available on `PATH`.
- `podman-desktop` may still be useful as a GUI, but it is not nimbus's
  dependency manager for the machine provider.
- `podman-mac-helper` stays optional. It only binds `/var/run/docker.sock`
  to a Podman-managed socket for Docker-compatible clients such as Compose,
  Testcontainers, or the Docker CLI.
- nimbus should talk to its own machine socket or vsock proxy directly. Do
  not make the machine lifecycle or API path depend on `podman-mac-helper`.
- Installing `podman-mac-helper` can take over the global Docker socket path,
  so treat it as an explicit compatibility mode instead of a default nimbus
  requirement.

```ruby
 cask "nimbus" do
  name "nimbus"
  desc "Reactive document database with microVM runtime"
  homepage "https://github.com/nimbus/nimbus"
  version "0.1.14"  # updated by release workflow on each v* tag

  binary "nimbus"

  on_macos do
    depends_on arch: :arm64
    depends_on macos: ">= :sonoma"
    depends_on formula: "slp/krunkit/krunkit"

    on_arm do
      url "https://github.com/nimbus/nimbus/releases/download/v#{version}/nimbus_darwin_arm64.tar.gz"
      sha256 "..."
    end
  end
end
```

```bash
brew install nimbus/tap/nimbus
# Installs: nimbus CLI, krunkit, gvproxy

nimbus machine init   # One-time: record the default machine contract
nimbus start          # Auto-starts that initialized machine if needed
```

#### Developer experience

```bash
nimbus machine init     # one-time: record image/resources/SSH contract
nimbus machine start    # optional explicit boot (~3-5s)
nimbus machine stop     # graceful shutdown (via krunkit REST API)
nimbus machine rm       # delete VM and disk image
nimbus machine ssh      # debug: SSH into the VM
nimbus machine status   # show VM state, resource usage
```

`nimbus start` on macOS auto-starts the initialized machine if not running.

#### Optional Docker compatibility

If a developer wants third-party Docker clients on macOS to talk to the
machine VM through the default `/var/run/docker.sock` path, `podman-mac-helper`
or an equivalent `DOCKER_HOST` export can provide that compatibility layer.
This is optional for nimbus itself. The nimbus CLI should work without taking
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
# Download the released Nimbus host binary bundle
curl -L -o nimbus.tar.gz \
  https://github.com/nimbus/nimbus/releases/download/v0.1.14/nimbus_linux_x86_64.tar.gz

# Download the matching Linux private runtime separately
curl -L -o nimbus-crun \
  https://github.com/nimbus/nimbus-crun/releases/download/v1.27-nimbus.2/nimbus-crun-linux-amd64

# Extract
tar xzf nimbus.tar.gz
sudo mv nimbus /usr/local/bin/
sudo mkdir -p /usr/libexec/nimbus
sudo mv nimbus-crun /usr/libexec/nimbus/crun

# Install deps manually
sudo apt install conmon buildah catatonit passt
# For Debian: also install libkrun and libkrunfw from nimbus apt repo
```

The released `nimbus` tarball includes `nimbus`, `README.md`, and `LICENSE`.
On macOS it also includes `libexec/gvproxy`. The Linux private runtime
(`nimbus-crun`) remains a separate release asset from `nimbus/nimbus-crun`.

### Channel 6: Container Image (for CI/CD tooling)

```dockerfile
FROM debian:13-slim
RUN apt-get update && apt-get install -y \
    conmon buildah catatonit passt uidmap fuse-overlayfs
COPY nimbus /usr/local/bin/
COPY nimbus-crun /usr/libexec/nimbus/crun
# Note: This container must run with --privileged and /dev/kvm access
```

**Use case:** CI/CD pipelines that need to run nimbus. The container
provides all dependencies. Must run with `--privileged` and
`--device /dev/kvm` for KVM access.

```bash
docker run --privileged --device /dev/kvm \
  ghcr.io/nimbus/nimbus:latest serve
```

### Channel 7: Cloud VM Images (Production)

Pre-baked VM images with everything installed.

**AWS AMI:**
- Based on Debian 13 or Amazon Linux 2023
- nimbus + all deps pre-installed
- KVM enabled (use `.metal` or nested-virt-capable instance types)
- Published to AWS Marketplace or as community AMI

**GCP Image:**
- Based on Debian 13
- Nested virtualization enabled
- Published to GCP Compute Image library

**Build system:** Packer (HashiCorp) for reproducible image builds.

```hcl
# packer.hcl
source "amazon-ebs" "nimbus" {
  ami_name      = "nimbus-{{timestamp}}"
  instance_type = "c5.metal"
  source_ami    = "ami-debian-13-..."
}

build {
  sources = ["source.amazon-ebs.nimbus"]
  provisioner "shell" {
    inline = [
      "curl -fsSL https://github.com/nimbus/nimbus/releases/latest/download/install.sh | sh",
    ]
  }
}
```

**Implementation reference:**
- [Packer](https://www.packer.io/) — VM image builder

---

## Phase Plan

### Phase D1: CI Build Pipeline

**Goal:** Automated builds of nimbus and nimbus-crun for Linux x86_64 and
aarch64.

**Scope:**
- GitHub Actions workflow: build nimbus (cargo build --release)
- GitHub Actions workflow: build nimbus-crun (clone upstream crun at pinned
  tag inside Fedora 43 container with `libkrun-devel` from repos, apply
  patch, autotools `--with-libkrun`)
- Matrix: amd64 (`ubuntu-latest`) + arm64 (`ubuntu-24.04-arm`)
- GitHub Releases: upload binaries as release assets with attestation
- Tarball (Channel 5): nimbus + crun + README

**nimbus-crun release status:** `done` — `nimbus/nimbus-crun`
publishes the Linux `nimbus-crun-linux-amd64` and `nimbus-crun-linux-arm64`
artifacts from its own tagged release workflow. This repo now consumes that
external release contract rather than owning an in-repo `crun/v*` workflow.

**nimbus binary CI status:** `done` — `.github/workflows/release.yml`
verifies the tag/version contract, builds and publishes Nimbus release assets
for Linux `x86_64` + `arm64`, macOS `arm64`, and Windows `x86_64`, attaches
provenance/checksums, dispatches the matching machine-os publish workflow, and
updates the Homebrew cask on tagged releases.

**Acceptance criteria:**
- a tagged `nimbus/nimbus-crun` release exists and publishes
  `nimbus-crun-linux-amd64` + `nimbus-crun-linux-arm64`
- `git tag v0.1.14 && git push --tags` triggers nimbus build
- Nimbus release assets include `nimbus_linux_x86_64.tar.gz`,
  `nimbus_linux_arm64.tar.gz`, `nimbus_darwin_arm64.tar.gz`,
  checksums/provenance, and the matching machine-os publish handoff
- the darwin tarball includes the bundled `libexec/gvproxy` helper

### Phase D2: Apt Repository (Debian/Ubuntu)

**Goal:** `apt install nimbus` works on Debian 13 and Ubuntu 24.04+.

**Scope:**
- Shared package-build foundation now exists in-repo:
  `scripts/build-linux-release-packages.sh`,
  `scripts/verify-build-linux-release-packages-helper.sh`, and
  `.github/workflows/linux-packages.yml` render and build candidate `.deb`
  artifacts for `nimbus` and `nimbus-crun` from released binaries
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
  Nimbus GitHub release into the Linux package/repo lanes using that single
  checked-in `nimbus-crun`/channel contract instead of requiring ad hoc
  operator inputs
- Final Debian/Ubuntu channel still needs the hosted apt repository layer:
  final custom-domain publication for that signed static repo bundle
- Resolve Debian/Ubuntu ownership for `libkrun` / `libkrunfw` before claiming
  `apt install nimbus` as a supported path
- Host apt repository (GitHub Pages or Cloudflare R2)
- GPG-sign packages
- Install script (Channel 1) adds the repo and installs

**Acceptance criteria:**
- Fresh Debian 13 VM: `curl ... | sh && nimbus start` works
- Dependencies automatically pulled (conmon, buildah, etc.)

### Phase D3: Fedora/COPR (Fedora/RHEL)

**Goal:** `dnf install nimbus` works on Fedora 40+.

**Scope:**
- Shared package-build foundation now exists in-repo:
  `scripts/build-linux-release-packages.sh`,
  `scripts/verify-build-linux-release-packages-helper.sh`, and
  `.github/workflows/linux-packages.yml` render and build candidate `.rpm`
  artifacts for `nimbus` and `nimbus-crun` from released binaries
- Shared Fedora/COPR source-package bridge now exists in-repo:
  `scripts/build-fedora-release-srpms.sh`,
  `scripts/verify-build-fedora-release-srpms-helper.sh`, and
  `.github/workflows/copr-srpms.yml` wrap those same released binaries into
  deterministic source bundles and `.src.rpm` artifacts suitable for direct
  `copr-cli build ... <path-to-srpm>` submission
- Shared Linux distribution release contract now exists in-repo:
  `packaging/linux-distribution-contract.env` plus
  `.github/workflows/linux-distribution-release.yml` mirror each published
  Nimbus GitHub release into the Debian/Fedora packaging workflows from the
  same released assets instead of maintaining a separate distro-build stack
- libkrun/libkrunfw already in Fedora repos — just depend on them
- Final Fedora channel still needs the live COPR project/publication contract,
  `dnf copr enable ...` install docs, and first real repo proof
- Publish via COPR (free RPM build service)

**Acceptance criteria:**
- Fresh Fedora 40 VM: `dnf copr enable ... && dnf install nimbus` works

### Phase D4: Homebrew + Machine VM (macOS)

macOS is a development environment, not production. Nimbus follows Podman's
one-machine-VM model for service execution, but the authoritative Nimbus
server/runtime/storage loop stays on the macOS host. See Channel 4 above.

#### Phase D4a: Homebrew cask + krunkit integration

**Goal:** `brew install nimbus/tap/nimbus` works. `nimbus machine start`
boots a VM.

**Scope:**
- Build nimbus macOS CLI for `aarch64-apple-darwin`
- Create Homebrew cask for Apple Silicon depending on `slp/krunkit/krunkit`;
  bundle `gvproxy` in the macOS release archive under `libexec/gvproxy`
- `nimbus machine init/start/stop`: spawn krunkit with virtiofs,
  virtio-net/gvproxy, and any required machine-level ready/bootstrap devices
- Graceful shutdown via krunkit REST API

**Acceptance criteria:**
- `brew install nimbus/tap/nimbus` installs the CLI on Apple Silicon
  macOS, owns `slp/krunkit/krunkit` explicitly, and ships bundled
  `libexec/gvproxy`
- `nimbus machine start` boots a Fedora CoreOS VM
- SSH into the VM works; virtiofs mounts work

#### Phase D4b: Current machine-image contract

**Goal:** Ship the current macOS machine-image contract intentionally and keep
future image ownership separate.

**Scope:**
- Current macOS v1 contract uses Podman's published machine image directly at
  an immutable `quay.io/podman/machine-os@sha256:...` reference owned by the
  host `nimbus` release
- `nimbus machine start` is the primary convergence path:
  cache missing machine-image and guest-binary artifacts, rebuild boot
  artifacts when the recorded base image drifts, hash-sync the guest
  `/usr/local/bin/nimbus`, and validate the forwarded machine API before
  reporting success
- Ignition stays machine-specific and version-agnostic: SSH keys, writable
  Nimbus dirs, guest units, virtiofs mounts, readiness wiring
- explicit `nimbus machine os apply` / `nimbus machine os upgrade` surfaces
  remain host-managed rollout controls rather than ad hoc guest mutation
- a Nimbus-owned bootc image in `nimbus/machine-os` remains the later
  ownership/supply-side track once the active bootc default plan proves
  macOS parity and lifecycle evidence

**Acceptance criteria:**
- `nimbus machine init` records the pinned Podman digest instead of a floating
  tag
- `nimbus machine start` can repopulate a clean machine root from the pinned
  image and a matching guest Linux `nimbus` asset
- the macOS recovery drill is documented against the supported default
  contract, not a bespoke local raw-disk workflow
- future Nimbus-owned image work stays explicitly separated from the current
  shipped macOS v1 contract

#### Phase D4c: API forwarding + port forwarding

**Goal:** `nimbus start` on macOS feels transparent while remaining a
host-resident server.

**Scope:**
- host-local control socket/channel for the guest Nimbus API
- `nimbus start` on macOS auto-starts the machine and proxies through that
  control channel
- gvproxy port forwarding: services accessible from macOS localhost
- machine-level readiness, guest Nimbus readiness, and guest service readiness
  remain distinct probe stages

**Acceptance criteria:**
- `nimbus start` on macOS starts the initialized machine, stays host-resident,
  and proxies
  transparently to the guest machine API
- WebSocket subscriptions work through the macOS guest-control proxy
- A guest-managed service is accessible from macOS localhost via gvproxy port
  forwarding (proved with a Compose-backed healthz service at `localhost:18080`;
  the same mechanism applies to any forwarded port including postgres at `5432`)

### Channels 5 and 6: No Dedicated Phase

Channel 5 (binary tarball) is a byproduct of the D1 CI build pipeline — the
release workflow already publishes the tarballs. No additional phase work is
needed beyond keeping the archive layout guard in the release workflow.

Channel 6 (container image) does not yet have a dedicated phase. When it
becomes a priority, add a D6 phase with scope, acceptance criteria, and a
ledger row.

### Phase D5: Cloud VM Images

**Goal:** Pre-baked VM images for AWS and GCP.

**Scope:**
- Packer templates for AWS AMI and GCP Image
- Based on Debian 13
- All deps pre-installed
- KVM verified working

**Acceptance criteria:**
- Launch AMI on c5.metal → `nimbus start` works immediately
- Launch GCP VM with nested virt → `nimbus start` works immediately

---

## Phase Status Ledger

| Phase | Status | Hard deps | Notes |
|-------|--------|-----------|-------|
| D1: CI build pipeline | `done` | Nimbus compiles | release workflow now publishes Nimbus binary assets plus checksums/provenance on `v*` tags; nimbus-crun is already green on amd64+arm64 |
| D2: Apt repo (Debian/Ubuntu) | `in_progress` | D1 | shared `nfpm` package builder, signed static apt-repo builder, and release-driven mirror workflow landed; GitHub Pages deploy path exists, but final `nimbus.github.io/apt` cutover and Debian `libkrun` ownership remain |
| D3: COPR (Fedora) | `in_progress` | D1 | shared `nfpm`-based package builder, deterministic Fedora/COPR SRPM bridge, and release-driven mirror workflow landed; live COPR publication and first `dnf copr enable ...` proof still remain |
| D4a: Homebrew + krunkit | `done` | D1 | Apple Silicon, macOS 14+ cask ships bundled `gvproxy`, owns `krunkit`, auto-updates from the release workflow, and now has both isolated release-proof and real `brew upgrade` validation |
| D4b: Guest VM image | `done` | D4a | current macOS v1 contract is the pinned Podman machine image plus host-managed guest-binary sync; `nimbus/machine-os` remains the future Nimbus-owned bootc supply-side track |
| D4c: API + port forwarding | `done` | D4b | `nimbus start` now auto-starts an initialized macOS machine for container-backed Compose projects, then proves host `/health`, forwarded machine API, `ctx.services` activation, localhost service reachability, native `/ws` push, and tenant teardown on the real host |
| D5: Cloud VM images | `todo` | D2 or D3 | Packer |

---

## Execution Log

Earlier investigation and intermediate documentation entries (D4a prep
sequence, initial D1/D4a intermediate rows) were archived to
`docs/plans/archive/distribution-execution-log-early.md` on 2026-04-18.
The entries below record phase-completion milestones and current in-progress
work only. Older D4c proof rows retain then-current `nimbus serve` command
strings and `src/service` paths as historical evidence; the active public
surface is now `nimbus start` plus `nimbus compose`.

| Date | Phase | Status | Notes | Verification | Next |
|------|-------|--------|-------|--------------|------|
| 2026-04-14 | D4b | `done` | Machine-os CI workflow (`.github/workflows/nimbus-machine-os.yml`) migrated from self-hosted ARM64 runners to GitHub-hosted `ubuntu-24.04-arm`. Pipeline switched from rpm-ostree + custom-coreos-disk-images to `podman save --format oci-archive` + `bootc-image-builder`. Base image changed from Fedora CoreOS to `fedora-bootc:42`. Publishes raw-disk OCI artifact to GHCR on `machine-os/v*` tags with `actions/attest@v4` provenance. Consumer-side attestation verification added to `manager.rs`. | CI run green on `ubuntu-24.04-arm`; `actions/attest@v4` provenance attached; machine manager queries GitHub Attestations API after SHA256 verification | D4b acceptance criteria met: versioned GHCR reference, digest/provenance, dedicated ARM64 build lane |
| 2026-04-17 | D1 | `done` | Closed the stale Nimbus binary-release gap. The main release workflow succeeded for `v0.1.10` after the Windows type-gating and cache-failure fixes, and the published release now carries the expected asset set: `nimbus_linux_x86_64.tar.gz`, `nimbus_linux_arm64.tar.gz`, `nimbus_darwin_arm64.tar.gz`, `nimbus_windows_x86_64.zip`, plus `checksums-sha256.txt`. The same workflow also attaches build provenance, dispatches the matching `nimbus-machine-os` publish workflow, and updates the Homebrew cask, so the general binary CI/publish lane is no longer a plan gap. | `gh run list --workflow release.yml --limit 10 --json databaseId,displayTitle,headBranch,status,conclusion,url`; successful release run `24578780644` (`https://github.com/nimbus/nimbus/actions/runs/24578780644`) on tag `v0.1.10`; `gh release view v0.1.10 --json tagName,isPrerelease,isDraft,assets,url`; published release `https://github.com/nimbus/nimbus/releases/tag/v0.1.10` with uploaded Linux/macOS/Windows assets plus checksums | Resume the remaining distribution backlog at D2/D3/D5, or keep tightening release ergonomics and packaging evidence where the new landed pipeline exposed rough edges |
| 2026-04-18 | D1 | `documented` | Hardened the binary-release lane so the shipped archive contract is enforced in CI instead of living only in docs and post-release spot checks. The repo now owns `scripts/verify-release-archive-layout.sh`, `scripts/verify-release-archive-layout-helper.sh`, and `make verify-release-archive-layout-helper`; `.github/workflows/release.yml` runs that layout check immediately after artifact download, before checksums, GitHub Release creation, or Homebrew cask updates. The guard now fails the release if the macOS tarball ever drops the bundled `libexec/gvproxy`, if the unix archives lose `README.md` or `LICENSE`, or if the Windows zip drifts from the expected `nimbus.exe` layout. This mirrors the same packaging discipline Podman uses in its macOS pkginstaller flow: helper binaries are part of the shipped payload, and packaging correctness is something the release pipeline should verify, not something operators have to rediscover after install. A real download of the already-published `v0.1.10` release assets then confirmed the value of the new guard: the current public `nimbus_darwin_arm64.tar.gz` still contains only `nimbus`, `README.md`, and `LICENSE`, so it predates the bundled-`gvproxy` fix and the next tagged release must republish the darwin asset before the public Homebrew cask can be considered aligned with the checked-in macOS contract. | `bash -n scripts/verify-release-archive-layout.sh`; `bash -n scripts/verify-release-archive-layout-helper.sh`; `bash scripts/verify-release-archive-layout-helper.sh`; focused review against `/Users/jack/src/github.com/containers/podman/contrib/pkginstaller/Makefile` and `/Users/jack/src/github.com/containers/podman/contrib/pkginstaller/package.sh`; real-release check: `gh release download v0.1.10 --repo nimbus/nimbus --pattern 'nimbus_*' --dir /tmp/nimbus-release-assets.9PrBZQ`; `bash scripts/verify-release-archive-layout.sh --artifacts-dir /tmp/nimbus-release-assets.9PrBZQ` failed with missing `libexec/gvproxy` in the darwin archive as expected for the pre-fix tag | Cut the next Nimbus release from the fixed workflow so the public darwin asset and Homebrew cask finally match the documented macOS helper contract; after that, resume the higher-leverage D2/D3 live publication work |
| 2026-04-18 | D2/D3 | `in_progress` | Landed the shared Linux package-build foundation instead of splitting Debian and Fedora packaging into two unrelated paths. The repo now owns `scripts/build-linux-release-packages.sh`, `scripts/verify-build-linux-release-packages-helper.sh`, `make build-linux-release-packages`, `make verify-build-linux-release-packages-helper`, and manual workflow `.github/workflows/linux-packages.yml`. That foundation stages release payloads for `nimbus` and `nimbus-crun`, renders deterministic `nfpm` manifests for both `deb` and `rpm`, builds real candidate packages from released binaries for `amd64` / `arm64`, and emits package-level SHA-256 checksums beside the generated artifacts. This materially advances both distro channels, but it does not yet publish a signed apt repository or a COPR-backed Fedora install channel, so both phases stay `in_progress` rather than `done`. | `bash -n scripts/build-linux-release-packages.sh`; `bash -n scripts/verify-build-linux-release-packages-helper.sh`; `PATH=/tmp/nimbus-nfpm-bin:$PATH bash scripts/verify-build-linux-release-packages-helper.sh`; `actionlint .github/workflows/linux-packages.yml`; `cargo fmt --all --check`; direct real-package proof with temporary stubs under `/tmp/nimbus-linux-packages-debug.Z2zWOq/out`: `PATH=/tmp/nimbus-nfpm-bin:$PATH bash scripts/build-linux-release-packages.sh --output-dir /tmp/nimbus-linux-packages-debug.Z2zWOq/out --nimbus-binary /tmp/nimbus-linux-packages-debug.Z2zWOq/nimbus --nimbus-crun-binary /tmp/nimbus-linux-packages-debug.Z2zWOq/nimbus-crun --version 0.1.10 --crun-version 0.1.4 --arch amd64` produced `.deb`, `.rpm`, and `checksums-sha256.txt` successfully | Push D2 next by deciding the Debian/Ubuntu repo/signing contract and `libkrun` / `libkrunfw` ownership; then mirror the same release artifacts into COPR for D3 instead of inventing a second packaging stack |
| 2026-04-18 | D2 | `in_progress` | Landed the signed static apt-repo bundle path on top of the earlier `.deb` package builder. The repo now owns `scripts/build-apt-repository.sh`, `scripts/verify-build-apt-repository-helper.sh`, `make build-apt-repository`, `make verify-build-apt-repository-helper`, and manual workflow `.github/workflows/apt-repo.yml`. That D2 slice turns prebuilt `.deb` artifacts into a multi-arch repository tree with `pool/`, `dists/`, `Packages`, `Packages.gz`, `Release`, `InRelease`, detached `Release.gpg`, and exported public keyring material; the workflow can also optionally upload and deploy the static repo bundle through GitHub Pages, with `APT_REPOSITORY_CNAME` available for the later custom-domain handoff. Real verification from the current macOS host ran the helper through Docker-backed Ubuntu so Debian's `apt-ftparchive` and `gnupg` could build and verify the signed metadata path end to end. D2 still remains `in_progress` because the repo is not yet cut over at `nimbus.github.io/apt`, and Debian/Ubuntu ownership of `libkrun` / `libkrunfw` is still unresolved. | `bash -n scripts/build-apt-repository.sh`; `bash -n scripts/verify-build-apt-repository-helper.sh`; `bash scripts/verify-build-apt-repository-helper.sh` (Docker-backed Ubuntu path on the current macOS host; produced `verified: apt repository builder produced signed metadata via docker`); `actionlint .github/workflows/apt-repo.yml`; `cargo fmt --all --check` | Cut the repo over behind `nimbus.github.io/apt` next by enabling the Pages deploy path plus the custom-domain/DNS side, and decide whether Debian `libkrun` / `libkrunfw` ship as Nimbus-owned `.deb` packages or stay outside the supported apt path until that supply-side gap is closed |
| 2026-04-18 | D3 | `in_progress` | Added the Fedora/COPR bridge on top of the shared Linux release-artifact contract instead of creating a second Fedora-specific compile pipeline. The repo now owns `scripts/build-fedora-release-srpms.sh`, `scripts/verify-build-fedora-release-srpms-helper.sh`, `make build-fedora-release-srpms`, `make verify-build-fedora-release-srpms-helper`, and manual workflow `.github/workflows/copr-srpms.yml`. That path wraps the released `nimbus_linux_x86_64.tar.gz`, `nimbus_linux_arm64.tar.gz`, `nimbus-crun-linux-amd64`, and `nimbus-crun-linux-arm64` artifacts into deterministic source bundles plus `nimbus` / `nimbus-crun` `.src.rpm` files suitable for direct `copr-cli build` submission. The docker-backed helper also rebuilds installable x86_64 and aarch64 RPMs inside Fedora 42 userspace, verifies the expected dependency metadata, and proves the installed stubs execute after `dnf install` from the rebuilt local RPMs. The live COPR project, credentials, and first published `dnf copr enable ... && dnf install nimbus` proof remain open, so D3 stays `in_progress`. | `bash -n scripts/build-fedora-release-srpms.sh`; `bash -n scripts/verify-build-fedora-release-srpms-helper.sh`; `bash scripts/verify-build-fedora-release-srpms-helper.sh`; `actionlint .github/workflows/copr-srpms.yml`; `cargo fmt --all --check` | Use the new workflow to submit the SRPMs to the real `nimbus/nimbus` COPR project, then capture a fresh-Fedora install proof and document the final `dnf copr enable ...` operator path |
| 2026-04-18 | D2/D3 release mirror | `in_progress` | Promoted the Linux packaging lanes from manual-only helpers to a release-driven mirror pipeline. The repo now owns the checked-in contract at `packaging/linux-distribution-contract.env`, reusable-call support in `.github/workflows/linux-packages.yml`, `.github/workflows/apt-repo.yml`, and `.github/workflows/copr-srpms.yml`, plus the new tag/release-triggered orchestrator `.github/workflows/linux-distribution-release.yml`. That mirror workflow resolves the pinned `nimbus-crun` version and default channel targets once, then reuses the already-published Nimbus GitHub release assets to build Linux packages, the apt repository bundle, and Fedora/COPR SRPMs without asking the operator to restate those downstream inputs. Publication still stays explicit: GitHub Pages deploy and COPR submission remain gated behind repo variables/secrets, so the next closeout step is to run the mirror lane against a real release with those publication switches enabled and then capture fresh operator install proof from the public channels. | `actionlint .github/workflows/linux-packages.yml`; `actionlint .github/workflows/apt-repo.yml`; `actionlint .github/workflows/copr-srpms.yml`; `actionlint .github/workflows/linux-distribution-release.yml`; `cargo fmt --all --check` | Run the release-driven mirror lane against `v0.1.10` or the next tag with the publication toggles enabled, then capture `nimbus.github.io/apt` and `dnf copr enable ...` proof from fresh Linux VMs |
| 2026-04-18 | D4a | `done` | Revalidated the shipped macOS distribution contract against the public `v0.1.14` release and the live Homebrew cask. The released `nimbus_darwin_arm64.tar.gz` asset was downloaded, matched against the published `checksums-sha256.txt`, and confirmed to contain `nimbus`, `README.md`, `LICENSE`, and `libexec/gvproxy`. The checked-in isolated proof harness then installed those exact bits under a temporary Homebrew tap/token at `/tmp/nimbus-v0.1.14-homebrew-proof/run`, proved host `nimbus 0.1.14`, `machine init`, `machine start`, guest SSH, guest `nimbus 0.1.14`, forwarded machine API `reachable: true`, guest machine-API `HTTP/1.1 200 OK`, packaged `gvproxy`, and `/Users` virtiofs, then cleaned up the proof tap/cask. Finally, the real named cask path was refreshed with `brew update` and `brew upgrade --cask nimbus`, moving the installed machine from `0.1.11` to `0.1.14`; `/opt/homebrew/bin/nimbus --version` returned `nimbus 0.1.14`, and the installed `nimbus` plus `libexec/gvproxy` matched the downloaded release bytes exactly. Durable conclusion: Channel 4 is no longer just an internal proof lane; the published release archive, Homebrew tap metadata, and live operator upgrade path are aligned. | `gh release view v0.1.14 --repo nimbus/nimbus --json tagName,assets,url`; `curl --fail -L -o /tmp/nimbus-v0.1.14-homebrew-proof/release/checksums-sha256.txt https://github.com/nimbus/nimbus/releases/download/v0.1.14/checksums-sha256.txt`; `curl --fail -L -o /tmp/nimbus-v0.1.14-homebrew-proof/release/nimbus_darwin_arm64.tar.gz https://github.com/nimbus/nimbus/releases/download/v0.1.14/nimbus_darwin_arm64.tar.gz`; `shasum -a 256 -c <(grep ' nimbus_darwin_arm64.tar.gz$' /tmp/nimbus-v0.1.14-homebrew-proof/release/checksums-sha256.txt)`; `env NIMBUS_MACHINE_API_READY_TIMEOUT_SECS=180 bash scripts/collect-nimbus-homebrew-cask-proof.sh --output-dir /tmp/nimbus-v0.1.14-homebrew-proof/run --host-binary /tmp/nimbus-v0.1.14-homebrew-proof/release/unpack/nimbus --gvproxy /tmp/nimbus-v0.1.14-homebrew-proof/release/unpack/libexec/gvproxy`; `brew update`; `HOMEBREW_NO_AUTO_UPDATE=1 brew upgrade --cask nimbus`; `/opt/homebrew/bin/nimbus --version`; `diff -q /tmp/nimbus-v0.1.14-homebrew-proof/release/unpack/nimbus /opt/homebrew/Caskroom/nimbus/0.1.14/nimbus`; `diff -q /tmp/nimbus-v0.1.14-homebrew-proof/release/unpack/libexec/gvproxy /opt/homebrew/Caskroom/nimbus/0.1.14/libexec/gvproxy` | Keep Channel 4 stable, then implement Channel 1 as a bootstrapper that reuses this Homebrew path on macOS and the existing release artifacts on Linux instead of inventing a second macOS install mechanism |
| 2026-04-15 | D4b | `documented` | The machine-image repo split has now landed. The guest image source and workflow moved out of the nimbus monorepo into `nimbus/nimbus-machine-os`, and the host `v*` release workflow now calls the external reusable build workflow with the same version tag. Follow-on hardening then converted the repo boundary into an explicit artifact contract: standalone machine-os `v*` tags now resolve the matching Nimbus release tag instead of `latest`, the packaged OCI artifact carries source/attestation/version annotations, and the host machine manager reads those annotations before falling back to the older dual-repo attestation lookup. Durable conclusion: the host repo should treat machine-image production as an external dependency with a versioned, machine-readable cross-repo release contract, not as a future monorepo refactor. | repo review of `nimbus/nimbus/.github/workflows/release.yml`; repo review of `nimbus/nimbus-machine-os/.github/workflows/build.yml`; repo review of `nimbus/nimbus-machine-os/scripts/package-oci.sh`; focused `cargo check -p nimbus-bin`; `bash /Users/jack/src/github.com/nimbus/nimbus-machine-os/scripts/verify-oci-layout-helper.sh`; `cargo fmt --all --check` | Keep host docs version-pinned (`v{CARGO_PKG_VERSION}`), keep publishing explicit OCI metadata, and continue removing host-side fallbacks once all live machine images carry the new annotations |
| 2026-04-17 | D4c | `done` | Closed the host-resident macOS API-forwarding and port-forwarding gap in `crates/nimbus-bin/src/machine/mod.rs` and `crates/nimbus-bin/src/service/mod.rs`. The default-machine path now has an explicit `ensure_default_machine_api_client_started()` helper that reuses the existing per-machine lock and `machine start` convergence path, and the host-backed `serve` loader now uses it only for macOS container-backed Compose projects instead of failing with "run `nimbus machine start` first". Real-host proof on the existing isolated root at `/tmp/nimbus-mac-closeout.FNcv0I/serve-proof-d4c-autostart` then started from a stopped machine, launched `nimbus serve` directly, captured `serve-health.txt` (`GET /health -> 200 {"ok":true}`), `machine-status-after-serve.txt` (`lifecycle: running`, `machine_api.reachable: true`, `service_execution_ready: true`), `activate-query.txt` (`POST /convex/demo/query {"name":"services:activate","args":{}} -> 200 18080`), `service-health-via-port.txt` (`GET http://127.0.0.1:18080/healthz -> 200 ok`), `websocket-messages.jsonl` (initial empty `subscription_result` plus a pushed `subscription_result` after `websocket-insert.txt`), and `delete-tenant*.txt` plus `service-after-delete.txt` to prove tenant teardown withdraws the localhost service again. | `cargo fmt --all --check`; `cargo test -p nimbus-bin macos_host_loader_auto_starts_default_machine_only_for_container_projects -- --nocapture`; `cargo test -p nimbus-bin host_loader_accepts_default_projects_with_ready_forwarded_machine_api_on_macos -- --nocapture`; `cargo test -p nimbus-bin macos_service_commands_use_forwarded_machine_api_for_container_projects -- --nocapture`; `cargo check -p nimbus-bin`; real-host commands under `HOME=/tmp/nimbus-mac-closeout.FNcv0I/home` and `NIMBUS_MACHINE_RUNTIME_ROOT=/tmp/nimbus-mac-closeout.FNcv0I/runtime`: `target/debug/nimbus machine status`; `target/debug/nimbus serve --compose-file /tmp/nimbus-mac-closeout.FNcv0I/ctx-services-app/compose.yaml --convex-app-dir /tmp/nimbus-mac-closeout.FNcv0I/ctx-services-app --data-dir /tmp/nimbus-mac-closeout.FNcv0I/serve-data-d4c --control-data-dir /tmp/nimbus-mac-closeout.FNcv0I/serve-control-d4c --port 18084`; `curl -i -sS http://127.0.0.1:18084/health`; `curl -i -sS -X POST http://127.0.0.1:18084/api/tenants --data '{"id":"demo"}'`; `curl -i -sS -X POST http://127.0.0.1:18084/convex/demo/query --data '{"name":"services:activate","args":{}}'`; `curl -i -sS http://127.0.0.1:18080/healthz`; `curl -i -sS -X POST http://127.0.0.1:18084/api/tenants --data '{"id":"demo-ws"}'`; `node /tmp/nimbus-mac-closeout.FNcv0I/serve-proof-d4c-autostart/websocket-proof.mjs ...`; `curl -i -sS -X DELETE http://127.0.0.1:18084/api/tenants/demo`; `curl -i -sS -X DELETE http://127.0.0.1:18084/api/tenants/demo-ws`; `target/debug/nimbus machine stop` | Resume D4a packaging/install closeout and D1 binary-release automation against the now fully proved macOS runtime contract |
