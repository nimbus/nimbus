# Plan: macOS Machine Support — Podman-Aligned Developer Machines

Canonical execution plan for finishing Nimbus macOS support for engineers who
develop on Apple Silicon Macs and deploy to Linux production hosts.

Reviewed against:

- `docs/reference/microvm-service-baseline.md`
- `docs/research/macos-host-vs-guest-control-plane-rationale.md`
- `docs/plans/distribution-plan.md`
- `crates/nimbus-bin/src/main.rs`
- `crates/nimbus-bin/src/service/mod.rs`
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
- **Primary owner:** this plan; all `MAC1` through `MAC7` roadmap items are now
  done, and this file remains the closeout record until the archive sweep
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
  Nimbus starts krun-backed service microVMs on Linux and exposes them through
  the server-owned `ctx.services.*` surface.
- macOS developer support is now complete for the current v1 contract. The
  repo ships the `nimbus machine ...` command surface, typed
  config/state/runtime-root model, direct `krunkit` + `gvproxy` host-manager
  seam, pinned Podman machine-image digest on macOS, host-managed Linux guest
  `nimbus` asset sync into FCOS's writable `/usr/local/bin`, forwarded guest
  machine-API readiness, host-resident `nimbus serve` / `nimbus service ...`
  flows, published localhost ports, and a checked-in repair drill that now
  defaults to the supported pinned-image contract instead of a bespoke local
  raw-disk path. Real closeout evidence lives under
  `/tmp/nimbus-mac-closeout.FNcv0I` for first boot, cached reuse, forwarded
  service control, runtime-level `ctx.services.<name>.port`, and machine
  recreate/recovery proof.
- Historical bullets below describe how the work landed. Where they mention
  earlier blockers or remaining `MAC*` gaps, read them as execution history,
  not current state.
- The stable machine topology decision remains unchanged: macOS is a developer
  delivery surface only, Nimbus still boots exactly one Linux machine VM, and
  service workloads inside that guest still run as standard Linux containers.
- What changed after the DX review is the **control-plane placement** for the
  remaining work. `MAC5` and `MAC6` now target a hybrid model where the
  authoritative Nimbus API/runtime/storage loop stays on the macOS host while
  the guest owns only a narrow service-runtime seam for OCI materialization,
  conmon/crun, and Podman-style networking helpers.
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
- The repo now also owns the first Nimbus-specific manager helpers for MAC3:
  - `scripts/collect-nimbus-machine-diagnostics.sh`
  - `scripts/recreate-nimbus-machine.sh`
  - `scripts/verify-nimbus-machine-diagnostics-helper.sh`
  - `scripts/verify-nimbus-machine-recreate-helper.sh`
  - those helpers now model the same flat short runtime-root layout the code
    uses: host `<machine>-api.sock`, sibling helper sockets, and sibling
    `*.log` / `*.pid` files under one short runtime root
- The repo now also owns the first Nimbus-specific guest bootstrap generator
  for MAC4: when a machine config does not point at an explicit ignition file,
  `nimbus-bin` renders a Nimbus-owned ignition payload with Podman-aligned
  ready signaling, the current temporary `nimbus.socket` plus
  `nimbus.service` guest bootstrap units, and virtiofs mount units derived
  from the recorded machine volumes.
- The machine-image supply side is no longer owned in this repo. The guest
  image recipe, Linux build helpers, OCI packaging/publish scripts, and build
  workflow now live in `nimbus/nimbus-machine-os`, which is the Nimbus
  equivalent of Podman's `containers/podman-machine-os`. This repo keeps the
  guest bootstrap assets, image-consumption logic, and host integration seams.
- The durable machine-image decision is now explicit: Podman's published
  machine image is the active bring-up contract for MAC4. The checked-in
  macOS default now targets an immutable pinned Podman digest owned by the
  host `nimbus` release. Nimbus layers
  only machine-specific bootstrap on top of that image: SSH keys, mounts,
  writable directories, and guest units. The versioned Linux guest `nimbus`
  binary is part of the host-managed convergence path, not Ignition. A
  Nimbus-owned image remains later follow-on work once the Podman-based macOS
  flow is complete. The longer-term supply-side direction in
  `nimbus/nimbus-machine-os` remains `fedora-bootc`-shaped, but it is
  not the current macOS closeout contract until it can preserve the same
  FCOS/ignition/libkrun semantics.
- The checked-in `nimbus/nimbus-machine-os` GHCR flow remains a separate
  follow-on image-ownership track rather than the current macOS default. The
  host `v*` release workflow still proves the cross-repo image contract, but
  current MAC4 bring-up and operator guidance must describe the pinned Podman
  digest plus host-managed guest-binary sync rather than the old
  `ghcr.io/nimbus/nimbus-machine-os:v{CARGO_PKG_VERSION}` default.
- Historical references to `images/nimbus-machine-os/`,
  `scripts/build-nimbus-machine-os.sh`, and the old local workflow in the
  execution log below are pre-split evidence. They remain useful as historical
  validation, but future supply-side work should be performed in
  `nimbus/nimbus-machine-os`.
- The repo now also owns an explicit shared OCI-runtime seam inside
  `nimbus-sandbox`. The generic buildah/image-lowering, command-spec,
  conmon-launch, and published-port allocation helpers no longer live only
  under `backends/krun/`; they now live under `backends/oci/`. That keeps the
  current krun backend working unchanged while giving MAC4 a canonical place
  to land the future guest standard-container backend.
- The repo now also owns the first real guest standard-container backend under
  `crates/nimbus-sandbox/src/backends/container/`, and `nimbus machine api`
  now instantiates that backend instead of stopping at a placeholder
  capability contract. That backend now owns the first Podman-shaped guest
  networking slice too: it auto-allocates published ports from the shared OCI
  port range, writes an explicit Linux network namespace into the OCI bundle,
  and carries Nimbus-owned `netavark` plus optional `gvproxy`-forwarder
  plumbing instead of the earlier guest host-network shortcut. MAC4 and MAC5
  still remain `in_progress` because this path still needs live guest-image
  proof and real macOS host-local connectivity evidence, but the guest machine
  API is now backed by a real executor with a concrete bridge/published-port
  design instead of a stub.
- The repo now also owns a MAC4 guest-image proof helper:
  `scripts/collect-nimbus-machine-guest-proof.sh`, plus
  `scripts/verify-nimbus-machine-guest-proof-helper.sh` and the
  `make collect-nimbus-machine-guest-proof` /
  `make verify-nimbus-machine-guest-proof-helper` entrypoints. That gives the
  control plane a repeatable host-local lane for proving a booted guest image
  actually contains `/usr/local/bin/nimbus`, the expected standard-container
  runtime binaries, the guest `nimbus.socket` / `nimbus.service` units, the
  guest machine-API health/capabilities surface, the shared virtiofs target,
  and the host-side first-boot log tail.
- The repo now also owns the first Podman-aligned typed guest-image source
  model in `nimbus machine` config: published OCI reference by default, with
  explicit local raw-disk and `http(s)` override shapes preserved for
  diagnostics. The missing MAC4 step is not the config model anymore. It is
  now the Podman-based guest provisioning contract itself: a reliable way to
  land the guest executable and answer behind `nimbus.socket` on top of the
  pinned Podman base image.
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
  `nimbus-machine-os.raw.gz`.
- Real host validation on 2026-04-13 refined the earlier bootstrap diagnosis.
  Reusing a mutated Podman machine raw disk as if it were a pristine base
  artifact can still boot into FCOS emergency mode under the Nimbus manager,
  but a pristine raw decompressed from Podman's cache does not show that
  failure. With that pristine raw plus Podman's ignition file, Nimbus reaches
  machine-ready, guest SSH, and clean host-managed stop/remove on the current
  Mac host. A first pristine generated-ignition run failed SSH readiness once,
  but the next fresh generated-ignition run on `/tmp/nimbus-libkrun-pristine3.raw`
  reached `running` / `ready` under the Nimbus manager as well. Durable
  conclusion: the remaining MAC4 blocker is guest image contents and guest API
  packaging, not first-boot ignition delivery or basic host lifecycle.
- The public `nimbus machine ssh` path now applies the same localhost-only
  host-key bypass options that the internal guest-SSH readiness probe already
  used (`IdentitiesOnly=yes`, `StrictHostKeyChecking=no`,
  `UserKnownHostsFile=/dev/null`, `CheckHostIP=no`). That makes the operator
  surface match the actual readiness path instead of failing on first-contact
  host-key prompts.
- The guest-proof helper is now explicitly best-effort so a missing guest
  `nimbus` binary does not abort the rest of the MAC4 evidence capture. It
  now records more deterministic proof too: guest `nimbus --version`,
  guest `nimbus` sha256, per-binary presence lines for the required runtime
  toolchain, machine-readable `systemctl show` output for `nimbus.socket` and
  `nimbus.service`, machine-API health/capabilities, the shared virtiofs
  mount, and the host machine log tail. On the successful Podman-reference
  boot with the host-managed guest-binary sync path, that helper now proves
  the current base FCOS fixture has `/usr/local/bin/nimbus`,
  `/run/nimbus/nimbus.sock`, `conmon`, `crun`, `fuse-overlayfs`, and a working
  `/Users` virtiofs mount. With the current Podman-helper discovery alignment
  landed in the guest machine API, live host proof now also shows
  `netavark` and `aardvark-dns` resolving from `/usr/libexec/podman/`, and the
  image-backed guest execution lane now materializes OCI rootfs content
  directly instead of shelling out to `buildah`. The remaining MAC5/MAC6 gap
  is no longer guest image contents; it is host-side macOS service dispatch,
  where `nimbus service up` still routes default `backend: krun` projects into
  the local Linux-only krun executor instead of a forwarded guest-aware path.
- The repo now also owns a MAC5/MAC6 host-flow proof helper:
  `scripts/collect-nimbus-machine-service-proof.sh`, plus
  `scripts/verify-nimbus-machine-service-proof-helper.sh` and the
  `make collect-nimbus-machine-service-proof` /
  `make verify-nimbus-machine-service-proof-helper` entrypoints. That gives
  the control plane a repeatable host-local lane for proving forwarded
  `<machine>-api.sock` health/capabilities, direct guest service-sandbox
  listing through that socket, host `nimbus service up/list/inspect/ps/logs/down`,
  and an optional localhost published-port probe without inventing a second
  operator workflow outside the shipped CLI.
- The Linux machine-image build summary is now more suitable as a durable
  artifact contract for MAC4 closeout. `images/nimbus-machine-os/build.sh`
  records sha256s for the staged Linux `nimbus` binary, the checked-in recipe
  files, the emitted OCI archive, and the optional raw/compressed raw disk in
  `summary.txt`. That gives the Linux build lane and the later macOS guest
  proof lane a concrete provenance seam they can compare instead of relying
  only on paths and tags.
- The intended publish/consume model is now fully Podman-shaped and
  cross-repo: Linux/CI builds the guest artifact in
  `nimbus/nimbus-machine-os`, while macOS consumes it from GHCR through
  the host machine manager in this repo. Immutable version tags are the release
  truth; moving aliases such as `stable` or `latest` are convenience pointers
  on top.
- The cross-repo release contract is now explicit in code: nimbus `v*` releases
  call the external machine-os reusable workflow with the same `v*` tag so the
  default host image reference always resolves to a matching guest artifact.
  Any standalone machine-os `v*` release must embed the same nimbus version it
  publishes.
- Historical validation on the current Mac host already proved two critical
  operational facts we should preserve:
  - a short runtime root such as `/tmp/podman` avoids Darwin unix-socket path
    overflow for the libkrun/gvproxy lane
  - stale machine state can wedge a working provider/image combination, and a
    clean recreate flow is part of the real operator story
- The repo now also owns the first guest machine-API scaffold for MAC4:
  hidden `nimbus machine api` wiring, direct unix-socket and systemd
  socket-activation listeners, honest health/capabilities responses, and
  bootstrap assets that point the guest at that narrow surface instead of a
  public guest Nimbus server. That capability contract is no longer a bare
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
  client that can talk to the guest `nimbus.sock` once the forwarded host
  socket exists.
- The repo now also owns the first host-side **forwarded sandbox backend**
  slice on top of that client. `nimbus-bin` now has a
  `ForwardedMachineApiSandboxBackend`, a typed default-machine API resolver,
  and a host-backed service-manager loader that can select the forwarded guest
  machine API for container-backed Compose projects on macOS while keeping the
  Linux krun manager path intact.
- The repo now also owns the first typed **service-sandbox** control seam
  across that machine API. The guest daemon no longer stops at health and
  capability discovery: it now has Nimbus-owned routes for image-backed start,
  build-backed start, inspect, and stop using the existing
  `SandboxImageLaunchSpec`, `SandboxBuildLaunchSpec`, and `SandboxHandle`
  types. That keeps the protocol service-runtime-scoped and Nimbus-specific
  instead of drifting toward a generic container-engine API.
- The repo now also owns the first real MAC5 forwarding shape in the host
  manager: when a machine config includes an SSH identity, the `gvproxy`
  launch plan now reserves host `<machine>-api.sock` and wires Podman-shaped
  `-forward-sock`, `-forward-dest`, `-forward-user`, and `-forward-identity`
  arguments so the guest `nimbus.sock` can be reached without Podman's
  connection layer. Published localhost service-port behavior still depends on
  the guest standard-container lane, so MAC5 is started but not closed.
- The repo now also owns the first host-side guest-machine-API probe surface:
  `nimbus machine status` renders whether `<machine>-api.sock` exists and
  whether it is actually answering the Nimbus machine API. That keeps
  machine-ready and guest-API-ready separate instead of treating socket
  existence as proof of control-plane reachability.
- That status surface now also renders the configured forwarding contract when
  one exists: host `<machine>-api.sock`, guest `/run/nimbus/nimbus.sock`,
  `gvproxy`'s SSH-forwarded unix-socket transport, the forwarding user, and
  the configured identity path. That keeps the control-channel story explicit
  in the operator UX instead of leaving it implicit in helper arguments alone.
- That closes the earlier generated-asset mismatch from the abandoned
  guest-authoritative direction. The remaining MAC4/MAC5 gap is not bootstrap
  naming anymore; it is the real service-execution protocol, guest
  standard-container launch path, and host-local forwarding/client seam.

## Current Review Findings

- Podman remains the canonical implementation reference for Nimbus's macOS
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
  Nimbus now mirrors that layering by treating machine-ready and guest-SSH
  reachability as separate prerequisites before reporting the manager ready.
- Borrowed Podman machine disks are mutable machine state, not reusable base
  images. The 2026-04-13 host differential showed that reusing a previously
  booted raw disk can fail in FCOS emergency mode, while a pristine raw
  decompressed from Podman's cache boots cleanly under the same Nimbus host
  manager. MAC4 validation must therefore use a pristine cached/materialized
  raw disk or a Nimbus-owned published artifact, never a reused mutable
  machine disk.
- A pristine raw disk plus Podman's ignition file now proves the Nimbus
  host-manager seam is already solid on this Mac: `krunkit` + `gvproxy`
  booted, reached machine-ready, reached guest SSH, exposed the host
  `<machine>-api.sock`, and stopped/removed cleanly under Nimbus ownership.
- A pristine raw disk plus Nimbus-generated ignition now proves the generated
  ignition is parse-valid and host-valid too: after one transient SSH-readiness
  miss on `/tmp/nimbus-machine-proof-run4`, the next fresh run reached
  `running` / `ready` under the Nimbus manager on `/tmp/nimbus-machine-proof-run5`.
  The remaining delta versus Podman's reference ignition is therefore no longer
  basic machine readiness; it is the guest image package contract and guest API
  contents that still sit behind `nimbus.socket`.
- Podman's strongest reusable DX seam is not "run the whole engine in the
  guest no matter what." It is the combination of machine lifecycle plus a
  host-local forwarded guest socket that makes guest execution feel local.
- Podman's machine-image delivery path is also source-backed and now mirrored
  in Nimbus: `pkg/machine/ocipull/ociartifact.go`,
  `pkg/machine/stdpull/url.go`, and `pkg/machine/shim/diskpull/diskpull.go`
  confirm that Podman treats OCI artifact pulls and URL downloads as sibling
  machine-image sources. Nimbus now does the same at the host-manager layer
  instead of treating OCI references as documentation-only placeholders.
- One supply-side correction is now explicit in the plan as well: a bootc OCI
  archive is not, by itself, the machine artifact that Nimbus macOS consumes.
  The current manager selects a linux/current-arch OCI descriptor annotated
  `disktype=raw` and then materializes the referenced raw disk blob. The
  build/publish lane must therefore wrap the built raw disk in that exact OCI
  shape instead of pushing the bootc archive directly.
- The same source-backed lesson applies to the raw-disk builder helper: Podman
  pins `coreos/custom-coreos-disk-images` as a submodule rather than treating
  the raw-disk transformation as folklore. Nimbus should do the same
  architecturally, but because vendoring that GPL-licensed helper into this
  repo is the wrong dependency shape, the Nimbus build lane now resolves that
  helper by pinned upstream commit instead.
- The same runner lesson applies to CI as well, but the concrete Nimbus answer
  is now narrower than "copy Podman's full disk-builder lane verbatim." MAC4
  now uses Podman's published machine image as the active contract, so the
  machine-os repo is no longer on the immediate closeout critical path. That
  keeps hosted-runner compatibility and supply-side flexibility as
  implementation details instead of turning them into runtime dead paths.
- Any existing `fedora-bootc` raw-image work remains useful only as a future
  research or Linux-oriented supply-side track until it can prove the same
  FCOS/ignition/runtime semantics. It is not the shipping macOS v1 contract.
- The plain FCOS diagnostic fixture characterized earlier is still useful as a
  host-lifecycle proof aid, but it is no longer the active macOS closeout
  contract. The current MAC4 contract instead uses Podman's published machine
  image and then layers the Nimbus guest payload on top. Any runtime package
  gaps should therefore be verified first against Podman's actual published
  image before introducing any Nimbus-specific image divergence.
