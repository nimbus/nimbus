# Plan: macOS Machine Support — Podman-Aligned Developer Machines

Canonical execution plan for finishing Neovex macOS support for engineers who
develop on Apple Silicon Macs and deploy to Linux production hosts.

Reviewed against:

- `docs/reference/microvm-service-baseline.md`
- `docs/research/macos-host-vs-guest-control-plane-rationale.md`
- `docs/plans/distribution-plan.md`
- `crates/neovex-bin/src/main.rs`
- `crates/neovex-bin/src/service/mod.rs`
- `/Users/jack/src/github.com/containers/podman/pkg/machine/provider/platform_darwin.go`
- `/Users/jack/src/github.com/containers/podman/pkg/machine/libkrun/stubber.go`
- `/Users/jack/src/github.com/containers/podman/pkg/machine/apple/apple.go`
- `/Users/jack/src/github.com/containers/podman/pkg/machine/apple/vfkit.go`
- `/Users/jack/src/github.com/containers/podman/pkg/machine/apple/ignition.go`
- `/Users/jack/src/github.com/containers/podman/pkg/machine/shim/networking.go`
- `/Users/jack/src/github.com/containers/podman/pkg/machine/shim/networking_unix.go`

---

## Status

- **Status:** `active`
- **Primary owner:** this plan
- **Activation gate:** met on 2026-04-13 after the Linux microVM runtime and
  Compose-backed service control plane were archived into the stable baseline
- **Related plans:**
  - `docs/reference/microvm-service-baseline.md` — current landed Linux
    microVM and service-control baseline
  - `docs/plans/distribution-plan.md` — packaging/distribution umbrella; this
    plan owns the detailed execution of Channel 4
  - `docs/plans/archive/vmm-infrastructure-plan.md` — historical Linux/macOS
    validation evidence, including the short-runtime-dir and machine-recreate
    findings on the current Mac host

## Current Assessed State

- Linux production support is complete and stable in the landed baseline:
  Neovex starts krun-backed service microVMs on Linux and exposes them through
  the server-owned `ctx.services.*` surface.
- macOS support is not complete. The repo now implements the `neovex machine ...`
  command surface, typed machine config/state/runtime-root model, and the
  direct `krunkit` + `gvproxy` host-manager seam. MAC3 is now effectively a
  host-manager problem rather than a guest-image problem: a clean Fedora
  CoreOS raw disk decompressed from Podman's libkrun cache boots under the
  Neovex manager on the current Mac host, reaches machine-ready, reaches guest
  SSH, and stops/removes cleanly without Podman owning the runtime. The
  remaining blocker is MAC4: the guest image contract still needs a
  Neovex-owned artifact that actually installs `/usr/local/bin/neovex`.
- The stable machine topology decision remains unchanged: macOS is a developer
  delivery surface only, Neovex still boots exactly one Linux machine VM, and
  service workloads inside that guest still run as standard Linux containers.
- What changed after the DX review is the **control-plane placement** for the
  remaining work. `MAC5` and `MAC6` now target a hybrid model where the
  authoritative Neovex API/runtime/storage loop stays on the macOS host while
  the guest owns only a narrow service-runtime seam for buildah/conmon/crun.
- The current docs did still overstate `vsock` in some Channel 4 wording.
  Source review shows Podman's Apple machine path is more specific than that:
  `vsock` is real on macOS, but it is not the general-purpose host↔guest API
  story that earlier wording implied.
- The repo already owns useful macOS diagnostic and recovery helpers derived
  from real host validation:
  - `scripts/check-podman-machine-socket-paths.sh`
  - `scripts/collect-podman-machine-diagnostics.sh`
  - `scripts/validate-podman-machine-readiness.sh`
  - `scripts/recreate-podman-machine.sh`
- The repo now also owns the first Neovex-specific manager helpers for MAC3:
  - `scripts/collect-neovex-machine-diagnostics.sh`
  - `scripts/recreate-neovex-machine.sh`
  - `scripts/verify-neovex-machine-diagnostics-helper.sh`
  - `scripts/verify-neovex-machine-recreate-helper.sh`
  - those helpers now model the same flat short runtime-root layout the code
    uses: host `<machine>-api.sock`, sibling helper sockets, and sibling
    `*.log` / `*.pid` files under one short runtime root
- The repo now also owns the first Neovex-specific guest bootstrap generator
  for MAC4: when a machine config does not point at an explicit ignition file,
  `neovex-bin` renders a Neovex-owned ignition payload with Podman-aligned
  ready signaling, guest `neovex.socket` plus `neovex.service` units, and
  virtiofs mount units derived from the recorded machine volumes.
- The repo now also owns the first checked-in Neovex guest-image recipe under
  `images/neovex-machine-os/`, plus a verifier script
  `scripts/verify-neovex-machine-os-recipe.sh` that checks the FCOS/bootc
  build shape, guest package contract, and the bootstrap asset anchors used by
  the generated Ignition path.
- The repo now also owns a canonical Linux/CI wrapper for that guest-image
  recipe: `scripts/build-neovex-machine-os.sh`, plus
  `scripts/verify-neovex-machine-os-build-helper.sh` and the
  `make build-neovex-machine-os` / `make verify-neovex-machine-os-build-helper`
  entrypoints. That makes MAC4's remaining supply-side work explicit and
  repeatable instead of leaving Linux builders to reconstruct the recipe
  invocation by hand.
- The repo now also owns the packaging/publish seam that matches the existing
  macOS machine-manager OCI pull contract. `scripts/package-neovex-machine-os-oci.sh`
  wraps a produced raw disk into an OCI image layout with
  linux/current-arch plus `disktype=raw`, and
  `scripts/publish-neovex-machine-os.sh` pushes that layout to a registry. The
  repo also now owns deterministic verifiers plus Makefile entrypoints for
  that lane. The remaining MAC4 blocker is therefore no longer "what should a
  published artifact look like?" It is the live Linux-host build/publish
  execution that produces the real guest image and registry evidence.
- The repo now also owns the pinned external raw-disk-helper resolution seam
  for that Linux build lane. Because Neovex does not vendor Podman's
  GPL-licensed `custom-coreos-disk-images` helper, the build wrapper can now
  resolve the exact upstream commit Podman currently pins before invoking the
  image recipe. That keeps the raw-disk build step reproducible without
  copying that helper into the repo.
- The repo now also owns a dedicated GitHub Actions workflow for the machine
  image contract. `.github/workflows/neovex-machine-os.yml` now splits this
  work the same way the plan does: hosted Linux verifies the helper/build
  contract quickly, while a dedicated self-hosted Linux ARM64 runner owns the
  real Fedora CoreOS image build, raw-disk OCI packaging, and optional GHCR
  publish path for the Apple Silicon guest artifact.
- The repo now also owns an explicit shared OCI-runtime seam inside
  `neovex-sandbox`. The generic buildah/image-lowering, command-spec,
  conmon-launch, and published-port allocation helpers no longer live only
  under `backends/krun/`; they now live under `backends/oci/`. That keeps the
  current krun backend working unchanged while giving MAC4 a canonical place
  to land the future guest standard-container backend.
- The repo now also owns the first real guest standard-container backend under
  `crates/neovex-sandbox/src/backends/container/`, and `neovex machine api`
  now instantiates that backend instead of stopping at a placeholder
  capability contract. That backend now owns the first Podman-shaped guest
  networking slice too: it auto-allocates published ports from the shared OCI
  port range, writes an explicit Linux network namespace into the OCI bundle,
  and carries Neovex-owned `netavark` plus optional `gvproxy`-forwarder
  plumbing instead of the earlier guest host-network shortcut. MAC4 and MAC5
  still remain `in_progress` because this path still needs live guest-image
  proof and real macOS host-local connectivity evidence, but the guest machine
  API is now backed by a real executor with a concrete bridge/published-port
  design instead of a stub.
- The repo now also owns a MAC4 guest-image proof helper:
  `scripts/collect-neovex-machine-guest-proof.sh`, plus
  `scripts/verify-neovex-machine-guest-proof-helper.sh` and the
  `make collect-neovex-machine-guest-proof` /
  `make verify-neovex-machine-guest-proof-helper` entrypoints. That gives the
  control plane a repeatable host-local lane for proving a booted guest image
  actually contains `/usr/local/bin/neovex`, the expected standard-container
  runtime binaries, the guest `neovex.socket` / `neovex.service` units, the
  guest machine-API health/capabilities surface, the shared virtiofs target,
  and the host-side first-boot log tail.
- The repo now also owns the first Podman-aligned typed guest-image source
  model in `neovex machine` config: published OCI reference by default, with
  explicit local raw-disk and `http(s)` override shapes preserved for
  diagnostics. The missing MAC4 step is not the config model anymore. It is
  now the Neovex-owned guest artifact itself: a built/published image that
  includes the guest executable and can answer behind `neovex.socket`.
- That typed model already reuses a reserved materialized disk path under the
  machine state root when one exists, so the eventual downloader/cache lane has
  a stable target to populate instead of inventing another one later.
- The repo now also owns the first complete image-materialization slice for
  that lane: published OCI references and `http(s)` image sources now both
  materialize directly into the reserved raw-disk path. The OCI path now
  follows Podman's `ocipull` shape closely: select a linux/arch
  `disktype=raw` layer, verify the downloaded blob digest, cache it under the
  machine image cache root, then decompress gzip/zstd artifacts into the
  launchable raw disk. The remaining MAC4 gap is no longer image download; it
  is the guest artifact contents.
- The Linux guest-image recipe now also emits a compressed raw artifact when it
  produces a raw disk, so the build side and the current macOS URL
  materialization lane now share one concrete artifact shape:
  `neovex-machine-os.raw.gz`.
- Real host validation on 2026-04-13 refined the earlier bootstrap diagnosis.
  Reusing a mutated Podman machine raw disk as if it were a pristine base
  artifact can still boot into FCOS emergency mode under the Neovex manager,
  but a pristine raw decompressed from Podman's cache does not show that
  failure. With that pristine raw plus Podman's ignition file, Neovex reaches
  machine-ready, guest SSH, and clean host-managed stop/remove on the current
  Mac host. A first pristine generated-ignition run failed SSH readiness once,
  but the next fresh generated-ignition run on `/tmp/neovex-libkrun-pristine3.raw`
  reached `running` / `ready` under the Neovex manager as well. Durable
  conclusion: the remaining MAC4 blocker is guest image contents and guest API
  packaging, not first-boot ignition delivery or basic host lifecycle.
- The public `neovex machine ssh` path now applies the same localhost-only
  host-key bypass options that the internal guest-SSH readiness probe already
  used (`IdentitiesOnly=yes`, `StrictHostKeyChecking=no`,
  `UserKnownHostsFile=/dev/null`, `CheckHostIP=no`). That makes the operator
  surface match the actual readiness path instead of failing on first-contact
  host-key prompts.
- The guest-proof helper is now explicitly best-effort so a missing guest
  `neovex` binary does not abort the rest of the MAC4 evidence capture. It
  now records more deterministic proof too: guest `neovex --version`,
  guest `neovex` sha256, per-binary presence lines for the required runtime
  toolchain, machine-readable `systemctl show` output for `neovex.socket` and
  `neovex.service`, machine-API health/capabilities, the shared virtiofs
  mount, and the host machine log tail. On the successful Podman-reference
  boot, that helper now proves the current base FCOS fixture has `conmon`,
  `crun`, `fuse-overlayfs`, and a working `/Users` virtiofs mount, but does
  not yet have `/usr/local/bin/neovex`, `/run/neovex/neovex.sock`, `buildah`,
  `netavark`, or `aardvark-dns`.
- The Linux machine-image build summary is now more suitable as a durable
  artifact contract for MAC4 closeout. `images/neovex-machine-os/build.sh`
  records sha256s for the staged Linux `neovex` binary, the checked-in recipe
  files, the emitted OCI archive, and the optional raw/compressed raw disk in
  `summary.txt`. That gives the Linux build lane and the later macOS guest
  proof lane a concrete provenance seam they can compare instead of relying
  only on paths and tags.
- The intended publish/consume model is also explicit now: keep the
  Podman-like split where Linux/CI builds the guest artifact and macOS
  consumes it, but use Neovex-native delivery infrastructure. That means
  GitHub-hosted Actions for contract verification, a dedicated self-hosted
  `linux arm64 neovex-machine-os` runner for the real Apple Silicon build, and
  GHCR for the published raw-disk OCI artifact. Immutable version tags are the
  release truth; moving aliases such as `stable` or `latest` are convenience
  entrypoints on top.
- The repo behavior now follows that policy in two concrete places:
  `neovex machine init` defaults to the `stable` machine-image alias for
  consumption, and the machine-os workflow now validates that real publishes
  use immutable version tags while letting operators attach `stable` and
  `latest` aliases deliberately.
- The workflow now also has a non-interactive release trigger: pushing a
  dedicated repo tag such as `machine-os/v0.1.0` is enough to kick off the
  real Linux ARM64 build/publish lane. That removes the current dependence on
  a valid local `gh workflow run` session for the first machine-image release.
- Historical validation on the current Mac host already proved two critical
  operational facts we should preserve:
  - a short runtime root such as `/tmp/podman` avoids Darwin unix-socket path
    overflow for the libkrun/gvproxy lane
  - stale machine state can wedge a working provider/image combination, and a
    clean recreate flow is part of the real operator story
