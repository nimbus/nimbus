# macOS Machine Image And Control Flows

Current source-backed reference for how Neovex:

- publishes the macOS guest VM image
- version-links that image to a host `neovex` release
- pulls and materializes the guest image on a macOS host
- splits control-plane responsibility between the macOS host and the Linux
  guest

The release ownership and host-consumption paths below are the current
checked-in `neovex-machine-os` flow. The settled current macOS contract is
narrower: use Podman's published machine image by pinned immutable reference or
digest for the shipped macOS bring-up path, and layer Neovex guest bootstrap
on top. A Neovex-owned image remains later follow-on work rather than the
current shipped contract.

Reviewed against:

- `.github/workflows/release.yml`
- `/Users/jack/src/github.com/agentstation/neovex-machine-os/.github/workflows/build.yml`
- [crates/neovex-bin/src/machine/mod.rs](/Users/jack/src/github.com/agentstation/neovex/crates/neovex-bin/src/machine/mod.rs)
- [crates/neovex-bin/src/machine/manager.rs](/Users/jack/src/github.com/agentstation/neovex/crates/neovex-bin/src/machine/manager.rs)
- [crates/neovex-bin/src/machine/api.rs](/Users/jack/src/github.com/agentstation/neovex/crates/neovex-bin/src/machine/api.rs)
- [crates/neovex-bin/src/machine/client.rs](/Users/jack/src/github.com/agentstation/neovex/crates/neovex-bin/src/machine/client.rs)
- [crates/neovex-bin/src/machine/backend.rs](/Users/jack/src/github.com/agentstation/neovex/crates/neovex-bin/src/machine/backend.rs)
- [crates/neovex-bin/src/service/mod.rs](/Users/jack/src/github.com/agentstation/neovex/crates/neovex-bin/src/service/mod.rs)
- `/Users/jack/src/github.com/agentstation/neovex-machine-os/scripts/package-oci.sh`
- `/Users/jack/src/github.com/agentstation/neovex-machine-os/scripts/publish.sh`

## Overview

The current macOS architecture is a hybrid control plane:

- the macOS host owns the main Neovex server, runtime, storage, and
  `ctx.services.*` activation path
- the Linux guest owns a narrow machine API and standard-container execution
  lane for service workloads
- the current bring-up image comes from Podman's published machine-image
  stream on Quay, while `agentstation/neovex-machine-os` remains the later
  image-ownership track
- the host `neovex` release owns the desired Podman image reference/digest and
  the matching Linux guest `neovex` asset for the local host architecture

The checked-in macOS default image reference recorded by `neovex machine init`
is currently:

```text
docker://quay.io/podman/machine-os@sha256:02ce56eb3a353f3d909eeb6742db7052e13fcad01937ef9536d41178c4865000
```

Current contract note:

- Podman's published image is the current bring-up contract for macOS
- the host `neovex` release owns the desired image digest and the
  matching Linux guest `neovex` binary asset
- `neovex machine start` is the primary convergence path: cache missing
  artifacts, boot or rebuild from the desired image, sync the guest binary by
  hash, and validate the forwarded machine API before reporting success
- Neovex-owned image publishing remains later follow-on work instead of the
  current shipped macOS contract

## Flow 1: Current Checked-In Host Release To Guest Image Release

```mermaid
flowchart TD
    A["git push tag vX.Y.Z to agentstation/neovex"] --> B["neovex release workflow"]
    B --> C["verify cross-repo release contract"]
    C --> D["build neovex binaries for supported targets"]
    D --> E["run reusable machine-os workflow as contract build<br/>publish=false"]
    D --> F["create agentstation/neovex GitHub Release"]
    F --> G["dispatch native agentstation/neovex-machine-os release<br/>release_tag=vX.Y.Z publish=true"]
    G --> H["neovex-machine-os build workflow"]
    H --> I["build/publish machine-os raw guest artifact<br/>current checked-in repo flow"]
    I --> J["emit bootable raw-image metadata for the macOS release flow"]
    J --> K["prove parity against Podman machine image when needed"]
    K --> L["wrap raw disk as OCI layout<br/>annotations include disktype=raw"]
    L --> M["publish to GHCR<br/>ghcr.io/agentstation/neovex-machine-os:vX.Y.Z"]
    M --> N["create neovex-machine-os GitHub Release"]
```

### What Each Repo Owns

- `agentstation/neovex`
  owns host CLI/server/runtime binaries and the host GitHub Release
- `agentstation/neovex-machine-os`
  owns the guest VM image build, GHCR publish, and machine-image GitHub
  Release

### Why The Flow Is Two-Phase

The host repo uses the machine-os workflow twice for different reasons:

1. reusable workflow call
   proves the cross-repo build contract against the exact host release inputs
