# MicroVM And Service-Control Baseline

This document is the stable baseline for Nimbus's landed krun-backed microVM
runtime and Compose-backed service-control architecture.

It is not a roadmap. Historical execution detail, verification logs, and
phase-by-phase closeout evidence live in the archived plans:

- [`docs/plans/archive/vmm-infrastructure-plan.md`](../plans/archive/vmm-infrastructure-plan.md)
- [`docs/plans/archive/microvm-runtime-plan.md`](../plans/archive/microvm-runtime-plan.md)
- [`docs/plans/archive/service-control-plane-plan.md`](../plans/archive/service-control-plane-plan.md)
- [`docs/plans/archive/macos-machine-support-plan.md`](../plans/archive/macos-machine-support-plan.md)

## Scope

- Linux is the production platform for hardware-isolated service microVMs.
- macOS is a developer delivery surface only: Nimbus runs inside one Linux
  machine VM, and services run as standard containers inside that guest, the
  same way Podman works on macOS today.
- `nimbus-runtime` stays execution-only.
- `nimbus-sandbox` stays isolation-only.
- `nimbus-server` owns service activation, request-time binding, and
  `ctx.services.*` projection.
- `nimbus-bin` owns Compose parsing, service CLI commands, and server startup
  wiring such as `--compose-file`.

## Architecture

Current implementation by layer:

| Layer | Current implementation | Ownership |
| --- | --- | --- |
| Execution runtime | V8 backend in `nimbus-runtime` | code execution only |
| Isolation backend | krun backend in `nimbus-sandbox` | OCI lowering, lifecycle, logs, manifests |
| VM launch stack | `buildah` + `conmon` + `nimbus-crun` + `nimbus-libkrun` | private Nimbus runtime stack orchestration |
| Service manager | `SandboxServiceManager` in `nimbus-server` | declared services, activation, readiness, teardown |
| Developer/operator UX | `nimbus-bin` | Compose validation and `nimbus compose ...` |

Linux request path:

```text
compose.yaml / image / build context
  -> nimbus-bin validates and lowers service intent
  -> nimbus-server owns declared services and activation
  -> runtime snapshots ready bindings for ctx.services.<name>
  -> await ctx.services.get("<name>") triggers cancellable activation when needed
  -> nimbus-sandbox krun backend materializes OCI bundle + state
  -> conmon -> patched /usr/libexec/nimbus/crun -> private nimbus-libkrun VM
  -> guest service answers via TSI-mapped host port
```

macOS development path:

```text
macOS host
  -> krunkit machine VM
  -> Linux guest running nimbus
  -> services run as standard containers in the guest
```

Nimbus does not add a second host-side orchestration path on macOS, and it
does not rely on nested per-service microVMs there for v1.

Current macOS completion notes:

- the workspace now carries a generic backend-selection seam (`Container` plus
  `Krun`) and Compose/control-plane carry-through for backend choice
- Linux production execution still runs through the landed krun backend
- the host server startup path can now select a forwarded guest machine-API
  backend for container-backed Compose projects on macOS when the guest
  machine API advertises `service_execution_ready`
- the explicit `nimbus compose ...` lifecycle commands now share that
  forwarded guest path on macOS for container-backed projects: `up`, `down`,
  `ps`, `inspect`, `logs`, and `top` talk to the guest machine API instead of
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
  boundary through krun/TSI port mappings. Nimbus publishes host-side ports
  and treats those as the application-facing bindings. TSI host bindings must
  preserve the configured `SandboxPortBinding::host_address`; the default is
  loopback-only, and address-bearing port maps must fail closed if the
  patched libkrun bind-address hook is unavailable.
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
| macOS machine VM | `krunkit` / `gvproxy` / machine state | guest control socket or SSH reachable | guest Nimbus API or published service responds |

## Core Invariants

- `nimbus-runtime` must not absorb sandbox/orchestration concerns.
- `nimbus-sandbox` must expose generic sandbox nouns, not krun-specific public
  API.
- The server owns the service registry and activation lifecycle.
- `ctx.services.<name>` exposes only bindings that were already resolved before
  invocation; `await ctx.services.get("<name>")` is the activation path for
  missing declared services. Protocol-specific clients remain the application's
  responsibility.
- The guest service is not treated as ready just because OCI/crun reports
  `"running"`; readiness is gated on actual service reachability.
- Host-side krun bundles stay root for `/dev/kvm`; image `USER` is preserved
  and applied inside the guest. The current krun/libkrun stack is validated
  for a root-owned Nimbus service path; non-root `/dev/kvm` access remains the
  tracked F5 hardening lane.