- The repo now also owns the first guest machine-API scaffold for MAC4:
  hidden `neovex machine api` wiring, direct unix-socket and systemd
  socket-activation listeners, honest health/capabilities responses, and
  bootstrap assets that point the guest at that narrow surface instead of a
  public guest Neovex server. That capability contract is no longer a bare
  placeholder boolean: it now reports the intended `standard_containers`
  execution mode, the `container` backend family, the exact required guest
  runtime binaries from the machine image contract, and keeps
  `service_execution_ready` false until those binaries are actually present in
  the guest runtime environment and the configured guest machine-port
  forwarder is reachable.
- The machine path model now also reserves a Podman-aligned host-local
  `<machine>-api.sock` path under the short runtime root instead of the older
  generic `api.sock` placeholder, so MAC5 forwarding work has a canonical path
  seam to land on.
- The repo now also owns the first host-side machine-API client scaffold for
  MAC5: shared protocol types for health/capabilities plus a typed unix-socket
  client that can talk to the guest `neovex.sock` once the forwarded host
  socket exists.
- The repo now also owns the first typed **service-sandbox** control seam
  across that machine API. The guest daemon no longer stops at health and
  capability discovery: it now has Neovex-owned routes for image-backed start,
  build-backed start, inspect, and stop using the existing
  `SandboxImageLaunchSpec`, `SandboxBuildLaunchSpec`, and `SandboxHandle`
  types. That keeps the protocol service-runtime-scoped and Neovex-specific
  instead of drifting toward a generic container-engine API.
- The repo now also owns the first real MAC5 forwarding shape in the host
  manager: when a machine config includes an SSH identity, the `gvproxy`
  launch plan now reserves host `<machine>-api.sock` and wires Podman-shaped
  `-forward-sock`, `-forward-dest`, `-forward-user`, and `-forward-identity`
  arguments so the guest `neovex.sock` can be reached without Podman's
  connection layer. Published localhost service-port behavior still depends on
  the guest standard-container lane, so MAC5 is started but not closed.
- The repo now also owns the first host-side guest-machine-API probe surface:
  `neovex machine status` renders whether `<machine>-api.sock` exists and
  whether it is actually answering the Neovex machine API. That keeps
  machine-ready and guest-API-ready separate instead of treating socket
  existence as proof of control-plane reachability.
- That status surface now also renders the configured forwarding contract when
  one exists: host `<machine>-api.sock`, guest `/run/neovex/neovex.sock`,
  `gvproxy`'s SSH-forwarded unix-socket transport, the forwarding user, and
  the configured identity path. That keeps the control-channel story explicit
  in the operator UX instead of leaving it implicit in helper arguments alone.
- That closes the earlier generated-asset mismatch from the abandoned
  guest-authoritative direction. The remaining MAC4/MAC5 gap is not bootstrap
  naming anymore; it is the real service-execution protocol, guest
  standard-container launch path, and host-local forwarding/client seam.

## Current Review Findings

- Podman remains the canonical implementation reference for Neovex's macOS
  machine architecture. Podman Desktop is secondary installer/UX context, not
  the authoritative runtime design reference.
- Podman on macOS does **not** run per-service microVMs in the guest. The
  machine VM is the isolation boundary; containers run as standard Linux
  containers inside the guest.
- Podman's Apple provider selection on current source supports both
  `applehv` and `libkrun`, with `libkrun` accepted as the default fallback in
  `platform_darwin.go`.
- Podman's Apple networking path is source-backed:
  - `apple.StartGenericNetworking(...)` wires `gvproxy` to the VM's
    `virtio-net` device through a host unix socket
  - `shim/startHostForwarder(...)` configures `gvproxy` to forward the guest
    Podman socket to host unix sockets using SSH identity/user information
  - `setupForwardingLinks(...)` then optionally links that machine socket into
    the standard Docker socket path through `podman-mac-helper`
- Podman's readiness layering is also source-backed: `conductVMReadinessCheck`
  does not stop at process existence or the ready signal; it waits for the VM
  to be running, then waits for localhost SSH to accept a real connection.
  Neovex now mirrors that layering by treating machine-ready and guest-SSH
  reachability as separate prerequisites before reporting the manager ready.
- Borrowed Podman machine disks are mutable machine state, not reusable base
  images. The 2026-04-13 host differential showed that reusing a previously
  booted raw disk can fail in FCOS emergency mode, while a pristine raw
  decompressed from Podman's cache boots cleanly under the same Neovex host
  manager. MAC4 validation must therefore use a pristine cached/materialized
  raw disk or a Neovex-owned published artifact, never a reused mutable
  machine disk.
- A pristine raw disk plus Podman's ignition file now proves the Neovex
  host-manager seam is already solid on this Mac: `krunkit` + `gvproxy`
  booted, reached machine-ready, reached guest SSH, exposed the host
  `<machine>-api.sock`, and stopped/removed cleanly under Neovex ownership.
- A pristine raw disk plus Neovex-generated ignition now proves the generated
  ignition is parse-valid and host-valid too: after one transient SSH-readiness
  miss on `/tmp/neovex-machine-proof-run4`, the next fresh run reached
  `running` / `ready` under the Neovex manager on `/tmp/neovex-machine-proof-run5`.
  The remaining delta versus Podman's reference ignition is therefore no longer
  basic machine readiness; it is the guest image package contract and guest API
  contents that still sit behind `neovex.socket`.
- Podman's strongest reusable DX seam is not "run the whole engine in the
  guest no matter what." It is the combination of machine lifecycle plus a
  host-local forwarded guest socket that makes guest execution feel local.
- Podman's machine-image delivery path is also source-backed and now mirrored
  in Neovex: `pkg/machine/ocipull/ociartifact.go`,
  `pkg/machine/stdpull/url.go`, and `pkg/machine/shim/diskpull/diskpull.go`
  confirm that Podman treats OCI artifact pulls and URL downloads as sibling
  machine-image sources. Neovex now does the same at the host-manager layer
  instead of treating OCI references as documentation-only placeholders.
- One supply-side correction is now explicit in the plan as well: a bootc OCI
  archive is not, by itself, the machine artifact that Neovex macOS consumes.
  The current manager selects a linux/current-arch OCI descriptor annotated
  `disktype=raw` and then materializes the referenced raw disk blob. The
  build/publish lane must therefore wrap the built raw disk in that exact OCI
  shape instead of pushing the bootc archive directly.
- The same source-backed lesson applies to the raw-disk builder helper: Podman
  pins `coreos/custom-coreos-disk-images` as a submodule rather than treating
  the raw-disk transformation as folklore. Neovex should do the same
  architecturally, but because vendoring that GPL-licensed helper into this
  repo is the wrong dependency shape, the Neovex build lane now resolves that
  helper by pinned upstream commit instead.
- The same runner lesson applies to CI as well: Podman does not pretend this
  image class is a generic hosted-ubuntu build. Neovex now mirrors that
  reality by using hosted CI only for contract verification while reserving
  the real guest-image build lane for a dedicated Linux ARM64 runner.
- The current base FCOS fixture is now characterized with host proof rather
  than assumption. Under the successful Podman-reference boot it exposes
  `conmon`, `crun`, and `fuse-overlayfs`, and the `/Users` virtiofs mount is
  live, but `/usr/local/bin/neovex`, `/run/neovex/neovex.sock`, `buildah`,
  `netavark`, and `aardvark-dns` are absent. That means MAC4 still needs both
  the guest Neovex daemon and at least part of the standard-container runtime
  package contract in the Neovex-owned image.
- The same naming lesson applies inside the sandbox crate: the guest standard-
  container backend should not have to depend on modules living under a
  `krun/` path just because krun landed first. The shared OCI runtime plumbing
  is now explicit under `backends/oci/`, which is the right internal seam for
  conmon/crun/buildah-backed container execution on both the current Linux and
  future macOS guest paths.
- For Neovex, the source-backed lesson is to copy that forwarded-socket and
  readiness pattern while keeping the guest API narrow. The host Neovex server
  stays authoritative on macOS; the guest exposes only a Neovex-owned
  machine-API surface for service execution.
- Podman's Apple `vsock` usage is source-backed and narrower:
  - `apple/vfkit.go` adds a ready-signal `virtio-vsock` device
  - `apple/apple.go` adds an ignition `virtio-vsock` device on first boot only
  - `apple/ignition.go` serves the ignition payload over that socket
- Neovex now reaches that same host-helper seam directly: the current manager
  launches real `krunkit` and `gvproxy`, persists helper/runtime metadata,
  produces the expected short-root socket/log/pid inventory, and has now been
  host-validated through machine-ready, guest SSH reachability, and
  stop/remove cleanup on the current Mac host. The remaining uncertainty is
  the guest artifact and machine-API packaging contract, not the existence of
  a direct manager seam.
- There is also one important code-level gap to own explicitly before MAC4
  implementation runs too far: the repo now has a generic backend-selection
  seam (`Container` plus `Krun`) and Compose/control-plane carry-through for
  backend choice, but the executable service-control surface still executes
  only the krun backend today. The current binary now rejects container-only
  and mixed-backend project-wide operations explicitly instead of silently
  routing them through krun. A Podman-aligned macOS guest therefore still
  needs a real guest-side container backend family, even though the
  vocabulary and control-plane seam are no longer blocked on `krun`-only
  naming.
- Podman's first-boot bootstrap path is also more specific than the earlier
  Neovex experimentation: `apple/ignition.go` serves the ignition payload as a
  normal HTTP handler bound to the unix socket behind the first-boot vsock
  device. Reused initialized Podman machine disks are therefore poor proof
  artifacts for Neovex, but a clean raw Fedora CoreOS base image decompressed
  from Podman's cache is a valid MAC3 diagnostic fixture while MAC4 still owns
  the Neovex-specific guest artifact.
- A read-through of `crates/neovex-sandbox/src/backends/krun/` also clarified
  the likely implementation seam for the future guest-side container backend:
  `buildah.rs`, `conmon.rs`, `port_manager.rs`, and much of the manifest/state
  model are already generic enough in spirit, while the krun-specific pieces
  cluster around `bundle.rs` annotations, VM launch/stop behavior in `vm.rs`,
  and the current readiness/liveness interpretation for TSI-backed ports.
- The first landed guest-side container backend slice is intentionally
  narrower than Podman's fully host-validated bridge/networking stack. It now
  gives Neovex a real guest executor behind the machine API, owns a dedicated
  guest bridge/network-namespace path through `netavark`, and mirrors
  Podman's machine behavior by stripping host IPs from the guest-side
  `netavark` request when machine forwarding is enabled. What it does **not**
  have yet is live macOS proof that the guest `gvproxy` forwarder API plus the
  guest bridge lane make those published endpoints reachable from the host.
  That is strong MAC4/MAC5 progress because it replaces the placeholder and
  the host-network shortcut with the right architecture seam, but it is not
  enough to call either phase complete.
- `neovex machine ssh` now follows the same localhost machine-SSH contract as
  Podman and Neovex's internal probe path: no host-key prompts, no persistent
  known-hosts writes, and no host-IP checks for localhost-managed VMs.
- Podman's machine port exposure path is source-backed too:
  `libpod/networking_machine.go` shows that guest-side libpod reaches
  `http://gateway.containers.internal/services/forwarder/{expose,unexpose}` to
  ask `gvproxy` to publish machine ports on the host. Neovex now mirrors that
  shape in the guest container backend as an optional machine-forwarder mode
  instead of inventing a custom localhost-publishing story.
- The first guest machine-API scaffold is now landed and intentionally
  narrow: it supports direct bind plus systemd socket activation, keeps the
  protocol service-runtime-scoped, and now reports
  `service_execution_ready` based on real guest runtime availability instead
  of a permanent placeholder. That contract is now materially more useful: it
  advertises the target `standard_containers` execution mode, the `container`
  backend family, the required guest runtime binaries
  (`buildah`, `conmon`, `crun`, `netavark`, `aardvark-dns`,
  `fuse-overlayfs`), and explicit blockers when those binaries are missing or
  when the configured guest machine-port forwarder is not reachable. That is
  the correct MAC4/MAC5 shape because it gives the host/guest seam a stable
  bootstrap contract without pretending that forwarded host-local publishing
  is ready when the guest cannot actually reach `gvproxy`.
- The first host-side machine-API client scaffold is now landed too. That
  matters because MAC5 no longer starts from raw stringly socket I/O: the host
  and guest already share typed health/capabilities responses plus the
  Podman-shaped guest `neovex.sock` and host `<machine>-api.sock` naming, so
  the remaining MAC5 work is forwarding and richer service-runtime operations.
- That richer service-runtime surface now has its first real implementation
  slice too. The guest machine API can now round-trip Neovex's own
  image-backed launch, build-backed launch, inspect, and stop operations over
  the unix-socket seam, and the host-side typed client can drive those routes
  directly. This is the right architecture seam for the hybrid macOS model:
  it preserves the host-resident service manager shape while making the guest
  own only the narrow service-runtime boundary.
- The guest-image lane now also has a Neovex-owned verification shape beyond
  the recipe/build scripts themselves. `collect-neovex-machine-guest-proof.sh`
  uses `neovex machine status` plus `neovex machine ssh -- ...` to capture the
  exact MAC4 proof bundle from a booted guest image: `neovex --version`,
  runtime-binary presence, `neovex.socket` / `neovex.service` state, guest
  machine-API health/capabilities over `/run/neovex/neovex.sock`, virtiofs
  mount evidence, and the host-side machine log tail. That keeps the image
  artifact lane verifiable even before MAC5 host-forwarding is the primary
  operator path.
