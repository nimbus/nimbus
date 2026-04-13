# Plan: macOS Machine Support — Podman-Aligned Developer Machines

Canonical execution plan for finishing Neovex macOS support for engineers who
develop on Apple Silicon Macs and deploy to Linux production hosts.

Reviewed against:

- `docs/reference/microvm-service-baseline.md`
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
- macOS support is not complete. The current repo does **not** implement a
  `neovex machine ...` command surface yet; `crates/neovex-bin/src/main.rs`
  currently exposes only `Service(ServiceCommand)`.
- The current docs already carry the correct high-level platform decision:
  macOS is a developer delivery surface only, and Neovex should run inside one
  Linux machine VM with standard containers inside that guest.
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
- Historical validation on the current Mac host already proved two critical
  operational facts we should preserve:
  - a short runtime root such as `/tmp/podman` avoids Darwin unix-socket path
    overflow for the libkrun/gvproxy lane
  - stale machine state can wedge a working provider/image combination, and a
    clean recreate flow is part of the real operator story

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
- Podman's Apple `vsock` usage is source-backed and narrower:
  - `apple/vfkit.go` adds a ready-signal `virtio-vsock` device
  - `apple/apple.go` adds an ignition `virtio-vsock` device on first boot only
  - `apple/ignition.go` serves the ignition payload over that socket
- Conclusion: on macOS we should stop saying "API forwarding over vsock" as the
  default transport story. A better model is:
  - `virtio-net` + `gvproxy` for guest networking and published ports
  - a host-local forwarded control socket for the guest API
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
| Host topology | thin host CLI manages one Linux machine VM | thin host CLI manages one Linux machine VM | match |
| Guest control plane | guest `podman.socket` / Podman API | guest Neovex API owned by `neovex serve` | same pattern, Neovex-owned API |
| Guest workload implementation | standard guest containers | standard guest containers | match |
| Host↔guest API path | forwarded guest socket plus `gvproxy`/SSH-backed plumbing | host-local forwarded control socket/channel | match the pattern, not the exact API |
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
| Host runtime stack | `conmon -> patched crun -> libkrun` | `krunkit + gvproxy` on host, `crun` in guest | Linux path stays unchanged |
| Service networking | krun TSI host:guest ports | host localhost -> gvproxy -> guest container ports | `ctx.services.<name>.port` semantics |
| Readiness model | server waits for actual service reachability | same | no "running means ready" regression |
| Compose/service UX | landed `neovex --compose-file ...` and `neovex service ...` | same commands from mac host | one developer-facing workflow |
| Host orchestration | direct Linux runtime control | `neovex machine ...` thin host CLI | no host-side second service runtime |
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
| M0: host helper state | are `krunkit`, `gvproxy`, and machine sockets alive? | to implement |
| M1: machine ready state | has the VM crossed its machine-level ready boundary? | to implement |
| M2: guest control reachability | can the host reach guest SSH or the guest control socket? | to implement |
| M3: guest Neovex readiness | is `neovex serve` inside the guest actually ready? | to implement |
| M4: guest service readiness | are published guest services reachable from macOS localhost? | to implement |

macOS architectural rule:

- machine readiness and service readiness are separate
- a ready machine is not enough to declare `neovex serve` ready
- a ready `neovex serve` is not enough to declare every declared guest service
  ready

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
- When writing `vsock` in code or docs, name the exact role:
  readiness, first-boot bootstrap, or a consciously chosen control/data plane.
  Do not use `vsock` as a fuzzy synonym for all macOS host↔guest transport.
- Do not reintroduce the old "custom guest init / guest agent over vsock"
  design as a default requirement. Treat any future `vsock` control plane as an
  explicit, separately justified capability.
- Keep host responsibilities and guest responsibilities separate:
  - host: machine lifecycle, host-local control socket, file sharing, port publishing
  - guest: real `neovex serve`, Compose/service control, standard container runtime
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
  -> same ctx.services.<name>.port behavior
  -> same guest neovex binary semantics as Linux production
```

The macOS layer should be a delivery wrapper around the Linux server
environment, not a second service-orchestration stack.

## Target Architecture

### Accepted architecture

```text
macOS host
  └── neovex (thin machine-aware CLI)
        ├── neovex machine ...
        │     ├── krunkit
        │     ├── gvproxy
        │     ├── short runtime dir under /tmp/neovex-machine
        │     └── host-local control socket + published localhost ports
        │
        └── Linux guest VM
              ├── neovex serve
              ├── same Compose/service-control semantics as Linux
              └── services run as standard crun containers