- The same naming lesson applies inside the sandbox crate: the guest standard-
  container backend should not have to depend on modules living under a
  `krun/` path just because krun landed first. The shared OCI runtime plumbing
  is now explicit under `backends/oci/`, which is the right internal seam for
  conmon/crun/buildah-backed container execution on both the current Linux and
  future macOS guest paths.
- For Nimbus, the source-backed lesson is to copy that forwarded-socket and
  readiness pattern while keeping the guest API narrow. The host Nimbus server
  stays authoritative on macOS; the guest exposes only a Nimbus-owned
  machine-API surface for service execution.
- Podman's Apple `vsock` usage is source-backed and narrower:
  - `apple/vfkit.go` adds a ready-signal `virtio-vsock` device
  - `apple/apple.go` adds an ignition `virtio-vsock` device on first boot only
  - `apple/ignition.go` serves the ignition payload over that socket
- Nimbus now reaches that same host-helper seam directly: the current manager
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
  Nimbus experimentation: `apple/ignition.go` serves the ignition payload as a
  normal HTTP handler bound to the unix socket behind the first-boot vsock
  device. Reused initialized Podman machine disks are therefore poor proof
  artifacts for Nimbus, but a clean raw Fedora CoreOS base image decompressed
  from Podman's cache is a valid MAC3 diagnostic fixture while MAC4 still owns
  the Nimbus-specific guest artifact.
- A read-through of `crates/nimbus-sandbox/src/backends/krun/` also clarified
  the likely implementation seam for the future guest-side container backend:
  `buildah.rs`, `conmon.rs`, `port_manager.rs`, and much of the manifest/state
  model are already generic enough in spirit, while the krun-specific pieces
  cluster around `bundle.rs` annotations, VM launch/stop behavior in `vm.rs`,
  and the current readiness/liveness interpretation for TSI-backed ports.
- The first landed guest-side container backend slice is intentionally
  narrower than Podman's fully host-validated bridge/networking stack. It now
  gives Nimbus a real guest executor behind the machine API, owns a dedicated
  guest bridge/network-namespace path through `netavark`, and mirrors
  Podman's machine behavior by stripping host IPs from the guest-side
  `netavark` request when machine forwarding is enabled. What it does **not**
  have yet is live macOS proof that the guest `gvproxy` forwarder API plus the
  guest bridge lane make those published endpoints reachable from the host.
  That is strong MAC4/MAC5 progress because it replaces the placeholder and
  the host-network shortcut with the right architecture seam, but it is not
  enough to call either phase complete.
- `nimbus machine ssh` now follows the same localhost machine-SSH contract as
  Podman and Nimbus's internal probe path: no host-key prompts, no persistent
  known-hosts writes, and no host-IP checks for localhost-managed VMs.
- Podman's machine port exposure path is source-backed too:
  `libpod/networking_machine.go` shows that guest-side libpod reaches
  `http://gateway.containers.internal/services/forwarder/{expose,unexpose}` to
  ask `gvproxy` to publish machine ports on the host. Nimbus now mirrors that
  shape in the guest container backend as an optional machine-forwarder mode
  instead of inventing a custom localhost-publishing story.
- The first guest machine-API scaffold is now landed and intentionally
  narrow: it supports direct bind plus systemd socket activation, keeps the
  protocol service-runtime-scoped, and now reports
  `service_execution_ready` based on real guest runtime availability instead
  of a permanent placeholder. That contract is now materially more useful: it
  advertises the target `standard_containers` execution mode, the `container`
  backend family, the required guest runtime binaries for image-backed service
  execution (`conmon`, `crun`, `netavark`, `aardvark-dns`) plus the separate
  build-backed helper requirement (`buildah`, `fuse-overlayfs`), and explicit
  blockers when those binaries are missing or when the configured guest
  machine-port forwarder is not reachable. That is the correct MAC4/MAC5
  shape because it gives the host/guest seam a stable bootstrap contract
  without pretending that forwarded host-local publishing
  is ready when the guest cannot actually reach `gvproxy`.
- The first host-side machine-API client scaffold is now landed too. That
  matters because MAC5 no longer starts from raw stringly socket I/O: the host
  and guest already share typed health/capabilities responses plus the
  Podman-shaped guest `nimbus.sock` and host `<machine>-api.sock` naming, so
  the remaining MAC5 work is forwarding and richer service-runtime operations.
- That richer service-runtime surface now has its first real implementation
  slice too. The guest machine API can now round-trip Nimbus's own
  image-backed launch, build-backed launch, inspect, and stop operations over
  the unix-socket seam, and the host-side typed client can drive those routes
  directly. This is the right architecture seam for the hybrid macOS model:
  it preserves the host-resident service manager shape while making the guest
  own only the narrow service-runtime boundary.
- The guest-image lane now also has a Nimbus-owned verification shape beyond
  the recipe/build scripts themselves. `collect-nimbus-machine-guest-proof.sh`
  uses `nimbus machine status` plus `nimbus machine ssh -- ...` to capture the
  exact MAC4 proof bundle from a booted guest image: `nimbus --version`,
  runtime-binary presence, `nimbus.socket` / `nimbus.service` state, guest
  machine-API health/capabilities over `/run/nimbus/nimbus.sock`, virtiofs
  mount evidence, and the host-side machine log tail. That keeps the image
  artifact lane verifiable even before MAC5 host-forwarding is the primary
  operator path.
- The first actual forwarding command-line slice is now landed as well. The
  host manager no longer just reserve-names `<machine>-api.sock`; it now
  teaches `gvproxy` to forward that host socket to `/run/nimbus/nimbus.sock`
  over SSH when the machine has an identity configured. Because the guest
  socket is a system-owned socket, the forwarding user is deliberately `root`,
  mirroring Podman's rootful-socket pattern rather than the interactive guest
  SSH user.
- Conclusion: on macOS we should stop saying "API forwarding over vsock" as the
  default transport story, and we should also stop treating the guest as the
  authoritative Nimbus server for the remaining work. A better model is:
  - `virtio-net` + `gvproxy` for guest networking and published ports
  - a host-local forwarded control socket for a **guest `nimbus.sock` machine API**
  - host-resident `nimbus serve` and host-resident storage/runtime on macOS
  - `vsock` only where it is truly used: readiness, first-boot ignition, or an
    explicitly chosen future control/data plane
- The final Nimbus product should be **Podman-aligned**, not **Podman-dependent**:
  Podman's source is the reference; shipping `podman machine` as a hard runtime
  dependency is not the goal.

## Podman Alignment Matrix

We should mirror Podman's topology where that topology is the reason the
product works on macOS, while still keeping Nimbus's own product surface and
runtime architecture.

| Concern | Podman on macOS | Nimbus target on macOS | Alignment decision |
| --- | --- | --- | --- |
| Host topology | thin host CLI manages one Linux machine VM | host `nimbus serve` plus `nimbus machine ...` manage one Linux machine VM | match machine topology, deliberate DX divergence on control-plane placement |
| Host application/runtime | no host-resident app/runtime analogue | authoritative Nimbus API, V8 runtime, and storage stay on macOS host | deliberate divergence for local DX |
| Guest control plane | guest `podman.socket` / Podman API | guest `nimbus.socket` exposing `/run/nimbus/nimbus.sock` for a narrow Nimbus machine API | match forwarded-socket pattern, narrower API |
| Guest workload implementation | standard guest containers | standard guest containers | match |
| Host↔guest API path | forwarded guest socket plus `gvproxy`/SSH-backed plumbing | forwarded guest `nimbus.sock` to host `<machine>-api.sock` plus `gvproxy`/SSH-backed plumbing | match the pattern, not the exact API |
| Port publishing | localhost ports forwarded from guest workloads | localhost ports forwarded from guest services | match |
| Machine bootstrap | guest image + first-boot ignition + ready signaling | guest image + first-boot/bootstrap + ready signaling | match |
| Docker compatibility | optional helper and socket-claim flow | optional compatibility only, never a hard dependency | narrower than Podman |
| Linux production model | standard containers | krun-backed per-service microVMs | intentionally different |

Durable rule:

- copy Podman's machine topology, lifecycle layering, and host↔guest boundary
  choices where they are battle-tested and platform-driven
- keep Nimbus's guest API, Linux production runtime, and user-facing service
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

## Machine Image Decision

As of 2026-04-16, the active MAC4 machine-image decision is:

- use Podman's published machine image as the current macOS bring-up image,
  pinned by immutable reference or digest
- let the host `nimbus` release own both desired artifacts for macOS:
  the pinned Podman image reference/digest and the matching Linux guest
  `nimbus` binary asset for the host architecture
- keep Ignition/bootstrap narrow so first boot lands SSH keys, writable
  Nimbus directories, guest units, readiness wiring, mounts, and other
  machine-specific configuration, but does not fetch or install a versioned
  guest `nimbus` binary
- make `nimbus machine start` the primary convergence path: cache missing
  image and guest-binary artifacts, boot or rebuild the machine as needed,
  sync the guest binary by hash, and validate the forwarded machine API before
  reporting success
- treat base-image drift as a controlled machine rebuild boundary rather than
  an ad hoc guest mutation; treat guest-binary drift as an in-place sync under
  `/usr/local/bin/nimbus` on FCOS, which is backed by writable
  `/var/usrlocal/bin/nimbus`
- keep the guest runtime aligned with what Podman already ships in that image
  wherever possible; do not introduce Nimbus-specific image divergence unless a
  concrete missing requirement is proven
- keep Ignition and bootstrap narrow to the guest payload and machine-specific
  configuration; do not normalize an OS-switch or extra bootstrap reboot
- defer a Nimbus-owned image until after the Podman-based macOS closeout path
  is working and verified end to end
- keep the `fedora-bootc` work in `nimbus/nimbus-machine-os` as a future
  supply-side direction once it can preserve the same FCOS/ignition/libkrun
  runtime semantics

Why this is the chosen path:

- it keeps the runtime contract aligned with Podman's battle-tested FCOS,
  ignition, libkrun, and gvproxy model on macOS
- it uses the same published image Podman already proves in the field instead
  of reopening guest-image supply work before the host/guest seam is complete
- it gives Nimbus one concrete current path: bootstrap on the Podman image
  first, then revisit image ownership later
- it preserves a clean separation between machine-specific bootstrap, the
  current Podman image contract, and the future image-ownership track

Durable rules:

- Podman's `quay.io/podman/machine-os` image is the active MAC4 bring-up
  contract for now, not just a one-off diagnostic fixture
- the host `nimbus` release owns the desired Podman image digest and desired
  Linux guest `nimbus` asset for macOS, and `nimbus machine start` is the
  primary operator-facing convergence path for both
- any Nimbus-owned image is later replacement work, not the current MAC4 gate
- the current contract is "bootstrap on Podman's published image", not
  "finish building a Nimbus-owned image before macOS can work"
- generated Ignition must stay version-agnostic: it prepares the machine for
  Nimbus, but it does not encode the guest binary version or fetch logic
- if the recorded machine image digest differs from the desired pinned digest,
  the host must rebuild or recreate the machine from the desired image instead
  of mutating the guest OS in place
- do not treat a plain `fedora-bootc` raw disk as the current macOS machine
  image contract
- do not let "works on hosted GitHub runners" justify a guest contract that no
  longer matches Podman's FCOS/ignition assumptions
- do not normalize a first-boot rebase or extra bootstrap reboot as part of
  the target macOS architecture
- if Nimbus later adds a dedicated raw-disk build lane, that is a follow-on
  packaging or ownership choice, not the architectural boundary for MAC4

Current implementation note:

- as of 2026-04-16, the checked-in macOS default now points at the pinned
  immutable Podman digest owned by the host release
- the current bootstrap generator already injects `nimbus.socket` and
  `nimbus.service` through Ignition and prepares `/var/lib/nimbus` control/data
  roots, while the current manager resolves, downloads, and caches the
  matching Linux guest `nimbus` release asset under the machine state root
  before syncing it into FCOS's executable `/usr/local/bin` path; the
  remaining work is tightening that into a fully documented convergence
  contract with controlled rebuild semantics for base-image drift and
  forwarded-machine-API proof

### Host-Managed Convergence Path

The primary macOS operator path should be "run `nimbus machine start` and let
the host converge the machine", not "manually juggle a separate first-boot
upgrade workflow."

Target convergence contract:

- the host `nimbus` release records the desired Podman machine-image digest and
  the desired Linux guest `nimbus` asset for the local host architecture
- `nimbus machine start` checks the local caches first and pulls whichever
  artifacts are missing
- if no machine exists yet, the host boots the machine from the desired pinned
  image and then syncs the guest binary
- if a machine exists and its recorded base image matches the desired pinned
  digest, the host reuses that machine and only syncs the guest binary when
  the hash differs
- if a machine exists but its recorded base image differs from the desired
  pinned digest, the host performs a controlled rebuild or recreate from the
  desired image, then syncs the guest binary
- startup does not succeed until guest SSH, guest binary sync, and forwarded
  machine-API readiness all succeed

Durable rules:

- the guest binary lifecycle and base-image lifecycle are both host-owned, but
  they are not implemented the same way
- guest-binary drift is fixed in place under `/usr/local/bin/nimbus` without a
  reboot; on FCOS that path is the writable `/var/usrlocal/bin/nimbus`
- base-image drift is a machine rebuild boundary, even if the top-level user
  experience stays "run `nimbus machine start`"
- the supported image reference must be immutable; do not let the current
  macOS contract float on a mutable Podman tag
- do not tell operators to `dnf update` or mutate the guest ad hoc as the
  supported Nimbus macOS path
- an explicit `nimbus machine os apply <oci-ref-or-digest>` override may
  remain as a diagnostic or rollout surface, but it is not the primary
  day-to-day macOS developer workflow

## Feature Preservation Matrix

| Concern | Linux production baseline | macOS developer target | Must preserve |
| --- | --- | --- | --- |
| Service isolation | per-service krun microVMs | one machine VM + standard guest containers | same server/service API |
| Host runtime stack | `conmon -> patched crun -> libkrun` | `krunkit + gvproxy` on host, OCI materializer + `conmon/crun/netavark/aardvark-dns` in guest | Linux path stays unchanged |
| Host app/runtime locality | local Nimbus server owns runtime + storage | host Nimbus server still owns runtime + storage on macOS | fast local edit-run-observe loop |
| Remote control seam | n/a | host talks to a narrow guest Nimbus machine API | do not grow a generic remote engine |
| Sandbox backend selection | generic backend vocabulary exists, but only `krun` executes today | guest must not require krun/KVM | add a guest-side container launch family without regressing Linux |
| Service networking | krun TSI host:guest ports | host localhost -> gvproxy -> guest container ports | `ctx.services.<name>.port` semantics |
| Readiness model | server waits for actual service reachability | same layered contract across host and guest | no "running means ready" regression |
| Compose/service UX | landed `nimbus serve --compose-file ...` and `nimbus service ...` | same commands from mac host | one developer-facing workflow |
| Host orchestration | direct Linux runtime control | `nimbus machine ...` plus host `nimbus serve` | host remains the developer-facing authority |
| Docker compatibility | irrelevant | optional via helper/`DOCKER_HOST` | Nimbus must not require claiming `/var/run/docker.sock` |

## Terminology Notes

- **Service** is the Nimbus product noun: a declared workload from Compose and
  the thing exposed through `ctx.services.<name>`.
- **Container** is one possible implementation vehicle for that service.
- On Linux production today, a Nimbus service is implemented as a krun-backed
  microVM.
- On macOS v1, a Nimbus service should be implemented as a standard guest
  container inside the machine VM.
- So "guest service" is the user-facing abstraction, while "guest container"
  is the macOS v1 execution mechanism for that abstraction.

## Transport Reality Matrix

| Surface | Linux production | macOS source-backed reality | Decision for Nimbus |
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

| Layer | What it answers | Current Nimbus status |
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
| M2: guest runtime reachability | can the host reach SSH and the forwarded guest `nimbus.sock` / host `<machine>-api.sock` seam? | to implement |
| M3: host Nimbus readiness | is host `nimbus serve` ready with its guest machine-API client wired? | to implement |
| M4: guest service readiness | are published guest services reachable from macOS localhost? | to implement |

macOS architectural rule:

- machine readiness and service readiness are separate
- a ready machine is not enough to declare the guest machine API reachable
- a reachable guest machine API is not enough to declare host `nimbus serve`
  ready
- a ready host `nimbus serve` is not enough to declare every declared guest
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
- Follow Podman's machine-plumbing naming and layout by default unless Nimbus
  has an explicit product reason to diverge:
  guest `nimbus.sock`, host `<machine>-api.sock`, and a flat short runtime
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
  - guest: `nimbus.socket` / `nimbus.service`, standard container runtime, observed
    service state, guest logs, and published guest ports
- Use a short machine runtime root on macOS by default. Do not inherit long
  Darwin `TMPDIR` paths for machine sockets and pid files.
- Every substantive work burst must update this plan's ledger and execution log
  in the same change set.

## Problem Statement

Most Nimbus engineers will develop on macOS but deploy to Linux. We need a
macOS developer experience that feels native and reliable without creating a
second product architecture.

Target experience:

```text
macOS host
  -> nimbus machine init/start/stop/status/ssh
  -> nimbus serve
  -> nimbus service up/list/logs/down
  -> same compose.yaml
  -> host-local V8/runtime/storage/debug loop
  -> remote guest service execution through a forwarded guest Nimbus API seam
  -> same ctx.services.<name>.port behavior
```

The macOS layer should stay Podman-shaped at the machine boundary while
keeping Nimbus's highest-value developer loop on the host. That means one
Linux guest for service execution, but a host-resident authoritative Nimbus
server on macOS.

## Target Architecture

### Accepted architecture

```text
macOS host
  └── nimbus
        ├── nimbus machine ...
        │     ├── krunkit
        │     ├── gvproxy
        │     ├── short runtime dir under /tmp/nimbus
        │     └── forwarded guest `nimbus.sock` + published localhost ports
        ├── nimbus serve
        │     ├── authoritative API/runtime/storage
        │     └── guest machine-API client
        └── nimbus service ...
              └── same guest machine-API client

Linux guest VM
  ├── nimbus.socket / nimbus.service
  ├── OCI materializer + conmon + crun + netavark + aardvark-dns
  └── services run as standard crun containers
```