- The first actual forwarding command-line slice is now landed as well. The
  host manager no longer just reserve-names `<machine>-api.sock`; it now
  teaches `gvproxy` to forward that host socket to `/run/neovex/neovex.sock`
  over SSH when the machine has an identity configured. Because the guest
  socket is a system-owned socket, the forwarding user is deliberately `root`,
  mirroring Podman's rootful-socket pattern rather than the interactive guest
  SSH user.
- Conclusion: on macOS we should stop saying "API forwarding over vsock" as the
  default transport story, and we should also stop treating the guest as the
  authoritative Neovex server for the remaining work. A better model is:
  - `virtio-net` + `gvproxy` for guest networking and published ports
  - a host-local forwarded control socket for a **guest `neovex.sock` machine API**
  - host-resident `neovex serve` and host-resident storage/runtime on macOS
  - `vsock` only where it is truly used: readiness, first-boot ignition, or an
    explicitly chosen future control/data plane
- The final Neovex product should be **Podman-aligned**, not **Podman-dependent**:
  Podman's source is the reference; shipping `podman machine` as a hard runtime
  dependency is not the goal.

## Podman Alignment Matrix

We should mirror Podman's topology where that topology is the reason the
product works on macOS, while still keeping Neovex's own product surface and
runtime architecture.

| Concern | Podman on macOS | Neovex target on macOS | Alignment decision |
| --- | --- | --- | --- |
| Host topology | thin host CLI manages one Linux machine VM | host `neovex serve` plus `neovex machine ...` manage one Linux machine VM | match machine topology, deliberate DX divergence on control-plane placement |
| Host application/runtime | no host-resident app/runtime analogue | authoritative Neovex API, V8 runtime, and storage stay on macOS host | deliberate divergence for local DX |
| Guest control plane | guest `podman.socket` / Podman API | guest `neovex.socket` exposing `/run/neovex/neovex.sock` for a narrow Neovex machine API | match forwarded-socket pattern, narrower API |
| Guest workload implementation | standard guest containers | standard guest containers | match |
| Host↔guest API path | forwarded guest socket plus `gvproxy`/SSH-backed plumbing | forwarded guest `neovex.sock` to host `<machine>-api.sock` plus `gvproxy`/SSH-backed plumbing | match the pattern, not the exact API |
| Port publishing | localhost ports forwarded from guest workloads | localhost ports forwarded from guest services | match |
| Machine bootstrap | guest image + first-boot ignition + ready signaling | guest image + first-boot/bootstrap + ready signaling | match |
| Docker compatibility | optional helper and socket-claim flow | optional compatibility only, never a hard dependency | narrower than Podman |
| Linux production model | standard containers | krun-backed per-service microVMs | intentionally different |

Durable rule:

- copy Podman's machine topology, lifecycle layering, and host↔guest boundary
  choices where they are battle-tested and platform-driven
- keep Neovex's guest API, Linux production runtime, and user-facing service
  abstraction product-specific

## Historical Decision Review

Two earlier planning turns are worth preserving explicitly:

- **`b506ff5` got one important thing right:** `vsock` has real architectural
  value for private host↔guest control traffic. That review also correctly
  noticed that libkrun's host-side `vsock` mapping is not "port type magic" but
  a guest-port to host-UDS model.
- **`b506ff5` also overreached for v1:** it bundled that capability into a
  custom guest-init / custom control-agent direction. That would have added a
  lot of moving parts before we had the simpler Podman-aligned machine model
  settled.
- **`0c3fcf2` made the right simplification:** it removed the requirement for a
  custom guest-side `vsock` agent and kept Linux service traffic on the already
  working TSI/TCP path.
- **`0c3fcf2` should not be read as "vsock is gone":** what was deferred was
  the custom guest-agent design, not the broader architectural option to use
  `vsock` for a future control, observability, or bootstrap channel.

Resulting direction:

- Linux v1 stays on the landed host-driven lifecycle model.
- macOS v1 stays Podman-aligned: one machine VM, standard guest containers,
  host-local control channel, published localhost ports.
- `vsock` remains a capability we can adopt deliberately where it improves the
  architecture, rather than a default requirement everywhere.

## Feature Preservation Matrix

| Concern | Linux production baseline | macOS developer target | Must preserve |
| --- | --- | --- | --- |
| Service isolation | per-service krun microVMs | one machine VM + standard guest containers | same server/service API |
| Host runtime stack | `conmon -> patched crun -> libkrun` | `krunkit + gvproxy` on host, `buildah/conmon/crun` in guest | Linux path stays unchanged |
| Host app/runtime locality | local Neovex server owns runtime + storage | host Neovex server still owns runtime + storage on macOS | fast local edit-run-observe loop |
| Remote control seam | n/a | host talks to a narrow guest Neovex machine API | do not grow a generic remote engine |
| Sandbox backend selection | generic backend vocabulary exists, but only `krun` executes today | guest must not require krun/KVM | add a guest-side container launch family without regressing Linux |
| Service networking | krun TSI host:guest ports | host localhost -> gvproxy -> guest container ports | `ctx.services.<name>.port` semantics |
| Readiness model | server waits for actual service reachability | same layered contract across host and guest | no "running means ready" regression |
| Compose/service UX | landed `neovex --compose-file ...` and `neovex service ...` | same commands from mac host | one developer-facing workflow |
| Host orchestration | direct Linux runtime control | `neovex machine ...` plus host `neovex serve` | host remains the developer-facing authority |
| Docker compatibility | irrelevant | optional via helper/`DOCKER_HOST` | Neovex must not require claiming `/var/run/docker.sock` |

## Terminology Notes

- **Service** is the Neovex product noun: a declared workload from Compose and
  the thing exposed through `ctx.services.<name>`.
- **Container** is one possible implementation vehicle for that service.
- On Linux production today, a Neovex service is implemented as a krun-backed
  microVM.
- On macOS v1, a Neovex service should be implemented as a standard guest
  container inside the machine VM.
- So "guest service" is the user-facing abstraction, while "guest container"
  is the macOS v1 execution mechanism for that abstraction.

## Transport Reality Matrix

| Surface | Linux production | macOS source-backed reality | Decision for Neovex |
| --- | --- | --- | --- |
| Per-service data plane | krun TSI over the service VM boundary | not used for standard guest containers | keep Linux-only |
| Machine ready signal | n/a | `virtio-vsock` ready device | preserve as machine-level detail |
| First-boot bootstrap | n/a | ignition served over a first-boot `virtio-vsock` device | preserve if we use FCOS-style first boot |
| Guest networking | native Linux/KVM + TSI | `gvproxy` attached to `virtio-net` through a host unix socket | canonical macOS networking path |
| Guest API exposure | local server | host unix socket forwarded to guest socket through `gvproxy` + SSH in Podman | preferred v1 alignment |
| File sharing | native Linux fs | `virtiofs` mounts | canonical macOS file-sharing path |

## Lifecycle And Probe Layers

We need separate probe stacks for Linux service microVMs and macOS machine VMs.
They solve different problems and should not be conflated.

### Linux service microVMs

| Layer | What it answers | Current Neovex status |
| --- | --- | --- |
| L0: process state | did `conmon`/`crun`/manifest observe a live sandbox process? | implemented |
| L1: transport state | is the TSI-mapped host port actually reachable? | implemented |
| L2: application readiness | is the guest service answering usefully on that endpoint? | implemented |
| L3: liveness regression | did the service stop answering while the VM still exists? | implemented |
| L4: optional guest diagnostics | can the guest provide structured internal state beyond endpoint checks? | future |

Linux architectural rule:

- keep the current host-driven lifecycle as the default
- do not reintroduce a custom `vsock` guest agent just to recover behavior we
  already have through TSI endpoint probes, manifests, and host supervision

### macOS machine VMs

| Layer | What it answers | Target status |
| --- | --- | --- |
| M0: host helper state | are `krunkit`, `gvproxy`, forwarded sockets, and machine helpers alive? | to implement |
| M1: machine ready state | has the VM crossed its machine-level ready boundary? | to implement |
| M2: guest runtime reachability | can the host reach SSH and the forwarded guest `neovex.sock` / host `<machine>-api.sock` seam? | to implement |
| M3: host Neovex readiness | is host `neovex serve` ready with its guest machine-API client wired? | to implement |
| M4: guest service readiness | are published guest services reachable from macOS localhost? | to implement |

macOS architectural rule:

- machine readiness and service readiness are separate
- a ready machine is not enough to declare the guest machine API reachable
- a reachable guest machine API is not enough to declare host `neovex serve`
  ready
- a ready host `neovex serve` is not enough to declare every declared guest
  service ready

## Future `vsock` Capabilities Worth Preserving

`vsock` is not mandatory for v1, but it does have real future upside if we use
it intentionally.

| Capability | Why it is attractive | Best fit |
| --- | --- | --- |
| Private host↔guest control RPC | avoids publishing admin/control traffic on guest TCP ports | macOS machine VM, future Linux control plane |
| Early-boot bootstrap | works before full guest networking is ready | macOS machine bootstrap, image provisioning |
| Stronger control/data separation | app traffic stays on published ports while control stays off the app plane | both |
| Structured guest health/telemetry | richer lifecycle/debug data than TCP-open checks alone | both |
| Secret/config delivery | avoids leaving long-lived material on shared filesystems or public ports | both |
| Snapshot/checkpoint coordination | future Firecracker-style pause/resume/checkpoint flows often want a private control path | future Linux backends |
| Better engine portability | a generic control-channel abstraction could span krun today and Firecracker later | future cross-backend seam |

Risks and cost:

- custom guest agents increase complexity, protocol-versioning burden, and
  failure modes
- a `vsock` control plane should be introduced only when it buys something the
  current host-driven lifecycle or host-local socket path cannot provide cleanly
- do not block macOS v1 or Linux's landed runtime on a speculative guest-agent
  design

## Control Plan Rules

Source of truth:
1. the current git worktree
2. this plan's `Roadmap Status Ledger` and `Execution Log`
3. `docs/reference/microvm-service-baseline.md`
4. `docs/plans/distribution-plan.md`
5. the reviewed Podman source files listed at the top of this document

General rules:

- Keep the Linux production runtime exactly as landed. This plan is for macOS
  developer support, not for re-architecting the Linux microVM path.
- Do not add nested per-service microVMs on macOS v1.
- Do not make Podman CLI or Podman Desktop a product dependency. Use them as
  architecture and diagnostics references only.
- Follow Podman's machine-plumbing naming and layout by default unless Neovex
  has an explicit product reason to diverge:
  guest `neovex.sock`, host `<machine>-api.sock`, and a flat short runtime
  root for per-machine sockets/logs/pids.
- When writing `vsock` in code or docs, name the exact role:
  readiness, first-boot bootstrap, or a consciously chosen control/data plane.
  Do not use `vsock` as a fuzzy synonym for all macOS host↔guest transport.
- Do not reintroduce the old "custom guest init / guest agent over vsock"
  design as a default requirement. Treat any future `vsock` control plane as an
  explicit, separately justified capability.
- Keep host responsibilities and guest responsibilities separate:
  - host: machine lifecycle, host-local API/runtime/storage, Compose/service
    intent, file sharing, and developer-facing control entrypoints
  - guest: `neovex.socket` / `neovex.service`, standard container runtime, observed
    service state, guest logs, and published guest ports
- Use a short machine runtime root on macOS by default. Do not inherit long
  Darwin `TMPDIR` paths for machine sockets and pid files.
- Every substantive work burst must update this plan's ledger and execution log
  in the same change set.

## Problem Statement

Most Neovex engineers will develop on macOS but deploy to Linux. We need a
macOS developer experience that feels native and reliable without creating a
second product architecture.

Target experience:

```text
macOS host
  -> neovex machine init/start/stop/status/ssh
  -> neovex serve
  -> neovex service up/list/logs/down
  -> same compose.yaml
  -> host-local V8/runtime/storage/debug loop
  -> remote guest service execution through a forwarded guest Neovex API seam
  -> same ctx.services.<name>.port behavior
```

The macOS layer should stay Podman-shaped at the machine boundary while
keeping Neovex's highest-value developer loop on the host. That means one
Linux guest for service execution, but a host-resident authoritative Neovex
server on macOS.

## Target Architecture

### Accepted architecture

```text
macOS host
  └── neovex
        ├── neovex machine ...
        │     ├── krunkit
        │     ├── gvproxy
        │     ├── short runtime dir under /tmp/neovex
        │     └── forwarded guest `neovex.sock` + published localhost ports
        ├── neovex serve
        │     ├── authoritative API/runtime/storage
        │     └── guest machine-API client
        └── neovex service ...
              └── same guest machine-API client

Linux guest VM
  ├── neovex.socket / neovex.service
  ├── buildah + conmon + crun
  └── services run as standard crun containers
```

### Control-plane boundary

Neovex on macOS should follow Podman's machine topology closely while making a
different product tradeoff for DX:

- the **host binary** is the authoritative Neovex API/runtime/storage loop on
  macOS
- the **guest binary/service** is a narrow machine API for service execution,
  not a second public Neovex control plane
- the **guest** owns container lifecycle, observed container state, logs,
  readiness checks inside the guest, and published guest ports
- the **host** owns machine lifecycle, image materialization/cache, Compose
  intent, local API/runtime/storage, and the developer-facing control surface

This means `neovex serve` on macOS is a **real host-resident Neovex server**
that calls into a forwarded guest machine-API seam instead of booting the
authoritative server in the guest.

### Why this is Podman-aligned but not a copy of Podman's connection model

Podman's host config is built around a **generic remote container-engine
connection** model, and its DX strength comes from making that remote engine
feel local through socket forwarding plus layered readiness.