```

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
                    └── per-service krun microVMs inside the guest
```

Rejected because:

- the first option ignores the Linux-only assumptions in the landed VMM stack
- the second option adds nested isolation that Podman itself does not use as
  the normal macOS container model

## Scope

This plan covers:

- the canonical macOS machine architecture and transport model
- a `neovex machine ...` host CLI surface
- direct `krunkit` + `gvproxy` host orchestration
- a Linux guest image and bootstrap contract for Neovex
- transparent macOS host routing for `neovex serve` and `neovex service ...`
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
  - host-local control socket proof
  - guest SSH proof
  - localhost port-publish proof
  - clean recreate-from-stale-state proof
- **Linux guest lane inside the macOS machine**
  - `neovex serve` runs with the same runtime/service behavior expected on Linux
  - Compose-backed service flows work from inside the guest
  - guest container networking and published ports match the host-facing claims

### Required evidence discipline

- If a verification artifact cannot live in git, record:
  - absolute path
  - exact command that produced it
  - exact command that proved it worked
- Prefer checked-in scripts/runbooks over ad hoc terminal history.
- Reuse the existing Podman-derived diagnostics scripts until Neovex has its
  own machine-manager equivalents, then replace those references explicitly.

## Roadmap Status Ledger

| Item | Status | Summary | Hard Dependencies |
| --- | --- | --- | --- |
| MAC1 | done | Lock the macOS architecture, transport vocabulary, and probe model docs | none |
| MAC2 | todo | Add `neovex machine ...` CLI surface and host-side config/runtime roots | MAC1 |
| MAC3 | todo | Implement direct host machine lifecycle around `krunkit` + `gvproxy` | MAC2 |
| MAC4 | todo | Build the Linux guest image and first-boot/bootstrap contract | MAC2 |
| MAC5 | todo | Implement host-local control socket and published-port plumbing | MAC3, MAC4 |
| MAC6 | todo | Make `neovex serve` and `neovex service ...` work transparently from macOS | MAC5 |
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
- machine-level readiness evidence is captured separately from guest Neovex
  readiness
- the stale-state recreate drill is reproducible

### MAC4 — Guest image and bootstrap

Repo outputs:

- guest image build recipe
- guest bootstrap/systemd units
- documented mount strategy and guest package contract

Required host-local outputs:

- built image artifact path
- first-boot log proof
- guest SSH proof
- guest `neovex --version` proof

Acceptance criteria:

- the guest image boots reproducibly
- `neovex serve` is installed and runnable inside the guest
- host project paths are available inside the guest through `virtiofs`

### MAC5 — Control channel and port publishing

Repo outputs:

- host-local control socket/proxy implementation
- published localhost port plumbing
- focused integration tests around the control channel

Required host-local outputs:

- local control socket path
- command showing the guest endpoint behind it
- localhost connectivity proof to a guest service

Acceptance criteria:

- the macOS host can reach the guest Neovex control surface without shelling
  out to Podman's connection layer
- the chosen control-channel implementation is described precisely as either a
  forwarded guest socket or a deliberate `vsock` control channel
- published guest service ports are reachable from macOS localhost

### MAC6 — Transparent developer UX

Repo outputs:

- mac-aware `neovex serve` path
- mac-aware `neovex service ...` path
- docs for expected developer workflow

Required host-local outputs:

- one clean end-to-end project root
- `neovex serve` startup log
- `neovex service up/list/logs/down` transcript or checked-in helper summary

Acceptance criteria:

- from a macOS host, a developer can run the same compose-backed workflow they
  use on Linux without manually SSHing into the guest
- the end-to-end flow proves machine readiness, guest Neovex readiness, and
  guest service readiness as separate steps
- `ctx.services.<name>.port` behavior matches the Linux UX contract

### MAC7 — Packaging and closeout

Repo outputs:

- distribution-plan alignment for Channel 4
- Homebrew/dependency contract updates
- final runbook and verification summary

Required host-local outputs:

- install/init/start verification bundle
- recovery-drill bundle
- packaging/install notes

Acceptance criteria:

- the macOS developer path is documented, testable, and repeatable
- this plan can be archived and the stable baseline updated

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