### Control-plane boundary

Nimbus on macOS should follow Podman's machine topology closely while making a
different product tradeoff for DX:

- the **host binary** is the authoritative Nimbus API/runtime/storage loop on
  macOS
- the **guest binary/service** is a narrow machine API for service execution,
  not a second public Nimbus control plane
- the **guest** owns container lifecycle, observed container state, logs,
  readiness checks inside the guest, and published guest ports
- the **host** owns machine lifecycle, image materialization/cache, Compose
  intent, local API/runtime/storage, and the developer-facing control surface

This means `nimbus serve` on macOS is a **real host-resident Nimbus server**
that calls into a forwarded guest machine-API seam instead of booting the
authoritative server in the guest.

### Why this is Podman-aligned but not a copy of Podman's connection model

Podman's host config is built around a **generic remote container-engine
connection** model, and its DX strength comes from making that remote engine
feel local through socket forwarding plus layered readiness.

Nimbus should copy the **machine topology and forwarding pattern**, but keep a
narrower product seam:

- host `nimbus` commands target one guest Nimbus machine-API surface
- the host does **not** need a generic container-engine registry or
  connection-switching model
- the guest does **not** expose a Podman-compatible engine API; it exposes only
  the service-runtime operations the host Nimbus server needs

So the similarity is real:

- one Linux machine VM
- forwarded guest socket exposed locally
- published localhost ports
- battle-tested readiness layering

But the scope is intentionally narrower:

- Podman: generic remote container engine
- Nimbus: host-authoritative app/server with a guest machine-API executor

### Target command flows

#### `nimbus serve` on macOS

```text
macOS shell
  -> nimbus serve
      -> load machine config
      -> ensure machine is started
      -> wait for machine-ready proof
      -> reach host `<machine>-api.sock`
      -> ensure guest `nimbus.sock` is running behind it
      -> build the remote guest machine-API client
      -> start the authoritative host Nimbus API/runtime/storage loop
      -> expose the developer-facing API on localhost
      -> on `ctx.services.*`, call the guest machine API and wait for guest
         service readiness
```

Durable rule:

- from the developer's perspective and in the actual architecture, `nimbus
  serve` starts the authoritative Nimbus server on the Mac
- the guest is an execution substrate for services, not the public Nimbus API

#### `nimbus service ...` on macOS

```text
macOS shell
  -> nimbus service up/list/logs/down
      -> ensure machine is started
      -> ensure host `<machine>-api.sock` is reachable
      -> host Nimbus resolves Compose/service state
      -> send the service-control request to the guest machine API
      -> guest machine API uses guest Linux container runtime pieces
         (OCI materializer + conmon + crun + netavark + aardvark-dns)
      -> host reuses/presents forwarded ports and control sockets
```

Durable rule:

- the host CLI and host Nimbus server are the operator surface and desired-state
  authority
- the guest machine API is the execution authority for guest containers
- do not grow a generic remote-engine cache or registry in the host just to
  mirror guest runtime state

### Rejected architecture

```text
macOS host
  └── nimbus
        └── conmon -> patched crun -> libkrun service microVMs directly on macOS
```

```text
macOS host
  └── nimbus
        └── machine VM
              └── guest nimbus
                    └── authoritative server/runtime/control plane in the guest
                    └── service containers
```

Rejected because:

- the first option ignores the Linux-only assumptions in the landed VMM stack
- the second option gives up too much local DX for macOS engineers and AI
  agents by moving the authoritative Nimbus runtime/debug/storage loop into the
  guest
- nested per-service microVMs inside the guest remain rejected because Podman
  itself does not use them as the normal macOS container model

## Scope

This plan covers:

- the canonical macOS machine architecture and transport model
- a `nimbus machine ...` host CLI surface
- direct `krunkit` + `gvproxy` host orchestration
- a Linux guest image and bootstrap contract for Nimbus
- transparent macOS host routing for host-resident `nimbus serve` and
  `nimbus service ...` through a forwarded guest machine-API seam
- real macOS verification artifacts and operator recovery drills

This plan does not cover:

- changing the Linux production microVM architecture
- Intel macOS support
- Windows developer support
- Docker socket takeover as a required Nimbus feature

## Verification Contract

### Minimum verification for every code item

- `cargo fmt --all --check`
- focused `cargo check` for touched crates
- targeted tests for the touched CLI, machine-manager, or guest/bootstrap seam
- `actionlint /Users/jack/src/github.com/nimbus/nimbus-machine-os/.github/workflows/build.yml`
  when the machine-image workflow changes
- plan ledger and execution-log update in the same change set

### Required real-host verification lanes

- **macOS host lane**
  - machine init/start/stop/rm from a clean state
  - runtime-dir/socket-budget proof
  - forwarded guest `nimbus.sock` / host `<machine>-api.sock` proof
  - guest SSH proof
  - host `nimbus serve` readiness proof
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
- Reuse the existing Podman-derived diagnostics scripts where Nimbus does not
  yet have a manager-owned equivalent. For direct Nimbus machine-manager state,
  prefer the Nimbus-owned helpers and record their bundle paths explicitly.

## Roadmap Status Ledger

| Item | Status | Summary | Hard Dependencies |
| --- | --- | --- | --- |
| MAC1 | done | Lock the macOS architecture, transport vocabulary, and probe model docs | none |
| MAC2 | done | Add `nimbus machine ...` CLI surface and host-side config/runtime roots | MAC1 |
| MAC3 | done | Implement direct host machine lifecycle around `krunkit` + `gvproxy` | MAC2 |
| MAC4 | done | Closed the host-managed macOS convergence contract on the pinned Podman digest: a fresh isolated boot now proves first boot, guest SSH, guest `nimbus --version`, guest machine-API health, and forwarded `<machine>-api.sock` readiness using a matching Linux guest asset synced into FCOS's executable `/usr/local/bin` path without an extra bootstrap reboot | MAC2 |
| MAC5 | done | Closed the forwarded control and published-port path on the pinned Podman digest: fresh isolated macOS proof now reaches host `<machine>-api.sock`, `service up/list/inspect/ps/logs/down`, machine-API sandbox listing, and published localhost service health on `http://127.0.0.1:18080/healthz` without a guest `buildah` dependency | MAC3, MAC4 |
| MAC6 | done | Closed the macOS host-resident DX proof: a clean end-to-end app root now proves `nimbus serve` readiness, tenant creation, runtime `services:activate` returning `ctx.services.db.port = 18080`, live localhost reachability on that port, and teardown on tenant deletion without moving the authoritative Nimbus server into the guest | MAC5 |
| MAC7 | done | Closed the install/recovery/runbook closeout on the same isolated macOS root: the recreate helper now defaults to the host-managed pinned Podman image contract, the CLI/baseline/distribution docs match that contract, and `/tmp/nimbus-mac-closeout.FNcv0I` now contains first-boot, cached-reuse, forwarded-service, runtime-level `ctx.services.<name>.port`, and real recovery-drill bundles | MAC3, MAC4, MAC5, MAC6 |

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

- `crates/nimbus-bin/src/machine/`
- `MachineCommand` wiring in `crates/nimbus-bin/src/main.rs`
- typed machine config/runtime-dir/state-root model
- CLI parser tests and unit tests for path/state behavior

Acceptance criteria:

- `nimbus machine init`
- `nimbus machine start`
- `nimbus machine stop`
- `nimbus machine status`
- `nimbus machine ssh`
- `nimbus machine rm`

### MAC3 — Host machine manager

Repo outputs:

- direct `krunkit` + `gvproxy` orchestration layer
- checked-in diagnostics and recreate helpers owned by Nimbus
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
  machine-API reachability and host Nimbus readiness
- the stale-state recreate drill is reproducible

### MAC4 — Guest image and bootstrap

Repo outputs:

- pinned Podman machine-image reference/digest contract for the current macOS
  bring-up path
- generated Ignition assets plus a host-managed cache/sync path that land
  Nimbus on top of that image without encoding versioned guest binaries into
  Ignition
- documented mount strategy and guest runtime expectations against Podman's
  published image
- guest-side machine API plus standard-container backend or equivalent
  launch-family selection contract

Required host-local outputs:

- pinned OCI reference and digest actually used for bring-up
- materialized raw disk path
- first-boot log proof
- guest SSH proof
- guest `nimbus --version` proof
- guest machine-API health proof

Acceptance criteria:

- the pinned Podman image boots reproducibly under the Nimbus manager
- the Nimbus bootstrap path lands the guest machine API prerequisites and
  guest units without an extra OS-switch reboot
- `nimbus machine start` caches missing machine-image and guest-binary
  artifacts automatically before it tries to report success
- when the base image matches, `nimbus machine start` reuses the machine and
  only syncs the guest binary if its hash differs
- when the base image drifts from the desired pinned digest,
  `nimbus machine start` performs a controlled rebuild or recreate from the
  desired image instead of mutating the guest OS ad hoc
- the guest machine API is installed and runnable inside the guest
- the guest machine API can activate standard guest containers without
  requiring nested krun/KVM
- host project paths are available inside the guest through `virtiofs`
- the exact Podman image reference/digest, materialized disk, and resulting
  guest proof are recorded deterministically
- the host-managed convergence path is explicit, version-aware, and
  operator-friendly

Immediate implementation plan:

1. Base-image contract
   Pin the Podman machine image by immutable reference or digest and treat it
   as the active MAC4 bring-up image. Use it to prove host lifecycle,
   ignition, SSH, forwarded control-socket plumbing, and guest runtime
   expectations under the same image Podman already ships.
2. Artifact cache contract
   Make the host own both desired artifacts for macOS: the pinned Podman image
   and the matching Linux guest `nimbus` binary. Cache both locally with
   deterministic keys so startup can converge from cache before hitting the
   network again.
3. Guest payload contract
   Keep generated Ignition narrow and version-agnostic: SSH identity, readiness
   units, mounts, writable Nimbus directories, `nimbus.socket`, and
   `nimbus.service`. Do not fetch or install a versioned guest binary from
   Ignition.
4. Start-time convergence
   Make `nimbus machine start` compare the current machine contract against the
   desired host-owned contract. Reuse an existing machine when the base image
   digest matches, sync the guest binary in place when only the binary drifts,
   and rebuild or recreate the machine when the base image drifts.
5. Guest runtime verification
   Verify what the Podman image already carries for the standard-container
   runtime contract and rely on that directly wherever possible. If a concrete
   runtime gap remains, record it explicitly before introducing image
   divergence.
6. Host lifecycle and proof
   Keep the host manager focused on direct boot plus readiness proof for the
   Podman image contract, not a first-boot OS switch. Operator-facing status
   and error messages should distinguish machine-ready, guest-SSH-ready,
   guest-binary-sync-ready, rebuild-required, and guest machine-API-ready
   without special bootstrap-reboot logic.
7. Future manual overrides and image ownership
   Keep any explicit `machine os apply` override secondary to the primary
   convergence path, and only revisit a Nimbus-owned image after the
   Podman-based macOS flow is working and verified end to end.
8. Future image ownership
   Revisit a Nimbus-owned image only after the Podman-based macOS flow is
   working and verified end to end. Any later image-owned contract must
   preserve the same Podman-aligned runtime model, and the `fedora-bootc` work
   remains a separate future direction until it proves that parity.

### MAC5 — Control channel and port publishing

Repo outputs:

- host-local forwarded control socket/proxy implementation
- host client plus forwarded sandbox-backend adapter for the guest machine-API protocol
- host-aware service-manager loader that selects the forwarded guest backend for container-backed Compose projects on macOS
- published localhost port plumbing
- focused integration tests around the control channel

Required host-local outputs:

- local control socket path
- command showing the forwarded guest `nimbus.sock` endpoint behind host
  `<machine>-api.sock`
- localhost connectivity proof to a guest service

Acceptance criteria:

- the macOS host can reach the guest machine-API surface without shelling out
  to Podman's connection layer
- the chosen control-channel implementation is described precisely as either a
  forwarded guest socket or a deliberate `vsock` control channel
- the guest protocol remains Nimbus-specific and service-runtime-scoped rather
  than turning into a generic container-engine API
- published guest service ports are reachable from macOS localhost

### MAC6 — Transparent developer UX

Repo outputs:

- mac-aware host-resident `nimbus serve` path
- mac-aware `nimbus service ...` path
- docs for expected developer workflow

Required host-local outputs:

- one clean end-to-end project root
- `nimbus serve` startup log
- `nimbus service up/list/logs/down` transcript or checked-in helper summary

Acceptance criteria:

- from a macOS host, a developer can run the same compose-backed workflow they
  use on Linux without manually SSHing into the guest
- the end-to-end flow proves machine readiness, guest machine-API
  reachability, host Nimbus readiness, and guest service readiness as separate
  steps
- `ctx.services.<name>.port` behavior matches the Linux UX contract
- pure runtime/storage edits on macOS do not require moving the authoritative
  Nimbus server into the guest

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

- 2026-04-17: Closed MAC7 on the same isolated macOS proof root at
  `/tmp/nimbus-mac-closeout.FNcv0I`. Tightened the checked-in recovery lane so
  `scripts/recreate-nimbus-machine.sh` and `make recreate-nimbus-machine` now
  default to the supported host-managed pinned Podman image contract, keep
  `IMAGE=...` as an explicit diagnostic override only, and record the guest
  Linux `nimbus` override plus machine-API timeout env in the captured command
  bundle. Updated the closeout docs in
  `docs/reference/cli.md`,
  `docs/reference/microvm-service-baseline.md`,
  `docs/reference/macos-machine-flow.md`,
  `docs/README.md`,
  and `docs/plans/distribution-plan.md`
  so the operator story consistently matches the current implementation:
  pinned `quay.io/podman/machine-os@sha256:...`, host-managed guest-binary
  sync, explicit `machine os apply` / `machine os upgrade`, host-resident
  `nimbus serve`, and the supported repair drill. Focused verification passed
  with:
  `bash scripts/verify-nimbus-machine-recreate-helper.sh`;
  `cargo fmt --all --check`;
  `cargo check -p nimbus-bin`.
  Real host proof then reran the repair drill on the same isolated roots via
  `env HOME=/tmp/nimbus-mac-closeout.FNcv0I/home NIMBUS_MACHINE_RUNTIME_ROOT=/tmp/nimbus-mac-closeout.FNcv0I/runtime NIMBUS_MACHINE_GUEST_BINARY=/Users/jack/src/github.com/nimbus/nimbus/target/aarch64-unknown-linux-gnu/release/nimbus NIMBUS_MACHINE_API_READY_TIMEOUT_SECS=120 bash scripts/recreate-nimbus-machine.sh --home /tmp/nimbus-mac-closeout.FNcv0I/home --runtime-root /tmp/nimbus-mac-closeout.FNcv0I/runtime --output-dir /tmp/nimbus-mac-closeout.FNcv0I/recovery-proof-final --nimbus /Users/jack/src/github.com/nimbus/nimbus/target/debug/nimbus --ssh-identity /tmp/nimbus-mac-closeout.FNcv0I/id_ed25519`.
  The first sandboxed attempt failed exactly at Quay manifest resolution,
  which confirmed the recovery drill really exercises the pinned-image pull
  path; the unrestricted rerun succeeded and captured:
  `/tmp/nimbus-mac-closeout.FNcv0I/recovery-proof-final/summary.txt`,
  `nimbus-machine-start-command.txt`,
  `nimbus-machine-start.txt`,
  `nimbus-machine-status.txt`,
  and `post-diagnostics/summary.txt`.
  Those artifacts prove a real stop/remove/init/start repair cycle on the
  supported default contract, with `machine_image_contract.recorded_matches_desired:
  true`, `machine_api.reachable: true`,
  `service_execution_ready: true`,
  and the expected Podman-aligned guest runtime binaries present through the
  forwarded machine API. Stopped the isolated machine again afterward with
  `env HOME=/tmp/nimbus-mac-closeout.FNcv0I/home NIMBUS_MACHINE_RUNTIME_ROOT=/tmp/nimbus-mac-closeout.FNcv0I/runtime /Users/jack/src/github.com/nimbus/nimbus/target/debug/nimbus machine stop`
  so the proof root remains clean. Durable conclusion: MAC7 is now done and
  the current macOS plan is closed out with real first-boot, reuse,
  service-control, runtime, and recovery evidence.
- 2026-04-17: Closed the remaining MAC6 runtime-level proof on the same
  isolated macOS root at `/tmp/nimbus-mac-closeout.FNcv0I`. Created one clean
  end-to-end app root at
  `/tmp/nimbus-mac-closeout.FNcv0I/ctx-services-app` with:
  `compose.yaml`,
  `.nimbus/convex/functions.json`,
  `.nimbus/convex/http_routes.json`,
  `.nimbus/convex/bundle.mjs`,
  and `.nimbus/convex/bundle.sha256`,
  where the runtime query `services:activate` is exactly
  `async (ctx) => ctx.services.db.port`. Re-started the cached pinned Podman
  machine on the same isolated roots, launched host `nimbus serve` with
  `RUST_LOG=info`, and captured a real host-side runtime proof bundle at
  `/tmp/nimbus-mac-closeout.FNcv0I/ctx-services-proof`.
  The concrete host artifacts are:
  `serve.log` showing `nimbus listening on 0.0.0.0:18082`,
  `serve-health.txt` showing `GET /health -> 200 {"ok":true}`,
  `create-tenant.txt` showing `POST /api/tenants -> 201 {"id":"demo"}`,
  `activate-query.txt` showing
  `POST /convex/demo/query {"name":"services:activate","args":{}} -> 200 18080`,
  `service-health-via-port.txt` showing `GET http://127.0.0.1:18080/healthz`
  returns `200 ok`,
  `delete-tenant.txt` showing `DELETE /api/tenants/demo -> 204`,
  and `service-gone-after-delete.txt` showing the published port disappears
  after tenant teardown. Durable conclusion: MAC6 is now closed. Nimbus on
  macOS now has real evidence for machine readiness, guest machine-API
  readiness, host `nimbus serve` readiness, guest service readiness, and the
  Linux-contract `ctx.services.<name>.port` behavior through the host-resident
  server. The next exact item is MAC7: package the verified contract into the
  final install/recovery/runbook and archive/baseline closeout.