Neovex should copy the **machine topology and forwarding pattern**, but keep a
narrower product seam:

- host `neovex` commands target one guest Neovex machine-API surface
- the host does **not** need a generic container-engine registry or
  connection-switching model
- the guest does **not** expose a Podman-compatible engine API; it exposes only
  the service-runtime operations the host Neovex server needs

So the similarity is real:

- one Linux machine VM
- forwarded guest socket exposed locally
- published localhost ports
- battle-tested readiness layering

But the scope is intentionally narrower:

- Podman: generic remote container engine
- Neovex: host-authoritative app/server with a guest machine-API executor

### Target command flows

#### `neovex serve` on macOS

```text
macOS shell
  -> neovex serve
      -> load machine config
      -> ensure machine is started
      -> wait for machine-ready proof
      -> reach host `<machine>-api.sock`
      -> ensure guest `neovex.sock` is running behind it
      -> build the remote guest machine-API client
      -> start the authoritative host Neovex API/runtime/storage loop
      -> expose the developer-facing API on localhost
      -> on `ctx.services.*`, call the guest machine API and wait for guest
         service readiness
```

Durable rule:

- from the developer's perspective and in the actual architecture, `neovex
  serve` starts the authoritative Neovex server on the Mac
- the guest is an execution substrate for services, not the public Neovex API

#### `neovex service ...` on macOS

```text
macOS shell
  -> neovex service up/list/logs/down
      -> ensure machine is started
      -> ensure host `<machine>-api.sock` is reachable
      -> host Neovex resolves Compose/service state
      -> send the service-control request to the guest machine API
      -> guest machine API uses guest Linux container runtime pieces
         (buildah + conmon + crun)
      -> host reuses/presents forwarded ports and control sockets
```

Durable rule:

- the host CLI and host Neovex server are the operator surface and desired-state
  authority
- the guest machine API is the execution authority for guest containers
- do not grow a generic remote-engine cache or registry in the host just to
  mirror guest runtime state

### Rejected architecture

```text
macOS host
  └── neovex
        └── conmon -> patched crun -> libkrun service microVMs directly on macOS
```

```text
macOS host
  └── neovex
        └── machine VM
              └── guest neovex
                    └── authoritative server/runtime/control plane in the guest
                    └── service containers
```

Rejected because:

- the first option ignores the Linux-only assumptions in the landed VMM stack
- the second option gives up too much local DX for macOS engineers and AI
  agents by moving the authoritative Neovex runtime/debug/storage loop into the
  guest
- nested per-service microVMs inside the guest remain rejected because Podman
  itself does not use them as the normal macOS container model

## Scope

This plan covers:

- the canonical macOS machine architecture and transport model
- a `neovex machine ...` host CLI surface
- direct `krunkit` + `gvproxy` host orchestration
- a Linux guest image and bootstrap contract for Neovex
- transparent macOS host routing for host-resident `neovex serve` and
  `neovex service ...` through a forwarded guest machine-API seam
- real macOS verification artifacts and operator recovery drills

This plan does not cover:

- changing the Linux production microVM architecture
- Intel macOS support
- Windows developer support
- Docker socket takeover as a required Neovex feature

## Verification Contract

### Minimum verification for every code item

- `cargo fmt --all --check`
- focused `cargo check` for touched crates
- targeted tests for the touched CLI, machine-manager, or guest/bootstrap seam
- plan ledger and execution-log update in the same change set

### Required real-host verification lanes

- **macOS host lane**
  - machine init/start/stop/rm from a clean state
  - runtime-dir/socket-budget proof
  - forwarded guest `neovex.sock` / host `<machine>-api.sock` proof
  - guest SSH proof
  - host `neovex serve` readiness proof
  - localhost port-publish proof
  - clean recreate-from-stale-state proof
- **Linux guest lane inside the macOS machine**
  - the guest machine API boots predictably and can drive standard guest
    containers
  - Compose-backed service flows work through the guest machine API
  - guest container networking and published ports match the host-facing claims

### Required evidence discipline

- If a verification artifact cannot live in git, record:
  - absolute path
  - exact command that produced it
  - exact command that proved it worked
- Prefer checked-in scripts/runbooks over ad hoc terminal history.
- Reuse the existing Podman-derived diagnostics scripts where Neovex does not
  yet have a manager-owned equivalent. For direct Neovex machine-manager state,
  prefer the Neovex-owned helpers and record their bundle paths explicitly.

## Roadmap Status Ledger

| Item | Status | Summary | Hard Dependencies |
| --- | --- | --- | --- |
| MAC1 | done | Lock the macOS architecture, transport vocabulary, and probe model docs | none |
| MAC2 | done | Add `neovex machine ...` CLI surface and host-side config/runtime roots | MAC1 |
| MAC3 | done | Implement direct host machine lifecycle around `krunkit` + `gvproxy` | MAC2 |
| MAC4 | in_progress | Build the Linux guest image, bootstrap contract, and guest `neovex.sock` machine API plus standard-container launch family | MAC2 |
| MAC5 | in_progress | Implement the forwarded guest `neovex.sock` to host `<machine>-api.sock` plumbing and published-port plumbing | MAC3, MAC4 |
| MAC6 | todo | Make host-resident `neovex serve` and `neovex service ...` work transparently from macOS | MAC5 |
| MAC7 | todo | Close out packaging, diagnostics, and real-host validation evidence | MAC3, MAC4, MAC5, MAC6 |

## Implementation Checkpoints

### MAC1 — Architecture lock and doc corrections

Repo outputs:

- this plan
- corrected Channel 4 transport wording in `distribution-plan.md`
- plan-index / agent-entrypoint references to this control plane

Acceptance criteria:

- the docs no longer claim "API forwarding over vsock" as the default macOS
  architecture
- the docs explicitly distinguish Linux TSI from macOS machine transports
- the docs record the machine-level versus service-level probe hierarchy
- a fresh agent can find this plan from `AGENTS.md` and `docs/plans/README.md`

### MAC2 — Host CLI and state model

Repo outputs:

- `crates/neovex-bin/src/machine/`
- `MachineCommand` wiring in `crates/neovex-bin/src/main.rs`
- typed machine config/runtime-dir/state-root model
- CLI parser tests and unit tests for path/state behavior

Acceptance criteria:

- `neovex machine init`
- `neovex machine start`
- `neovex machine stop`
- `neovex machine status`
- `neovex machine ssh`
- `neovex machine rm`

### MAC3 — Host machine manager

Repo outputs:

- direct `krunkit` + `gvproxy` orchestration layer
- checked-in diagnostics and recreate helpers owned by Neovex
- short-runtime-dir enforcement in the host manager

Required host-local outputs:

- machine config artifact
- runtime-dir socket inventory
- krunkit and gvproxy logs
- recreate drill bundle

Acceptance criteria:

- a fresh machine boots on the current Mac host without Podman as the runtime
  owner
- the manager can stop and remove that machine cleanly
- machine-level readiness evidence is captured separately from guest
  machine-API reachability and host Neovex readiness
- the stale-state recreate drill is reproducible

### MAC4 — Guest image and bootstrap

Repo outputs:

- guest image build recipe
- guest bootstrap/systemd units
- documented mount strategy and guest package contract
- guest-side machine API plus standard-container backend or equivalent
  launch-family selection contract

Required host-local outputs:

- built image artifact path
- first-boot log proof
- guest SSH proof
- guest `neovex --version` proof
- published OCI reference and digest for the machine artifact
- versioned release tag plus any moving alias used for host consumption

Acceptance criteria:

- the guest image boots reproducibly
- the guest machine API is installed and runnable inside the guest
- the guest machine API can activate standard guest containers without
  requiring nested krun/KVM
- host project paths are available inside the guest through `virtiofs`
- the built guest artifact is publishable to a versioned GHCR reference with
  recorded digest/provenance

### MAC5 — Control channel and port publishing

Repo outputs:

- host-local forwarded control socket/proxy implementation
- host client for the guest machine-API protocol
- published localhost port plumbing
- focused integration tests around the control channel

Required host-local outputs:

- local control socket path
- command showing the forwarded guest `neovex.sock` endpoint behind host
  `<machine>-api.sock`
- localhost connectivity proof to a guest service

Acceptance criteria:

- the macOS host can reach the guest machine-API surface without shelling out
  to Podman's connection layer
- the chosen control-channel implementation is described precisely as either a
  forwarded guest socket or a deliberate `vsock` control channel
- the guest protocol remains Neovex-specific and service-runtime-scoped rather
  than turning into a generic container-engine API
- published guest service ports are reachable from macOS localhost

### MAC6 — Transparent developer UX

Repo outputs:

- mac-aware host-resident `neovex serve` path
- mac-aware `neovex service ...` path
- docs for expected developer workflow

Required host-local outputs:

- one clean end-to-end project root
- `neovex serve` startup log
- `neovex service up/list/logs/down` transcript or checked-in helper summary

Acceptance criteria:

- from a macOS host, a developer can run the same compose-backed workflow they
  use on Linux without manually SSHing into the guest
- the end-to-end flow proves machine readiness, guest machine-API
  reachability, host Neovex readiness, and guest service readiness as separate
  steps
- `ctx.services.<name>.port` behavior matches the Linux UX contract
- pure runtime/storage edits on macOS do not require moving the authoritative
  Neovex server into the guest

### MAC7 — Packaging and closeout

Repo outputs:

- distribution-plan alignment for Channel 4
- Homebrew/dependency contract updates
- final runbook and verification summary

Required host-local outputs:

- install/init/start verification bundle
- recovery-drill bundle
- packaging/install notes
- versioning and registry-hosting contract for the machine artifact

Acceptance criteria:

- the macOS developer path is documented, testable, and repeatable
- this plan can be archived and the stable baseline updated
- the machine artifact hosting/tagging policy is explicit enough that macOS
  defaults can move from ad hoc tags to a supported release channel

## Dependency Graph

- `MAC1` is the documentation/control-plane foundation.
- `MAC2` depends on `MAC1`.
- `MAC3` and `MAC4` both depend on `MAC2` and can proceed in parallel once the
  CLI/state model is settled.
- `MAC5` depends on both `MAC3` and `MAC4`.
- `MAC6` depends on `MAC5`.
- `MAC7` depends on `MAC3` through `MAC6`.

## Recommended Delivery Order

1. `MAC1`
2. `MAC2`
3. `MAC3` and `MAC4`
4. `MAC5`
5. `MAC6`
6. `MAC7`

## Execution Log

- 2026-04-13: Created the dedicated macOS machine-support control plane after
  the Linux microVM and service-control plans were archived. Verified against
  the local Podman source that the current docs needed one important transport
  correction: on Apple's Podman machine path, `gvproxy` is the primary guest
  networking and API-forwarding component, while `vsock` is used for the ready
  signal and first-boot ignition injection rather than as the general-purpose
  API transport. Also re-verified that Neovex does not yet expose
  `neovex machine ...` in `crates/neovex-bin/src/main.rs`, so machine support
  is still an owned implementation gap rather than a packaging-only task.
- 2026-04-13: Reviewed the earlier planning split between `b506ff5` and
  `0c3fcf2` and recorded the durable conclusion here. The older design was
  right that `vsock` is strategically useful for private control traffic, but
  too aggressive in coupling that to a custom guest-init/guest-agent design.
  The later simplification was right to drop that default requirement and keep
  Linux service traffic on TSI/TCP. This plan now preserves both truths:
  `vsock` remains an intentional future capability, while macOS v1 and the
  landed Linux runtime stay on the simpler Podman-aligned / host-driven model.
- 2026-04-13: Added an explicit Podman-alignment matrix and terminology notes
  so future work does not blur product nouns with implementation mechanisms.
  The durable mapping is now documented as: guest Neovex API parallels Podman's
  guest Podman socket, macOS guest workloads remain standard containers, and
  "service" stays the Neovex abstraction while "container" names the macOS v1
  execution mechanism. Also aligned the stable CLI docs with the current binary:
  server startup is still flag-driven today, while `neovex serve` remains
  target command taxonomy rather than shipped subcommand behavior.
- 2026-04-13: Completed `MAC2`. The repo now has `crates/neovex-bin/src/machine/`
  with `MachineCommand` wiring in `crates/neovex-bin/src/main.rs`, a typed
  XDG-style config root plus state root, a short `/tmp/neovex` runtime
  root with typed socket/pid/log paths, persisted machine config/status files,
  and focused parser/unit tests for `init`, `start`, `stop`, `status`, `ssh`,
  and `rm`. The landed MAC2 surface is intentionally honest about the phase
  boundary: `init`, `status`, and `rm` operate on real local machine state,
  while `start`, `stop`, and `ssh` validate initialized state and return a
  clear MAC3-owned error until direct `krunkit` + `gvproxy` orchestration
  lands. Verification: `cargo fmt --all --check`; `cargo check -p neovex-bin`;
  `cargo test -p neovex-bin`; `cargo build -p neovex-bin`; `target/debug/neovex --help`
  showed both `machine` and `service`; temp-home CLI verification under
  `/tmp/neovex-mac2-cli.1H8o4C` used
  `env HOME=/tmp/neovex-mac2-cli.1H8o4C target/debug/neovex machine status`,
  `env HOME=/tmp/neovex-mac2-cli.1H8o4C target/debug/neovex machine init --cpus 4 --memory-mib 4096 --disk-gib 40 --image ghcr.io/agentstation/neovex-machine-os:test --volume /Users:/Users`,
  `env HOME=/tmp/neovex-mac2-cli.1H8o4C target/debug/neovex machine start`, and
  `env HOME=/tmp/neovex-mac2-cli.1H8o4C target/debug/neovex machine rm`.
- 2026-04-13: Promoted `MAC3` to `in_progress`. The repo now has a real
  `crates/neovex-bin/src/machine/manager.rs` seam that resolves `krunkit` and
  `gvproxy`, enforces a short runtime root, persists runtime metadata
  (helper paths, EFI store, SSH port, ready-vsock port, REST endpoint), and
  launches both helpers with Podman-aligned device wiring for `virtio-net`,
  ready-signal `vsock` (`1025`), first-boot ignition `vsock` (`1024`), and
  `virtiofs` mount tags. Host validation on the current Mac used
  `HOME=/tmp/neovex-mac3-cli.w0y1sy`,
  `NEOVEX_MACHINE_RUNTIME_ROOT=/tmp/neovex-machine-mac3.w0y1sy`,
  `target/debug/neovex machine init --image /Users/jack/.local/share/containers/podman/machine/libkrun/neovex-libkrun-users-only-arm64.raw --ssh-identity /Users/jack/.local/share/containers/podman/machine/machine --ignition-file /Users/jack/.config/containers/podman/machine/libkrun/neovex-libkrun-users-only.ign --efi-store /Users/jack/.local/share/containers/podman/machine/libkrun/efi-bl-neovex-libkrun-users-only --volume /Users:/Users`,
  and `target/debug/neovex machine start`. The resulting runtime inventory was
  real and host-owned under `/tmp/neovex-machine-mac3.w0y1sy/default/`:
  `sockets/gvproxy.sock`, `sockets/gvproxy.sock-krun.sock`,
  `sockets/krunkit.sock`, `sockets/ready.sock`, `sockets/ignition.sock`,
  plus `logs/machine.log`, `logs/gvproxy.log`, and `logs/krunkit.log`. The
  current blocker is not helper discovery or socket budgeting anymore. It is
  guest compatibility when booting a borrowed Podman disk under the Neovex
  manager: without ignition the guest timed out fetching first-boot config
  from `vsock` `1024`; with ignition wired, the guest reached the bootstrap
  path but still failed to reach ready, including runs that reused Podman's
  EFI store. Podman source review tightened the boundary here:
  `apple/ignition.go` serves ignition as an HTTP handler over the unix socket,
  while `apple/ignition/ready.go` sends the ready signal over `vsock` `1025`.
  Durable conclusion: the direct host-manager seam is real and worth keeping,
  but a Neovex-owned guest image/bootstrap lane is the next reliable closeout
  path for machine-ready proof. Verification to this point:
  `cargo fmt --all --check`; `cargo check -p neovex-bin`; `cargo test -p neovex-bin`;
  `cargo build -p neovex-bin`.
- 2026-04-13: Added the first Neovex-owned MAC3 operator helpers and verified
  them deterministically. The repo now has `scripts/collect-neovex-machine-diagnostics.sh`
  plus `scripts/recreate-neovex-machine.sh`, with matching `make
  collect-neovex-machine-diagnostics`, `make recreate-neovex-machine`,
  `make verify-neovex-machine-diagnostics-helper`, and
  `make verify-neovex-machine-recreate-helper` entrypoints. The checked-in
  helper verification lane passed with:
  `bash -n scripts/collect-neovex-machine-diagnostics.sh`;
  `bash -n scripts/recreate-neovex-machine.sh`;
  `bash -n scripts/verify-neovex-machine-diagnostics-helper.sh`;
  `bash -n scripts/verify-neovex-machine-recreate-helper.sh`;
  `bash scripts/verify-neovex-machine-diagnostics-helper.sh`;
  `bash scripts/verify-neovex-machine-recreate-helper.sh`;
  `cargo fmt --all --check`;
  `cargo check -p neovex-bin`;
  `cargo test -p neovex-bin`.
  A real host bundle now exists at `/tmp/neovex-machine-mac3-diagnostics`,
  produced by:
  `bash scripts/collect-neovex-machine-diagnostics.sh --home /tmp/neovex-mac3-cli.w0y1sy --runtime-root /tmp/neovex-machine-mac3.w0y1sy --output-dir /tmp/neovex-machine-mac3-diagnostics --neovex target/debug/neovex`
  and rerun outside the sandbox so `ps` capture could succeed. That bundle
  records a real failed machine-manager state (`lifecycle: failed`,
  `manager: failed`), helper paths (`/opt/homebrew/bin/krunkit`,
  `/opt/homebrew/opt/podman/libexec/podman/gvproxy`), the machine SSH port
  (`56215`), the ready-signal port (`1025`), and the short-root socket/log
  inventory under `/tmp/neovex-machine-mac3.w0y1sy/default/`. It also sharpened
  the remaining blocker with better evidence than the earlier shell notes:
  the guest log in `machine-log-tail.txt` now shows both `Ignition has failed`
  and the root-device failure
  `Failed to detect device /dev/disk/by-uuid/5ce9072d-0f5c-4dd7-9aba-4db470edc836`,
  followed by emergency mode. That is a strong signal that the borrowed
  Podman disk/bootstrap pair is a diagnostics fixture, not a durable Neovex
  machine-ready artifact.
- 2026-04-13: Re-read the live workspace against the macOS target before
  starting MAC4 and found one architectural gap the earlier plan draft needed
  to own more explicitly. The current codebase had a krun-only executable
  service path, which meant a Podman-aligned macOS guest could not be
  completed by image/bootstrap work alone. The plan now records this directly
  under MAC4: we need a guest-side standard-container launch family (or an
  equivalent backend-selection seam) so guest Neovex can run standard
  containers inside the machine VM without pretending they are krun microVMs.
- 2026-04-13: Added the missing upstream guest-image reference locally for
  MAC4 research. Cloned `containers/podman-machine-os` to
  `/Users/jack/src/github.com/containers/podman-machine-os` with
  `git clone https://github.com/containers/podman-machine-os /Users/jack/src/github.com/containers/podman-machine-os`
  and verified the relevant build inputs with:
  `sed -n '1,240p' /Users/jack/src/github.com/containers/podman-machine-os/build.sh`;
  `sed -n '1,260p' /Users/jack/src/github.com/containers/podman-machine-os/podman-image/Containerfile.COREOS`;
  `sed -n '1,320p' /Users/jack/src/github.com/containers/podman-machine-os/podman-image/build_common.sh`.
  That source confirms Podman's guest contract directly: Fedora CoreOS base,
  ostree/container build flow, virtiofs-related systemd units injected via
  Ignition, and standard guest container dependencies (`crun`,
  `containers-common`, `netavark`, `aardvark-dns`, `openssh-server`, etc.).
  It also reinforces the newly recorded backend gap: the Neovex guest-image
  recipe should be FCOS-like and container-oriented, but guest-image work
  alone is insufficient until the guest can select a standard-container launch
  family instead of the current krun-only backend.
