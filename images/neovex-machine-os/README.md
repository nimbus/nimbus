# Neovex Machine OS

This directory owns the Neovex macOS guest-image recipe and registry-artifact
contract.

It is intentionally Podman-aligned:

- base image: Fedora CoreOS
- build flow: `podman build` -> `rpm-ostree compose build-chunked-oci`
- guest workload model: standard Linux containers, not nested guest-side krun
  microVMs
- first-boot bootstrap: Ignition
- host file sharing: virtiofs

This recipe is for Linux hosts and CI, not for day-to-day image builds on a
developer Mac. The intended product shape is Podman-like:

- Linux/CI builds and publishes versioned guest artifacts
- macOS downloads, caches, and reuses those artifacts by default
- local raw-disk and URL overrides remain available for diagnostics

## Hosting And Versioning

Neovex should follow Podman's build/consume shape here, but use delivery
infrastructure that fits this repo:

- GitHub Actions on hosted Linux verifies the recipe/build/package contract
- a dedicated self-hosted `linux arm64 neovex-machine-os` runner builds the
  real Apple Silicon guest image
- GHCR hosts the published raw-disk OCI artifact
- GitHub Actions artifacts retain the build/publish evidence bundle

Recommended publish policy:

- immutable release tags such as
  `ghcr.io/agentstation/neovex-machine-os:v0.1.0`
- a moving `stable` alias for the newest supported machine artifact
- an optional moving `latest` alias for the newest published artifact
- `dev` only for manual validation and pre-release testing

For enterprise trust, macOS machines should ultimately consume a versioned
artifact by digest, even if the user-facing default begins from a moving alias
such as `stable`.

Recommended Git trigger:

- push a dedicated repo tag such as `machine-os/v0.1.0`
- let `.github/workflows/neovex-machine-os.yml` derive the immutable GHCR
  reference `ghcr.io/agentstation/neovex-machine-os:v0.1.0`
- attach `stable` automatically from that release lane

The current machine-manager seam already reserves the stable reuse target a
future downloader should populate:

- `~/.local/state/neovex/machine/<name>/images/<name>.raw`

When a machine config points at a published OCI reference or remote URL, the
manager can already reuse that reserved raw disk if it is present there.

Current host-side materialization support:

- absolute local raw disks can be used directly
- `http(s)` URLs can be downloaded into the reserved raw-disk path
- `.gz` URL downloads are decompressed into that reserved raw-disk path
- published OCI references can be pulled into the machine image cache and
  materialized into the reserved raw-disk path
- OCI blobs are selected by linux/current-arch plus `disktype=raw`, verified by
  sha256, and decompressed from gzip or zstd when needed

## Registry Artifact Contract

The registry-published machine image is not a generic bootc/container image.
The macOS machine manager currently consumes a more specific shape:

- an OCI image index
- a linux/current-arch manifest descriptor annotated with `disktype=raw`
- one layer blob containing the bootable raw disk artifact
- `org.opencontainers.image.title` set to the raw-disk filename

That means the supply side has two distinct steps:

1. build the Fedora CoreOS-based guest image and emit a raw disk such as
   `neovex-machine-os.raw.gz`
2. wrap that raw disk in the OCI layout/index shape the manager already pulls

Neovex does not vendor Podman's GPL-licensed `custom-coreos-disk-images`
submodule. Instead, the Linux build lane can resolve the pinned upstream
helper revision on demand through:

```bash
bash scripts/resolve-custom-coreos-disk-images.sh \
  --checkout-dir /tmp/neovex-machine-os-helper
```

Neovex invokes that helper with `bash`, not `sh`. The pinned upstream helper
is Bash-specific (`#!/usr/bin/bash`, `set -euo pipefail`, associative arrays),
so this keeps the build lane portable across Linux hosts where `/bin/sh` is
not Bash.

Repo-owned entrypoints for that packaging/publish lane:

```bash
bash scripts/package-neovex-machine-os-oci.sh \
  --build-output-dir /tmp/neovex-machine-os \
  --image-reference docker://ghcr.io/agentstation/neovex-machine-os:v0.1.0 \
  --layout-dir /tmp/neovex-machine-os/oci-layout

bash scripts/publish-neovex-machine-os.sh \
  --layout-dir /tmp/neovex-machine-os/oci-layout \
  --image-reference docker://ghcr.io/agentstation/neovex-machine-os:v0.1.0 \
  --additional-reference docker://ghcr.io/agentstation/neovex-machine-os:stable \
  --release-dir /tmp/neovex-machine-os/release
```

The guest bootstrap units themselves live in
`crates/neovex-bin/src/machine/assets/` and are injected by the host-generated
Ignition payload. The current bootstrap contract now installs guest
`neovex.socket` plus `neovex.service`, reserving `/run/neovex/neovex.sock`
for the narrow Neovex machine API. That keeps the public Neovex API on the
macOS host while the guest exposes only the service-execution seam it needs.
This image recipe owns the guest package set and the durable
filesystem/runtime contract those units depend on.