- 2026-04-17: Closed the live MAC5 stop/published-port seam and materially
  advanced MAC6 on a fresh isolated macOS proof root at
  `/tmp/nimbus-mac-closeout.FNcv0I`. The guest standard-container lane now
  follows Podman's battle-tested behavior more closely in two places:
  `crates/nimbus-sandbox/src/backends/oci/network.rs` now pre-allocates and
  persists host-local static IPs before calling `netavark`, mirrors Podman's
  structured stdout error parsing for `netavark` failures, and preserves
  helper-dir discovery for `/usr/libexec/podman/*`; and
  `crates/nimbus-bin/src/machine/client.rs` now sends `Content-Length: 0` on
  bodyless POST requests and uses a longer mutation timeout so guest
  `service-sandboxes.stop` calls do not falsely surface as empty responses
  during real teardown. Also tightened the proof tooling itself:
  `scripts/collect-nimbus-machine-guest-proof.sh` now searches Podman helper
  directories before reporting guest binaries missing, and
  `scripts/verify-nimbus-machine-guest-proof-helper.sh` was updated to match
  the current collector contract.
  Focused repo verification passed with:
  `cargo fmt --all --check`;
  `cargo check -p nimbus-bin`;
  `cargo test -p nimbus-bin stop_service_sandbox_sends_a_content_length_zero_post_request -- --nocapture`;
  `cargo test -p nimbus-sandbox backends::oci::network::tests -- --nocapture`;
  `bash scripts/verify-nimbus-machine-guest-proof-helper.sh`;
  `bash scripts/verify-nimbus-machine-service-proof-helper.sh`.
  Real-host proof then:
  `env HOME=/tmp/nimbus-mac-closeout.FNcv0I/home NIMBUS_MACHINE_RUNTIME_ROOT=/tmp/nimbus-mac-closeout.FNcv0I/runtime /Users/jack/src/github.com/nimbus/nimbus/target/debug/nimbus machine init --ssh-identity /tmp/nimbus-mac-closeout.FNcv0I/id_ed25519`;
  `env HOME=/tmp/nimbus-mac-closeout.FNcv0I/home NIMBUS_MACHINE_RUNTIME_ROOT=/tmp/nimbus-mac-closeout.FNcv0I/runtime NIMBUS_MACHINE_GUEST_BINARY=/Users/jack/src/github.com/nimbus/nimbus/target/aarch64-unknown-linux-gnu/release/nimbus NIMBUS_MACHINE_API_READY_TIMEOUT_SECS=120 /Users/jack/src/github.com/nimbus/nimbus/target/debug/nimbus machine start`;
  `bash scripts/collect-nimbus-machine-guest-proof.sh --home /tmp/nimbus-mac-closeout.FNcv0I/home --runtime-root /tmp/nimbus-mac-closeout.FNcv0I/runtime --output-dir /tmp/nimbus-mac-closeout.FNcv0I/guest-proof-final --nimbus /Users/jack/src/github.com/nimbus/nimbus/target/debug/nimbus`;
  `bash scripts/collect-nimbus-machine-service-proof.sh --home /tmp/nimbus-mac-closeout.FNcv0I/home --runtime-root /tmp/nimbus-mac-closeout.FNcv0I/runtime --output-dir /tmp/nimbus-mac-closeout.FNcv0I/service-proof-current --nimbus /Users/jack/src/github.com/nimbus/nimbus/target/debug/nimbus --compose-file /tmp/nimbus-mac-service-proof-compose.yaml --service demo --published-url http://127.0.0.1:18080/healthz`;
  `env HOME=/tmp/nimbus-mac-closeout.FNcv0I/home NIMBUS_MACHINE_RUNTIME_ROOT=/tmp/nimbus-mac-closeout.FNcv0I/runtime /Users/jack/src/github.com/nimbus/nimbus/target/debug/nimbus machine stop`;
  `env HOME=/tmp/nimbus-mac-closeout.FNcv0I/home NIMBUS_MACHINE_RUNTIME_ROOT=/tmp/nimbus-mac-closeout.FNcv0I/runtime NIMBUS_MACHINE_GUEST_BINARY=/Users/jack/src/github.com/nimbus/nimbus/target/aarch64-unknown-linux-gnu/release/nimbus NIMBUS_MACHINE_API_READY_TIMEOUT_SECS=120 /Users/jack/src/github.com/nimbus/nimbus/target/debug/nimbus machine start`;
  `env HOME=/tmp/nimbus-mac-closeout.FNcv0I/home NIMBUS_MACHINE_RUNTIME_ROOT=/tmp/nimbus-mac-closeout.FNcv0I/runtime ./target/debug/nimbus serve --compose-file /tmp/nimbus-mac-service-proof-compose.yaml --data-dir /tmp/nimbus-mac-closeout.FNcv0I/serve-data --control-data-dir /tmp/nimbus-mac-closeout.FNcv0I/serve-control --port 18082`.
  Durable conclusion: MAC5 is now closed with real host evidence. The fresh
  first boot proves the pinned Podman digest pull plus guest-binary sync, the
  cached-image restart proves the reuse path when `recorded_matches_desired:
  true`, `guest-proof-final` captures guest `nimbus 0.1.3` and sha256
  `de7cccbef8f75a5e903b24e3e44e2c1931e9663eb13c206f09697d1ab95a347b` at
  `/usr/local/bin/nimbus`, and `service-proof-current` proves machine API
  health, `service up/list/inspect/ps/logs/down`, and published localhost
  service reachability all succeed on macOS. The remaining MAC6 gap is
  narrower and explicit: capture one runtime-level `ctx.services.<name>.port`
  proof through host `nimbus serve`, then finish MAC7 archival/baseline
  closeout.
- 2026-04-16: Replaced the remaining guest `buildah` dependency for
  image-backed macOS service execution with a Nimbus-owned OCI materializer
  aligned to Podman's in-process image path instead of the external `buildah`
  CLI. `crates/nimbus-sandbox/src/backends/oci/materializer.rs` now caches OCI
  blobs by digest, verifies them, extracts a materialized rootfs with whiteout
  handling, and is safe to call from inside an existing Tokio runtime.
  `crates/nimbus-sandbox/src/backends/krun/vm.rs` now uses that materialized
  rootfs path for image-backed guest sandboxes while leaving build-backed
  launches on the existing buildah seam, so the active macOS v1 contract stays
  Podman-aligned without introducing a direct Podman dependency. Focused local
  verification passed with:
  `cargo check -p nimbus-sandbox`;
  `cargo test -p nimbus-sandbox plan_only_backend_lowers_image_launch_through_generic_trait_surface -- --nocapture`;
  `cargo test -p nimbus-sandbox start_from_image_plan_only -- --nocapture`;
  `cargo test -p nimbus-sandbox materializer_can_run_inside_an_existing_tokio_runtime -- --nocapture`;
  `cargo check -p nimbus-bin`.
  Real-host proof then rebuilt the host binary plus the Linux arm64 guest
  binary at
  `/Users/jack/src/github.com/nimbus/nimbus/target/aarch64-unknown-linux-gnu/release/nimbus`,
  restarted the isolated proof VM rooted at
  `/tmp/nimbus-mac4-versionproof.lHXHRO`, and re-ran
  `scripts/collect-nimbus-machine-service-proof.sh` into
  `/tmp/nimbus-mac4-versionproof.lHXHRO/service-proof-materialized-image`.
  That live run proves the specific `buildah` blocker is gone: the machine API
  still reports `service_execution_ready: true`, guest `nimbus --version`
  remains reachable over SSH, and `service up` no longer fails on missing
  `buildah` or nested-runtime panics. The next real blocker is higher in the
  host stack: default Compose `backend: krun` services on macOS still route to
  the local krun executor and fail with `krun execution requires a Linux
  host`, so MAC5/MAC6 now hinge on resolving that service-dispatch mismatch
  rather than guest runtime-image contents.
- 2026-04-16: Tightened the live MAC5/MAC6 runtime diagnosis on the real macOS
  proof machine by aligning guest helper-binary discovery with Podman's actual
  guest contract. `crates/nimbus-bin/src/machine/api.rs` now searches Podman
  helper directories (`/usr/local/libexec/podman`, `/usr/local/lib/podman`,
  `/usr/libexec/podman`, `/usr/lib/podman`) before plain `PATH` lookup when it
  reports required guest runtime binaries and when it seeds the guest
  container-backend config. That keeps the guest machine-API capability probe
  and the actual service runtime on the same contract instead of advertising a
  stricter Nimbus-only `PATH` requirement. Focused verification passed with:
  `cargo fmt --all --check`;
  `cargo check -p nimbus-bin`;
  `cargo test -p nimbus-bin capability_response_ -- --test-threads=1`;
  `cargo test -p nimbus-bin apply_resolved_runtime_paths_updates_backend_config_from_helper_dirs -- --test-threads=1`;
  `cargo test -p nimbus-bin client_reads_health_and_capabilities_from_machine_api_socket -- --test-threads=1`;
  `cargo test -p nimbus-bin host_loader_accepts_container_projects_with_ready_forwarded_machine_api_on_macos -- --test-threads=1`.
  Real-host proof then rebuilt the Linux arm64 guest binary locally at
  `/Users/jack/src/github.com/nimbus/nimbus/target/aarch64-unknown-linux-gnu/release/nimbus`,
  restarted the isolated proof VM rooted at
  `/tmp/nimbus-mac4-versionproof.lHXHRO`, and re-ran the forwarded service
  proof into `/tmp/nimbus-mac4-versionproof.lHXHRO/service-proof-helperdirs`.
  The live machine status and `machine-api-capabilities.txt` now prove that the
  current pinned Podman image exposes `netavark` and `aardvark-dns` at
  `/usr/libexec/podman/netavark` and `/usr/libexec/podman/aardvark-dns`, while
  SSH inspection with `rpm -q` confirms those packages are installed and
  `buildah` is not. The remaining MAC5/MAC6 blocker is therefore narrower and
  more trustworthy: `service up` still fails with `No such file or directory
  (os error 2)` only because Nimbus currently shells out to the external
  `buildah` CLI and the active Podman proof image does not ship it.
- 2026-04-16: Re-ran the active MAC4 convergence path from fresh isolated
  macOS roots using the pinned Podman digest plus the published
  `v0.1.8` Linux guest asset and corrected the next real blocker. A clean
  first boot now proves the earlier `useradd: cannot lock /etc/group`
  emergency-mode failure was not a stable ignition-contract bug; it was a
  poisoned artifact after a failed host-side start. The fresh proof reached
  machine-ready, guest SSH, and guest-binary sync, but forwarded
  machine-API readiness failed because `systemd` could not `exec`
  `/var/lib/nimbus/bin/nimbus` (`status=203/EXEC`, `Permission denied`) on
  FCOS. Live guest inspection showed the copied binary landed with
  `var_lib_t`, while FCOS's writable `/usr/local` path is the symlinked
  `/var/usrlocal` tree with executable `bin_t` labeling. Updated the active
  MAC4 contract accordingly: host-managed guest-binary sync now targets
  `/usr/local/bin/nimbus`, backed by `/var/usrlocal/bin/nimbus`, and the
  guest-proof helpers now probe that path. Durable conclusion: the remaining
  MAC4 work is no longer "why doesn't Ignition boot?" but "finish the
  forwarded machine-API readiness proof on top of the corrected FCOS
  executable path." Verification during diagnosis:
  `cargo fmt --all --check`,
  `cargo check -p nimbus-bin`,
  `cargo test -p nimbus-bin machine:: -- --test-threads=1`,
  `cargo test -p nimbus-bin remote_shell_command_single_quotes_guest_scripts_for_ssh -- --test-threads=1`,
  `bash scripts/verify-nimbus-machine-guest-proof-helper.sh`,
  plus fresh isolated host boot/SSH/systemd proof under `/tmp/nimbus-mac4-fresh.rcYPF1`
  and `/tmp/nimbus-mac4-live.f1HfVQ`.
- 2026-04-16: Reframed the active MAC4 contract around one host-managed
  convergence path instead of two operator-facing upgrade stories. The plan
  now makes the host `nimbus` release authoritative for both desired macOS
  artifacts: the pinned Podman machine-image digest and the matching Linux
  guest `nimbus` asset for the local host architecture. `nimbus machine start`
  is now the primary operator-facing path: it should cache missing artifacts,
  reuse a machine when the recorded base image already matches the desired
  digest, rebuild or recreate the machine when the base image drifts, sync
  the guest executable by hash, and require forwarded machine-API readiness
  before reporting success. Ignition is now explicitly documented as
  version-agnostic and limited to SSH keys, mounts, writable directories, and
  guest units rather than versioned guest-binary delivery. Updated the MAC4
  roadmap summary, machine-image decision, convergence-path guidance,
  acceptance criteria, implementation plan, and
  `docs/reference/macos-machine-flow.md` to match the current code shape in
  `crates/nimbus-bin/src/machine/{mod.rs,manager.rs,bootstrap.rs}`. Durable
  conclusion: the next implementation slice should tighten the checked-in
  Podman `6.0` tag into an immutable digest, harden artifact cache keys and
  machine rebuild semantics, and then prove the full end-to-end convergence
  contract on the current Mac host. Verification: docs-only review against the
  current worktree.
- 2026-04-13: Created the dedicated macOS machine-support control plane after
  the Linux microVM and service-control plans were archived. Verified against
  the local Podman source that the current docs needed one important transport
  correction: on Apple's Podman machine path, `gvproxy` is the primary guest
  networking and API-forwarding component, while `vsock` is used for the ready
  signal and first-boot ignition injection rather than as the general-purpose
  API transport. Also re-verified that Nimbus does not yet expose
  `nimbus machine ...` in `crates/nimbus-bin/src/main.rs`, so machine support
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
  The durable mapping is now documented as: guest Nimbus API parallels Podman's
  guest Podman socket, macOS guest workloads remain standard containers, and
  "service" stays the Nimbus abstraction while "container" names the macOS v1
  execution mechanism. Also aligned the stable CLI docs with the current binary:
  server startup is still flag-driven today, while `nimbus serve` remains
  target command taxonomy rather than shipped subcommand behavior.
- 2026-04-13: Completed `MAC2`. The repo now has `crates/nimbus-bin/src/machine/`
  with `MachineCommand` wiring in `crates/nimbus-bin/src/main.rs`, a typed
  XDG-style config root plus state root, a short `/tmp/nimbus` runtime
  root with typed socket/pid/log paths, persisted machine config/status files,
  and focused parser/unit tests for `init`, `start`, `stop`, `status`, `ssh`,
  and `rm`. The landed MAC2 surface is intentionally honest about the phase
  boundary: `init`, `status`, and `rm` operate on real local machine state,
  while `start`, `stop`, and `ssh` validate initialized state and return a
  clear MAC3-owned error until direct `krunkit` + `gvproxy` orchestration
  lands. Verification: `cargo fmt --all --check`; `cargo check -p nimbus-bin`;
  `cargo test -p nimbus-bin`; `cargo build -p nimbus-bin`; `target/debug/nimbus --help`
  showed both `machine` and `service`; temp-home CLI verification under
  `/tmp/nimbus-mac2-cli.1H8o4C` used
  `env HOME=/tmp/nimbus-mac2-cli.1H8o4C target/debug/nimbus machine status`,
  `env HOME=/tmp/nimbus-mac2-cli.1H8o4C target/debug/nimbus machine init --cpus 4 --memory-mib 4096 --disk-gib 40 --image ghcr.io/nimbus/nimbus-machine-os:test --volume /Users:/Users`,
  `env HOME=/tmp/nimbus-mac2-cli.1H8o4C target/debug/nimbus machine start`, and
  `env HOME=/tmp/nimbus-mac2-cli.1H8o4C target/debug/nimbus machine rm`.
- 2026-04-13: Promoted `MAC3` to `in_progress`. The repo now has a real
  `crates/nimbus-bin/src/machine/manager.rs` seam that resolves `krunkit` and
  `gvproxy`, enforces a short runtime root, persists runtime metadata
  (helper paths, EFI store, SSH port, ready-vsock port, REST endpoint), and
  launches both helpers with Podman-aligned device wiring for `virtio-net`,
  ready-signal `vsock` (`1025`), first-boot ignition `vsock` (`1024`), and
  `virtiofs` mount tags. Host validation on the current Mac used
  `HOME=/tmp/nimbus-mac3-cli.w0y1sy`,
  `NIMBUS_MACHINE_RUNTIME_ROOT=/tmp/nimbus-machine-mac3.w0y1sy`,
  `target/debug/nimbus machine init --image /Users/jack/.local/share/containers/podman/machine/libkrun/nimbus-libkrun-users-only-arm64.raw --ssh-identity /Users/jack/.local/share/containers/podman/machine/machine --ignition-file /Users/jack/.config/containers/podman/machine/libkrun/nimbus-libkrun-users-only.ign --efi-store /Users/jack/.local/share/containers/podman/machine/libkrun/efi-bl-nimbus-libkrun-users-only --volume /Users:/Users`,
  and `target/debug/nimbus machine start`. The resulting runtime inventory was
  real and host-owned under `/tmp/nimbus-machine-mac3.w0y1sy/default/`:
  `sockets/gvproxy.sock`, `sockets/gvproxy.sock-krun.sock`,
  `sockets/krunkit.sock`, `sockets/ready.sock`, `sockets/ignition.sock`,
  plus `logs/machine.log`, `logs/gvproxy.log`, and `logs/krunkit.log`. The
  current blocker is not helper discovery or socket budgeting anymore. It is
  guest compatibility when booting a borrowed Podman disk under the Nimbus
  manager: without ignition the guest timed out fetching first-boot config
  from `vsock` `1024`; with ignition wired, the guest reached the bootstrap
  path but still failed to reach ready, including runs that reused Podman's
  EFI store. Podman source review tightened the boundary here:
  `apple/ignition.go` serves ignition as an HTTP handler over the unix socket,
  while `apple/ignition/ready.go` sends the ready signal over `vsock` `1025`.
  Durable conclusion: the direct host-manager seam is real and worth keeping,
  but a Nimbus-owned guest image/bootstrap lane is the next reliable closeout
  path for machine-ready proof. Verification to this point:
  `cargo fmt --all --check`; `cargo check -p nimbus-bin`; `cargo test -p nimbus-bin`;
  `cargo build -p nimbus-bin`.