- 2026-04-13: Landed the first code-level MAC4 backend-selection seam without
  changing Linux execution behavior. `crates/neovex-sandbox/src/backend.rs`
  now includes `SandboxBackendKind::Container` alongside `Krun`,
  `crates/neovex-bin/src/service/compose.rs` now carries
  `x-neovex.backend: container|krun` through Compose lowering, and
  `crates/neovex-bin/src/service/project.rs` now derives generic backend roots
  under `services/projects/<project>/backends/<backend>/...` instead of
  assuming every future backend is `krun`. This does **not** mean the guest
  container backend is finished: Linux production behavior is unchanged and
  the MAC4 executor work is still open. Verification:
  `cargo fmt --all --check`;
  `cargo check -p neovex-bin -p neovex-sandbox`;
  `cargo test -p neovex-bin`.
- 2026-04-13: Hardened that MAC4 seam so the executable surface is now honest
  about what it can run. `crates/neovex-bin/src/service/mod.rs` now validates
  Compose backend selection before building the service manager or running
  `service up/down/list/inspect/logs/ps`: container-only projects now fail
  fast with a clear "krun only today" error, and mixed-backend projects now
  fail fast for project-wide operations while still allowing service-scoped
  commands that target a krun-backed service explicitly. This keeps Linux
  behavior unchanged, prevents silent misrouting of future macOS container
  intent through the krun executor, and gives MAC4 a clean place to land the
  real guest-side container backend next. Verification:
  `cargo fmt --all --check`;
  `cargo check -p neovex-bin -p neovex-sandbox`;
  `cargo test -p neovex-bin`.
- 2026-04-13: Landed the first Neovex-owned MAC4 guest bootstrap generator in
  the executable path. `crates/neovex-bin/src/machine/bootstrap.rs` now
  renders a generated ignition file under the machine config root whenever the
  machine config does not point at an explicit ignition override, and
  `MachineLaunchPlan::build` now always wires the first-boot ignition vsock
  device with that resolved file. The generated payload preserves Podman's
  narrow vsock roles and probe layering: `ready.service` only signals
  machine-ready over `vsock` `1025`, while guest Neovex readiness remains a
  separate later probe, and the payload also carries a guest
  `neovex-serve.service` plus virtiofs mount units derived from the recorded
  host volume map. Focused tests now cover generated ignition rendering,
  SSH-key carry-through, mount-unit generation, and launch-plan ignition
  wiring. Verification:
  `cargo fmt --all --check`;
  `cargo check -p neovex-bin -p neovex-sandbox`;
  `cargo test -p neovex-bin`.
- 2026-04-13: Added the first checked-in Neovex machine-image recipe and made
  the bootstrap units explicit repo assets instead of leaving them only as
  inline Rust strings. `crates/neovex-bin/src/machine/assets/` now holds the
  generated-ignition systemd templates (`ready.service`, `neovex-serve`, and
  the virtiofs mount helpers), and `images/neovex-machine-os/` now owns a
  Podman-aligned Fedora CoreOS recipe with `Containerfile.COREOS`,
  `build-common.sh`, `build.sh`, and a package-contract README. The companion
  repo-owned verifier `scripts/verify-neovex-machine-os-recipe.sh` now checks
  shell syntax, FCOS/bootc build anchors, the required guest package set
  (`crun`, `conmon`, `buildah`, `containers-common`, `netavark`,
  `aardvark-dns`, `openssh-server`, `socat`, `uidmap`), the explicit removal
  of `runc`/Docker-era runtimes, and the expected bootstrap placeholders. This
  closes more of the MAC4 repo-output gap while leaving the real Linux-host
  image build and boot proof for the required host-validation lane. Verification:
  `cargo fmt --all --check`;
  `cargo check -p neovex-bin -p neovex-sandbox`;
  `cargo test -p neovex-bin`;
  `bash scripts/verify-neovex-machine-os-recipe.sh`.
- 2026-04-13: Strengthened the MAC4 recipe verification lane so it now
  exercises the build orchestration instead of only grepping static files.
  `images/neovex-machine-os/build.sh` now has narrow test-only overrides for
  OS/root detection, and `scripts/verify-neovex-machine-os-recipe.sh` now
  launches the full build recipe against fake `podman`, `rpm-ostree`, and
  `custom-coreos-disk-images` helpers. That verifier now proves the staged
  context includes the Linux `neovex` binary, the `podman build` call carries
  the FCOS base-image argument, the `rpm-ostree compose build-chunked-oci`
  output path is wired correctly, and the optional disk-image conversion path
  requests `--platforms applehv`. Verification:
  `cargo fmt --all --check`;
  `bash scripts/verify-neovex-machine-os-recipe.sh`.
- 2026-04-13: Re-read the landed krun backend to choose the next MAC4/MAC5
  implementation slice based on code instead of plan prose. The useful result
  is a concrete extraction map for the future guest-side container backend:
  `buildah.rs`, `conmon.rs`, `port_manager.rs`, and the persisted manifest
  model already look reusable, while the krun-specific behavior is concentrated
  in the OCI annotation/rendering logic and the VM lifecycle/readiness path in
  `bundle.rs` and `vm.rs`. That makes a shared OCI/conmon/buildah substrate a
  realistic next refactor if we choose to land the guest container backend
  before more macOS host plumbing.
- 2026-04-13: Tightened the MAC4 machine-image contract to follow Podman's
  build/publish/consume shape instead of treating the guest image as an opaque
  string. `crates/neovex-bin/src/machine/mod.rs` now records a typed guest
  image source in machine config: published OCI reference by default
  (`docker://ghcr.io/agentstation/neovex-machine-os:latest`), with explicit
  absolute local-disk and `http(s)` URL variants for diagnostics. The machine
  path model now also reserves a materialized disk target under the machine
  state root (`state/<machine>/images/<machine>.raw`) so the future
  downloader/cache lane has a stable target. `manager.rs` still launches only
  local raw disks today, but its failure mode is now honest and Podman-shaped:
  OCI and URL sources explain that they must be materialized into the reserved
  raw-disk path before launch. Updated docs now say the same thing explicitly:
  Linux/CI owns image build/publish, while macOS owns image
  download/cache/reuse. Verification:
  `cargo fmt --all --check`;
  `cargo check -p neovex-bin`;
  `cargo test -p neovex-bin`.
- 2026-04-13: Extended that MAC4 image-source seam so published-image configs
  can already reuse a materialized raw disk when it exists at the reserved
  state-root path. `manager.rs` now resolves OCI-reference and `http(s)` image
  sources to `state/<machine>/images/<machine>.raw` if that file is already
  present, and the focused tests now prove both failure and reuse paths for a
  published OCI source. This keeps the current launcher honest while also
  making the next downloader/cache step incremental instead of architectural:
  the future fetcher only needs to populate the reserved raw-disk target.
  Verification:
  `cargo fmt --all --check`;
  `cargo check -p neovex-bin`;
  `cargo test -p neovex-bin`.