2. native workflow dispatch in `agentstation/neovex-machine-os`
   lets the machine-image repo own its own GHCR publish and GitHub Release

That keeps the release ownership aligned with the repo boundary, which mirrors
Podman's `containers/podman` plus `containers/podman-machine-os` split.

## Flow 2: Current Checked-In Machine Image Packaging Flow

```mermaid
flowchart LR
    A["Current neovex-machine-os recipe"] --> B["current repo-owned machine artifact packaging flow"]
    B --> C["package-oci.sh"]
    C --> D["OCI image layout"]
    D --> E["manifest annotations"]
    E --> I["disktype=raw"]
    E --> J["org.opencontainers.image.source"]
    E --> K["io.neovex.machine.attestation.repository"]
    E --> L["io.neovex.machine.neovex.version"]
    D --> M["publish.sh"]
    M --> N["skopeo copy"]
    N --> O["docker://ghcr.io/agentstation/neovex-machine-os:vX.Y.Z"]
```

Current decision:

- Podman's published machine image is the current macOS bring-up contract
- Neovex guest bootstrap is layered on top of that image for the current
  closeout path
- the checked-in `neovex-machine-os` packaging flow above is therefore future
  image-ownership work, not the current shipped macOS contract
- any separate `fedora-bootc` image pipeline work in
  `agentstation/neovex-machine-os` remains a future supply-side direction, not
  the current shipped macOS contract

Current implementation note:

- as of 2026-04-16, the checked-in macOS default already points at the pinned
  immutable Podman digest above
- the full start-time convergence contract has now been proved end to end,
  including guest-binary sync, forwarded machine-API readiness, host service
  control, runtime `ctx.services.<name>.port`, and the supported recreate
  drill on isolated roots

### Important Packaging Contract

The host machine manager does not pull an arbitrary OCI image and hope it is a
disk. It looks for a specific artifact shape:

- operating system: `linux`
- architecture: current host-compatible machine arch
- manifest annotation: `disktype=raw`
- exactly one disk layer
- disk layer title suffix such as `.raw`, `.raw.gz`, or `.raw.zst`

That packaging contract is what lets the host treat GHCR as a versioned VM
image registry instead of inventing a separate image service.

## Flow 3: How `neovex` Pulls The VM Image On macOS

```mermaid
flowchart TD
    A["neovex machine init"] --> B["config.json records image source"]
    B --> C["docker://quay.io/podman/machine-os:<pinned-ref-or-digest>"]
    C --> D["neovex machine start"]
    D --> E["converge desired artifacts"]
    E --> F["ensure machine image artifact is cached"]
    E --> G["ensure matching linux guest neovex asset is cached"]
    F --> H["resolve_bootable_image_path()"]
    H --> I{"image source kind"}
    I -->|OciReference| J["materialize_oci_image()"]
    I -->|HttpUrl| K["materialize_http_image()"]
    I -->|LocalDisk| L["use local disk directly"]

    J --> M["pull OCI manifest/index"]
    M --> N["select linux current-arch manifest with disktype=raw"]
    N --> O["pull one disk layer blob into image cache"]
    O --> P["verify digest"]
    P --> Q["decompress cached blob if raw.gz or raw.zst"]
    Q --> R["persist materialized raw disk"]

    K --> S["download to temp file"]
    S --> T["decompress gzip if needed"]
    T --> R

    L --> U["launch uses local raw disk path"]
    R --> V["boot or rebuild machine from desired image"]
    U --> V
    V --> W["sync guest /usr/local/bin/neovex by hash"]
    W --> X["validate forwarded machine API readiness"]
    X --> Y["krunkit-backed machine is ready"]
```

### Where The Image Comes From

By default on the current macOS contract, it comes from Podman's
published machine-image stream:

```text
quay.io/podman/machine-os
```

The host supports three source kinds:

- OCI reference
- `http(s)` URL
- local raw disk path

The OCI reference is the canonical release path. The target contract is an
immutable pinned Podman digest owned by the host `neovex` release, not a
floating tag.

### Where The Image Lands On Disk

For a machine named `default`, Neovex reserves:

- cache directory:
  `state/default/images/`
- materialized bootable raw disk:
  `state/default/images/default.raw`

The manager reuses `default.raw` if it already exists.

## Flow 4: macOS Machine Launch Plumbing

```mermaid
flowchart LR
    A["macOS host"] --> B["neovex machine start"]
    B --> C["materialized raw disk"]
    B --> D["generated.ign or explicit ignition file"]
    B --> E["start gvproxy"]
    B --> F["start krunkit"]
    E --> G["host runtime root under /tmp/neovex"]
    F --> G

    G --> H["<machine>-api.sock"]
    G --> I["<machine>.sock ready socket"]
    G --> J["<machine>-ignition.sock"]
    G --> K["<machine>-gvproxy.sock"]
    G --> L["<machine>-krunkit.sock"]
    G --> M["<machine>.log and helper logs"]

    F --> N["Linux guest VM"]
    J --> N
    I --> N
    K --> N
```

