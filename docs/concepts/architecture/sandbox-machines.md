---
title: Sandboxes and machines
description: How the sandbox seam launches tenant workloads as containers or libkrun microVMs, and how a machine provides the Linux host on non-Linux machines.
sidebar:
  label: Sandbox & machines
  order: 7
---

Nimbus runs untrusted, process-shaped workloads inside **sandboxes** — isolated
Linux execution environments owned by a tenant. On hosts that are not Linux,
Nimbus first boots a **machine**: a single outer Linux VM that supplies the
kernel features sandboxes need. This page tours the crates that implement both
layers: `crates/nimbus-sandbox` (the sandbox seam and its backends) and
`crates/nimbus-machine` plus the `nimbus machine` CLI flow in
`crates/nimbus-bin/src/machine/`.

For what sandboxes *are* in the product model — how they relate to services and
sessions — see [the resource model](/concepts/resource-model/). This page is
about how they are launched and enforced.

## The sandbox seam

`crates/nimbus-sandbox` defines a backend-agnostic contract. The
`SandboxBackend` trait (`crates/nimbus-sandbox/src/backend.rs`) is the whole
seam: a backend reports its `kind()`, and implements `start`, `inspect`, and
`stop` against a validated spec. Two trait methods have deliberate defaults:

- `reload_egress_policy` errors by default — a backend must explicitly opt in
  to live egress-policy reloads, otherwise callers learn the backend
  "does not support live egress reload" instead of silently keeping a stale
  policy.
- `remove_tenant_artifacts` defaults to a no-op success, so backends without
  per-tenant on-disk state need no extra code.

`SandboxBackendKind` has exactly two values today: `Container` and `Krun`.

## The container backend

The container backend (`crates/nimbus-sandbox/src/backends/container/`) is the
production path for process-capable launches. It composes standard OCI
tooling:

- `crun` as the OCI runtime (the compiled-in default runtime path is `crun`),
- `conmon` as the per-container monitor,
- `buildah` for `SandboxOciImageSource::Build` image builds,
- `netavark` and `aardvark-dns` for sandbox networking.

Launches run in one of two modes (`ContainerStartMode`): `Execute` actually
starts the workload; `PlanOnly` materializes and validates everything up to
execution without starting a process. The backend can also be configured with
a machine port forwarder (`OciMachinePortForwarderConfig`, backed by
`gvproxy`) so port bindings made inside a machine reach the host, and it
enforces network policy through the `SandboxEgressProxy` described below.

## The krun backend is fail-closed for execution

The krun backend (`crates/nimbus-sandbox/src/backends/krun/`) targets libkrun
microVMs on Linux KVM hosts. Its launch planning is real — image
materialization, rootfs assembly, and guest configuration all work in
`KrunStartMode::PlanOnly` — but execute mode is intentionally blocked. At the
top of launch planning, before any image work, the backend checks whether it
can enforce egress for a running guest and refuses:

> krun execute-mode is fail-closed until Nimbus has a packet-level egress
> enforcement path for libkrun TSI; use the container backend for
> process-capable launches or plan-only krun materialization

libkrun's TSI networking hands the guest a socket-level path to the host, and
Nimbus's egress proxy cannot interpose on it the way it interposes on
container traffic. Rather than launch microVMs with weaker egress enforcement
than containers, the backend fails closed. Execute paths also require a Linux
host outright (`crates/nimbus-sandbox/src/backends/krun/vm/lifecycle.rs`).

## What a sandbox spec says

`SandboxSpec` (`crates/nimbus-sandbox/src/spec.rs`) is the validated launch
input: tenant id, owner, backend kind, root, process, resources, lifecycle,
port bindings, mounts, and egress policy. Three parts deserve attention:

- **Root.** `SandboxRootSpec` is either `Rootfs` (a host directory, an
  operator-only input) or `OciImage` with a `SandboxOciImageSource` of
  `Reference` (pull) or `Build` (buildah build from an operator-provided
  context).
- **Owner.** `SandboxOwnerSpec` is `Service { name }` for sandboxes backing a
  declared service, or `Standalone { display_name }` for directly created
  ones.
- **Mounts.** Only `SandboxMountSource::TenantVolume` exists — there is no
  arbitrary host-bind mount in the public spec. Validation caps mounts at 32,
  requires absolute destinations with no `.`/`..` segments, and rejects
  destinations that touch `/proc`, `/sys`, `/dev`, or `/.nimbus` (the path the
  backends reserve for Nimbus guest helpers).