- Host-side krun bundles carry the SMH hardening baseline: explicit
  `process.noNewPrivileges`, explicit krun VMM capabilities, and an explicit
  seccomp allowlist validated by Linux krun smoke.
- Tenant isolation is a control-plane contract above the VM boundary. A
  production service sandbox must be admitted with tenant-owned identity,
  bundle/state/rootfs/log/volume roots, network exposure policy, resource
  quotas, image provenance policy, and HostBridge/runtime grants before OCI
  bundle generation. The completed cross-layer baseline is
  `docs/plans/archive/tenant-isolation-control-plane-plan.md`.

## Tenant Isolation Boundary

The current krun stack gives Nimbus the compute isolation primitive: one
service sandbox launches one microVM, and the server-owned service manager
keys active handles by tenant plus service name. That is not the whole
tenant-isolation story.

Production tenant isolation requires these additional boundaries:

- **Admission:** Compose and programmatic service declarations lower into a
  tenant-scoped sandbox spec. Unsafe mounts, devices, privileged container
  controls, public ports, images, secrets, and broad resource requests are
  rejected or rewritten before `config.json` exists.
- **Filesystem and storage:** mutable sandbox artifacts live under
  tenant-owned roots. Shared image/blob caches are content-addressed,
  immutable after verification, and never shared writable state.
- **Image admission:** production registry images must meet the admitted image
  policy before launch. The current policy surface has a digest-pinned floor,
  optional allowed registries, optional signature issuer/subject, optional
  SLSA-style provenance builder and predicate requirements, optional SBOM
  evidence, and explicit local-build allowance. Sigstore/Cosign should plug in
  behind `TenantImageVerificationProvider` through
  `docs/plans/artifact-provenance-verification-plan.md`; Nimbus owns policy and
  evidence normalization, not hand-rolled cryptographic verification or OCI
  reference/referrer parsing.
- **Networking:** service ports are loopback-only by default and mediated
  through tenant-scoped service bindings. Non-loopback exposure requires an
  explicit operator policy record. Arbitrary outbound guest egress now has a
  typed deny-by-default `SandboxEgressPolicy` contract that validates host
  wildcards, malformed host shapes, internal-IP allowlists, reserved-IP
  targets, and L7 method/path rules, then compiles to a canonical policy before
  launch comparison/materialization. Egress policy changes require sandbox
  recreation until a live enforcement path exists. The actual sandbox-local
  proxy or equivalent Linux enforcement path remains owned by EPS4b in
  `docs/plans/enterprise-policy-and-sandbox-egress-plan.md`.
- **HostBridge and runtime grants:** in-process runtime code receives only the
  invocation tenant and exact grants. It cannot request another tenant's
  service binding, and network grants must not let it bypass `ctx.services` by
  scanning localhost ports.
- **Cleanup:** tenant deletion stops tenant-owned services, releases ports,
  removes tenant-owned sandbox artifacts and volumes, and does not touch other
  tenants.

Do not cite this baseline alone as proof of production multi-tenant isolation;
pair it with the active tenant-isolation plan and its two-tenant harness.

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
- manifest-backed recovery after Nimbus/backend restart
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

Nimbus currently exposes three operator paths relevant to services and macOS
developer machines:

- `nimbus start --compose-file ./compose.yaml`
  starts the server with a declared service catalog available for
  snapshot projection through `ctx.services.<name>` and request-time activation
  through `ctx.services.get(...)`
- `nimbus compose ...`
  manages those services explicitly through the same backend-owned state model
- `nimbus machine ...`
  owns the shipped macOS machine CLI and persisted machine-state foundation

Supported CLI commands today:

- `nimbus compose config`
- `nimbus compose up`
- `nimbus compose down`
- `nimbus compose ps`
- `nimbus compose inspect`
- `nimbus compose logs`
- `nimbus compose top`
- `nimbus machine init`
- `nimbus machine start`
- `nimbus machine stop`
- `nimbus machine status`
- `nimbus machine ssh`
- `nimbus machine rm`
- `nimbus machine os apply`
- `nimbus machine os upgrade`

Server startup now uses the explicit `nimbus start` subcommand. `compose` is
the managed-service namespace for Compose-declared local dependencies. The
current command taxonomy is:

- `nimbus start` for explicit server startup
- `nimbus compose ...` for managed service lifecycle
- `nimbus machine ...` for macOS machine lifecycle

The current `machine` surface now includes the direct `krunkit` + `gvproxy`
host-manager seam, the pinned-Podman-image macOS convergence contract, the
host-managed guest-`nimbus` binary sync path, and the explicit `machine os
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