## Guest Package Contract

The Neovex macOS guest must contain:

- `neovex` at `/usr/local/bin/neovex`
- `crun`, `conmon`, `buildah`, `containers-common`, `netavark`,
  `aardvark-dns`, `fuse-overlayfs`, and `uidmap`
- `openssh-server` for guest SSH verification and operator recovery
- `socat` for the Ignition `ready.service` callback over `vsock`
- `git-core` and `cpp` for common Dockerfile / Buildah build paths

The guest must not depend on:

- a guest-side `krun` runtime path
- a guest-side Podman daemon
- nested `/dev/kvm` availability

## Mount Strategy

The macOS host shares project paths into the guest through `virtiofs`.

For each configured host:guest volume:

1. `krunkit` exposes a `virtio-fs` device tagged with the hashed guest target.
2. Ignition installs a corresponding one-shot mount unit in the guest.
3. The guest mount unit creates the target directory, mounts the share with the
   same SELinux context Podman uses for macOS machine mounts, and unmounts it
   on stop.

The generated units also bracket those mounts with immutable-root-off /
immutable-root-on helpers, following Podman's Fedora CoreOS pattern.

## Build Flow

This recipe is Linux-only and expects a Linux `neovex` binary to be staged into
the image build context.

Preferred repo-owned entrypoint:

```bash
sudo bash scripts/build-neovex-machine-os.sh \
  --neovex-binary /absolute/path/to/neovex-linux-aarch64 \
  --output-dir /tmp/neovex-machine-os
```

That wrapper can also build the Linux `neovex` binary first:

```bash
sudo bash scripts/build-neovex-machine-os.sh \
  --cargo-profile release \
  --output-dir /tmp/neovex-machine-os
```

To produce the bootable raw disk artifact non-interactively from a Linux/CI
builder, point the wrapper at the pinned upstream helper checkout:

```bash
sudo bash scripts/build-neovex-machine-os.sh \
  --cargo-profile release \
  --output-dir /tmp/neovex-machine-os \
  --fetch-custom-coreos-disk-images /tmp/neovex-machine-os-helper
```

Repo-owned GitHub Actions workflow:

- `.github/workflows/neovex-machine-os.yml`
- hosted `ubuntu-latest` verifies the shell/helper/build contract
- a dedicated self-hosted `linux arm64 neovex-machine-os` runner builds the
  Apple Silicon guest image for real via `workflow_dispatch`
- that same workflow can optionally publish the packaged raw-disk OCI artifact
  to `ghcr.io/agentstation/neovex-machine-os`
- release publishes should use an immutable version tag first and may also
  attach moving aliases such as `stable` or `latest`

Direct recipe entrypoint:

```bash
sudo bash images/neovex-machine-os/build.sh \
  --neovex-binary /absolute/path/to/neovex-linux-aarch64 \
  --output-dir /tmp/neovex-machine-os
```

That script produces:

- a bootc-style OCI archive at
  `/tmp/neovex-machine-os/neovex-machine-os.ociarchive`
- optionally, a CoreOS raw disk image if
  `--custom-coreos-disk-images /absolute/path/to/custom-coreos-disk-images.sh`
  is provided
- when that raw disk is produced, a compressed publishable artifact at
  `/tmp/neovex-machine-os/neovex-machine-os.raw.gz`
- a `summary.txt` provenance record that includes the source `neovex` binary
  path plus sha256, the recipe file sha256s, and the emitted OCI/raw artifact
  sha256s

If the raw disk is produced, package it into the manager-consumable registry
shape with:

```bash
bash scripts/package-neovex-machine-os-oci.sh \
  --build-output-dir /tmp/neovex-machine-os \
  --image-reference docker://ghcr.io/agentstation/neovex-machine-os:v0.1.0 \
  --layout-dir /tmp/neovex-machine-os/oci-layout
```

## Verification

The repo-owned contract verifier is:

```bash
bash scripts/verify-neovex-machine-os-recipe.sh
```

The repo-owned wrapper verifier is:

```bash
bash scripts/verify-neovex-machine-os-build-helper.sh
```

The repo-owned OCI layout verifier is:

```bash
bash scripts/verify-neovex-machine-os-oci-layout-helper.sh
```

The repo-owned custom-coreos resolver verifier is:

```bash
bash scripts/verify-custom-coreos-disk-images-resolver-helper.sh
```

The repo-owned publish-wrapper verifier is:

```bash
bash scripts/verify-neovex-machine-os-publish-helper.sh
```

That lane checks:

- shell syntax for the image build scripts
- required package contract entries
- expected `rpm-ostree` / `bootc` build flow anchors
- expected bootstrap asset references and guest `neovex` install path
- expected `disktype=raw` OCI index/manifest packaging for published machine
  artifacts