### Socket Roles

- `<machine>-ignition.sock`
  first-boot ignition delivery
- `<machine>.sock`
  machine-ready signal
- `<machine>-api.sock`
  host-local forwarded guest machine API
- `<machine>-gvproxy.sock`
  gvproxy networking socket used by krunkit virtio-net
- `<machine>-krunkit.sock`
  krunkit REST/control endpoint

### Transport Reality

`vsock` exists on macOS here, but its role is narrow:

- first-boot bootstrap
- machine-ready signaling

It is not the generic host API transport.

The host control path uses:

- `gvproxy`
- SSH-backed forwarded Unix socket
- guest target socket: `/run/neovex/neovex.sock`

## Flow 5: Host Runtime To Guest Service Execution

```mermaid
flowchart TD
    A["developer or runtime code on macOS host"] --> B["host neovex server"]
    B --> C["ctx.services.db.port access"]
    C --> D["SandboxServiceManager on host"]
    D --> E["ForwardedMachineApiSandboxBackend"]
    E --> F["MachineApiClient"]
    F --> G["host <machine>-api.sock"]
    G --> H["gvproxy SSH-forwarded socket"]
    H --> I["guest /run/neovex/neovex.sock"]
    I --> J["guest neovex machine api"]
    J --> K["guest ContainerSandboxBackend"]
    K --> L["OCI materializer + conmon + crun + netavark + aardvark-dns"]
    L --> M["guest standard Linux container"]
    M --> N["published localhost port through gvproxy"]
    N --> O["host returns bound service endpoint"]
```

### Current Responsibility Split

Host:

- main Neovex API
- runtime execution
- storage
- `ctx.services.*` activation
- service catalog and manager orchestration

Guest:

- machine API
- image-backed service sandbox execution through in-process OCI materialization
- standard-container runtime binaries
- published port plumbing for service workloads

This is intentionally not "guest Neovex owns the full product surface". The
current architecture keeps the authoritative Neovex server on the macOS host
and forwards only the service-execution seam into the guest.

## Flow 6: Linux Production Contrast

```mermaid
flowchart TD
    A["Linux host neovex"] --> B["SandboxServiceManager"]
    B --> C["krun-backed sandbox backend"]
    C --> D["conmon -> crun -> libkrun or KVM path"]
    D --> E["per-service microVM"]
    E --> F["published endpoint"]
    F --> G["ctx.services.<name>.port"]
```

macOS is different:

- one Linux machine VM per developer environment
- guest standard containers for service workloads
- host Neovex runtime/server remains on macOS

Linux production:

- no outer machine VM
- service workloads can be real per-service microVMs

## Proof Helpers

The repo now owns two checked-in macOS proof collectors for this flow:

- `make collect-neovex-machine-guest-proof`
  captures guest-image and guest machine-API proof through `neovex machine ssh`
- `make collect-neovex-machine-service-proof`
  captures host `<machine>-api.sock` health/capabilities, direct forwarded
  machine-API sandbox listing, host `neovex service up/list/inspect/ps/logs/down`,
  and an optional localhost published-port probe

The repo also owns two checked-in operator drill helpers for the same contract:

- `make collect-neovex-machine-diagnostics`
  captures the persisted config/state records plus the flat short runtime-root
  socket, pid, and log inventory for an isolated machine root
- `make recreate-neovex-machine`
  performs the supported stop/remove/init/start repair drill on isolated roots;
  by default it follows the current pinned machine-image contract, while
  `IMAGE=...` remains an explicit diagnostic override only

## Practical Summary

If you want the shortest accurate explanation:

1. A `neovex` host release owns two desired macOS artifacts: a pinned Podman
   machine-image reference or digest and a matching Linux guest `neovex`
   binary asset for the local host architecture.
2. `neovex machine init` records the machine contract; the checked-in default
   currently uses Podman's machine image stream on Quay.
3. `neovex machine start` checks the local caches, pulls any missing image or
   guest-binary artifact, and materializes the bootable raw disk.
4. If the machine's recorded base image already matches the desired digest, the
   host reuses the machine; if it does not match, the host performs a
   controlled rebuild or recreate from the desired image.
5. After boot, the host hash-checks and syncs
   `/usr/local/bin/neovex` inside the guest. On FCOS that is the writable
   `/var/usrlocal/bin/neovex` path with executable labeling, and then the host
   validates the forwarded machine API.
6. The host Neovex server talks to the guest machine API through a forwarded
   Unix socket, and the guest starts standard Linux containers for declared
   services.