- 2026-04-13: Clarified the macOS control-plane boundary directly in the active
  plan so future work does not drift into a split-brain host/guest design.
  The plan now states explicitly that macOS `neovex serve` should be a thin
  local launcher/proxy into a **guest-resident** authoritative Neovex server,
  and that `neovex service ...` on macOS should route service-control requests
  to that guest Neovex server rather than inventing a second host-owned
  service-control database. It also records the precise way Neovex is similar
  to Podman and the precise way it is narrower than Podman's generic remote
  connection model. Verification: docs-only plan update.
- 2026-04-13: Added a durable architecture rationale for the macOS fork point
  at `docs/research/macos-host-vs-guest-control-plane-rationale.md`. That note
  compares the two viable options explicitly:
  (A) host-resident Neovex plus guest containers, and
  (B) guest-resident authoritative Neovex plus guest containers.
  It records the decision heuristic that matters most: Linux parity is about
  Neovex living next to the workload runtime on the platform host, not merely
  about sharing the same physical laptop. The current recommendation remains
  Option B for macOS v1, while preserving the hybrid Option A as a consciously
  rejected-but-revisitable design. Verification: docs-only rationale update.
- 2026-04-13: Expanded that ADR from a pure topology argument into a DX-focused
  evaluation after re-reading Podman's socket-forwarding and readiness code in
  `pkg/machine/shim/networking.go`, `pkg/machine/shim/networking_unix.go`, and
  `pkg/machine/ssh.go`. The rationale now records why the hybrid model
  (host-resident Neovex plus guest containers) is materially attractive for
  developer and AI-agent feedback loops, which exact Podman seams could be
  reused (`gvproxy`-forwarded guest socket, SSH-backed readiness, local
  localhost ergonomics), and what a narrow remote guest machine-API seam would
  look like if MAC5/MAC6 were rewritten around that choice. The active macOS
  plan itself is **not** switched yet; Option B remains the current default
  until an explicit plan rewrite occurs. Verification: docs-only ADR update.
- 2026-04-13: Landed the first real MAC4 image-materialization implementation
  instead of keeping the whole lane as plan prose. `crates/neovex-bin/src/machine/manager.rs`
  now downloads `http(s)` guest-image sources directly into the reserved
  machine-state raw-disk path and transparently decompresses `.gz` artifacts on
  the way in. The existing typed image-source model still keeps the default
  published OCI reference (`docker://...`) and continues to reuse the reserved
  raw disk when one is already staged there, but OCI registry pull itself
  remains the next missing slice. Focused tests now prove four paths:
  published OCI rejected when unstaged, published OCI reused when staged,
  raw `http(s)` materialization, and gzip `http(s)` materialization. Verification:
  `cargo fmt --all --check`;
  `cargo check -p neovex-bin`;
  `cargo test -p neovex-bin`.
- 2026-04-13: Closed the loop between the Linux build recipe and the new macOS
  URL materialization path. `images/neovex-machine-os/build.sh` now detects the
  raw disk produced by `custom-coreos-disk-images`, compresses it into a stable
  publishable artifact name (`neovex-machine-os.raw.gz`), and records both raw
  and compressed paths in `summary.txt`. The repo-owned verifier
  `scripts/verify-neovex-machine-os-recipe.sh` now proves that compressed
  artifact exists and can be decompressed after the fake-tool build run.
  Together with the new `http(s)`/`.gz` materializer in the host manager, that
  gives MAC4 one real end-to-end artifact shape even before the OCI
  registry-pull lane lands. Verification:
  `cargo fmt --all --check`;
  `cargo check -p neovex-bin`;
  `cargo test -p neovex-bin`;
  `bash scripts/verify-neovex-machine-os-recipe.sh`.
- 2026-04-13: Switched the active macOS execution direction for `MAC5` and
  `MAC6` from the earlier guest-authoritative `neovex serve` model to the
  hybrid DX-first model: host-resident authoritative Neovex API/runtime/storage
  plus a narrow guest machine-API seam for service execution. This rewrite is
  intentionally Podman-aligned at the machine boundary, not Podman-dependent:
  it copies `startHostForwarder(...)`, `setupForwardingLinks(...)`, and
  `conductVMReadinessCheck(...)` as the reference shape for socket forwarding
  and layered readiness, while keeping the guest protocol Neovex-specific and
  service-runtime-scoped instead of growing a generic remote engine. The active
  checkpoints now explicitly treat the already-landed guest
  `neovex-serve.service` assets as transitional MAC4 output that must evolve
  into a guest `neovex.sock` / machine-API contract. Verification: docs-only plan
  rewrite against the checked-in rationale and local Podman source.
- 2026-04-13: Landed the first real MAC4 guest machine-API scaffold. The repo
  now has hidden `neovex machine api` wiring plus a narrow unix-socket HTTP
  surface with `/healthz` and `/v1/machine-api/capabilities`, including both
  direct socket binding and systemd socket-activation support. The generated
  guest Ignition path now renders `neovex.socket` and `neovex.service` instead
  of the earlier guest-authoritative `neovex-serve.service`, and the
  machine-os recipe verifier now checks those assets directly. This
  intentionally keeps `service_execution_ready: false` until MAC4 lands the
  guest standard-container launch family and MAC5 lands the forwarded host
  control socket plus guest machine-API client. Verification:
  `cargo fmt --all --check`;
  `cargo check -p neovex-bin`;
  `cargo test -p neovex-bin`;
  `bash scripts/verify-neovex-machine-os-recipe.sh`.
- 2026-04-13: Tightened the reserved control-socket naming to match Podman's
  machine layout more closely. `MachinePaths` no longer records a generic
  `api.sock`; it now reserves host `<machine>-api.sock` under the short
  machine runtime root, while the guest bootstrap reserves `/run/neovex/neovex.sock`
  behind `neovex.socket`. The MAC3 cleanup path treats that typed host socket
  as part of the runtime artifact inventory, keeping the host-manager/state
  model aligned with the guest contract before MAC5 lands the actual
  forwarding/proxy implementation. Verification:
  `cargo fmt --all --check`;
  `cargo check -p neovex-bin`;
  `cargo test -p neovex-bin`.
- 2026-04-13: Added the first host-side machine-API client scaffold to pair
  with the guest `neovex.sock` server. `neovex-bin` now has shared protocol
  types for health/capabilities plus a typed unix-socket client that can read
  those endpoints from a forwarded guest socket. This is intentionally still a
  narrow MAC5 bridge: it gives the upcoming forwarding/proxy work and host
  `neovex serve` integration a stable typed control seam without pretending
  that guest container execution or host-local socket forwarding already exist.
  Verification:
  `cargo fmt --all --check`;
  `cargo check -p neovex-bin`;
  `cargo test -p neovex-bin`;
  `bash scripts/verify-neovex-machine-os-recipe.sh`.
- 2026-04-13: Started the first real MAC5 control-channel plumbing in the host
  manager. `MachineLaunchPlan::build` now configures `gvproxy` with
  Podman-shaped SSH forwarding arguments when the machine has an SSH identity:
  host `<machine>-api.sock` via `-forward-sock`, guest `/run/neovex/neovex.sock`
  via `-forward-dest`, `root` via `-forward-user`, and the configured machine
  key via `-forward-identity`. This keeps the host-side forwarding shape close
  to Podman's machine plumbing while preserving the narrower Neovex guest API.
  The work is intentionally recorded as MAC5 `in_progress`, not `done`:
  host-local forwarded-socket proof on a real Mac and published guest-service
  port proof are still open. Verification:
  `cargo fmt --all --check`;
  `cargo check -p neovex-bin`;
  `cargo test -p neovex-bin machine::manager::tests::launch_plan_requires_bootable_local_disk_image -- --nocapture`;
  `cargo test -p neovex-bin machine::manager::tests::launch_plan_adds_gvproxy_machine_api_forwarding_when_ssh_identity_exists -- --nocapture`.
- 2026-04-13: Added the first host-side guest-machine-API probe/reporting slice
  on top of that MAC5 forwarding shape. `neovex machine status` now renders a
  dedicated machine-API section that distinguishes:
  socket path, socket existence, and actual API reachability. This preserves
  the planned probe layering instead of collapsing "host socket file exists"
  into "guest control plane is ready". Verification:
  `cargo fmt --all --check`;
  `cargo check -p neovex-bin`;
  `cargo test -p neovex-bin machine::tests::machine_status_marks_missing_machine_api_socket_as_unreachable -- --nocapture`;
  `cargo test -p neovex-bin machine::tests::machine_status_detects_reachable_machine_api_socket -- --nocapture`.
- 2026-04-13: Renamed the active guest/host control-socket vocabulary to match
  Podman's machine layout more closely and remove the older overloaded guest
  daemon label. The guest bootstrap now owns `neovex.socket` plus
  `neovex.service` for `/run/neovex/neovex.sock`, the hidden guest daemon is
  `neovex machine api`, and the host runtime root now reserves
  `<machine>-api.sock`. This keeps the machine-layer nouns explicit, product-
  branded, and easier to distinguish from the `neovex-runtime` crate while
  preserving the deliberate divergence that the guest API stays narrower than
  Podman's generic engine API. Verification:
  `cargo fmt --all --check`;
  `cargo check -p neovex-bin`;
  `cargo test -p neovex-bin`;
  `bash scripts/verify-neovex-machine-os-recipe.sh`.
- 2026-04-13: Aligned the Neovex-owned MAC3 diagnostics and recreate helpers
  with the newer Podman-shaped flat runtime-root layout. The helper scripts no
  longer assume `runtime-root/<machine>/sockets` and `runtime-root/<machine>/logs`;
  they now capture and verify the actual machine-manager shape:
  `<machine>-api.sock`, `<machine>.sock`, `<machine>-gvproxy.sock`,
  `<machine>-krunkit.sock`, and sibling `*.log` / `*.pid` files under the
  short runtime root. This closes an operator-tooling drift point that would
  have made the checked-in diagnostics evidence disagree with the code. Verification:
  `cargo fmt --all --check`;
  `bash scripts/verify-neovex-machine-diagnostics-helper.sh`;
  `bash scripts/verify-neovex-machine-recreate-helper.sh`.
- 2026-04-13: Tightened the default short runtime-root from
  `/tmp/neovex-machine` to `/tmp/neovex`. This keeps the Podman-aligned
  short-path principle while dropping an unnecessary extra path segment from
  every host socket, pid, and log path. The rationale is source-backed and
  operationally grounded: Podman uses `$TMPDIR/podman` as a generic runtime-dir
  convention, but the Neovex macOS host evidence says short Darwin unix-socket
  paths matter more than inheriting the longer per-user `$TMPDIR` prefix. The
  code, CLI docs, distribution notes, and Neovex-owned helper defaults now all
  point at `/tmp/neovex`, while the historical execution evidence keeps the
  original `/tmp/neovex-machine-*` paths unchanged. Verification:
  `cargo fmt --all --check`;
  `cargo check -p neovex-bin`;
  `cargo test -p neovex-bin`.
- 2026-04-13: Extended the host-side `neovex machine status` machine-API view
  from pure reachability reporting into an explicit forwarding-contract view.
  When a machine has an SSH identity configured, status now reports the exact
  MAC5 control-channel mapping:
  host `<machine>-api.sock` -> `gvproxy` SSH-forwarded unix socket ->
  guest `/run/neovex/neovex.sock`, plus the forwarding user (`root`) and the
  configured identity path. This makes the Podman-aligned control channel
  visible in the shipped CLI instead of forcing operators to reverse-engineer
  it from helper command lines. Verification:
  `cargo fmt --all --check`;
  `cargo check -p neovex-bin`;
  `cargo test -p neovex-bin`.
- 2026-04-13: Closed `MAC3` with a fresh host-managed proof on the current Mac
  and clarified the exact remaining MAC4 blocker. Using the already-cached
  Podman Fedora CoreOS base raw image at
  `/Users/jack/.local/share/containers/podman/machine/libkrun/cache/45d6e5983955ea9d6e4cef451847ede7a5acbf27d77fb661999c7f33d595b0b0.raw.zst`,
  Neovex materialized a clean diagnostic disk at
  `/tmp/neovex-mac-images/podman-libkrun-cache-run8.raw` via
  `zstd -dc ... > /tmp/neovex-mac-images/podman-libkrun-cache-run8.raw`, then
  launched it directly with:
  `HOME=/tmp/neovex-mac-home-run8 NEOVEX_MACHINE_RUNTIME_ROOT=/tmp/neovex-mac-runtime-run8 target/debug/neovex machine init --image /tmp/neovex-mac-images/podman-libkrun-cache-run8.raw --ssh-identity /Users/jack/.ssh/id_ed25519 --volume /Users:/Users`;
  `HOME=/tmp/neovex-mac-home-run8 NEOVEX_MACHINE_RUNTIME_ROOT=/tmp/neovex-mac-runtime-run8 target/debug/neovex machine start`.
  The machine reached `lifecycle: running` plus `manager: ready`, allocated SSH
  port `54059`, and produced a full diagnostics bundle at
  `/tmp/neovex-machine-mac3-run8-diagnostics` via
  `bash scripts/collect-neovex-machine-diagnostics.sh --home /tmp/neovex-mac-home-run8 --runtime-root /tmp/neovex-mac-runtime-run8 --output-dir /tmp/neovex-machine-mac3-run8-diagnostics --neovex target/debug/neovex`.
  A direct SSH proof then succeeded with
  `ssh -i /Users/jack/.ssh/id_ed25519 -p 54059 -o BatchMode=yes -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -o ConnectTimeout=5 core@127.0.0.1 ...`,
  and the captured proof file
  `/tmp/neovex-machine-mac3-run8-diagnostics/guest-neovex-proof.txt` shows the
  exact MAC4 seam: `neovex.socket` is `active`, `neovex.service` is `inactive`
  until activated, and `/usr/local/bin/neovex` is missing, so the host
  `<machine>-api.sock` exists but the guest machine API still cannot answer.
  The same machine was then stopped and removed cleanly with:
  `HOME=/tmp/neovex-mac-home-run8 NEOVEX_MACHINE_RUNTIME_ROOT=/tmp/neovex-mac-runtime-run8 target/debug/neovex machine stop`;
  `HOME=/tmp/neovex-mac-home-run8 NEOVEX_MACHINE_RUNTIME_ROOT=/tmp/neovex-mac-runtime-run8 target/debug/neovex machine rm`;
  followed by `ps -axww -o pid=,ppid=,stat=,command= | rg 'neovex-mac-runtime-run8|podman-libkrun-cache-run8|default-gvproxy|default-krunkit' || true`,
  which left no matching helper processes. Durable conclusion: MAC3 is done;
  MAC4 now owns the guest-image packaging and guest machine-API executable.
  Verification:
  `cargo fmt --all --check`;
  `cargo check -p neovex-bin`;
  `cargo test -p neovex-bin`.
