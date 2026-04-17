# MicroVM And Service-Control Baseline

This document is the stable baseline for Neovex's landed krun-backed microVM
runtime and Compose-backed service-control architecture.

It is not a roadmap. Historical execution detail, verification logs, and
phase-by-phase closeout evidence live in the archived plans:

- [`docs/plans/archive/vmm-infrastructure-plan.md`](../plans/archive/vmm-infrastructure-plan.md)
- [`docs/plans/archive/microvm-runtime-plan.md`](../plans/archive/microvm-runtime-plan.md)
- [`docs/plans/archive/service-control-plane-plan.md`](../plans/archive/service-control-plane-plan.md)
- [`docs/plans/archive/macos-machine-support-plan.md`](../plans/archive/macos-machine-support-plan.md)

## Scope

- Linux is the production platform for hardware-isolated service microVMs.
- macOS is a developer delivery surface only: Neovex runs inside one Linux
  machine VM, and services run as standard containers inside that guest, the
  same way Podman works on macOS today.
- `neovex-runtime` stays execution-only.
- `neovex-sandbox` stays isolation-only.
- `neovex-server` owns service activation, request-time binding, and
  `ctx.services.*` projection.
- `neovex-bin` owns Compose parsing, service CLI commands, and server startup
  wiring such as `--compose-file`.

## Architecture

Current implementation by layer:

| Layer | Current implementation | Ownership |
| --- | --- | --- |
| Execution runtime | V8 backend in `neovex-runtime` | code execution only |
| Isolation backend | krun backend in `neovex-sandbox` | OCI lowering, lifecycle, logs, manifests |
| VM launch stack | `buildah` + `conmon` + patched `crun` + `libkrun` | subprocess orchestration |
| Service manager | `SandboxServiceManager` in `neovex-server` | declared services, activation, readiness, teardown |
| Developer/operator UX | `neovex-bin` | Compose validation and `neovex service ...` |

Linux request path:

```text
compose.yaml / image / build context
  -> neovex-bin validates and lowers service intent
  -> neovex-server owns declared services and activation
  -> ctx.services.<name> triggers ensure_service_binding(...)
  -> neovex-sandbox krun backend materializes OCI bundle + state
  -> conmon -> patched /usr/libexec/neovex/crun -> libkrun VM
  -> guest service answers via TSI-mapped host port
```

macOS development path:

```text
macOS host
  -> krunkit machine VM
  -> Linux guest running neovex
  -> services run as standard containers in the guest
```

Neovex does not add a second host-side orchestration path on macOS, and it
does not rely on nested per-service microVMs there for v1.

Current macOS completion notes:

- the workspace now carries a generic backend-selection seam (`Container` plus
  `Krun`) and Compose/control-plane carry-through for backend choice
- Linux production execution still runs through the landed krun backend
- the host server startup path can now select a forwarded guest machine-API
  backend for container-backed Compose projects on macOS when the guest
  machine API advertises `service_execution_ready`
- the explicit `neovex service ...` lifecycle commands now share that
  forwarded guest path on macOS for container-backed projects: `up`, `down`,
  `list`, `inspect`, `logs`, and `ps` talk to the guest machine API instead of
  host-local krun state, while Linux production and krun-backed projects stay
  unchanged
- mixed-backend project-wide operations still reject until the repo chooses a
  broader multi-backend UX contract, so operators must target one backend
  family per project-wide command
- the current macOS developer-machine contract has now landed the live guest
  artifact, forwarded-socket proof, localhost published-port proof, and
  end-to-end real-host validation; use the archived macOS machine-support
  plan for the exact bundle paths and execution history

## Transport And Probe Semantics

- **Linux production data plane:** service traffic crosses the service-VM
  boundary through krun/TSI port mappings. Neovex publishes host-side ports
  and treats those as the application-facing bindings.
- **Linux production control/lifecycle plane:** the landed baseline does not
  require a custom guest-side `vsock` control agent. Startup, readiness,
  liveness, restart, logs, and stop behavior are currently driven from the
  host side through `conmon`, `crun`, manifests, and real service reachability
  checks.
- **macOS developer-machine data plane:** the host should talk to guest
  services through published localhost ports from the machine VM, not through a
  second per-service microVM layer.
- **macOS developer-machine control plane:** this is intentionally distinct
  from Linux TSI. Podman's source-backed model uses `gvproxy`, forwarded guest
  sockets, and machine-level readiness/bootstrap devices; `vsock` should not
  be used as a fuzzy synonym for the whole host↔guest channel.