- 2026-04-13: Added the first Nimbus-owned MAC3 operator helpers and verified
  them deterministically. The repo now has `scripts/collect-nimbus-machine-diagnostics.sh`
  plus `scripts/recreate-nimbus-machine.sh`, with matching `make
  collect-nimbus-machine-diagnostics`, `make recreate-nimbus-machine`,
  `make verify-nimbus-machine-diagnostics-helper`, and
  `make verify-nimbus-machine-recreate-helper` entrypoints. The checked-in
  helper verification lane passed with:
  `bash -n scripts/collect-nimbus-machine-diagnostics.sh`;
  `bash -n scripts/recreate-nimbus-machine.sh`;
  `bash -n scripts/verify-nimbus-machine-diagnostics-helper.sh`;
  `bash -n scripts/verify-nimbus-machine-recreate-helper.sh`;
  `bash scripts/verify-nimbus-machine-diagnostics-helper.sh`;
  `bash scripts/verify-nimbus-machine-recreate-helper.sh`;
  `cargo fmt --all --check`;
  `cargo check -p nimbus-bin`;
  `cargo test -p nimbus-bin`.
  A real host bundle now exists at `/tmp/nimbus-machine-mac3-diagnostics`,
  produced by:
  `bash scripts/collect-nimbus-machine-diagnostics.sh --home /tmp/nimbus-mac3-cli.w0y1sy --runtime-root /tmp/nimbus-machine-mac3.w0y1sy --output-dir /tmp/nimbus-machine-mac3-diagnostics --nimbus target/debug/nimbus`
  and rerun outside the sandbox so `ps` capture could succeed. That bundle
  records a real failed machine-manager state (`lifecycle: failed`,
  `manager: failed`), helper paths (`/opt/homebrew/bin/krunkit`,
  `/opt/homebrew/opt/podman/libexec/podman/gvproxy`), the machine SSH port
  (`56215`), the ready-signal port (`1025`), and the short-root socket/log
  inventory under `/tmp/nimbus-machine-mac3.w0y1sy/default/`. It also sharpened
  the remaining blocker with better evidence than the earlier shell notes:
  the guest log in `machine-log-tail.txt` now shows both `Ignition has failed`
  and the root-device failure
  `Failed to detect device /dev/disk/by-uuid/5ce9072d-0f5c-4dd7-9aba-4db470edc836`,
  followed by emergency mode. That is a strong signal that the borrowed
  Podman disk/bootstrap pair is a diagnostics fixture, not a durable Nimbus
  machine-ready artifact.
- 2026-04-13: Re-read the live workspace against the macOS target before
  starting MAC4 and found one architectural gap the earlier plan draft needed
  to own more explicitly. The current codebase had a krun-only executable
  service path, which meant a Podman-aligned macOS guest could not be
  completed by image/bootstrap work alone. The plan now records this directly
  under MAC4: we need a guest-side standard-container launch family (or an
  equivalent backend-selection seam) so guest Nimbus can run standard
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
  It also reinforces the newly recorded backend gap: the Nimbus guest-image
  recipe should be FCOS-like and container-oriented, but guest-image work
  alone is insufficient until the guest can select a standard-container launch
  family instead of the current krun-only backend.
- 2026-04-13: Landed the first code-level MAC4 backend-selection seam without
  changing Linux execution behavior. `crates/nimbus-sandbox/src/backend.rs`
  now includes `SandboxBackendKind::Container` alongside `Krun`,
  `crates/nimbus-bin/src/service/compose.rs` now carries
  `x-nimbus.backend: container|krun` through Compose lowering, and
  `crates/nimbus-bin/src/service/project.rs` now derives generic backend roots
  under `services/projects/<project>/backends/<backend>/...` instead of
  assuming every future backend is `krun`. This does **not** mean the guest
  container backend is finished: Linux production behavior is unchanged and
  the MAC4 executor work is still open. Verification:
  `cargo fmt --all --check`;
  `cargo check -p nimbus-bin -p nimbus-sandbox`;
  `cargo test -p nimbus-bin`.
- 2026-04-13: Hardened that MAC4 seam so the executable surface is now honest
  about what it can run. `crates/nimbus-bin/src/service/mod.rs` now validates
  Compose backend selection before building the service manager or running
  `service up/down/list/inspect/logs/ps`: container-only projects now fail
  fast with a clear "krun only today" error, and mixed-backend projects now
  fail fast for project-wide operations while still allowing service-scoped
  commands that target a krun-backed service explicitly. This keeps Linux
  behavior unchanged, prevents silent misrouting of future macOS container
  intent through the krun executor, and gives MAC4 a clean place to land the
  real guest-side container backend next. Verification:
  `cargo fmt --all --check`;
  `cargo check -p nimbus-bin -p nimbus-sandbox`;
  `cargo test -p nimbus-bin`.
- 2026-04-13: Landed the first Nimbus-owned MAC4 guest bootstrap generator in
  the executable path. `crates/nimbus-bin/src/machine/bootstrap.rs` now
  renders a generated ignition file under the machine config root whenever the
  machine config does not point at an explicit ignition override, and
  `MachineLaunchPlan::build` now always wires the first-boot ignition vsock
  device with that resolved file. The generated payload preserves Podman's
  narrow vsock roles and probe layering: `ready.service` only signals
  machine-ready over `vsock` `1025`, while guest Nimbus readiness remains a
  separate later probe, and the payload also carries a guest
  `nimbus-serve.service` plus virtiofs mount units derived from the recorded
  host volume map. Focused tests now cover generated ignition rendering,
  SSH-key carry-through, mount-unit generation, and launch-plan ignition
  wiring. Verification:
  `cargo fmt --all --check`;
  `cargo check -p nimbus-bin -p nimbus-sandbox`;
  `cargo test -p nimbus-bin`.
- 2026-04-13: Added the first checked-in Nimbus machine-image recipe and made
  the bootstrap units explicit repo assets instead of leaving them only as
  inline Rust strings. `crates/nimbus-bin/src/machine/assets/` now holds the
  generated-ignition systemd templates (`ready.service`, `nimbus-serve`, and
  the virtiofs mount helpers), and `images/nimbus-machine-os/` now owns a
  Podman-aligned Fedora CoreOS recipe with `Containerfile.COREOS`,
  `build-common.sh`, `build.sh`, and a package-contract README. The companion
  repo-owned verifier `scripts/verify-nimbus-machine-os-recipe.sh` now checks
  shell syntax, FCOS/bootc build anchors, the required guest package set
  (`crun`, `conmon`, `buildah`, `containers-common`, `netavark`,
  `aardvark-dns`, `openssh-server`, `socat`, `uidmap`), the explicit removal
  of `runc`/Docker-era runtimes, and the expected bootstrap placeholders. This
  closes more of the MAC4 repo-output gap while leaving the real Linux-host
  image build and boot proof for the required host-validation lane. Verification:
  `cargo fmt --all --check`;
  `cargo check -p nimbus-bin -p nimbus-sandbox`;
  `cargo test -p nimbus-bin`;
  `bash scripts/verify-nimbus-machine-os-recipe.sh`.
- 2026-04-13: Strengthened the MAC4 recipe verification lane so it now
  exercises the build orchestration instead of only grepping static files.
  `images/nimbus-machine-os/build.sh` now has narrow test-only overrides for
  OS/root detection, and `scripts/verify-nimbus-machine-os-recipe.sh` now
  launches the full build recipe against fake `podman`, `rpm-ostree`, and
  `custom-coreos-disk-images` helpers. That verifier now proves the staged
  context includes the Linux `nimbus` binary, the `podman build` call carries
  the FCOS base-image argument, the `rpm-ostree compose build-chunked-oci`
  output path is wired correctly, and the optional disk-image conversion path
  requests `--platforms applehv`. Verification:
  `cargo fmt --all --check`;
  `bash scripts/verify-nimbus-machine-os-recipe.sh`.
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
  string. `crates/nimbus-bin/src/machine/mod.rs` now records a typed guest
  image source in machine config: published OCI reference by default
  (`docker://ghcr.io/nimbus/nimbus-machine-os:latest`), with explicit
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
  `cargo check -p nimbus-bin`;
  `cargo test -p nimbus-bin`.
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
  `cargo check -p nimbus-bin`;
  `cargo test -p nimbus-bin`.
- 2026-04-13: Clarified the macOS control-plane boundary directly in the active
  plan so future work does not drift into a split-brain host/guest design.
  The plan now states explicitly that macOS `nimbus serve` should be a thin
  local launcher/proxy into a **guest-resident** authoritative Nimbus server,
  and that `nimbus service ...` on macOS should route service-control requests
  to that guest Nimbus server rather than inventing a second host-owned
  service-control database. It also records the precise way Nimbus is similar
  to Podman and the precise way it is narrower than Podman's generic remote
  connection model. Verification: docs-only plan update.
- 2026-04-13: Added a durable architecture rationale for the macOS fork point
  at `docs/research/macos-host-vs-guest-control-plane-rationale.md`. That note
  compares the two viable options explicitly:
  (A) host-resident Nimbus plus guest containers, and
  (B) guest-resident authoritative Nimbus plus guest containers.
  It records the decision heuristic that matters most: Linux parity is about
  Nimbus living next to the workload runtime on the platform host, not merely
  about sharing the same physical laptop. The current recommendation remains
  Option B for macOS v1, while preserving the hybrid Option A as a consciously
  rejected-but-revisitable design. Verification: docs-only rationale update.
- 2026-04-13: Expanded that ADR from a pure topology argument into a DX-focused
  evaluation after re-reading Podman's socket-forwarding and readiness code in
  `pkg/machine/shim/networking.go`, `pkg/machine/shim/networking_unix.go`, and
  `pkg/machine/ssh.go`. The rationale now records why the hybrid model
  (host-resident Nimbus plus guest containers) is materially attractive for
  developer and AI-agent feedback loops, which exact Podman seams could be
  reused (`gvproxy`-forwarded guest socket, SSH-backed readiness, local
  localhost ergonomics), and what a narrow remote guest machine-API seam would
  look like if MAC5/MAC6 were rewritten around that choice. The active macOS
  plan itself is **not** switched yet; Option B remains the current default
  until an explicit plan rewrite occurs. Verification: docs-only ADR update.
- 2026-04-13: Landed the first real MAC4 image-materialization implementation
  instead of keeping the whole lane as plan prose. `crates/nimbus-bin/src/machine/manager.rs`
  now downloads `http(s)` guest-image sources directly into the reserved
  machine-state raw-disk path and transparently decompresses `.gz` artifacts on
  the way in. The existing typed image-source model still keeps the default
  published OCI reference (`docker://...`) and continues to reuse the reserved
  raw disk when one is already staged there, but OCI registry pull itself
  remains the next missing slice. Focused tests now prove four paths:
  published OCI rejected when unstaged, published OCI reused when staged,
  raw `http(s)` materialization, and gzip `http(s)` materialization. Verification:
  `cargo fmt --all --check`;
  `cargo check -p nimbus-bin`;
  `cargo test -p nimbus-bin`.
- 2026-04-13: Closed the loop between the Linux build recipe and the new macOS
  URL materialization path. `images/nimbus-machine-os/build.sh` now detects the
  raw disk produced by `custom-coreos-disk-images`, compresses it into a stable
  publishable artifact name (`nimbus-machine-os.raw.gz`), and records both raw
  and compressed paths in `summary.txt`. The repo-owned verifier
  `scripts/verify-nimbus-machine-os-recipe.sh` now proves that compressed
  artifact exists and can be decompressed after the fake-tool build run.
  Together with the new `http(s)`/`.gz` materializer in the host manager, that
  gives MAC4 one real end-to-end artifact shape even before the OCI
  registry-pull lane lands. Verification:
  `cargo fmt --all --check`;
  `cargo check -p nimbus-bin`;
  `cargo test -p nimbus-bin`;
  `bash scripts/verify-nimbus-machine-os-recipe.sh`.
- 2026-04-13: Switched the active macOS execution direction for `MAC5` and
  `MAC6` from the earlier guest-authoritative `nimbus serve` model to the
  hybrid DX-first model: host-resident authoritative Nimbus API/runtime/storage
  plus a narrow guest machine-API seam for service execution. This rewrite is
  intentionally Podman-aligned at the machine boundary, not Podman-dependent:
  it copies `startHostForwarder(...)`, `setupForwardingLinks(...)`, and
  `conductVMReadinessCheck(...)` as the reference shape for socket forwarding
  and layered readiness, while keeping the guest protocol Nimbus-specific and
  service-runtime-scoped instead of growing a generic remote engine. The active
  checkpoints now explicitly treat the already-landed guest
  `nimbus-serve.service` assets as transitional MAC4 output that must evolve
  into a guest `nimbus.sock` / machine-API contract. Verification: docs-only plan
  rewrite against the checked-in rationale and local Podman source.
- 2026-04-13: Landed the first real MAC4 guest machine-API scaffold. The repo
  now has hidden `nimbus machine api` wiring plus a narrow unix-socket HTTP
  surface with `/healthz` and `/v1/machine-api/capabilities`, including both
  direct socket binding and systemd socket-activation support. The generated
  guest Ignition path now renders `nimbus.socket` and `nimbus.service` instead
  of the earlier guest-authoritative `nimbus-serve.service`, and the
  machine-os recipe verifier now checks those assets directly. This
  intentionally keeps `service_execution_ready: false` until MAC4 lands the
  guest standard-container launch family and MAC5 lands the forwarded host
  control socket plus guest machine-API client. Verification:
  `cargo fmt --all --check`;
  `cargo check -p nimbus-bin`;
  `cargo test -p nimbus-bin`;
  `bash scripts/verify-nimbus-machine-os-recipe.sh`.
- 2026-04-13: Tightened the reserved control-socket naming to match Podman's
  machine layout more closely. `MachinePaths` no longer records a generic
  `api.sock`; it now reserves host `<machine>-api.sock` under the short
  machine runtime root, while the guest bootstrap reserves `/run/nimbus/nimbus.sock`
  behind `nimbus.socket`. The MAC3 cleanup path treats that typed host socket
  as part of the runtime artifact inventory, keeping the host-manager/state
  model aligned with the guest contract before MAC5 lands the actual
  forwarding/proxy implementation. Verification:
  `cargo fmt --all --check`;
  `cargo check -p nimbus-bin`;
  `cargo test -p nimbus-bin`.
- 2026-04-13: Added the first host-side machine-API client scaffold to pair
  with the guest `nimbus.sock` server. `nimbus-bin` now has shared protocol
  types for health/capabilities plus a typed unix-socket client that can read
  those endpoints from a forwarded guest socket. This is intentionally still a
  narrow MAC5 bridge: it gives the upcoming forwarding/proxy work and host
  `nimbus serve` integration a stable typed control seam without pretending
  that guest container execution or host-local socket forwarding already exist.
  Verification:
  `cargo fmt --all --check`;
  `cargo check -p nimbus-bin`;
  `cargo test -p nimbus-bin`;
  `bash scripts/verify-nimbus-machine-os-recipe.sh`.
- 2026-04-13: Started the first real MAC5 control-channel plumbing in the host
  manager. `MachineLaunchPlan::build` now configures `gvproxy` with
  Podman-shaped SSH forwarding arguments when the machine has an SSH identity:
  host `<machine>-api.sock` via `-forward-sock`, guest `/run/nimbus/nimbus.sock`
  via `-forward-dest`, `root` via `-forward-user`, and the configured machine
  key via `-forward-identity`. This keeps the host-side forwarding shape close
  to Podman's machine plumbing while preserving the narrower Nimbus guest API.
  The work is intentionally recorded as MAC5 `in_progress`, not `done`:
  host-local forwarded-socket proof on a real Mac and published guest-service
  port proof are still open. Verification:
  `cargo fmt --all --check`;
  `cargo check -p nimbus-bin`;
  `cargo test -p nimbus-bin machine::manager::tests::launch_plan_requires_bootable_local_disk_image -- --nocapture`;
  `cargo test -p nimbus-bin machine::manager::tests::launch_plan_adds_gvproxy_machine_api_forwarding_when_ssh_identity_exists -- --nocapture`.
- 2026-04-13: Added the first host-side guest-machine-API probe/reporting slice
  on top of that MAC5 forwarding shape. `nimbus machine status` now renders a
  dedicated machine-API section that distinguishes:
  socket path, socket existence, and actual API reachability. This preserves
  the planned probe layering instead of collapsing "host socket file exists"
  into "guest control plane is ready". Verification:
  `cargo fmt --all --check`;
  `cargo check -p nimbus-bin`;
  `cargo test -p nimbus-bin machine::tests::machine_status_marks_missing_machine_api_socket_as_unreachable -- --nocapture`;
  `cargo test -p nimbus-bin machine::tests::machine_status_detects_reachable_machine_api_socket -- --nocapture`.
- 2026-04-13: Renamed the active guest/host control-socket vocabulary to match
  Podman's machine layout more closely and remove the older overloaded guest
  daemon label. The guest bootstrap now owns `nimbus.socket` plus
  `nimbus.service` for `/run/nimbus/nimbus.sock`, the hidden guest daemon is
  `nimbus machine api`, and the host runtime root now reserves
  `<machine>-api.sock`. This keeps the machine-layer nouns explicit, product-
  branded, and easier to distinguish from the `nimbus-runtime` crate while
  preserving the deliberate divergence that the guest API stays narrower than
  Podman's generic engine API. Verification:
  `cargo fmt --all --check`;
  `cargo check -p nimbus-bin`;
  `cargo test -p nimbus-bin`;
  `bash scripts/verify-nimbus-machine-os-recipe.sh`.