- 2026-04-13: Landed the remaining MAC4 image-materialization lane by adding a
  Podman-aligned OCI machine-artifact puller to
  `crates/neovex-bin/src/machine/manager.rs`. Published `docker://...` guest
  image references are no longer placeholder-only config: the manager now
  resolves an OCI manifest/index, selects the linux/current-arch
  `disktype=raw` artifact the way Podman's `pkg/machine/ocipull` does,
  downloads the blob into the machine image cache, verifies the compressed
  layer sha256, and materializes the launchable raw disk with gzip/zstd
  decompression. The focused test lane now includes a fake local OCI registry
  proving end-to-end materialization plus a zstd cache-materialization test,
  and the CLI-level MAC4 contract now reports OCI pull failure cleanly for an
  unreachable registry image instead of the older "must be materialized"
  placeholder. Durable conclusion: MAC4 no longer needs the OCI pull lane; it
  now needs a real Neovex machine image build/publish path so the guest
  contains `/usr/local/bin/neovex` and can answer over `neovex.socket`.
  Verification:
  `cargo fmt --all --check`;
  `cargo check -p neovex-bin`;
  `cargo test -p neovex-bin machine::manager::tests::registry_image_reference_materializes_raw_disk_from_oci_registry -- --nocapture`;
  `cargo test -p neovex-bin`.
- 2026-04-13: Added the canonical Linux/CI guest-image build wrapper for the
  remaining MAC4 supply-side work. `scripts/build-neovex-machine-os.sh` now
  owns the repo-level build entrypoint above
  `images/neovex-machine-os/build.sh`: it can either consume an explicit Linux
  `neovex` binary or build one first with `cargo build -p neovex-bin`, then
  pass the correct arguments through to the machine-os recipe. The repo now
  also has `scripts/verify-neovex-machine-os-build-helper.sh` plus the Makefile
  targets `build-neovex-machine-os` and
  `verify-neovex-machine-os-build-helper`, so Linux hosts and CI runners have
  one canonical invocation path instead of ad hoc shell history. Durable
  conclusion: MAC4's supply-side tooling is now checked in; the remaining MAC4
  blocker is a real Linux-built/published guest artifact and the corresponding
  host-local proofs (`neovex --version` inside the guest, first-boot logs,
  and guest machine-API reachability). Verification:
  `bash -n scripts/build-neovex-machine-os.sh`;
  `bash -n scripts/verify-neovex-machine-os-build-helper.sh`;
  `bash scripts/verify-neovex-machine-os-build-helper.sh`;
  `bash scripts/verify-neovex-machine-os-recipe.sh`;
  `cargo fmt --all --check`.
- 2026-04-13: Strengthened the MAC4 guest machine-API contract so it now
  reports real service-runtime intent instead of a bare placeholder boolean.
  `crates/neovex-bin/src/machine/protocol.rs` and
  `crates/neovex-bin/src/machine/api.rs` now advertise the target
  `standard_containers` execution mode, the `container` backend family, the
  required guest runtime binaries from the checked-in machine image contract
  (`buildah`, `conmon`, `crun`, `netavark`, `aardvark-dns`,
  `fuse-overlayfs`), and the explicit blockers that still keep
  `service_execution_ready` false until service lifecycle operations land. The
  host-side status surface in `crates/neovex-bin/src/machine/mod.rs` now also
  fetches and renders that decoded capability contract whenever
  `<machine>-api.sock` is reachable, so operator output distinguishes "socket
  answered" from "guest runtime seam is actually ready" with much more
  precision. Durable conclusion: MAC4 now has a truthful, typed readiness
  contract for the future guest standard-container executor, but the guest
  still does not execute service lifecycle operations yet. Verification:
  `cargo fmt --all --check`;
  `cargo check -p neovex-bin`;
  `cargo test -p neovex-bin -- --nocapture`.
- 2026-04-13: Landed the first typed Neovex-owned service-sandbox operations
  on top of that MAC4/MAC5 machine-API seam. The guest daemon in
  `crates/neovex-bin/src/machine/api.rs` now exposes image-backed start,
  build-backed start, inspect, and stop routes under
  `/v1/machine-api/service-sandboxes/...`, all expressed in existing Neovex
  sandbox nouns (`SandboxImageLaunchSpec`, `SandboxBuildLaunchSpec`,
  `SandboxHandle`) rather than a generic container-engine vocabulary. The
  host-side typed unix-socket client in
  `crates/neovex-bin/src/machine/client.rs` now drives those routes directly,
  and the capability contract now reports those operations dynamically when a
  real service backend is present. Durable conclusion: the macOS control plane
  now has the correct RPC shape for a future host-side remote sandbox backend,
  but MAC4/MAC5 still need the real guest standard-container executor and the
  host-local forwarded-socket proof on a live machine. Verification:
  `cargo fmt --all --check`;
  `cargo check -p neovex-bin`;
  `cargo test -p neovex-bin machine::api::tests::machine_api_serves_health_and_capabilities_over_unix_socket -- --nocapture`;
  `cargo test -p neovex-bin machine::client::tests::client_round_trips_service_sandbox_operations_when_backend_is_available -- --nocapture`;
  `cargo test -p neovex-bin`.
- 2026-04-13: Added the first Neovex-owned MAC4 guest-image proof collector on
  top of the existing machine CLI and guest bootstrap contract.
  `scripts/collect-neovex-machine-guest-proof.sh` now captures a booted
  machine's host-local proof bundle using `neovex machine status` plus
  `neovex machine ssh -- ...`: guest `neovex --version`, required runtime
  binaries (`buildah`, `conmon`, `crun`, `netavark`, `aardvark-dns`,
  `fuse-overlayfs`), `neovex.socket` / `neovex.service` state, guest
  machine-API health/capabilities on `/run/neovex/neovex.sock`, virtiofs mount
  evidence, and the host-side machine log tail. The repo now also owns the
  deterministic verifier `scripts/verify-neovex-machine-guest-proof-helper.sh`
  plus the Makefile entrypoints
  `collect-neovex-machine-guest-proof` and
  `verify-neovex-machine-guest-proof-helper`. Durable conclusion: MAC4 now has
  a checked-in proof lane for the built Linux guest image once a real artifact
  is booted on macOS; the remaining MAC4 supply-side blocker is still the
  actual produced/published Neovex guest image artifact itself. Verification:
  `bash -n scripts/collect-neovex-machine-guest-proof.sh`;
  `bash -n scripts/verify-neovex-machine-guest-proof-helper.sh`;
  `bash scripts/verify-neovex-machine-guest-proof-helper.sh`;
  `cargo fmt --all --check`.
- 2026-04-13: Closed the MAC4 registry-artifact ambiguity by packaging the
  guest raw disk in the exact OCI shape the existing manager already consumes.
  `scripts/package-neovex-machine-os-oci.sh` now turns a produced raw disk
  into an OCI image layout whose manifest/index carry the linux/current-arch +
  `disktype=raw` contract, while `scripts/publish-neovex-machine-os.sh` now
  owns pushing that layout to a registry and optionally staging release
  assets. The repo also now owns deterministic verifiers plus Makefile
  entrypoints for both scripts. Durable conclusion: MAC4 no longer has an
  undefined publish contract. The remaining blocker is the live Linux-host
  build/publish run that produces a real Neovex guest image artifact and
  registry evidence. Verification:
  `bash -n scripts/package-neovex-machine-os-oci.sh`;
  `bash -n scripts/publish-neovex-machine-os.sh`;
  `bash -n scripts/verify-neovex-machine-os-oci-layout-helper.sh`;
  `bash -n scripts/verify-neovex-machine-os-publish-helper.sh`;
  `bash scripts/verify-neovex-machine-os-oci-layout-helper.sh`;
  `bash scripts/verify-neovex-machine-os-publish-helper.sh`;
  `cargo fmt --all --check`.
- 2026-04-13: Closed the remaining MAC4 raw-disk-helper ambiguity on the build
  side as well. `scripts/resolve-custom-coreos-disk-images.sh` now resolves
  the exact upstream `coreos/custom-coreos-disk-images` commit Podman pins
  today (`e017ddda3b20b09627f90f68ef1b708016d10864`), and
  `scripts/build-neovex-machine-os.sh` can now opt into that pinned helper via
  `--fetch-custom-coreos-disk-images <dir>` instead of relying on an operator
  to remember an out-of-band path. The repo also now owns the deterministic
  verifier `scripts/verify-custom-coreos-disk-images-resolver-helper.sh` plus
  a Makefile entrypoint. Durable conclusion: MAC4's remaining blocker is now a
  live Linux-host build/publish execution and proof bundle, not missing helper
  resolution logic. Verification:
  `bash -n scripts/resolve-custom-coreos-disk-images.sh`;
  `bash -n scripts/verify-custom-coreos-disk-images-resolver-helper.sh`;
  `bash scripts/verify-custom-coreos-disk-images-resolver-helper.sh`;
  `cargo fmt --all --check`.
- 2026-04-13: Added the repo-owned machine-os workflow lane that turns the
  verified scripts into an actual CI/release contract.
  `.github/workflows/neovex-machine-os.yml` now runs fast contract checks on
  `ubuntu-latest` for the machine image recipe/build/package/publish seam, and
  exposes a `workflow_dispatch` lane on a dedicated self-hosted
  `linux arm64 neovex-machine-os` runner for the real Apple Silicon guest
  image build, raw-disk OCI packaging, artifact upload, and optional GHCR
  publish. Durable conclusion: MAC4 now has a checked-in Linux builder lane;
  the remaining blocker is still the first successful live run and captured
  artifact/proof bundle from that runner plus the resulting macOS boot proof.
  Verification: `cargo check -p neovex-bin`; `cargo fmt --all --check`;
  `bash scripts/verify-neovex-machine-os-recipe.sh`;
  `bash scripts/verify-neovex-machine-os-build-helper.sh`;
  `bash scripts/verify-neovex-machine-os-oci-layout-helper.sh`;
  `bash scripts/verify-neovex-machine-os-publish-helper.sh`;
  `bash scripts/verify-custom-coreos-disk-images-resolver-helper.sh`.
- 2026-04-13: Extracted the shared OCI-runtime plumbing out of the krun-only
  internal path inside `neovex-sandbox`. The generic buildah/image-lowering,
  command-spec, conmon-launch, and published-port-allocation helpers now live
  under `crates/neovex-sandbox/src/backends/oci/`, while the krun backend
  keeps only the krun-specific bundle, VM config, and lifecycle logic. Durable
  conclusion: MAC4's next code step is no longer "re-find the generic OCI
  pieces inside krun"; it is "land a real container backend on top of the
  explicit `backends/oci/` seam and then wire it into the guest machine API."
  Verification: `cargo check -p neovex-sandbox -p neovex-bin`;
  `cargo test -p neovex-sandbox backends::oci::buildah::tests::wrap_unshare_prefixes_existing_command -- --exact`;
  `cargo test -p neovex-sandbox backends::oci::conmon::tests::conmon_launch_plan_uses_private_runtime_and_buildah_unshare -- --exact`;
  `cargo test -p neovex-sandbox backends::oci::port_manager::tests::allocate_missing_bindings_uses_range_and_skips_existing_guest_ports -- --exact`;
  `cargo fmt --all --check`.
- 2026-04-13: Landed the first real guest standard-container executor behind
  the macOS machine API. `neovex-sandbox` now has
  `crates/neovex-sandbox/src/backends/container/`, the shared
  `backends/oci/` conmon types are no longer krun-branded, and
  `neovex machine api` now instantiates a real `ContainerSandboxBackend`
  rooted under the guest control dir instead of reporting a permanent
  placeholder. This first executor slice is intentionally narrower than the
  final Podman-shaped bridge/networking path: it launches standard guest
  containers through buildah/conmon/crun on the guest host network, auto-adds
  same-port bindings from exposed TCP ports, and rejects remapped
  `host_port -> guest_port` bindings until the bridge/netavark published-port
  lane lands. Durable conclusion: MAC4 now has a real guest lifecycle
  backend, but it still needs live Linux guest proof and the guest-side
  bridge/published-port path before the phase can close. Verification:
  `cargo fmt --all --check`;
  `cargo check -p neovex-sandbox -p neovex-bin`;
  `cargo test -p neovex-sandbox plan_only_backend_persists_a_container_manifest -- --nocapture`;
  `cargo test -p neovex-sandbox bundle_config_rejects_non_matching_host_and_guest_ports -- --nocapture`;
  `cargo test -p neovex-bin capability_response_reports_required_binaries_and_explicit_blockers -- --nocapture`;
  `cargo test -p neovex-bin machine_api_serves_health_and_capabilities_over_unix_socket -- --nocapture`.
