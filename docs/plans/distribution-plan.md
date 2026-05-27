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
| `nimbus-crun` | `nimbus/nimbus-crun` | ~2MB | autotools (C), linked against private `nimbus-libkrun` |
| `nimbus-libkrun` | `nimbus/nimbus-libkrun` | ~40MB | Cargo/C, bundled private libkrun + libkrunfw runtime archive |
| `nimbus-desktop` | [`nimbus/desktop`](https://github.com/nimbus/desktop) | ~150-200MB | electron-builder (Electron 42) |

`nimbus-desktop` is an independently-released Electron shell wrapping
the operator console UI served at `/ui/` by `nimbus`. Its release
cadence, signing credentials, and packaging matrix are isolated from
the core server. See
[`docs/plans/archive/desktop-shell-plan.md`](./archive/desktop-shell-plan.md) for the
build, sign, and notarize pipeline, and the `nimbus/desktop`
repository for installers.

### OCI images

| Image | Registry | Built by | Role |
|-------|----------|----------|------|
| `ghcr.io/nimbus/nimbus:<version>` | GHCR | `nimbus/nimbus` tag-driven release workflow | Canonical application/server image for Kubernetes, Compose, Docker, and Podman. Runs `nimbus` directly in the foreground; no systemd inside the image. |
| `ghcr.io/nimbus/nimbus:<version>-<arch>` | GHCR | same release workflow | Per-architecture image tags used to assemble the multi-arch manifest and for emergency/debug pinning. |
| `ghcr.io/nimbus/machine-os:<version>` | GHCR | `nimbus/machine-os` release workflow dispatched by Nimbus release | Bootable OS/container-machine image. This is the systemd-in-image path, separate from the normal Nimbus application image. |

### Release license payload contract

The Nimbus Community License requires redistributed copies to provide the
license text and preserve copyright, attribution, license, and notice files.
Treat that as a release invariant across every distributed surface:

The full license text does not need to be duplicated into every singleton
support file. The release rule is: bundle/package/image artifacts carry or
install the license text directly; direct-fetch helper assets carry a short
machine-readable pointer to the adjacent release `LICENSE`; and the final
GitHub Release publishes one byte-identical top-level `LICENSE` asset covered
by checksums and release-asset provenance.

This matches the packaging norms we want enterprises to recognize: GitHub
recommends a repository license file for published source, Debian Policy
requires a verbatim license copy at `/usr/share/doc/PACKAGE/copyright`, RPM
packages carry explicit license metadata and license files, OCI defines
`org.opencontainers.image.licenses` as an SPDX license expression, and SPDX
permits `LicenseRef-*` identifiers for licenses outside the SPDX list.

- source and binary archives include `LICENSE`
- standalone release support assets, including `install.sh`, are accompanied
  by a top-level GitHub Release `LICENSE` asset; direct-fetch support scripts
  also carry an SPDX-style `LicenseRef-Nimbus-Community` header pointing to
  that release license asset
- public JS/npm package tarballs, when any workspace package is made
  publishable, use npm's custom-license metadata shape
  (`"license": "SEE LICENSE IN LICENSE"`) and include a top-level `LICENSE`
  file in the packed package; private-only workspaces either remain
  `"private": true` or adopt the same rule before publication
- Linux deb/rpm packages install the same text under
  `/usr/share/doc/<package>/LICENSE`, include the Debian-canonical
  `/usr/share/doc/<package>/copyright` path, and carry package license metadata
- the default OCI application image installs
  `/usr/local/share/doc/nimbus/LICENSE` and sets
  `org.opencontainers.image.licenses=LicenseRef-Nimbus-Community` on both the
  image config and multi-arch index
- release checksums and evidence verifiers cover the top-level `LICENSE` asset
  whenever they validate the final release bundle

### System dependencies — Linux (not shipped, installed from OS repos)

| Package | Debian/Ubuntu | Fedora/RHEL |
|---------|--------------|-------------|
| conmon | `apt install conmon` | `dnf install conmon` |
| buildah | `apt install buildah` | `dnf install buildah` |
| containers-common | Comes with buildah | Comes with buildah |
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
   released `nimbus` + `nimbus-libkrun` + `nimbus-crun` artifacts directly
   from GitHub Releases
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
Version: 1.27.1+nimbus2
Architecture: amd64
Depends: nimbus-libkrun (= 1.18.1+nimbus1), libcap2, libseccomp2, libyajl2
Description: crun OCI runtime with krun TSI port mapping (patched for nimbus)
```

Built from the Nimbus-owned `nimbus/nimbus-crun` release line. The first
post-hardening release is planned as `v1.27.1-nimbus.2`, based on upstream
crun `1.27.1` plus the Nimbus krun TSI port-map patch. Installs to
`/usr/libexec/nimbus/crun`. Does
NOT conflict with or replace the system `crun` — nimbus invokes it via
`conmon -r /usr/libexec/nimbus/crun`. System Podman/CRI-O continue using
the distro `crun` undisturbed.

Version format: `{upstream_version}+nimbus{patch_revision}`. The `+` separator
follows Debian convention for local modifications. GitHub release tags use
the matching exact upstream version format, for example
`v1.27.1-nimbus.2`.

**Package: `nimbus-libkrun`**

```
Package: nimbus-libkrun
Version: 1.18.1+nimbus1
Architecture: amd64
Depends: libc6
Description: Nimbus-private libkrun stack for KVM-based process isolation
```

Built from the Nimbus-owned `nimbus/nimbus-libkrun` release line and installed
under `/usr/libexec/nimbus/lib`. It may bundle the pinned `libkrunfw` runtime
library used by Nimbus, but it must not replace the distro/system
`libkrun` or `libkrunfw`.

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

On Fedora, Nimbus still uses the private `nimbus-libkrun` package for service
execution even though distro `libkrun` and `libkrunfw` exist. The
`nimbus-crun` package installs to `/usr/libexec/nimbus/crun` alongside the
system crun and resolves libkrun from `/usr/libexec/nimbus/lib`.

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
curl -L -o nimbus-libkrun.tar.gz \
  https://github.com/nimbus/nimbus-libkrun/releases/download/v1.18.1-nimbus.1/nimbus-libkrun-linux-amd64.tar.gz
curl -L -o nimbus-crun \
  https://github.com/nimbus/nimbus-crun/releases/download/v1.27.1-nimbus.2/nimbus-crun-linux-amd64

# Extract
tar xzf nimbus.tar.gz
sudo mv nimbus /usr/local/bin/
sudo mkdir -p /usr/libexec/nimbus
sudo tar xzf nimbus-libkrun.tar.gz -C /usr/libexec/nimbus
sudo mv nimbus-crun /usr/libexec/nimbus/crun

# Install deps manually
sudo apt install conmon buildah catatonit passt
```

The released `nimbus` tarball includes `nimbus`, `README.md`, and `LICENSE`.
On macOS it also includes `libexec/gvproxy`. The Linux private runtime stack
is the paired `nimbus-libkrun` archive plus `nimbus-crun` binary; neither path
uses distro `libkrun` or asks operators to build upstream libkrun manually.
The GitHub Release also publishes the top-level `LICENSE` as a standalone
asset because `install.sh` is itself a standalone distributed script. The
script carries a compact SPDX-style license pointer to that release asset
instead of embedding the full license text. Package formats install the same
license text under their conventional documentation paths, including Debian's
`/usr/share/doc/<package>/copyright` path, and container images install it under
`/usr/local/share/doc/nimbus/LICENSE`.

### Channel 6: Container Image (Release Artifact)

The canonical container image is part of every tagged Nimbus release. It is a
normal application OCI image, not a nested service-manager image:

- publish `ghcr.io/nimbus/nimbus:<version>` as a multi-architecture Linux image
  with immutable digest examples in docs; `latest` may exist as a stable-tag
  convenience, but operator docs should prefer version plus digest
- build the image from the release-produced Linux `nimbus` binary artifacts
  rather than rebuilding an untracked binary inside the image job
- set `ENTRYPOINT ["nimbus"]` and a foreground default command such as
  `start --host 0.0.0.0 --allow-network`
- run as a fixed non-root user by default, log to stdout/stderr, expose the
  server port, document writable state paths, and document `/health` for probes
- use a minimal runtime base and exclude build toolchains, Podman, buildah,
  conmon, crun, KVM, and systemd from the default image
- install the Nimbus license text inside the image and set OCI license
  metadata (`org.opencontainers.image.licenses=LicenseRef-Nimbus-Community`)
  on both the image config and multi-arch index
- attach OCI annotations, SBOM, GitHub/Sigstore signature, SLSA/GitHub
  provenance, checksums where relevant, and vulnerability-scan evidence
- upload a release asset such as `nimbus_oci_image.txt` that records the
  image tag, immutable digest, per-arch digests, verification commands, and
  provenance/signature/SBOM locations
- provide Kubernetes/Compose/Podman examples that use restart policy, probes,
  volumes, and security context outside the image

If Nimbus needs a containerized node-daemon mode that manages tenant workloads
through host systemd, Podman, cgroups, or KVM, make that an explicit image
variant or install mode with separate documentation and verification. It must
not be the default `docker run` path, and it must still run Nimbus directly in
the foreground while the host service manager owns lifecycle.

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

**Goal:** Automated builds of nimbus, nimbus-libkrun, and nimbus-crun for
Linux x86_64 and aarch64.

**Scope:**
- GitHub Actions workflow: build nimbus (cargo build --release)
- GitHub Actions workflow: build nimbus-libkrun from the Nimbus-owned
  `nimbus/nimbus-libkrun` fork and publish private runtime archives
- GitHub Actions workflow: build nimbus-crun from the Nimbus-owned
  `nimbus/nimbus-crun` fork against the paired `nimbus-libkrun` headers and
  libraries, then embed/prove private lib lookup
- Matrix: amd64 (`ubuntu-latest`) + arm64 (`ubuntu-24.04-arm`)
- GitHub Releases: upload binaries as release assets with attestation
- Tarball (Channel 5): nimbus + paired `nimbus-libkrun` / `nimbus-crun` +
  README

**nimbus libkrun/crun release status:** `done` — the paired
`nimbus-libkrun` `v1.18.1-nimbus.1` and `nimbus-crun`
`v1.27.1-nimbus.2` release contract is published and consumed by the direct
installer plus Linux package builders. Historical `nimbus-crun`
`v1.27-nimbus.1/.2` releases remain archival.

**nimbus binary CI status:** `done` — `.github/workflows/release.yml`
verifies the tag/version contract, builds and publishes Nimbus release assets
for Linux `x86_64` + `arm64`, macOS `arm64`, and Windows `x86_64`, attaches
provenance/checksums, dispatches the matching machine-os publish workflow, and
updates the Homebrew cask on tagged releases.

**Acceptance criteria:**
- a tagged `nimbus/nimbus-libkrun` release exists and publishes private
  runtime archives for amd64 and arm64
- a tagged `nimbus/nimbus-crun` release exists and publishes
  `nimbus-crun-linux-amd64` + `nimbus-crun-linux-arm64`
- `git tag v0.1.14 && git push --tags` triggers nimbus build
- Nimbus release assets include `nimbus_linux_x86_64.tar.gz`,
  `nimbus_linux_arm64.tar.gz`, `nimbus_darwin_arm64.tar.gz`,
  checksums/provenance, and the matching machine-os publish handoff
- the darwin tarball includes the bundled `libexec/gvproxy` helper

### Phase D6: Release OCI Image

**Goal:** publish a first-class Nimbus OCI application image on every tagged
release.

**Scope:**
- `Containerfile` or equivalent build definition for the default Nimbus
  application image, pinned to a digest-qualified Dockerfile frontend and
  runtime base
- tag-driven release workflow builds per-arch images from
  `nimbus_linux_x86_64.tar.gz` and `nimbus_linux_arm64.tar.gz`, then assembles
  a multi-arch `ghcr.io/nimbus/nimbus:<version>` manifest
- no systemd, Podman, buildah, conmon, crun, KVM, or host workload-management
  tooling in the default image
- fixed non-root user, foreground `nimbus` entrypoint, documented state volume,
  exposed server port, and `/health` probe examples
- OCI annotations, SBOM, GitHub/Sigstore signature, GitHub/SLSA provenance,
  vulnerability-scan evidence, and digest report release asset
- license distribution contract: archives include `LICENSE`, the GitHub
  Release includes a top-level `LICENSE` asset for standalone release support
  scripts, direct-fetch scripts carry a compact `LicenseRef-Nimbus-Community`
  pointer to that release asset, distro packages install `LICENSE` plus the
  Debian-canonical `copyright` file in their package documentation path, and
  the OCI image installs `/usr/local/share/doc/nimbus/LICENSE` while carrying
  OCI license metadata
- release evidence assets: `nimbus_oci_image.txt`,
  `nimbus_oci_attestation.json`, `nimbus_oci_sbom.json`, and
  `nimbus_oci_vulns.sarif.json`
- release evidence verifier:
  `scripts/verify-release-oci-image-assets.sh --artifacts-dir <downloaded>
  --require-license --checksums <downloaded>/checksums-sha256.txt` must
  validate the report, image digest, certificate-backed release tag ref,
  repository identity, release workflow identity, SLSA provenance, per-platform
  SBOM evidence for linux/amd64 and linux/arm64, vulnerability scan, release
  LICENSE asset, and checksum coverage
- local helper: `make verify-release-oci-image-helper`
- Docker-backed fixture helper when Docker is available:
  `make verify-release-oci-image-build-helper`
- live tagged-release proof helper:
  `make verify-release-oci-image-live TAG=vX.Y.Z OUTPUT_DIR=<proof-dir>`
- live verifier requires every uploaded release asset to be either
  `checksums-sha256.txt` or listed in `checksums-sha256.txt`, verifies all
  checksums, and verifies GitHub/Sigstore release-asset attestations for every
  checksummed asset plus the checksum manifest
- smoke proof for each architecture: image runs, `nimbus --version` reports the
  release version, `nimbus start --host 0.0.0.0 --allow-network` starts after
  the state volume admin token is rotated, and `/health` responds through a
  published port

**Acceptance criteria:**
- tagged Nimbus release publishes `ghcr.io/nimbus/nimbus:<version>` as a
  multi-arch image and records its immutable digest in `nimbus_oci_image.txt`
- image verification with GitHub/Sigstore attestation verification succeeds
  against the release tag, repository identity, and release workflow identity
  with the SLSA provenance predicate pinned, self-hosted-runner attestations
  denied, verified timestamp evidence present, and GitHub-hosted runner
  identity recorded
- SBOM evidence exists and is attached or discoverable for the image digest on
  both linux/amd64 and linux/arm64
- LICENSE evidence is present in the release archives, top-level release
  assets, direct-fetch support script headers, distro package manifests, and
  default OCI image filesystem/metadata
- `docker` or `podman` smoke tests prove the image has no nested systemd
  dependency and runs Nimbus as the foreground process
- Kubernetes/Compose/Podman examples use host/orchestrator lifecycle controls
  rather than systemd inside the image
- the live verifier downloads the public GitHub Release bundle, checks
  `LICENSE` and whole-release checksum coverage, verifies release-asset
  attestations for the checksum manifest and every checksummed artifact,
  re-verifies registry-pushed attestation evidence for the recorded digest, and
  smoke-tests the published image

**Current completion audit (2026-05-27):**

| Requirement | Current evidence | Status |
|-------------|------------------|--------|
| Default image definition uses release archive payload, foreground `ENTRYPOINT ["nimbus"]`, non-root runtime user, writable state volume, `/health`, OCI labels, installed `LICENSE`, and no systemd/Podman/buildah/conmon/crun/KVM tooling | `make verify-release-oci-image-helper`; Docker-backed `make verify-release-oci-image-build-helper`; hosted per-arch `Build OCI image` smoke steps in release run `26482097995` | `done` |
| Tag-driven workflow builds linux/amd64 and linux/arm64 images from release-produced Linux archives, pushes per-arch refs, assembles `ghcr.io/nimbus/nimbus:<version>`, optionally updates `latest`, and keeps image jobs least-permission | `.github/workflows/release.yml` static audit through `make verify-release-oci-image-helper`; `actionlint .github/workflows/release.yml`; `v0.1.33` release run `26482097995` completed both per-arch image jobs and `Publish OCI image` successfully | `done` |
| `nimbus_oci_image.txt` report is deterministic and names immutable multi-arch digest, per-arch digests, verification commands, SBOM, vulnerability scan, signature/provenance evidence, and smoke command | `scripts/render-release-oci-image-report.sh` plus `scripts/verify-release-oci-image-report.sh`, exercised by `make verify-release-oci-image-helper`; downloaded `v0.1.33` report records multi-arch digest `sha256:4e20da01cb53cad58498eff02f8b6f59f6ab8703455bcad9c535baf9c46863b5` and per-arch digests | `done` |
| Final GitHub Release bundle publishes `LICENSE`, binary archives, `install.sh`, optional adapter assets when present, `nimbus_oci_*` evidence, `checksums-sha256.txt`, and release-asset attestations for every checksummed asset plus the checksum manifest | `gh release view v0.1.33 --repo nimbus/nimbus --json tagName,targetCommitish,isDraft,isPrerelease,publishedAt,url,assets`; `make verify-release-oci-image-live TAG=v0.1.33 OUTPUT_DIR=/private/tmp/nimbus-release-v0.1.33-oci-live`; downloaded `LICENSE` SHA-256 matches the repo license | `done` |
| Published GHCR image has registry-pushed GitHub/Sigstore attestation, SLSA predicate, verified timestamp evidence, GitHub-hosted runner identity, SBOM evidence for linux/amd64 and linux/arm64, vulnerability SARIF, and real image smoke proof | Release run `26482097995` passed `Attest multi-arch image provenance`, `Verify multi-arch image attestation`, `Capture image SBOM evidence`, `Scan image for vulnerabilities`, and final evidence verification; live verifier saved `nimbus_oci_attestation.live.json`, proved GitHub-hosted runner identity and timestamp evidence, found SBOM platforms `linux/amd64,linux/arm64`, found Trivy SARIF 2.1.0, and smoke-tested `ghcr.io/nimbus/nimbus:v0.1.33@sha256:4e20da01cb53cad58498eff02f8b6f59f6ab8703455bcad9c535baf9c46863b5` | `done` |
| Operator docs avoid privileged/default `latest`/systemd-in-container guidance and show orchestrator-owned lifecycle for Compose, Podman, and Kubernetes | `make verify-release-oci-image-helper` scans current docs for stale `nimbus serve`, `--privileged`, and production `latest` guidance | `done` |

**Audit conclusion:** D6 is complete for `v0.1.33`. The tag points at
`49edb2b26c6e501c27ac36576afc6b802b7a7d08`, the hosted release workflow
published the top-level `LICENSE`, `nimbus_oci_*` assets, and
`ghcr.io/nimbus/nimbus:v0.1.33`, and the live verifier passed without
`--skip-smoke` against proof directory
`/private/tmp/nimbus-release-v0.1.33-oci-live`.

### Phase D2: Apt Repository (Debian/Ubuntu)

**Goal:** `apt install nimbus` works on Debian 13 and Ubuntu 24.04+.

**Scope:**
- Shared package-build foundation now exists in-repo:
  `scripts/build-linux-release-packages.sh`,
  `scripts/verify-build-linux-release-packages-helper.sh`, and
  `.github/workflows/linux-packages.yml` render and build candidate `.deb`
  artifacts for `nimbus`, `nimbus-libkrun`, and `nimbus-crun` from released
  binaries. The same package builder can now stage an optional
  `nimbus-bun-jsc-adapter` package from the verified
  `nimbus-bun-jsc-adapter-linux-x86_64.tar.gz` release asset when the
  adapter package lane is explicitly enabled.
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
  checked-in `nimbus-libkrun`/`nimbus-crun`/channel contract instead of
  requiring ad hoc operator inputs. Bun/JSC adapter packages remain explicit:
  release events require `NIMBUS_INCLUDE_BUN_JSC_ADAPTER_PACKAGES=true`, and
  manual dispatches require `include_bun_jsc_adapter=true`.
- Final Debian/Ubuntu channel still needs the hosted apt repository layer:
  final custom-domain publication for that signed static repo bundle
- The `nimbus-libkrun` package lane is rendered and repo-proved; the remaining
  blocker before claiming `apt install nimbus` is public apt repository
  cutover and a fresh install proof from that public channel
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
  artifacts for `nimbus`, `nimbus-libkrun`, and `nimbus-crun` from released
  binaries. Optional `nimbus-bun-jsc-adapter` RPM manifest rendering exists
  through the shared helper, but public COPR/SRPM adapter publication still
  needs a separate proof before it is claimed as a supported `dnf` path.
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
- Use `nimbus-libkrun` for Nimbus service execution; do not depend on Fedora's
  distro `libkrun`/`libkrunfw` for the patched service stack
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

### Channel 5: No Dedicated Phase

Channel 5 (binary tarball) is a byproduct of the D1 CI build pipeline — the
release workflow already publishes the tarballs. No additional phase work is
needed beyond keeping the archive layout guard in the release workflow.

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
| D1: CI build pipeline | `done` | Nimbus compiles | Nimbus binary release plus paired `nimbus-libkrun` / `nimbus-crun` release lines are published; future version bumps are normal release maintenance |
| D6: Release OCI image | `done` | D1 | `v0.1.33` release run `26482097995` published `ghcr.io/nimbus/nimbus:v0.1.33@sha256:4e20da01cb53cad58498eff02f8b6f59f6ab8703455bcad9c535baf9c46863b5` with linux/amd64 and linux/arm64 per-arch images, registry-pushed GitHub/Sigstore attestation, SBOM evidence, Trivy SARIF, release report, top-level `LICENSE`, and no-skip live smoke proof |
| D2: Apt repo (Debian/Ubuntu) | `in_progress` | D1 | shared `nfpm` package builder, signed static apt-repo builder, release-driven mirror workflow, and `nimbus-libkrun` package lane landed; GitHub Pages/custom-domain cutover and first public apt install proof remain |
| D3: COPR (Fedora) | `in_progress` | D1 | shared `nfpm`-based package builder, deterministic Fedora/COPR SRPM bridge, release-driven mirror workflow, and `nimbus-libkrun` SRPM/RPM lane landed; live COPR publication and first `dnf copr enable ...` proof remain |
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
| 2026-05-27 | D6 release OCI image | `done` | Closed D6 on the published `v0.1.33` release. The tag points at `49edb2b26c6e501c27ac36576afc6b802b7a7d08`; release run `https://github.com/nimbus/nimbus/actions/runs/26482097995` completed successfully on rerun attempt 2 after an earlier all-steps-success Windows attempt was marked cancelled by GitHub. The final run passed Windows, both Linux release binaries, both per-arch OCI image jobs, per-arch image smoke tests, multi-arch manifest assembly, registry-pushed GitHub/Sigstore attestation, attestation verification, BuildKit SBOM capture, digest-pinned Trivy vulnerability SARIF, OCI report verification, final release-bundle verification, machine-os publish, and GitHub Release creation. The public release `https://github.com/nimbus/nimbus/releases/tag/v0.1.33` is non-draft and non-prerelease, publishes `LICENSE`, `install.sh`, all platform archives, `checksums-sha256.txt`, `nimbus_oci_image.txt`, `nimbus_oci_attestation.json`, `nimbus_oci_sbom.json`, and `nimbus_oci_vulns.sarif.json`, and its release notes now name the OCI digest and runtime stack. `nimbus_oci_image.txt` records multi-arch digest `sha256:4e20da01cb53cad58498eff02f8b6f59f6ab8703455bcad9c535baf9c46863b5`, linux/amd64 digest `sha256:eacfa73c4a2b388433fb36671eb25bf15705594c7775e7cd12cc51d5877c2b43`, linux/arm64 digest `sha256:7d7335158c00eac72c0b9961fa6d12b438b236e99143d149847ab6d52f33bdd1`, foreground `ENTRYPOINT ["nimbus"]`, default `nimbus start --host 0.0.0.0`, `/var/lib/nimbus` state, `/health`, stdout/stderr logging, GitHub attestation verification, SBOM verification, Trivy scan command, pull command, and smoke command. | `make verify-release-oci-image-live TAG=v0.1.33 OUTPUT_DIR=/private/tmp/nimbus-release-v0.1.33-oci-live` passed and downloaded the release bundle, verified archive layout, optional Bun/JSC absence, `LICENSE` and whole-release checksum coverage, release-asset attestations for 11 assets, registry-pushed attestation JSON with GitHub-hosted runner identity and timestamp evidence, SBOM subject metadata with `linux/amd64,linux/arm64`, Trivy SARIF 2.1.0, then pulled and smoke-tested `ghcr.io/nimbus/nimbus:v0.1.33@sha256:4e20da01cb53cad58498eff02f8b6f59f6ab8703455bcad9c535baf9c46863b5`; `gh release view v0.1.33 --repo nimbus/nimbus --json tagName,targetCommitish,isDraft,isPrerelease,publishedAt,url,assets`; `shasum -a 256 /private/tmp/nimbus-release-v0.1.33-oci-live/LICENSE LICENSE` showed matching digest `e305b05e3645925c3c9fbc466b228b8faf09c61033b77dda6623d7cad409fbcf` | Keep D6's static, fixture, Docker-backed, and live verifiers as release gates; resume the remaining distribution backlog at D2/D3/D5. |
| 2026-05-26 | D6 clean release-candidate replay | `in_progress` | Replayed the D6 OCI-image release lane onto a dedicated clean worktree from current `origin/main` at `/private/tmp/nimbus-d6-release-candidate` on branch `codex/d6-oci-release-candidate`, resolved the final readiness conflict by preserving both downstream machine/desktop alignment and GHCR OCI evidence requirements, and verified the result as one release-candidate system instead of continuing piecemeal hardening in the dirty primary checkout. The candidate proves the local release machinery is ready, while the current public `v0.1.32` release remains an intentional negative control because it predates the D6 asset set. As part of the same lint-clean release candidate, `coverage.yml` now suppresses the intentional `cargo llvm-cov show-env` process substitutions explicitly and groups summary redirection so global workflow lint is clean. | `actionlint .github/workflows/release.yml`; `actionlint .github/workflows/*.yml`; `make verify-release-oci-image-helper`; `make verify-release-oci-image-live-helper`; Docker-backed `make verify-release-oci-image-build-helper`; `make verify-install-helper` reported 37 passing tests; `make verify-build-linux-release-packages-helper`; `make verify-release-archive-layout-helper`; `make proof-helpers`; `bash scripts/verify-machine-os-release-ref-contract-helper.sh`; `git diff --check`; `git diff --cached --check`; `bash scripts/verify-release-oci-image-live.sh --tag v0.1.32 --skip-smoke --output-dir /private/tmp/nimbus-d6-live-v0.1.32-candidate` failed as expected with `expected exactly one LICENSE release asset, found 0` | Commit/push the clean release candidate, cut the next tag from it, and run `make verify-release-oci-image-live TAG=<tag> OUTPUT_DIR=<proof-dir>` without `--skip-smoke` before moving D6 or the `/goal` to `done`. |
| 2026-05-26 | D6 systematic completion audit | `in_progress` | Switched from piecemeal pre-tag hardening to a requirement-by-requirement D6 completion audit. The current plan now separates `local_ready` requirements from `live_missing` requirements: image definition/runtime posture, release workflow wiring, deterministic report generation, docs guardrails, and local/live fixture verifiers are ready; public closeout remains missing because the latest public release is still the pre-D6 `v0.1.32` asset set and no post-D6 GHCR image/evidence bundle has been published. | `make verify-release-oci-image-helper`; `make verify-release-oci-image-live-helper`; `make verify-release-oci-image-build-helper` with Docker access; `actionlint .github/workflows/release.yml`; `git diff --check`; `gh release view --repo nimbus/nimbus --json tagName,url,assets,isDraft,isPrerelease` confirmed latest `v0.1.32` lacks `LICENSE` and `nimbus_oci_*` release assets | Stop local hardening unless the audit changes; next direct step is a clean release commit/tag and hosted `make verify-release-oci-image-live TAG=<tag> OUTPUT_DIR=<proof-dir>` proof. |
| 2026-05-26 | release license direct-fetch pointer | `in_progress` | Clarified the release license rule for singleton support files: archives, packages, and images carry or install the full `LICENSE`, while direct-fetch helpers such as `install.sh` carry an SPDX-style `LicenseRef-Nimbus-Community` pointer to the adjacent GitHub Release `LICENSE` asset instead of embedding the full text. `scripts/install.sh` now has that pointer, and the install helper guards it so the standalone bootstrap path stays legally traceable even when someone downloads only the script. | `make verify-install-helper` reported 37 passing tests; `make verify-release-oci-image-helper`; `make proof-helpers`; `actionlint .github/workflows/release.yml`; `git diff --check` | Keep the top-level release `LICENSE` asset byte-identical to the repo license and covered by checksums/provenance; keep singleton helper headers as pointers, not duplicated full license payloads. |
| 2026-05-26 | D6 live checksum subject exactness | `in_progress` | Hardened the post-tag live verifier's whole-release coverage check so it matches `checksums-sha256.txt` subjects by exact field equality instead of interpolating release asset names into a regex. The deterministic live fixture now includes a checksummed optional asset named `nimbus-extra+[proof].txt`, proving filenames with regex metacharacters remain accepted when they are checksummed and attested, while the existing unchecksummed-extra fixture still fails. | `bash -n scripts/verify-release-oci-image-live.sh scripts/verify-release-oci-image-live-helper.sh scripts/verify-release-oci-image-helper.sh`; `make verify-release-oci-image-live-helper`; `make verify-release-oci-image-helper`; `make proof-helpers`; `git diff --check` | Keep all release asset coverage checks exact and basename-scoped; preserve the live verifier output from the next tag as the final proof that every uploaded asset is uniquely checksummed and attested. |
| 2026-05-26 | D6 optional asset live-verifier coverage | `in_progress` | Extended the live verifier fixture so the success path includes a valid optional `nimbus-bun-jsc-adapter-*.tar.gz` release asset produced by `scripts/package-bun-jsc-adapter.sh`. The stubbed post-tag verifier now runs the real optional adapter package/checksum verifier through `scripts/verify-release-oci-image-live.sh`, verifies the optional adapter is included in the final checksum set, and expects 13 release-asset attestation checks: `checksums-sha256.txt`, the 10 required release assets, the optional adapter archive, and the optional punctuation-heavy checksum subject fixture. The static D6 helper now guards this fixture shape so optional release artifacts stay inside the same checksum/provenance envelope as the core binary and OCI assets. | `make verify-release-oci-image-live-helper`; `make verify-release-oci-image-helper`; `make proof-helpers`; `actionlint .github/workflows/release.yml`; `git diff --check` | Keep optional adapter assets optional, but require any emitted optional asset to be checksummed and attested by the final release/live verifier path. |
| 2026-05-26 | D6 release checksum de-duplication | `in_progress` | Made the final release checksum generation compatible with the stricter whole-release verifier. The release job now builds a unique checksum subject list from required `nimbus_*` release artifacts, optional `nimbus-bun-jsc-adapter-*.tar.gz` artifacts, `install.sh`, and `LICENSE` before running `sha256sum`, so future optional assets cannot create duplicate checksum subjects if naming overlaps. The static D6 helper now guards this duplicate-safe checksum shape. | `make verify-release-oci-image-helper`; `actionlint .github/workflows/release.yml`; `git diff --check` | Preserve the next tag's `checksums-sha256.txt` and live verifier output to prove every uploaded release asset is uniquely checksummed and attested. |
| 2026-05-26 | D6 public live-verifier recheck | `in_progress` | Re-ran the stricter live verifier against the current public stable release after the whole-release checksum/provenance hardening. The latest public tag remains `v0.1.32`, and it still fails before download because the GitHub Release has no top-level `LICENSE` asset. This keeps D6 open: local workflow/helper evidence is ready, but the required hosted release asset set and GHCR image evidence do not exist until the next tag runs the updated release workflow. | `bash scripts/verify-release-oci-image-live.sh --tag v0.1.32 --skip-smoke` failed as expected with `expected exactly one LICENSE release asset, found 0`; `gh release view --repo nimbus/nimbus --json tagName,url,assets,isDraft,isPrerelease` confirmed `v0.1.32` only has the pre-D6 assets | Cut the next tag from the D6 workflow changes and run `make verify-release-oci-image-live TAG=<tag> OUTPUT_DIR=<proof-dir>` without `--skip-smoke`. |
| 2026-05-26 | D6 live verifier asset completeness | `in_progress` | Tightened the post-tag live verifier so it proves whole-release asset integrity, not only the required D6 filenames. `scripts/verify-release-oci-image-live.sh` now verifies every `checksums-sha256.txt` entry, rejects duplicate or malformed checksum entries, rejects any uploaded release asset that is not the checksum manifest and is not listed in the checksum manifest, re-runs the optional Bun/JSC release-asset verifier with final checksums, and verifies GitHub/Sigstore release-asset attestations for every checksummed asset plus `checksums-sha256.txt`. The fixture helper now covers a complete release with a valid optional Bun/JSC adapter asset, missing top-level `LICENSE`, and an extra unchecksummed release asset. | `make verify-release-oci-image-live-helper` | Keep the stricter live verifier as the final post-tag gate; the next tag must preserve the `OUTPUT_DIR` with release-asset attestation JSONL and registry attestation JSON. |
| 2026-05-26 | D6 license and command guidance sweep | `in_progress` | Rechecked the legal/distribution convention for Nimbus's custom license and kept the plan's release invariant strict: every standalone release surface must carry the repo `LICENSE` text or install it in the packaging ecosystem's canonical documentation path, and the custom OCI license identity stays `LicenseRef-Nimbus-Community`. Cleaned current non-archive docs and future-facing research prompts that still showed the retired `nimbus serve` spelling, updated the Windows plan to use `nimbus.exe start`, and marked the macOS control-plane rationale as preserving `nimbus serve` only as historical evidence. `scripts/verify-release-oci-image-helper.sh` now guards current guidance files against stale `nimbus serve` command examples, `--privileged` application-image guidance, and `ghcr.io/nimbus/nimbus:latest` production examples. | npm package metadata docs confirmed the custom-license shape and top-level `LICENSE` tarball expectation; Debian Policy confirmed `/usr/share/doc/PACKAGE/copyright`; OCI annotations confirmed `org.opencontainers.image.licenses` is an SPDX expression; SPDX confirmed `LicenseRef-*` custom identifiers; `make verify-release-oci-image-helper`; `make proof-helpers`; `actionlint`; `git diff --check` | Keep D6 open until the next live tag publishes the top-level `LICENSE` asset, OCI evidence assets, and GHCR image, then run the live verifier against that tag. |
| 2026-05-26 | D6 attestation policy hardening | `in_progress` | Tightened the OCI image attestation evidence path so the release report and workflow pin the SLSA predicate explicitly with `--predicate-type https://slsa.dev/provenance/v1`, continue to reject self-hosted-runner attestations with `--deny-self-hosted-runners`, and require saved `gh attestation verify --format json` output to include verified timestamp evidence plus GitHub-hosted runner identity in both the verified identity and certificate material. The static helper now includes positive timestamp/runner evidence in its fixture, a negative fixture that deletes `verificationResult.verifiedTimestamps`, and a negative fixture that rewrites the runner environment to `self-hosted` to prove the verifier rejects both timestamp-free and non-GitHub-hosted attestation JSON. | `gh attestation verify --help` confirmed `--predicate-type`, `--bundle-from-oci`, and `--deny-self-hosted-runners`; real `v0.1.32` `checksums-sha256.txt` attestation JSON confirmed `verificationResult.verifiedTimestamps`, `verificationResult.verifiedIdentity.runnerEnvironment`, and `verificationResult.signature.certificate.runnerEnvironment`; `make verify-release-oci-image-helper`; `actionlint .github/workflows/release.yml`; `bash -n scripts/render-release-oci-image-report.sh scripts/verify-release-oci-image-report.sh scripts/verify-release-oci-image-assets.sh scripts/verify-release-oci-image-helper.sh`; `git diff --check` | Preserve the hosted `nimbus_oci_attestation.json` from the next tag and keep the final release verifier output with the bundle evidence. |
| 2026-05-26 | D6 live release audit | `in_progress` | Audited the current public latest release while keeping D6 open. `v0.1.32` is published and stable, but its asset set still contains only the pre-D6 binary archives, `install.sh`, and `checksums-sha256.txt`; it does not include the top-level `LICENSE` asset or any `nimbus_oci_image.txt`, `nimbus_oci_attestation.json`, `nimbus_oci_sbom.json`, or `nimbus_oci_vulns.sarif.json` release evidence. This confirms that local workflow/helper work is not enough to close D6; the next tag must run the new release graph and preserve hosted GHCR evidence before the phase can move to `done`. | `gh release view --repo nimbus/nimbus --json tagName,url,assets,isDraft,isPrerelease` returned latest `v0.1.32` with assets `checksums-sha256.txt`, `install.sh`, `nimbus_darwin_arm64.tar.gz`, `nimbus_linux_arm64.tar.gz`, `nimbus_linux_x86_64.tar.gz`, and `nimbus_windows_x86_64.zip` only | Cut the next release from the D6 workflow changes, then download and verify the final release bundle with `scripts/verify-release-oci-image-assets.sh --require-license --checksums`. |
| 2026-05-26 | release license payload contract | `in_progress` | Promoted license distribution into an explicit release invariant. Archives must include `LICENSE`; standalone release support assets such as `install.sh` are accompanied by a top-level GitHub Release `LICENSE`; deb/rpm packages install `/usr/share/doc/<package>/LICENSE`, include Debian's `/usr/share/doc/<package>/copyright` path, and carry package license metadata; the default OCI image installs `/usr/local/share/doc/nimbus/LICENSE` and carries `org.opencontainers.image.licenses=LicenseRef-Nimbus-Community` on both config and index; final-bundle verification requires `--require-license --checksums` so the released `LICENSE` asset is present, byte-identical to the repo license, and covered by checksums. The Linux package helper now directly compares every staged package license and copyright file to the repo `LICENSE` and checks both deb and rpm manifests for license metadata and doc-path installation. | `make verify-build-linux-release-packages-helper`; `make verify-release-archive-layout-helper`; `make verify-release-oci-image-helper`; `make verify-release-oci-image-build-helper`; `make proof-helpers`; `actionlint`; `git diff --check` | Preserve the live final-release evidence bundle after the next tag, including `LICENSE`, checksum coverage, package manifest proof, OCI image filesystem/metadata proof, and downloaded-bundle verifier output. |
| 2026-05-26 | D6 release checksum strictness | `in_progress` | Hardened the final OCI evidence verifier so checksum coverage is singular and unambiguous. `scripts/verify-release-oci-image-assets.sh` now rejects duplicate checksum subjects, malformed SHA-256 values, checksum mismatches, and missing `LICENSE` coverage when `--require-license --checksums` is used. The static release-image helper includes negative fixtures for duplicate checksum entries, malformed digests, and missing top-level `LICENSE` assets so the final release gate cannot pass with ambiguous or incomplete legal/evidence payloads. | `make verify-release-oci-image-helper`; `bash -n scripts/render-release-oci-image-report.sh scripts/verify-release-oci-image-report.sh scripts/verify-release-oci-image-assets.sh scripts/verify-release-oci-image-helper.sh`; `actionlint`; `make proof-helpers`; `make verify-release-oci-image-build-helper`; `git diff --check` | Preserve the downloaded next-tag `checksums-sha256.txt` beside the release assets and keep the successful final verifier output as release evidence. |
| 2026-05-26 | D6 live release verifier | `in_progress` | Added `scripts/verify-release-oci-image-live.sh` and `make verify-release-oci-image-live TAG=vX.Y.Z` as the post-tag proof path. The verifier checks the public GitHub Release metadata for the required binary, support, license, checksum, and `nimbus_oci_*` assets; downloads the bundle; runs the archive-layout and final OCI evidence verifiers with `--require-license --checksums`; re-runs registry-backed `gh attestation verify` against the digest recorded in `nimbus_oci_image.txt` with the release workflow, tag ref, SLSA predicate, and hosted-runner policy pinned; validates the live attestation JSON through the same asset verifier; and runs the published-image smoke test unless explicitly skipped. A current-release probe against `v0.1.32` fails before download with `expected exactly one LICENSE release asset, found 0`, proving the live verifier catches the known pre-D6 public asset set instead of silently accepting it. | `bash -n scripts/verify-release-oci-image-live.sh scripts/verify-release-oci-image-helper.sh`; `bash scripts/verify-release-oci-image-live.sh --tag v0.1.32 --skip-smoke` failed as expected on the missing top-level `LICENSE` asset; `make verify-release-oci-image-helper`; `make proof-helpers`; `actionlint`; `git diff --check` | Run the live verifier against the next tag and preserve its `OUTPUT_DIR` as the final D6 proof bundle. |
| 2026-05-26 | D6 live verifier fixture coverage | `in_progress` | Added `scripts/verify-release-oci-image-live-helper.sh` and `make verify-release-oci-image-live-helper` so the networked live verifier has deterministic local coverage. The helper builds a complete fake release bundle, stubs `gh release view`, `gh release download`, and `gh attestation verify`, runs the real live verifier through the success path with downloaded archive/OCI/LICENSE/checksum validation, verifies that registry image attestation JSON is written, verifies that all 11 required release assets get release-asset attestation checks, and then proves a fixture without top-level `LICENSE` is rejected before download. This pass also caught and fixed two pre-tag portability issues in the live verifier path: the fixture now compares against the verifier's canonicalized output directory, and `verify-release-oci-image-live.sh` uses a BSD/macOS-compatible `mktemp ...XXXXXX` template for per-asset attestation JSON. `make proof-helpers` now runs this helper. | `make verify-release-oci-image-live-helper`; `make verify-release-oci-image-helper`; `make proof-helpers`; `actionlint`; `make verify-release-oci-image-build-helper`; `git diff --check` | Keep this fixture as the no-network regression guard; run `make verify-release-oci-image-live TAG=<next-tag>` for the final hosted proof. |
| 2026-05-26 | final release checksum/provenance sweep | `in_progress` | Tightened the final release job after reviewing the merged artifact directory that now contains binary archives, optional adapter archives, top-level support files, and `nimbus_oci_*` evidence. After `checksums-sha256.txt` is generated, the workflow now re-runs `scripts/verify-bun-jsc-release-assets.sh --checksums artifacts/checksums-sha256.txt` so optional adapter assets get the same final checksum proof as downloaded-release verification. Release asset provenance now uses GitHub's documented `subject-checksums: artifacts/checksums-sha256.txt` mode so optional assets are attested exactly when they are listed in the checksum manifest, and a second attestation covers `checksums-sha256.txt` itself. | `make verify-release-oci-image-helper`; `actionlint`; `git diff --check` | Preserve the next tag's final release attestations and checksum file as evidence that all release-owned assets, including optional adapter payloads, were covered. |
| 2026-05-26 | D6 public release recheck | `in_progress` | Rechecked the latest public Nimbus release after the final checksum/provenance hardening. The latest stable release remains `v0.1.32`, and its asset list is still the pre-D6 set: `checksums-sha256.txt`, `install.sh`, `nimbus_darwin_arm64.tar.gz`, `nimbus_linux_arm64.tar.gz`, `nimbus_linux_x86_64.tar.gz`, and `nimbus_windows_x86_64.zip`. It still lacks the top-level `LICENSE` asset and all `nimbus_oci_*` evidence assets, so D6 cannot move to `done` until a new tag runs the updated release workflow. | `gh release view --repo nimbus/nimbus --json tagName,url,assets,isDraft,isPrerelease` | Cut the next tag from this release workflow and run `make verify-release-oci-image-live TAG=<tag> OUTPUT_DIR=<proof-dir>`. |
| 2026-05-26 | D6 release OCI image | `in_progress` | Added the first-class Nimbus application image lane to the tag-driven release workflow. The repo now owns a `Containerfile` with digest-pinned Dockerfile frontend and Debian runtime base, `scripts/render-release-oci-image-report.sh`, `scripts/verify-release-oci-image-report.sh`, `scripts/verify-release-oci-image-assets.sh`, `scripts/smoke-release-oci-image.sh`, `scripts/verify-release-oci-image-helper.sh`, `scripts/verify-release-oci-image-build-helper.sh`, `make verify-release-oci-image-helper`, and `make verify-release-oci-image-build-helper`. The workflow builds linux/amd64 and linux/arm64 images from the already-produced release archives, pushes per-arch tags, assembles `ghcr.io/nimbus/nimbus:<version>`, optionally updates `latest` for stable tags, pushes GitHub/Sigstore attestations to the registry, verifies them with `gh attestation verify --bundle-from-oci` constrained to the release workflow and tag ref, captures BuildKit SBOM evidence with subject image/tag/ref/digest metadata and explicit SPDX payloads for linux/amd64 and linux/arm64, runs a digest-pinned Trivy SARIF scan without mounting Docker credentials into the scanner container, smoke-tests `nimbus --version` plus foreground `nimbus start` + `/health`, verifies the `nimbus_oci_*` evidence bundle in the image publish job, verifies the final bundle again after `checksums-sha256.txt` exists, and uploads `nimbus_oci_image.txt`, `nimbus_oci_attestation.json`, `nimbus_oci_sbom.json`, and `nimbus_oci_vulns.sarif.json` as release assets. The final release job now also publishes the top-level `LICENSE` as a checksummed and attested release asset so standalone support assets such as `install.sh` are accompanied by the license text. The static helper proves the image definition uses OCI labels including `LicenseRef-Nimbus-Community`, the image jobs have scoped permissions and do not rebuild with Cargo, the report is deterministic and records stdout/stderr logging plus attestation/SBOM/vulnerability assets and a release-evidence verifier command that requires the final `LICENSE` asset, the asset verifier validates report/image digest/SLSA/certificate-backed repository/release-workflow/tag/per-platform-SBOM/SARIF/LICENSE/checksum coherence, and the smoke script checks UID/GID `10001:10001`, writable `/var/lib/nimbus`, image-installed license text, forbidden host tooling, admin-token rotation, and `/health`. The Docker-backed fixture helper builds the actual `Containerfile` from a release-layout archive and inspects/runs the image to prove entrypoint, default command, non-root runtime user, writable state volume, OCI labels, image-installed license text, stdout/stderr logs, and absence of forbidden host tools. Operator docs now document digest pinning, one-time admin-token rotation for non-loopback binds, Compose/Podman/Kubernetes probes, and no service-manager-in-container pattern. The release workflow now pins `actions/create-github-app-token@v3.2.0` so `client-id` inputs validate under `actionlint` and match the current upstream action contract; `actions/attest@v4` usage matches the current GitHub artifact attestation contract for default SLSA provenance and registry-pushed container attestations. | `bash scripts/verify-release-oci-image-helper.sh`; `make verify-release-oci-image-helper`; `make verify-release-oci-image-build-helper`; `actionlint .github/workflows/release.yml`; Ruby YAML parse for `.github/workflows/release.yml`; `git diff --check`; official Trivy docs confirmed `trivy image --format sarif --output ...`; pinned `ghcr.io/aquasecurity/trivy:0.69.3@sha256:bcc376de8d77cfe086a917230e818dc9f8528e3c852f7b1aff648949b6258d1c` ran against `ghcr.io/nimbus/nimbus:v0.1.33@sha256:360d058c09945d9a69ab921fec123255478758cf06674a2dfb57191aad6a9091` without mounted credentials and produced SARIF 2.1.0 with one `Trivy` run; Docker probe proving digest-pinned `# syntax=docker/dockerfile:1@sha256:87999aa3...` is accepted | Cut the next tagged release and preserve the hosted run URL, published GHCR digest, registry-pushed attestation verification JSON, per-platform SBOM, Trivy SARIF, `scripts/verify-release-oci-image-assets.sh --require-license --checksums` output, LICENSE evidence, and smoke evidence before marking D6 `done`. |
| 2026-05-25 | D2/D3 optional Bun/JSC adapter package lane | `in_progress` | Added an explicit optional package path for the in-process Bun/JSC adapter without changing the default `nimbus` package. `scripts/build-linux-release-packages.sh` now verifies a `nimbus-bun-jsc-adapter-linux-x86_64.tar.gz` archive, stages it under `/usr/libexec/nimbus/runtime/bun-jsc/<adapter_version>/`, points `current/` at that version, preserves the adapter SBOM/provenance evidence, and renders separate deb/rpm manifests for `nimbus-bun-jsc-adapter` with a dependency on `nimbus`. `linux-packages`, `apt-repo`, and `linux-distribution-release` only include the adapter when explicitly requested. `scripts/install.sh --with-bun-jsc` installs the Linux x86_64 release asset directly with release checksum, GitHub attestation, tar-layout, archive-internal checksum, SBOM, and provenance evidence verification; macOS Homebrew/cask remains a documented separate artifact lane until the tap has a package payload. | `bash -n scripts/build-linux-release-packages.sh scripts/verify-build-linux-release-packages-helper.sh scripts/install.sh scripts/verify-install.sh scripts/verify-install-helper.sh scripts/package-bun-jsc-adapter.sh scripts/verify-bun-jsc-adapter-package.sh scripts/verify-bun-jsc-release-assets.sh`; `dash -n scripts/install.sh`; Ruby YAML parse for `linux-packages.yml`, `apt-repo.yml`, `linux-distribution-release.yml`, and `ci.yml`; `bash scripts/verify-build-linux-release-packages-helper.sh`; `bash scripts/verify-install-helper.sh`; `bash scripts/verify-bun-jsc-release-assets-helper.sh`; `bash scripts/verify-artifact-provenance.sh` | Capture a real Linux package install proof after adapter release assets exist for the tag; add a Homebrew/tap payload or keep the separate artifact lane documented; promote COPR/SRPM adapter support only after a Fedora proof. |
| 2026-05-21 | D1/D2/D3 private krun stack | `done` | Closed the paired Linux krun runtime-stack refresh. `nimbus/nimbus-libkrun` now publishes `v1.18.1-nimbus.1` amd64/arm64 archives with private `libkrun`, bundled `libkrunfw`, checksums, and attestations; `nimbus/nimbus-crun` now publishes `v1.27.1-nimbus.2` built against that private stack. Nimbus direct install, verify, uninstall, package-build, apt-repo, COPR/SRPM, and release-mirror workflows now consume `nimbus + nimbus-libkrun + nimbus-crun` without distro or manual upstream libkrun for service execution. | `bash scripts/verify-install-helper.sh`; `bash scripts/verify-build-linux-release-packages-helper.sh`; `bash scripts/verify-build-apt-repository-helper.sh`; `bash scripts/verify-build-fedora-release-srpms-helper.sh` on Debian 13 `minicloud`; `sudo bash scripts/check-vmm-host.sh` reported `result supported`; `sudo env ... target/debug/deps/krun_linux_smoke-* krun_backend_image_backed_smoke_pulls_and_boots_busybox --ignored --nocapture` passed with non-loopback refusal for `192.168.4.29:18081`; Fedora/COPR helper rebuilt three SRPMs and installed/query-verified private-stack RPMs in Fedora 42 userspace. | Finish public apt/COPR publication and capture fresh installs from those public repos; keep future krun stack bumps on exact upstream-version tags such as `v1.27.1-nimbus.N`. |
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