- 2026-04-13: Aligned the Nimbus-owned MAC3 diagnostics and recreate helpers
  with the newer Podman-shaped flat runtime-root layout. The helper scripts no
  longer assume `runtime-root/<machine>/sockets` and `runtime-root/<machine>/logs`;
  they now capture and verify the actual machine-manager shape:
  `<machine>-api.sock`, `<machine>.sock`, `<machine>-gvproxy.sock`,
  `<machine>-krunkit.sock`, and sibling `*.log` / `*.pid` files under the
  short runtime root. This closes an operator-tooling drift point that would
  have made the checked-in diagnostics evidence disagree with the code. Verification:
  `cargo fmt --all --check`;
  `bash scripts/verify-nimbus-machine-diagnostics-helper.sh`;
  `bash scripts/verify-nimbus-machine-recreate-helper.sh`.
- 2026-04-13: Tightened the default short runtime-root from
  `/tmp/nimbus-machine` to `/tmp/nimbus`. This keeps the Podman-aligned
  short-path principle while dropping an unnecessary extra path segment from
  every host socket, pid, and log path. The rationale is source-backed and
  operationally grounded: Podman uses `$TMPDIR/podman` as a generic runtime-dir
  convention, but the Nimbus macOS host evidence says short Darwin unix-socket
  paths matter more than inheriting the longer per-user `$TMPDIR` prefix. The
  code, CLI docs, distribution notes, and Nimbus-owned helper defaults now all
  point at `/tmp/nimbus`, while the historical execution evidence keeps the
  original `/tmp/nimbus-machine-*` paths unchanged. Verification:
  `cargo fmt --all --check`;
  `cargo check -p nimbus-bin`;
  `cargo test -p nimbus-bin`.
- 2026-04-13: Extended the host-side `nimbus machine status` machine-API view
  from pure reachability reporting into an explicit forwarding-contract view.
  When a machine has an SSH identity configured, status now reports the exact
  MAC5 control-channel mapping:
  host `<machine>-api.sock` -> `gvproxy` SSH-forwarded unix socket ->
  guest `/run/nimbus/nimbus.sock`, plus the forwarding user (`root`) and the
  configured identity path. This makes the Podman-aligned control channel
  visible in the shipped CLI instead of forcing operators to reverse-engineer
  it from helper command lines. Verification:
  `cargo fmt --all --check`;
  `cargo check -p nimbus-bin`;
  `cargo test -p nimbus-bin`.
- 2026-04-13: Closed `MAC3` with a fresh host-managed proof on the current Mac
  and clarified the exact remaining MAC4 blocker. Using the already-cached
  Podman Fedora CoreOS base raw image at
  `/Users/jack/.local/share/containers/podman/machine/libkrun/cache/45d6e5983955ea9d6e4cef451847ede7a5acbf27d77fb661999c7f33d595b0b0.raw.zst`,
  Nimbus materialized a clean diagnostic disk at
  `/tmp/nimbus-mac-images/podman-libkrun-cache-run8.raw` via
  `zstd -dc ... > /tmp/nimbus-mac-images/podman-libkrun-cache-run8.raw`, then
  launched it directly with:
  `HOME=/tmp/nimbus-mac-home-run8 NIMBUS_MACHINE_RUNTIME_ROOT=/tmp/nimbus-mac-runtime-run8 target/debug/nimbus machine init --image /tmp/nimbus-mac-images/podman-libkrun-cache-run8.raw --ssh-identity /Users/jack/.ssh/id_ed25519 --volume /Users:/Users`;
  `HOME=/tmp/nimbus-mac-home-run8 NIMBUS_MACHINE_RUNTIME_ROOT=/tmp/nimbus-mac-runtime-run8 target/debug/nimbus machine start`.
  The machine reached `lifecycle: running` plus `manager: ready`, allocated SSH
  port `54059`, and produced a full diagnostics bundle at
  `/tmp/nimbus-machine-mac3-run8-diagnostics` via
  `bash scripts/collect-nimbus-machine-diagnostics.sh --home /tmp/nimbus-mac-home-run8 --runtime-root /tmp/nimbus-mac-runtime-run8 --output-dir /tmp/nimbus-machine-mac3-run8-diagnostics --nimbus target/debug/nimbus`.
  A direct SSH proof then succeeded with
  `ssh -i /Users/jack/.ssh/id_ed25519 -p 54059 -o BatchMode=yes -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -o ConnectTimeout=5 core@127.0.0.1 ...`,
  and the captured proof file
  `/tmp/nimbus-machine-mac3-run8-diagnostics/guest-nimbus-proof.txt` shows the
  exact MAC4 seam: `nimbus.socket` is `active`, `nimbus.service` is `inactive`
  until activated, and `/usr/local/bin/nimbus` is missing, so the host
  `<machine>-api.sock` exists but the guest machine API still cannot answer.
  The same machine was then stopped and removed cleanly with:
  `HOME=/tmp/nimbus-mac-home-run8 NIMBUS_MACHINE_RUNTIME_ROOT=/tmp/nimbus-mac-runtime-run8 target/debug/nimbus machine stop`;
  `HOME=/tmp/nimbus-mac-home-run8 NIMBUS_MACHINE_RUNTIME_ROOT=/tmp/nimbus-mac-runtime-run8 target/debug/nimbus machine rm`;
  followed by `ps -axww -o pid=,ppid=,stat=,command= | rg 'nimbus-mac-runtime-run8|podman-libkrun-cache-run8|default-gvproxy|default-krunkit' || true`,
  which left no matching helper processes. Durable conclusion: MAC3 is done;
  MAC4 now owns the guest-image packaging and guest machine-API executable.
  Verification:
  `cargo fmt --all --check`;
  `cargo check -p nimbus-bin`;
  `cargo test -p nimbus-bin`.
- 2026-04-13: Landed the remaining MAC4 image-materialization lane by adding a
  Podman-aligned OCI machine-artifact puller to
  `crates/nimbus-bin/src/machine/manager.rs`. Published `docker://...` guest
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
  now needs a real Nimbus machine image build/publish path so the guest
  contains `/usr/local/bin/nimbus` and can answer over `nimbus.socket`.
  Verification:
  `cargo fmt --all --check`;
  `cargo check -p nimbus-bin`;
  `cargo test -p nimbus-bin machine::manager::tests::registry_image_reference_materializes_raw_disk_from_oci_registry -- --nocapture`;
  `cargo test -p nimbus-bin`.
- 2026-04-13: Added the canonical Linux/CI guest-image build wrapper for the
  remaining MAC4 supply-side work. `scripts/build-nimbus-machine-os.sh` now
  owns the repo-level build entrypoint above
  `images/nimbus-machine-os/build.sh`: it can either consume an explicit Linux
  `nimbus` binary or build one first with `cargo build -p nimbus-bin`, then
  pass the correct arguments through to the machine-os recipe. The repo now
  also has `scripts/verify-nimbus-machine-os-build-helper.sh` plus the Makefile
  targets `build-nimbus-machine-os` and
  `verify-nimbus-machine-os-build-helper`, so Linux hosts and CI runners have
  one canonical invocation path instead of ad hoc shell history. Durable
  conclusion: MAC4's supply-side tooling is now checked in; the remaining MAC4
  blocker is a real Linux-built/published guest artifact and the corresponding
  host-local proofs (`nimbus --version` inside the guest, first-boot logs,
  and guest machine-API reachability). Verification:
  `bash -n scripts/build-nimbus-machine-os.sh`;
  `bash -n scripts/verify-nimbus-machine-os-build-helper.sh`;
  `bash scripts/verify-nimbus-machine-os-build-helper.sh`;
  `bash scripts/verify-nimbus-machine-os-recipe.sh`;
  `cargo fmt --all --check`.
- 2026-04-13: Strengthened the MAC4 guest machine-API contract so it now
  reports real service-runtime intent instead of a bare placeholder boolean.
  `crates/nimbus-bin/src/machine/protocol.rs` and
  `crates/nimbus-bin/src/machine/api.rs` now advertise the target
  `standard_containers` execution mode, the `container` backend family, the
  required guest runtime binaries from the checked-in machine image contract
  (`buildah`, `conmon`, `crun`, `netavark`, `aardvark-dns`,
  `fuse-overlayfs`), and the explicit blockers that still keep
  `service_execution_ready` false until service lifecycle operations land. The
  host-side status surface in `crates/nimbus-bin/src/machine/mod.rs` now also
  fetches and renders that decoded capability contract whenever
  `<machine>-api.sock` is reachable, so operator output distinguishes "socket
  answered" from "guest runtime seam is actually ready" with much more
  precision. Durable conclusion: MAC4 now has a truthful, typed readiness
  contract for the future guest standard-container executor, but the guest
  still does not execute service lifecycle operations yet. Verification:
  `cargo fmt --all --check`;
  `cargo check -p nimbus-bin`;
  `cargo test -p nimbus-bin -- --nocapture`.
- 2026-04-13: Landed the first typed Nimbus-owned service-sandbox operations
  on top of that MAC4/MAC5 machine-API seam. The guest daemon in
  `crates/nimbus-bin/src/machine/api.rs` now exposes image-backed start,
  build-backed start, inspect, and stop routes under
  `/v1/machine-api/service-sandboxes/...`, all expressed in existing Nimbus
  sandbox nouns (`SandboxImageLaunchSpec`, `SandboxBuildLaunchSpec`,
  `SandboxHandle`) rather than a generic container-engine vocabulary. The
  host-side typed unix-socket client in
  `crates/nimbus-bin/src/machine/client.rs` now drives those routes directly,
  and the capability contract now reports those operations dynamically when a
  real service backend is present. Durable conclusion: the macOS control plane
  now has the correct RPC shape for a future host-side remote sandbox backend,
  but MAC4/MAC5 still need the real guest standard-container executor and the
  host-local forwarded-socket proof on a live machine. Verification:
  `cargo fmt --all --check`;
  `cargo check -p nimbus-bin`;
  `cargo test -p nimbus-bin machine::api::tests::machine_api_serves_health_and_capabilities_over_unix_socket -- --nocapture`;
  `cargo test -p nimbus-bin machine::client::tests::client_round_trips_service_sandbox_operations_when_backend_is_available -- --nocapture`;
  `cargo test -p nimbus-bin`.
- 2026-04-13: Added the first Nimbus-owned MAC4 guest-image proof collector on
  top of the existing machine CLI and guest bootstrap contract.
  `scripts/collect-nimbus-machine-guest-proof.sh` now captures a booted
  machine's host-local proof bundle using `nimbus machine status` plus
  `nimbus machine ssh -- ...`: guest `nimbus --version`, required runtime
  binaries (`buildah`, `conmon`, `crun`, `netavark`, `aardvark-dns`,
  `fuse-overlayfs`), `nimbus.socket` / `nimbus.service` state, guest
  machine-API health/capabilities on `/run/nimbus/nimbus.sock`, virtiofs mount
  evidence, and the host-side machine log tail. The repo now also owns the
  deterministic verifier `scripts/verify-nimbus-machine-guest-proof-helper.sh`
  plus the Makefile entrypoints
  `collect-nimbus-machine-guest-proof` and
  `verify-nimbus-machine-guest-proof-helper`. Durable conclusion: MAC4 now has
  a checked-in proof lane for the built Linux guest image once a real artifact
  is booted on macOS; the remaining MAC4 supply-side blocker is still the
  actual produced/published Nimbus guest image artifact itself. Verification:
  `bash -n scripts/collect-nimbus-machine-guest-proof.sh`;
  `bash -n scripts/verify-nimbus-machine-guest-proof-helper.sh`;
  `bash scripts/verify-nimbus-machine-guest-proof-helper.sh`;
  `cargo fmt --all --check`.
- 2026-04-13: Closed the MAC4 registry-artifact ambiguity by packaging the
  guest raw disk in the exact OCI shape the existing manager already consumes.
  `scripts/package-nimbus-machine-os-oci.sh` now turns a produced raw disk
  into an OCI image layout whose manifest/index carry the linux/current-arch +
  `disktype=raw` contract, while `scripts/publish-nimbus-machine-os.sh` now
  owns pushing that layout to a registry and optionally staging release
  assets. The repo also now owns deterministic verifiers plus Makefile
  entrypoints for both scripts. Durable conclusion: MAC4 no longer has an
  undefined publish contract. The remaining blocker is the live Linux-host
  build/publish run that produces a real Nimbus guest image artifact and
  registry evidence. Verification:
  `bash -n scripts/package-nimbus-machine-os-oci.sh`;
  `bash -n scripts/publish-nimbus-machine-os.sh`;
  `bash -n scripts/verify-nimbus-machine-os-oci-layout-helper.sh`;
  `bash -n scripts/verify-nimbus-machine-os-publish-helper.sh`;
  `bash scripts/verify-nimbus-machine-os-oci-layout-helper.sh`;
  `bash scripts/verify-nimbus-machine-os-publish-helper.sh`;
  `cargo fmt --all --check`.
- 2026-04-13: Closed the remaining MAC4 raw-disk-helper ambiguity on the build
  side as well. `scripts/resolve-custom-coreos-disk-images.sh` now resolves
  the exact upstream `coreos/custom-coreos-disk-images` commit Podman pins
  today (`e017ddda3b20b09627f90f68ef1b708016d10864`), and
  `scripts/build-nimbus-machine-os.sh` can now opt into that pinned helper via
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
  `.github/workflows/nimbus-machine-os.yml` now runs fast contract checks on
  `ubuntu-latest` for the machine image recipe/build/package/publish seam, and
  exposes a `workflow_dispatch` lane on a dedicated self-hosted
  `linux arm64 nimbus-machine-os` runner for the real Apple Silicon guest
  image build, raw-disk OCI packaging, artifact upload, and optional GHCR
  publish. Durable conclusion: MAC4 now has a checked-in Linux builder lane;
  the remaining blocker is still the first successful live run and captured
  artifact/proof bundle from that runner plus the resulting macOS boot proof.
  Verification: `cargo check -p nimbus-bin`; `cargo fmt --all --check`;
  `bash scripts/verify-nimbus-machine-os-recipe.sh`;
  `bash scripts/verify-nimbus-machine-os-build-helper.sh`;
  `bash scripts/verify-nimbus-machine-os-oci-layout-helper.sh`;
  `bash scripts/verify-nimbus-machine-os-publish-helper.sh`;
  `bash scripts/verify-custom-coreos-disk-images-resolver-helper.sh`.
- 2026-04-13: Extracted the shared OCI-runtime plumbing out of the krun-only
  internal path inside `nimbus-sandbox`. The generic buildah/image-lowering,
  command-spec, conmon-launch, and published-port-allocation helpers now live
  under `crates/nimbus-sandbox/src/backends/oci/`, while the krun backend
  keeps only the krun-specific bundle, VM config, and lifecycle logic. Durable
  conclusion: MAC4's next code step is no longer "re-find the generic OCI
  pieces inside krun"; it is "land a real container backend on top of the
  explicit `backends/oci/` seam and then wire it into the guest machine API."
  Verification: `cargo check -p nimbus-sandbox -p nimbus-bin`;
  `cargo test -p nimbus-sandbox backends::oci::buildah::tests::wrap_unshare_prefixes_existing_command -- --exact`;
  `cargo test -p nimbus-sandbox backends::oci::conmon::tests::conmon_launch_plan_uses_private_runtime_and_buildah_unshare -- --exact`;
  `cargo test -p nimbus-sandbox backends::oci::port_manager::tests::allocate_missing_bindings_uses_range_and_skips_existing_guest_ports -- --exact`;
  `cargo fmt --all --check`.
- 2026-04-13: Landed the first real guest standard-container executor behind
  the macOS machine API. `nimbus-sandbox` now has
  `crates/nimbus-sandbox/src/backends/container/`, the shared
  `backends/oci/` conmon types are no longer krun-branded, and
  `nimbus machine api` now instantiates a real `ContainerSandboxBackend`
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
  `cargo check -p nimbus-sandbox -p nimbus-bin`;
  `cargo test -p nimbus-sandbox plan_only_backend_persists_a_container_manifest -- --nocapture`;
  `cargo test -p nimbus-sandbox bundle_config_rejects_non_matching_host_and_guest_ports -- --nocapture`;
  `cargo test -p nimbus-bin capability_response_reports_required_binaries_and_explicit_blockers -- --nocapture`;
  `cargo test -p nimbus-bin machine_api_serves_health_and_capabilities_over_unix_socket -- --nocapture`.
- 2026-04-13: Replaced that guest host-network shortcut with the first real
  Podman-shaped guest bridge/published-port lane. `nimbus-sandbox` now owns
  `backends/oci/network.rs`, `ContainerSandboxBackendConfig` now records
  `netavark`/`aardvark-dns` plus a shared published-port range and optional
  guest machine-forwarder config, container bundles now carry an explicit
  network-namespace path, image-exposed ports now auto-allocate from the
  shared OCI port manager instead of collapsing to same-port host networking,
  and `nimbus machine api` now enables a Podman-shaped default
  `gvproxy`-forwarder target
  (`gateway.containers.internal/services/forwarder`) for guest service
  publishing. Durable conclusion: the remaining MAC4/MAC5 gap is no longer
  "design the guest bridge/published-port seam." It is now "prove this exact
  seam on a real booted guest image and capture host-local connectivity
  evidence from macOS." Verification: `cargo fmt --all --check`;
  `cargo check -p nimbus-sandbox -p nimbus-bin`;
  `cargo test -p nimbus-sandbox backends::container::bundle::tests::bundle_config_includes_explicit_network_namespace_and_remapped_ports -- --exact`;
  `cargo test -p nimbus-sandbox backends::oci::network::tests::netavark_request_strips_host_ip_when_machine_forwarding_is_enabled -- --exact`;
  `cargo test -p nimbus-sandbox backends::container::runtime::tests::plan_only_backend_auto_assigns_exposed_ports_from_published_range -- --exact`;
  `cargo test -p nimbus-bin machine::api::tests::capability_response_reports_required_binaries_and_explicit_blockers -- --exact`;
  `cargo test -p nimbus-bin machine::api::tests::machine_api_serves_health_and_capabilities_over_unix_socket -- --exact`.