- 2026-04-13: Replaced that guest host-network shortcut with the first real
  Podman-shaped guest bridge/published-port lane. `neovex-sandbox` now owns
  `backends/oci/network.rs`, `ContainerSandboxBackendConfig` now records
  `netavark`/`aardvark-dns` plus a shared published-port range and optional
  guest machine-forwarder config, container bundles now carry an explicit
  network-namespace path, image-exposed ports now auto-allocate from the
  shared OCI port manager instead of collapsing to same-port host networking,
  and `neovex machine api` now enables a Podman-shaped default
  `gvproxy`-forwarder target
  (`gateway.containers.internal/services/forwarder`) for guest service
  publishing. Durable conclusion: the remaining MAC4/MAC5 gap is no longer
  "design the guest bridge/published-port seam." It is now "prove this exact
  seam on a real booted guest image and capture host-local connectivity
  evidence from macOS." Verification: `cargo fmt --all --check`;
  `cargo check -p neovex-sandbox -p neovex-bin`;
  `cargo test -p neovex-sandbox backends::container::bundle::tests::bundle_config_includes_explicit_network_namespace_and_remapped_ports -- --exact`;
  `cargo test -p neovex-sandbox backends::oci::network::tests::netavark_request_strips_host_ip_when_machine_forwarding_is_enabled -- --exact`;
  `cargo test -p neovex-sandbox backends::container::runtime::tests::plan_only_backend_auto_assigns_exposed_ports_from_published_range -- --exact`;
  `cargo test -p neovex-bin machine::api::tests::capability_response_reports_required_binaries_and_explicit_blockers -- --exact`;
  `cargo test -p neovex-bin machine::api::tests::machine_api_serves_health_and_capabilities_over_unix_socket -- --exact`.
- 2026-04-13: Tightened the guest machine-API readiness contract around that
  new published-port seam. `MachineApiState` now records the configured guest
  machine-port forwarder, `machine_api_capability_response(...)` now probes
  that endpoint before advertising service lifecycle operations, and the
  capability surface now withholds `image-start` / `build-start` / `inspect` /
  `stop` when the guest cannot actually reach the machine forwarder even if
  the runtime binaries are present. Durable conclusion: the guest API now
  reports MAC5 truthfully instead of advertising a host-local publishing path
  that the guest cannot use yet. Verification: `cargo fmt --all --check`;
  `cargo check -p neovex-sandbox -p neovex-bin`;
  `cargo test -p neovex-bin machine::api::tests::capability_response_reports_machine_port_forwarder_blocker_when_unreachable -- --exact`;
  `cargo test -p neovex-bin machine::api::tests::machine_api_serves_health_and_capabilities_over_unix_socket -- --exact`;
  `cargo test -p neovex-bin machine::client::tests::client_reads_health_and_capabilities_from_machine_api_socket -- --exact`.
- 2026-04-13: Fixed the public `neovex machine ssh` surface to match the
  existing localhost guest-SSH readiness probe. `build_ssh_command(...)` now
  applies the same Podman-aligned localhost-only SSH options
  (`BatchMode=yes`, `IdentitiesOnly=yes`, `StrictHostKeyChecking=no`,
  `UserKnownHostsFile=/dev/null`, `CheckHostIP=no`, `LogLevel=ERROR`), and
  `scripts/collect-neovex-machine-guest-proof.sh` now captures best-effort
  MAC4 evidence instead of aborting on the first missing guest artifact.
  Durable conclusion: the operator-facing SSH command and the checked-in guest
  proof lane now behave like the readiness path they are supposed to validate,
  which removes a false-negative UX blocker before the remaining MAC4 guest
  image work. Verification: `cargo fmt --all --check`;
  `cargo check -p neovex-bin`;
  `cargo test -p neovex-bin ssh_command_applies_localhost_machine_safety_options`;
  `bash scripts/verify-neovex-machine-guest-proof-helper.sh`.
- 2026-04-13: Ran the first clean host differential against local Podman
  machine fixtures on this Mac. The earlier emergency-mode result from
  `/tmp/neovex-machine-proof-run2` was corrected: the failure came from
  reusing a mutable Podman machine raw disk, not from invalid generated
  ignition. A pristine raw disk decompressed from Podman's cache to
  `/tmp/neovex-libkrun-pristine.raw` plus Podman's ignition file booted cleanly
  under Neovex ownership (`/tmp/neovex-machine-proof-run3`), reached
  `machine-ready`, reached guest SSH, and produced a guest proof bundle at
  `/tmp/neovex-machine-guest-proof-run3b`. That bundle proved the base FCOS
  fixture currently has `conmon`, `crun`, `fuse-overlayfs`, and a working
  `/Users` virtiofs mount, but does not yet have `/usr/local/bin/neovex`,
  `/run/neovex/neovex.sock`, `buildah`, `netavark`, or `aardvark-dns`. A
  second pristine raw disk at `/tmp/neovex-libkrun-pristine2.raw` plus
  Neovex-generated ignition booted the guest, mounted `/Users`, and emitted
  the ready signal, but `machine start` still failed at the second readiness
  layer with `guest SSH readiness did not arrive within 30 seconds`
  (`/tmp/neovex-machine-proof-run4`, final status via
  `HOME=/tmp/neovex-mac-proof-home4 NEOVEX_MACHINE_RUNTIME_ROOT=/tmp/neovex-macproof4 target/debug/neovex machine status`).
  Durable conclusion: generated ignition is parse-valid and boot-valid on a
  pristine base disk, so the remaining MAC4 bootstrap delta is now the
  localhost SSH/network readiness contract plus guest image contents, not
  first-boot ignition delivery. Verification: `cargo fmt --all --check`;
  `cargo build -p neovex-bin`; host recreate runs outside the sandbox with
  `bash scripts/recreate-neovex-machine.sh --home /tmp/neovex-mac-proof-home3 --runtime-root /tmp/neovex-macproof3 --output-dir /tmp/neovex-machine-proof-run3 --neovex /Users/jack/src/github.com/agentstation/neovex/target/debug/neovex --image /tmp/neovex-libkrun-pristine.raw --ssh-identity /Users/jack/.local/share/containers/podman/machine/machine --ignition-file /Users/jack/.config/containers/podman/machine/libkrun/neovex-libkrun-users-only.ign`,
  `bash scripts/collect-neovex-machine-guest-proof.sh --home /tmp/neovex-mac-proof-home3 --runtime-root /tmp/neovex-macproof3 --output-dir /tmp/neovex-machine-guest-proof-run3b --neovex /Users/jack/src/github.com/agentstation/neovex/target/debug/neovex --image /tmp/neovex-libkrun-pristine.raw`, and
  `bash scripts/recreate-neovex-machine.sh --home /tmp/neovex-mac-proof-home4 --runtime-root /tmp/neovex-macproof4 --output-dir /tmp/neovex-machine-proof-run4 --neovex /Users/jack/src/github.com/agentstation/neovex/target/debug/neovex --image /tmp/neovex-libkrun-pristine2.raw --ssh-identity /Users/jack/.local/share/containers/podman/machine/machine`.
- 2026-04-13: Tightened the generated virtiofs bootstrap toward Podman's
  exact systemd shape. The repo now renders proper `.mount` units instead of
  bespoke oneshot `.service` mount helpers, and the generated immutable-root
  helpers now use Podman's canonical names/descriptions
  (`immutable-root-off.service`, `immutable-root-on.service`). Durable
  conclusion: Neovex's generated bootstrap is now structurally closer to the
  reference path before the next round of host validation, even though the
  successful run below was still completed with the previously built binary.
  Verification: `cargo fmt --all --check`; `cargo check -p neovex-bin`;
  `cargo test -p neovex-bin generated_ignition_includes_ready_neovex_and_mount_units`;
  `bash scripts/verify-neovex-machine-os-recipe.sh`.
- 2026-04-13: Re-ran the pristine generated-ignition host lane and closed the
  bootstrap uncertainty. With a third fresh raw disk at
  `/tmp/neovex-libkrun-pristine3.raw`, Neovex's generated ignition reached a
  real `running` / `ready` machine on the current Mac host under
  `/tmp/neovex-machine-proof-run5`; `HOME=/tmp/neovex-mac-proof-home5
  NEOVEX_MACHINE_RUNTIME_ROOT=/tmp/neovex-macproof5 target/debug/neovex machine status`
  reported `lifecycle: running`, `manager: ready`, and the expected forwarded
  host `<machine>-api.sock`. A matching guest proof bundle at
  `/tmp/neovex-machine-guest-proof-run5` confirmed the same remaining MAC4
  gap as the Podman-reference boot: `/Users` is mounted, but
  `/usr/local/bin/neovex` and `/run/neovex/neovex.sock` are still missing, so
  the forwarded host socket exists without a guest Neovex API behind it.
  Durable conclusion: generated ignition and basic host lifecycle are now
  proven on this Mac; MAC4 is blocked by guest image contents and guest API
  packaging, not by host-manager readiness anymore. Verification:
  `cargo build -p neovex-bin`; `bash scripts/recreate-neovex-machine.sh --home /tmp/neovex-mac-proof-home5 --runtime-root /tmp/neovex-macproof5 --output-dir /tmp/neovex-machine-proof-run5 --neovex /Users/jack/src/github.com/agentstation/neovex/target/debug/neovex --image /tmp/neovex-libkrun-pristine3.raw --ssh-identity /Users/jack/.local/share/containers/podman/machine/machine`;
  `bash scripts/collect-neovex-machine-guest-proof.sh --home /tmp/neovex-mac-proof-home5 --runtime-root /tmp/neovex-macproof5 --output-dir /tmp/neovex-machine-guest-proof-run5 --neovex /Users/jack/src/github.com/agentstation/neovex/target/debug/neovex --image /tmp/neovex-libkrun-pristine3.raw`.
- 2026-04-13: Hardened the remaining MAC4 evidence contract instead of adding
  more speculative lifecycle theory. `scripts/collect-neovex-machine-guest-proof.sh`
  now captures deterministic guest `neovex` sha256 proof, per-binary presence
  lines for the required runtime toolchain, and machine-readable
  `systemctl show` output for `neovex.socket` / `neovex.service`. In parallel,
  `images/neovex-machine-os/build.sh` now records sha256 provenance for the
  staged Linux `neovex` binary, the checked-in recipe files, the OCI archive,
  and the optional raw/compressed raw disk artifact in `summary.txt`. Durable
  conclusion: the next Linux machine-image build and the next macOS guest boot
  proof can now be compared by concrete binary/artifact identity instead of
  only by filename and tag. Verification: `bash scripts/verify-neovex-machine-os-recipe.sh`;
  `bash scripts/verify-neovex-machine-guest-proof-helper.sh`;
  `cargo fmt --all --check`.
- 2026-04-13: Recorded the publish/hosting decision explicitly so the next
  MAC4/MAC7 work does not have to infer it from workflow YAML alone. Neovex
  will keep Podman's build/consume shape but use repo-native delivery
  infrastructure: GitHub-hosted Actions for contract verification, a dedicated
  self-hosted `linux arm64 neovex-machine-os` runner for the real Apple
  Silicon guest-image build, and GHCR for the published raw-disk OCI artifact.
  Immutable version tags are the release truth; moving aliases such as
  `stable` or `latest` are convenience pointers on top. Durable conclusion:
  the remaining work is not choosing a hosting model anymore, it is producing
  the first real versioned guest artifact and then teaching the macOS default
  image policy to consume that release channel deliberately. Verification:
  repo review of `.github/workflows/neovex-machine-os.yml`,
  `images/neovex-machine-os/README.md`, and this plan; `cargo fmt --all --check`.
- 2026-04-13: Turned that release-channel policy into repo behavior. The CLI
  default machine image now points at
  `docker://ghcr.io/agentstation/neovex-machine-os:stable`, so macOS consumes
  the supported channel by default instead of `latest`. In parallel,
  `.github/workflows/neovex-machine-os.yml` now validates that real publishes
  use immutable version tags and exposes explicit booleans for attaching the
  `stable` and `latest` aliases on top. Durable conclusion: the remaining
  machine-image release work is now operational, not semantic — produce the
  first real versioned artifact and publish it through the shaped workflow.
  Verification: `cargo test -p neovex-bin parses_machine_init_defaults_to_stable_release_channel -- --exact`;
  `cargo fmt --all --check`.
- 2026-04-13: Added the git-tagged release trigger to that workflow so the
  first machine-image release can run through normal git state instead of an
  interactive dispatch. Pushing a repo tag like `machine-os/v0.1.0` now
  drives the real `linux arm64 neovex-machine-os` build/publish lane, derives
  the immutable GHCR reference from the tag, and publishes the `stable` alias
  on top. Durable conclusion: the release control plane is now a normal git
  tag push. Verification: repo review of
  `.github/workflows/neovex-machine-os.yml`; `cargo fmt --all --check`.