Resource accounting is fail-closed too: unset per-sandbox resources are
accounted at defaults (1 vCPU, 512 MiB memory, 10 GiB disk, 64 MiB logs), and
per-tenant quotas default to 64 sandboxes, 128 vCPUs, 256 GiB memory, 2 TiB
disk, and 64 GiB logs.

## Egress is deny-by-default

`SandboxEgressPolicy` (`crates/nimbus-sandbox/src/egress.rs`) defaults to an
empty allow list, which means deny-all. Each allow rule names a protocol, a
host pattern, and a port, and HTTP rules may further restrict methods and path
prefixes; TCP rules must not carry HTTP-shaped fields. Destinations that
resolve to internal or non-global IP ranges are refused unless the rule
explicitly sets `allow_internal_ips`. Enforcement configuration travels to the
workload through reserved environment keys (the
`NIMBUS_SANDBOX_EGRESS_ENFORCEMENT_JSON` payload and proxy variables), and the
spec validator rejects any attempt by a launch spec to set those keys itself.

## Launch inputs are redacted at the API boundary

The HTTP surface in `crates/nimbus-compute/src/sandbox_spec.rs` separates
what callers may send from what responses may echo. Public launch input
rejects `Rootfs` roots and OCI `Build` sources as operator-only internal
inputs. Responses never echo sensitive launch material: rootfs and build roots
come back as `{"redacted": true, "reason": "operatorOnlyLaunchInput"}`, and
process argv, entrypoint, command, and environment come back as counts
(`{"redacted": true, "valueCount": N}`) rather than values.

## Machines: the outer Linux VM

`crates/nimbus-machine` models the machine itself; the `nimbus machine` CLI
flow lives in `crates/nimbus-bin/src/machine/`. A machine is one long-lived
Linux VM per development host, not a per-workload unit. Its image source is
integrity-anchored: an OCI reference (the default is `ghcr.io/nimbus/machine-os`,
digest-pinned on macOS), an HTTP URL that must carry a `#sha256=<digest>`
fragment, or a local disk file. Lifecycle is explicit —
uninitialized, stopped, starting, running, failed — and `nimbus machine`
exposes init/start/stop/status/list/inspect plus ssh, file copy, guest
configuration, and OS apply/upgrade/rollback subcommands.

Two providers are declared (`MachineProvider`):

- **Krunkit (macOS).** The working provider. It boots a raw image with
  Ignition bootstrap via the `krunkit` helper binary and uses `gvproxy` for
  host networking. `nimbus machine init` currently always selects Krunkit, and
  the default machine ("default") gets 2 CPUs, 2048 MiB memory, a 20 GiB
  disk, and shared volumes for `/Users`, `/private`, and `/var/folders`.
- **WSL2 (Windows).** Declared with its own capability set (tar image,
  shell-script bootstrap, provider-owned networking), but its lifecycle paths
  are not wired yet: start, stop, and readiness checks return "the WSL2
  machine provider is not available on this host yet".

## Inside a machine, sandboxes are containers — never nested microVMs

The topology rule is enforced by construction, not convention. Inside the
guest, `nimbus machine api` (`crates/nimbus-bin/src/machine/api.rs`) serves
sandbox operations over a unix socket and constructs only a container
backend — there is no krun backend in the guest. On the host side,
`ForwardedMachineApiSandboxBackend` (`crates/nimbus-bin/src/machine/backend.rs`)
implements `SandboxBackend` by forwarding image-based launches across that
socket; it reports `SandboxBackendKind::Container` and rejects rootfs roots
and standalone-owned specs at the boundary. So a sandbox is never a microVM
nested inside a machine: the krun backend only targets hosts that are
themselves Linux, and on macOS the isolation boundary is the machine itself,
with the per-workload sandboxes inside it running as crun containers.

## Where this connects

- [Resource model](/concepts/resource-model/) — services, sandboxes, and
  sessions as user-facing nouns.
- [Tenancy](/concepts/architecture/tenancy/) — how the admit-once tenant
  decision authorizes a sandbox launch in the first place.
- [Runtime isolates](/concepts/architecture/runtime-isolates/) — the
  in-process isolation tier that sandboxes complement.
- [Tenant isolation](/concepts/tenant-isolation/) — the user-facing trust
  model these mechanisms implement.