- 2026-04-13: Tightened the guest machine-API readiness contract around that
  new published-port seam. `MachineApiState` now records the configured guest
  machine-port forwarder, `machine_api_capability_response(...)` now probes
  that endpoint before advertising service lifecycle operations, and the
  capability surface now withholds `image-start` / `build-start` / `inspect` /
  `stop` when the guest cannot actually reach the machine forwarder even if
  the runtime binaries are present. Durable conclusion: the guest API now
  reports MAC5 truthfully instead of advertising a host-local publishing path
  that the guest cannot use yet. Verification: `cargo fmt --all --check`;
  `cargo check -p nimbus-sandbox -p nimbus-bin`;
  `cargo test -p nimbus-bin machine::api::tests::capability_response_reports_machine_port_forwarder_blocker_when_unreachable -- --exact`;
  `cargo test -p nimbus-bin machine::api::tests::machine_api_serves_health_and_capabilities_over_unix_socket -- --exact`;
  `cargo test -p nimbus-bin machine::client::tests::client_reads_health_and_capabilities_from_machine_api_socket -- --exact`.
- 2026-04-13: Fixed the public `nimbus machine ssh` surface to match the
  existing localhost guest-SSH readiness probe. `build_ssh_command(...)` now
  applies the same Podman-aligned localhost-only SSH options
  (`BatchMode=yes`, `IdentitiesOnly=yes`, `StrictHostKeyChecking=no`,
  `UserKnownHostsFile=/dev/null`, `CheckHostIP=no`, `LogLevel=ERROR`), and
  `scripts/collect-nimbus-machine-guest-proof.sh` now captures best-effort
  MAC4 evidence instead of aborting on the first missing guest artifact.
  Durable conclusion: the operator-facing SSH command and the checked-in guest
  proof lane now behave like the readiness path they are supposed to validate,
  which removes a false-negative UX blocker before the remaining MAC4 guest
  image work. Verification: `cargo fmt --all --check`;
  `cargo check -p nimbus-bin`;
  `cargo test -p nimbus-bin ssh_command_applies_localhost_machine_safety_options`;
  `bash scripts/verify-nimbus-machine-guest-proof-helper.sh`.
- 2026-04-13: Ran the first clean host differential against local Podman
  machine fixtures on this Mac. The earlier emergency-mode result from
  `/tmp/nimbus-machine-proof-run2` was corrected: the failure came from
  reusing a mutable Podman machine raw disk, not from invalid generated
  ignition. A pristine raw disk decompressed from Podman's cache to
  `/tmp/nimbus-libkrun-pristine.raw` plus Podman's ignition file booted cleanly
  under Nimbus ownership (`/tmp/nimbus-machine-proof-run3`), reached
  `machine-ready`, reached guest SSH, and produced a guest proof bundle at
  `/tmp/nimbus-machine-guest-proof-run3b`. That bundle proved the base FCOS
  fixture currently has `conmon`, `crun`, `fuse-overlayfs`, and a working
  `/Users` virtiofs mount, but does not yet have `/usr/local/bin/nimbus`,
  `/run/nimbus/nimbus.sock`, `buildah`, `netavark`, or `aardvark-dns`. A
  second pristine raw disk at `/tmp/nimbus-libkrun-pristine2.raw` plus
  Nimbus-generated ignition booted the guest, mounted `/Users`, and emitted
  the ready signal, but `machine start` still failed at the second readiness
  layer with `guest SSH readiness did not arrive within 30 seconds`
  (`/tmp/nimbus-machine-proof-run4`, final status via
  `HOME=/tmp/nimbus-mac-proof-home4 NIMBUS_MACHINE_RUNTIME_ROOT=/tmp/nimbus-macproof4 target/debug/nimbus machine status`).
  Durable conclusion: generated ignition is parse-valid and boot-valid on a
  pristine base disk, so the remaining MAC4 bootstrap delta is now the
  localhost SSH/network readiness contract plus guest image contents, not
  first-boot ignition delivery. Verification: `cargo fmt --all --check`;
  `cargo build -p nimbus-bin`; host recreate runs outside the sandbox with
  `bash scripts/recreate-nimbus-machine.sh --home /tmp/nimbus-mac-proof-home3 --runtime-root /tmp/nimbus-macproof3 --output-dir /tmp/nimbus-machine-proof-run3 --nimbus /Users/jack/src/github.com/nimbus/nimbus/target/debug/nimbus --image /tmp/nimbus-libkrun-pristine.raw --ssh-identity /Users/jack/.local/share/containers/podman/machine/machine --ignition-file /Users/jack/.config/containers/podman/machine/libkrun/nimbus-libkrun-users-only.ign`,
  `bash scripts/collect-nimbus-machine-guest-proof.sh --home /tmp/nimbus-mac-proof-home3 --runtime-root /tmp/nimbus-macproof3 --output-dir /tmp/nimbus-machine-guest-proof-run3b --nimbus /Users/jack/src/github.com/nimbus/nimbus/target/debug/nimbus --image /tmp/nimbus-libkrun-pristine.raw`, and
  `bash scripts/recreate-nimbus-machine.sh --home /tmp/nimbus-mac-proof-home4 --runtime-root /tmp/nimbus-macproof4 --output-dir /tmp/nimbus-machine-proof-run4 --nimbus /Users/jack/src/github.com/nimbus/nimbus/target/debug/nimbus --image /tmp/nimbus-libkrun-pristine2.raw --ssh-identity /Users/jack/.local/share/containers/podman/machine/machine`.
- 2026-04-13: Tightened the generated virtiofs bootstrap toward Podman's
  exact systemd shape. The repo now renders proper `.mount` units instead of
  bespoke oneshot `.service` mount helpers, and the generated immutable-root
  helpers now use Podman's canonical names/descriptions
  (`immutable-root-off.service`, `immutable-root-on.service`). Durable
  conclusion: Nimbus's generated bootstrap is now structurally closer to the
  reference path before the next round of host validation, even though the
  successful run below was still completed with the previously built binary.
  Verification: `cargo fmt --all --check`; `cargo check -p nimbus-bin`;
  `cargo test -p nimbus-bin generated_ignition_includes_ready_nimbus_and_mount_units`;
  `bash scripts/verify-nimbus-machine-os-recipe.sh`.
- 2026-04-13: Re-ran the pristine generated-ignition host lane and closed the
  bootstrap uncertainty. With a third fresh raw disk at
  `/tmp/nimbus-libkrun-pristine3.raw`, Nimbus's generated ignition reached a
  real `running` / `ready` machine on the current Mac host under
  `/tmp/nimbus-machine-proof-run5`; `HOME=/tmp/nimbus-mac-proof-home5
  NIMBUS_MACHINE_RUNTIME_ROOT=/tmp/nimbus-macproof5 target/debug/nimbus machine status`
  reported `lifecycle: running`, `manager: ready`, and the expected forwarded
  host `<machine>-api.sock`. A matching guest proof bundle at
  `/tmp/nimbus-machine-guest-proof-run5` confirmed the same remaining MAC4
  gap as the Podman-reference boot: `/Users` is mounted, but
  `/usr/local/bin/nimbus` and `/run/nimbus/nimbus.sock` are still missing, so
  the forwarded host socket exists without a guest Nimbus API behind it.
  Durable conclusion: generated ignition and basic host lifecycle are now
  proven on this Mac; MAC4 is blocked by guest image contents and guest API
  packaging, not by host-manager readiness anymore. Verification:
  `cargo build -p nimbus-bin`; `bash scripts/recreate-nimbus-machine.sh --home /tmp/nimbus-mac-proof-home5 --runtime-root /tmp/nimbus-macproof5 --output-dir /tmp/nimbus-machine-proof-run5 --nimbus /Users/jack/src/github.com/nimbus/nimbus/target/debug/nimbus --image /tmp/nimbus-libkrun-pristine3.raw --ssh-identity /Users/jack/.local/share/containers/podman/machine/machine`;
  `bash scripts/collect-nimbus-machine-guest-proof.sh --home /tmp/nimbus-mac-proof-home5 --runtime-root /tmp/nimbus-macproof5 --output-dir /tmp/nimbus-machine-guest-proof-run5 --nimbus /Users/jack/src/github.com/nimbus/nimbus/target/debug/nimbus --image /tmp/nimbus-libkrun-pristine3.raw`.
- 2026-04-13: Hardened the remaining MAC4 evidence contract instead of adding
  more speculative lifecycle theory. `scripts/collect-nimbus-machine-guest-proof.sh`
  now captures deterministic guest `nimbus` sha256 proof, per-binary presence
  lines for the required runtime toolchain, and machine-readable
  `systemctl show` output for `nimbus.socket` / `nimbus.service`. In parallel,
  `images/nimbus-machine-os/build.sh` now records sha256 provenance for the
  staged Linux `nimbus` binary, the checked-in recipe files, the OCI archive,
  and the optional raw/compressed raw disk artifact in `summary.txt`. Durable
  conclusion: the next Linux machine-image build and the next macOS guest boot
  proof can now be compared by concrete binary/artifact identity instead of
  only by filename and tag. Verification: `bash scripts/verify-nimbus-machine-os-recipe.sh`;
  `bash scripts/verify-nimbus-machine-guest-proof-helper.sh`;
  `cargo fmt --all --check`.
- 2026-04-13: Recorded the publish/hosting decision explicitly so the next
  MAC4/MAC7 work does not have to infer it from workflow YAML alone. Nimbus
  will keep Podman's build/consume shape but use repo-native delivery
  infrastructure: GitHub-hosted Actions for contract verification, a dedicated
  self-hosted `linux arm64 nimbus-machine-os` runner for the real Apple
  Silicon guest-image build, and GHCR for the published raw-disk OCI artifact.
  Immutable version tags are the release truth; moving aliases such as
  `stable` or `latest` are convenience pointers on top. Durable conclusion:
  the remaining work is not choosing a hosting model anymore, it is producing
  the first real versioned guest artifact and then teaching the macOS default
  image policy to consume that release channel deliberately. Verification:
  repo review of `.github/workflows/nimbus-machine-os.yml`,
  `images/nimbus-machine-os/README.md`, and this plan; `cargo fmt --all --check`.
- 2026-04-13: Turned that release-channel policy into repo behavior. The CLI
  default machine image now points at the matching versioned release reference
  `docker://ghcr.io/nimbus/nimbus-machine-os:v{CARGO_PKG_VERSION}` so
  macOS consumes the guest image that matches the host binary release by
  default. Moving aliases such as `stable` and `latest` remain convenience
  pointers on top rather than the default host contract. Durable conclusion:
  the remaining machine-image release work is now operational, not semantic —
  produce the first real versioned artifact and publish it through the shaped
  workflow. Verification: `cargo test -p nimbus-bin parses_machine_init_defaults_to_version_pinned_release_image -- --exact`; `cargo fmt --all --check`.
- 2026-04-15: Landed the Podman-shaped machine-image repo split for real. The
  guest image source and workflow now live in `nimbus/nimbus-machine-os`,
  while the host `nimbus/nimbus` release workflow now uses that repo in
  two phases on `v*` tags: reusable-workflow contract build first, then a
  native machine-os release dispatch with the same tag. Durable conclusion:
  future MAC4 supply-side fixes belong in the machine-os repo, while this repo
  owns only the consumer contract, guest bootstrap assets, and host machine
  manager integration.
  Verification: repo review of `nimbus/nimbus/.github/workflows/release.yml`;
  repo review of `nimbus/nimbus-machine-os/.github/workflows/build.yml`;
  `cargo fmt --all --check`.
- 2026-04-15: Tightened that split into an explicit cross-repo machine-image
  contract instead of relying on repo-name guessing. The external
  `nimbus/nimbus-machine-os` workflow now treats standalone `v*` tags as
  the embedded Nimbus version, annotates the packaged OCI artifact with
  `org.opencontainers.image.source`,
  `io.nimbus.machine.attestation.repository`, and
  `io.nimbus.machine.nimbus.version`, and the host machine manager now reads
  those annotations before falling back to the legacy dual-repo attestation
  lookup. Durable conclusion: new machine images can declare exactly which repo
  owns their attestations and which Nimbus release they embed, while older
  images still load through the existing fallback path. Verification: repo
  review of `nimbus/nimbus-machine-os/.github/workflows/build.yml`,
  `nimbus/nimbus-machine-os/scripts/package-oci.sh`, and
  `crates/nimbus-bin/src/machine/manager.rs`; focused `cargo check -p nimbus-bin`;
  `bash /Users/jack/src/github.com/nimbus/nimbus-machine-os/scripts/verify-oci-layout-helper.sh`;
  `cargo fmt --all --check`.
- 2026-04-13: Added the git-tagged release trigger to that workflow so the
  first machine-image release can run through normal git state instead of an
  interactive dispatch. Pushing a repo tag like `machine-os/v0.1.0` now
  drives the real `linux arm64 nimbus-machine-os` build/publish lane, derives
  the immutable GHCR reference from the tag, and publishes the `stable` alias
  on top. Durable conclusion: the release control plane is now a normal git
  tag push. Verification: repo review of
  `.github/workflows/nimbus-machine-os.yml`; `cargo fmt --all --check`.
- 2026-04-13: Validated that first tagged release path against GitHub's live
  workflow API and found a workflow-evaluation failure before any jobs were
  created. The concrete root cause matched local `actionlint` output: the
  `build-arm64` job used `${{ runner.temp }}` inside job-level `env`, but the
  `runner` context is not available there. The workflow now initializes those
  paths in a dedicated shell step via `${RUNNER_TEMP}` and the repo now carries
  `.github/actionlint.yaml` so the custom `nimbus-machine-os` self-hosted
  runner label is treated as intentional instead of an unknown-label warning.
  Durable conclusion: remote machine-image failures now point at real runner or
  build issues instead of a pre-job workflow-definition bug. Verification:
  `actionlint .github/workflows/nimbus-machine-os.yml`; `ruby -e 'require
  "yaml"; YAML.load_file(".github/workflows/nimbus-machine-os.yml")'`; `cargo
  fmt --all --check`; `curl -sS
  'https://api.github.com/repos/nimbus/nimbus/actions/workflows/nimbus-machine-os.yml/runs?event=push&per_page=5'
  | jq '{total_count, runs: [.workflow_runs[] | {id, head_sha, status,
  conclusion, html_url}]}'`.
- 2026-04-13: The next live hosted verifier reached the deterministic recipe
  lane and exposed a second concrete portability issue. The failing command was
  `bash scripts/verify-nimbus-machine-os-recipe.sh`, and the exact error was
  `/tmp/.../custom-coreos-disk-images.sh: 2: set: Illegal option -o pipefail`.
  Root cause: `images/nimbus-machine-os/build.sh` invoked the pinned upstream
  `custom-coreos-disk-images.sh` helper via `sh`, but the upstream helper is
  Bash-specific (`#!/usr/bin/bash`, `set -euo pipefail`, Bash arrays). Nimbus
  now invokes the helper with `bash`, which is a deliberate portability
  improvement over Podman's Fedora-shaped `sh` call and matches the helper's
  real contract on Ubuntu-hosted verification lanes. Durable conclusion: the
  recipe now follows the helper's actual shell contract, so future failures in
  that lane should reflect the helper itself or the build inputs, not distro
  differences in `/bin/sh`. Verification:
  `bash scripts/verify-nimbus-machine-os-recipe.sh`;
  `actionlint .github/workflows/nimbus-machine-os.yml`;
  `ruby -e 'require "yaml"; YAML.load_file(".github/workflows/nimbus-machine-os.yml")'`;
  `cargo fmt --all --check`.
- 2026-04-13: Published that helper-contract fix as commit `2099c85` and
  pushed a fresh release tag `machine-os/v0.1.2`. GitHub workflow evidence now
  shows the hosted verifier succeeding on both `main` and the tag-triggered
  lane: run `24378430670` completed `Verify machine-os contract` successfully
  on `main`, and tag run `24378451538` completed the same verifier successfully
  before queuing `Build machine-os (linux arm64)` as job `71196867362`. Durable
  conclusion: the machine-image control plane is now past workflow-definition
  bugs and hosted verifier portability gaps; the next real blocker, if any,
  will come from the self-hosted ARM64 builder or the actual FCOS image build /
  publish path. Verification: `curl -sS
  'https://api.github.com/repos/nimbus/nimbus/actions/runs/24378430670/jobs?per_page=20'
  | jq '{run_id: 24378430670, total_count, jobs: [.jobs[] | {name, status,
  conclusion, started_at, completed_at, html_url}]}'`; `curl -sS
  'https://api.github.com/repos/nimbus/nimbus/actions/runs/24378451538/jobs?per_page=20'
  | jq '{run_id: 24378451538, total_count, jobs: [.jobs[] | {name, status,
  conclusion, started_at, completed_at, html_url}]}'`.
- 2026-04-13: Landed the first real host-side forwarded sandbox-backend slice
  for MAC5 inside `nimbus-bin`. The repo now has
  `crates/nimbus-bin/src/machine/backend.rs` with a
  `ForwardedMachineApiSandboxBackend` that speaks the guest machine API over a
  typed `MachineApiClient`, maps machine-API failures into sandbox-backend
  errors, and uses `tokio::task::spawn_blocking` so the synchronous unix-socket
  client does not block async host execution. `crates/nimbus-bin/src/machine/mod.rs`
  now exposes a typed default-machine API resolver for the real host path, and
  `crates/nimbus-bin/src/service/mod.rs` plus `crates/nimbus-bin/src/main.rs`
  no longer assume every compose-backed server launch is local krun: the host
  loader now selects krun for krun projects, selects the forwarded guest
  machine API for container-backed projects on macOS, and fails fast with
  explicit guest readiness blockers when the forwarded machine API is not yet
  ready. The new proof lane is repo-owned and deterministic: the
  `machine::backend::*` tests validate image/build/inspect/stop round-trips and
  missing-socket error mapping, while the new
  `service::tests::host_loader_accepts_container_projects_with_ready_forwarded_machine_api_on_macos`
  and
  `service::tests::host_loader_reports_machine_api_readiness_blockers_for_container_projects`
  tests prove the host loader accepts a ready forwarded guest API and surfaces
  guest blockers without touching the real default-machine state. Durable
  conclusion: the host now has a real remote sandbox seam for MAC5 and
  `nimbus serve` can stop being hardwired to krun-only manager loading, but
  MAC5 is still not closed until the live forwarded `<machine>-api.sock` path
  and macOS localhost published-port proof are captured on a real machine.
  Verification:
  `cargo fmt --all --check`;
  `cargo check -p nimbus-bin`;
  `cargo test -p nimbus-bin`.