- **Probe rule across both platforms:** process state alone is never enough.
  A sandbox or machine is not considered ready just because a VM process is
  running; readiness is always gated on the next actually usable boundary.

Preferred probe hierarchy by platform:

| Platform | Process boundary | Transport boundary | Application boundary |
| --- | --- | --- | --- |
| Linux service microVM | `conmon` / `crun` / manifest state | TSI-mapped host port reachable | guest service responds |
| macOS machine VM | `krunkit` / `gvproxy` / machine state | guest control socket or SSH reachable | guest Neovex API or published service responds |

## Core Invariants

- `neovex-runtime` must not absorb sandbox/orchestration concerns.
- `neovex-sandbox` must expose generic sandbox nouns, not krun-specific public
  API.
- The server owns the service registry and activation lifecycle.
- `ctx.services.<name>` exposes bindings such as `.port`; protocol-specific
  clients remain the application's responsibility.
- The guest service is not treated as ready just because OCI/crun reports
  `"running"`; readiness is gated on actual service reachability.
- Host-side krun bundles stay root for `/dev/kvm`; image `USER` is preserved
  and applied inside the guest.

## Lifecycle Baseline

The landed krun backend supports:

- image-backed and build-backed launches
- OCI image-default lowering for `USER`, `STOPSIGNAL`, exposed ports, and
  working directory
- readiness gating before published endpoints appear
- liveness degradation and recovery without forcing a VM restart
- restart policy with bounded restart counts
- exponential restart backoff
- guest-side user switching inside the VM
- manifest-backed recovery after Neovex/backend restart
- persisted `ctr.log` and `oci.log`

The durable sandbox state model now includes:

- `Starting`
- `Ready`
- `NotReady`
- `Stopped`

Current Linux production lifecycle interpretation:

- `Starting`: VM process exists, but the service is not yet reachable on the
  published TSI endpoint.
- `Ready`: the service answered on the published endpoint and the binding is
  safe to hand to callers.
- `NotReady`: the VM/process may still exist, but the service probe failed and
  published endpoints are withdrawn.
- `Stopped`: the sandbox has terminated and the persisted stop/exit outcome is
  recorded.

Future macOS machine lifecycle work should keep machine-level states separate
from service-level states. A machine can be `Ready` while a specific declared
service in the guest is still `Starting` or `NotReady`.

## Operator Surface

Neovex currently exposes three operator paths relevant to services and macOS
developer machines:

- `neovex serve --compose-file ./compose.yaml`
  starts the server with a declared service catalog available for
  request-time activation through `ctx.services.*`
- `neovex service ...`
  manages those services explicitly through the same backend-owned state model
- `neovex machine ...`
  owns the shipped macOS machine CLI and persisted machine-state foundation

Supported CLI commands today:

- `neovex service config`
- `neovex service up`
- `neovex service down`
- `neovex service list`
- `neovex service inspect`
- `neovex service logs`
- `neovex service ps`
- `neovex machine init`
- `neovex machine start`
- `neovex machine stop`
- `neovex machine status`
- `neovex machine ssh`
- `neovex machine rm`
- `neovex machine os apply`
- `neovex machine os upgrade`

Server startup now uses the explicit `neovex serve` subcommand. `service` is
the managed-service namespace. The current command taxonomy is:

- `neovex serve` for explicit server startup
- `neovex service ...` for managed service lifecycle
- `neovex machine ...` for macOS machine lifecycle

The current `machine` surface now includes the direct `krunkit` + `gvproxy`
host-manager seam, the pinned-Podman-image macOS convergence contract, the
host-managed guest-`neovex` binary sync path, and the explicit `machine os
apply` / `machine os upgrade` rollout surfaces. Historical execution detail
and the exact real-host closeout bundles remain in the archived macOS
machine-support plan.

## Key References

- [CLI reference](cli.md)
- [Current capabilities](current-capabilities.md)
- [krun VMM host validation](krun-vmm-host-validation.md)
- [krun sandbox backend smoke](krun-sandbox-backend-smoke.md)
- [Distribution plan](../plans/distribution-plan.md)

## When To Open The Archived Plans

Open the archived plans only when you need one of these:

- exact Linux-host verification evidence and commands
- detailed phase-by-phase reasoning for how the current design landed
- historical tradeoffs around krun, buildah, conmon, TSI, or Compose lowering
- original control-plane sequencing for follow-on work

For ordinary implementation and review work against the landed system, start
with this baseline document instead of loading the archived execution plans.