- 2026-04-13: Re-checked the live machine-os release lane after that MAC5
  slice and recorded the still-open MAC4 blocker precisely instead of treating
  it as unknown. GitHub API evidence for run `24378451538` now shows
  `Verify machine-os contract` completed successfully as job `71196662456`,
  while `Build machine-os (linux arm64)` remains queued as job `71196867362`
  with labels `self-hosted`, `linux`, `arm64`, and `nimbus-machine-os` but no
  runner yet attached. Durable conclusion: MAC4 is still externally blocked on
  self-hosted ARM64 builder availability, not on the hosted verifier or the
  checked-in workflow definition. Because the repo now also has the host-side
  forwarded sandbox backend, the stable reference docs were updated in
  `docs/reference/microvm-service-baseline.md` and `docs/reference/cli.md` to
  reflect the new truth: host server startup can select the forwarded guest
  machine API for container-backed Compose projects on macOS, while the
  explicit `nimbus service ...` lifecycle commands remain krun-shaped until
  MAC6 lands. Verification: `curl -sS
  'https://api.github.com/repos/nimbus/nimbus/actions/runs/24378451538/jobs?per_page=20'`;
  `cargo fmt --all --check`.
- 2026-04-13: Started the first explicit MAC6 CLI-taxonomy slice by making
  `nimbus serve` the shipped server-start verb instead of leaving the
  authoritative server behind the old flag-driven root path. `crates/nimbus-bin/src/main.rs`
  now parses `Serve(ServeCommand)` explicitly, the server startup/config merge
  path runs through that typed command, and the old hidden root-start shape was
  removed rather than preserved as a compatibility alias. The repo follow-
  through is part of the same change set: `Makefile`, `package.json`,
  `demos/README.md`, `tests/demos.smoke.md`, `README.new.md`,
  `docs/reference/cli.md`, and `docs/reference/microvm-service-baseline.md`
  now point at `nimbus serve ...` / `cargo run -p nimbus-bin -- serve ...`
  instead of the older root-flag form, and the active macOS control plane now
  records MAC6 as honestly `in_progress`. Durable conclusion: the command
  taxonomy is now explicit and Podman-shaped enough for the remaining macOS DX
  work, but MAC6 still needs the actual mac-aware host `serve`/`service`
  machine-readiness flow and real host-local proof on top of the queued MAC4
  machine image. Verification:
  `cargo fmt --all --check`;
  `cargo check -p nimbus-bin`;
  `cargo test -p nimbus-bin`.
- 2026-04-14: Hardened the `nimbus-bin` coverage lane around the shared macOS
  machine helper environment so CI no longer flakes on the unreachable-registry
  OCI fixture. GitHub Actions run `24435896563` / job `71389968904` failed in
  coverage with `required helper 'krunkit' was not found` inside
  `machine_start_reports_oci_materialization_failure_for_unreachable_registry_image`,
  which narrowed the regression to process-global `NIMBUS_MACHINE_KRUNKIT` /
  `NIMBUS_MACHINE_GVPROXY` interference rather than OCI resolution itself.
  `crates/nimbus-bin/src/machine/manager.rs` now owns a shared
  `MachineHelperEnvGuard` that serializes helper-env mutation for tests,
  records prior values, restores them on drop instead of unconditionally
  clearing them, and recovers from poisoned mutex state so one unrelated panic
  does not cascade into false helper-resolution failures. The manager tests and
  the CLI-level unreachable-registry test in
  `crates/nimbus-bin/src/machine/mod.rs` now all use that same guard and
  shared stub-binary writer instead of open-coded `set_var` / `remove_var`
  sequences. Durable conclusion: the MAC4/MAC5 host-machine coverage proof is
  again deterministic under `cargo llvm-cov`, and the helper-resolution seam no
  longer depends on parallel test scheduling. Verification:
  `cargo fmt --all --check`;
  `cargo llvm-cov -p nimbus-bin --bin nimbus --no-report -- machine::tests::machine_start_reports_oci_materialization_failure_for_unreachable_registry_image --exact`;
  `cargo llvm-cov -p nimbus-bin --bin nimbus --no-report`.
- 2026-04-15: Tightened the remaining producer/consumer contract drift after
  the machine-image repo split. The host machine-manager tests no longer hard
  code the old `:stable` reference in default-path coverage, the external
  `nimbus/nimbus-machine-os` build workflow now passes
  `--nimbus-version` into the Linux image recipe itself, and the recipe summary
  now records `nimbus_version` so OCI packaging can recover the embedded
  release tag from build outputs instead of depending only on workflow flags.
  The machine-os recipe docs in `images/README.md` were also rewritten to
  match the current post-split script names, workflow path, and OCI metadata
  contract. Durable conclusion: the cross-repo MAC4 release contract is now
  explicit all the way from Linux guest build summary to packaged OCI
  annotations, and the active docs no longer point contributors at the old
  monorepo-era entrypoints. Verification:
  `cargo fmt --all --check`;
  `cargo check -p nimbus-bin`;
  `cargo test -p nimbus-bin machine::manager::tests:: -- --test-threads=1`;
  `cargo test -p nimbus-bin version_pinned_release_image`;
  `actionlint /Users/jack/src/github.com/nimbus/nimbus-machine-os/.github/workflows/build.yml`;
  `bash /Users/jack/src/github.com/nimbus/nimbus-machine-os/scripts/verify-recipe.sh`;
  `bash /Users/jack/src/github.com/nimbus/nimbus-machine-os/scripts/verify-build-helper.sh`;
  `bash /Users/jack/src/github.com/nimbus/nimbus-machine-os/scripts/verify-oci-layout-helper.sh`;
  `bash /Users/jack/src/github.com/nimbus/nimbus-machine-os/scripts/verify-publish-helper.sh`.
- 2026-04-16: The shared machine-lifecycle hardening control plan completed
  `MLH1` through `MLH7`. For the macOS path, that means provider-aligned
  krunkit graceful stop sequencing, startup signal cleanup that no longer
  leaves state stuck in `Starting`, versioned config/state records with rebuild
  policy, atomic per-machine record locking, shared SSH port allocation, and a
  capability-driven phased machine startup flow are now all landed in
  `crates/nimbus-bin/src/machine/`. Durable conclusion: `MAC7` no longer needs
  to absorb basic machine-lifecycle robustness work; the remaining macOS scope
  is real guest-image proof, forwarded-socket/service validation, diagnostics,
  packaging, and closeout evidence on top of a hardened shared lifecycle seam.
  Verification inherited from the shared hardening closeout:
  `cargo fmt --all --check`; `cargo check -p nimbus-bin`;
  `cargo test -p nimbus-bin machine::`.
- 2026-04-16: Advanced the first real MAC6 host-command bridge on top of the
  MAC5 forwarded machine-API seam. `crates/nimbus-bin/src/service/mod.rs` now
  resolves container-backed Compose projects on macOS through the forwarded
  guest machine API instead of stopping at the host loader: `nimbus service up`
  can start container-backed services through the guest backend, and
  `service list` / `inspect` / `logs` / `ps` / `down` now use guest-manifest
  state, guest `ctr.log`, and guest pidfiles through typed machine-API
  operations while leaving Linux/krun behavior unchanged. The guest contract
  also tightened underneath that UX slice: `nimbus-sandbox` now exposes a
  container-manifest state view, and the machine API/client surface now owns
  list/current/log/ps operations instead of making the host infer guest state
  from local krun paths. Durable conclusion: the repo no longer has a
  macOS-only gap where `serve` understood forwarded guest execution but
  explicit `service ...` commands did not; the remaining MAC5/MAC6 work is
  real-host forwarded-socket and localhost published-port proof plus the
  matching `nimbus serve` host validation once the guest artifact lane is
  ready. Verification:
  `cargo fmt --all --check`;
  `cargo check -p nimbus-bin`;
  `cargo test -p nimbus-bin machine::api::tests:: -- --test-threads=1`;
  `cargo test -p nimbus-bin machine::client::tests:: -- --test-threads=1`;
  `cargo test -p nimbus-bin service::tests:: -- --test-threads=1`;
  `cargo test -p nimbus-sandbox backends::container::state::tests:: -- --test-threads=1`.
- 2026-04-16: Added the first repo-owned MAC5/MAC6 host proof collector for
  the forwarded guest control path instead of leaving that evidence in ad hoc
  shell history. The repo now has
  `scripts/collect-nimbus-machine-service-proof.sh`,
  `scripts/verify-nimbus-machine-service-proof-helper.sh`,
  `make collect-nimbus-machine-service-proof`, and
  `make verify-nimbus-machine-service-proof-helper`. That helper records the
  operator-facing bundle the remaining macOS closeout needs: `nimbus machine status`,
  direct host `<machine>-api.sock` health/capabilities, direct forwarded
  guest service-sandbox listing, host `nimbus service up/list/inspect/ps/logs/down`,
  and an optional localhost published-port probe against one service. Durable
  conclusion: MAC5/MAC6/MAC7 now have a checked-in real-host evidence lane for
  the forwarded service workflow, but the repo still needs one successful run
  against a guest image that actually carries the guest `nimbus` machine API
  plus the remaining explicit `nimbus serve` startup proof before closeout.
  Verification:
  `bash -n scripts/collect-nimbus-machine-service-proof.sh`;
  `bash -n scripts/verify-nimbus-machine-service-proof-helper.sh`;
  `bash scripts/verify-nimbus-machine-service-proof-helper.sh`.
- 2026-04-16: Re-ran the live macOS host proof against Podman's local source
  tree and found one concrete libkrun contract drift in
  `crates/nimbus-bin/src/machine/manager.rs`: Nimbus was emitting
  `virtio-vsock,port=...,socketURL=...` for the ready and ignition devices
  without the explicit `,listen` mode that Podman's vfkit/libkrun path
  generates. Nimbus now emits the Podman-aligned `,listen` mode for both
  host-owned vsock sockets, and the focused machine-manager regression tests
  lock that contract down. Real-host proof outside the sandbox improved in the
  expected way: during startup the host now keeps `default-gvproxy.sock`
  alongside the krunkit peer socket, and `default-ignition.sock` serves the
  generated ignition payload on a pristine first-boot disk. The remaining
  blocker is now more precise, not smaller: even with the corrected host vsock
  role, the guest still does not consume the served ignition on first boot, so
  the injected SSH key never becomes valid, the forwarded
  `<machine>-api.sock` never answers, the guest reaches a plain `fedora login:`
  prompt, and `gvproxy` still exits before readiness. Durable conclusion:
  MAC4 is no longer blocked on the host-side ready/ignition vsock listen-mode
  contract; the next MAC4 slice is guest-image bootstrap analysis around why
  the Fedora guest is not consuming the ignition served over the corrected
  libkrun vsock path, and MAC5/MAC6 closeout remains blocked on that guest
  bootstrap/auth seam. Verification:
  `cargo fmt --all --check`;
  `cargo check -p nimbus-bin`;
  `cargo test -p nimbus-bin launch_plan_requires_bootable_local_disk_image -- --nocapture`;
  `cargo test -p nimbus-bin build_virtio_vsock_listen_arg_matches_podman_listen_mode -- --nocapture`;
  outside-sandbox macOS proof with isolated roots under
  `/tmp/nimbus-debug-proof5.XS0dSu`.
- 2026-04-16: Tightened the remaining MAC4 diagnosis and the operator-facing
  startup failure message so the repo no longer blames `gvproxy` alone when
  the guest image contract is the real problem. Comparing
  `/Users/jack/src/github.com/nimbus/nimbus-machine-os` against
  `/Users/jack/src/github.com/containers/podman-machine-os` showed that
  Nimbus's guest-image repo is still centered on `quay.io/fedora/fedora-bootc:42`
  plus `bootc-image-builder`, while Podman's battle-tested path uses Fedora
  CoreOS plus `custom-coreos-disk-images` for the AppleHV/libkrun-capable
  artifact contract. That explains the live proof boundary cleanly: the host
  now serves the ignition payload over the corrected libkrun vsock path, but
  the guest still reaches `fedora login:` without applying the SSH key or
  readying `nimbus.sock`. `crates/nimbus-bin/src/machine/manager.rs` now
  appends a Podman-aligned compatibility hint when startup fails after the
  guest reaches a console login prompt, so operators are told to verify the
  guest image contract instead of chasing a generic `gvproxy exited before
  machine readiness` symptom. Durable conclusion: the next real MAC4
  implementation step is a cross-repo rebase of `nimbus/nimbus-machine-os`
  onto Podman's Fedora CoreOS/custom-coreos-disk-images shape; the host repo
  should only carry diagnostics and compatibility surfacing for that gap.
  Verification:
  `cargo fmt --all --check`;
  `cargo check -p nimbus-bin`;
  `cargo test -p nimbus-bin annotate_machine_start_error_hints_when_guest_reaches_login_prompt -- --nocapture`;
  `cargo test -p nimbus-bin annotate_machine_start_error_leaves_unrelated_failures_unchanged -- --nocapture`.
- 2026-04-16: Corrected the MAC4 machine-image decision so the active plan no
  longer mixes today's bring-up contract with later image-ownership work.
  Durable rule is now explicit: macOS closes out first on Podman's published
  machine image, pinned by immutable reference or digest, with host-managed
  Nimbus bootstrap layered on top. A Nimbus-owned image is later replacement
  work rather than the current MAC4 gate, and any existing `fedora-bootc`
  supply-side work remains separate future direction until it proves the same
  Podman-aligned runtime semantics. Updated the MAC4 ledger summary, machine
  image decision section, supported-upgrade guidance, and implementation plan
  to match that contract. Verification: docs-only plan update.
- 2026-04-16: Closed the remaining MAC4 proof gap with a fresh isolated macOS
  run rooted at `/tmp/nimbus-mac4-versionproof.lHXHRO`. To avoid waiting on a
  new public release, the host repo now also supports a practical local proof
  lane for the matching Linux guest binary: enabled vendored OpenSSL in the
  workspace TLS dependencies, installed the `aarch64-unknown-linux-gnu` Rust
  target, and built a Linux arm64 guest binary locally on the current Mac with
  `zig`-backed cross wrappers plus `/usr/bin/ar`. The resulting guest artifact
  and tarball were staged at `/tmp/nimbus-linux-arm64-proof/`, with guest
  binary sha256 `60ba857fd5258b6bd12347da92c72c0eb773016254b94e045917b911c17561e0`
  and archive sha256 `d25b03e52cc5228a799e66cdff4c7196fbbe5b249d3d4b84f6c4e557d6c7e5fa`.
  A fresh `nimbus machine init` plus `nimbus machine start` under that proof
  root, using `NIMBUS_MACHINE_GUEST_BINARY=/tmp/nimbus-linux-arm64-proof/nimbus`,
  now reaches `running` / `ready`, records the pinned Podman digest as the
  desired and recorded machine image, and proves forwarded machine-API
  readiness on `/tmp/nimbus-mac4-versionproof.lHXHRO/runtime/default-api.sock`.
  The guest proof bundle at `/tmp/nimbus-mac4-versionproof.lHXHRO/guest-proof`
  now captures guest `nimbus --version` as `nimbus 0.1.3`, guest sha256,
  guest socket/service state, virtiofs mount presence, and guest machine-API
  health/capabilities. The follow-on host service proof bundle at
  `/tmp/nimbus-mac4-versionproof.lHXHRO/service-proof` shows the next blocker
  precisely: host machine/API health and `service config` pass, but
  `service up` fails with `No such file or directory (os error 2)` while
  invoking buildah, and the machine-API capability report confirms that the
  current Podman proof image still lacks `buildah`, `netavark`, and
  `aardvark-dns`. Durable conclusion: MAC4 is now closed in this repo; the
  remaining MAC5/MAC6 blocker is guest runtime-image content, which belongs to
  the image-ownership track rather than more host-manager convergence work.
  Verification:
  `cargo build --release -p nimbus-bin --target aarch64-unknown-linux-gnu`
  (outside sandbox, with `zig` wrappers);
  `cargo build -p nimbus-bin`;
  `env HOME=/tmp/nimbus-mac4-versionproof.lHXHRO/home NIMBUS_MACHINE_RUNTIME_ROOT=/tmp/nimbus-mac4-versionproof.lHXHRO/runtime NIMBUS_MACHINE_GUEST_BINARY=/tmp/nimbus-linux-arm64-proof/nimbus NIMBUS_MACHINE_API_READY_TIMEOUT_SECS=120 target/debug/nimbus machine start`;
  `bash scripts/collect-nimbus-machine-guest-proof.sh --home /tmp/nimbus-mac4-versionproof.lHXHRO/home --runtime-root /tmp/nimbus-mac4-versionproof.lHXHRO/runtime --output-dir /tmp/nimbus-mac4-versionproof.lHXHRO/guest-proof --nimbus /Users/jack/src/github.com/nimbus/nimbus/target/debug/nimbus`;
  `bash scripts/collect-nimbus-machine-service-proof.sh --home /tmp/nimbus-mac4-versionproof.lHXHRO/home --runtime-root /tmp/nimbus-mac4-versionproof.lHXHRO/runtime --output-dir /tmp/nimbus-mac4-versionproof.lHXHRO/service-proof --nimbus /Users/jack/src/github.com/nimbus/nimbus/target/debug/nimbus --compose-file /tmp/nimbus-mac-service-proof-compose.yaml --service demo`.
