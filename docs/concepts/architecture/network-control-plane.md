---
title: Network control plane
description: How nimbus-network owns portable connectivity-resource identity, plans, leases, capability evidence, and recovery without owning transport or provider effects.
sidebar:
  label: Network control plane
  order: 6
---

Nimbus manages network resources through explicit lifecycles instead of socket
helpers. `crates/nimbus-network` is the transport-free control plane for that
work. It gives attachments, segments, endpoints, routes, listeners, and ports
stable identities. It also defines how Nimbus plans, reserves, observes, and
releases those resources.

This role sits beside compute and storage:

| Plane | Lifecycle owner |
| --- | --- |
| Compute | `crates/nimbus-compute` coordinates workload provision, restart, and teardown. |
| Network | `crates/nimbus-network` owns portable connectivity-resource identity and authority. |
| Storage | `crates/nimbus-storage` owns durable application data and persistence providers. |

The boundaries matter. The network crate does not absorb workload policy,
service naming, socket binding, packet forwarding, or cluster membership.

## A low-dependency contract

`crates/nimbus-network/Cargo.toml` gives the crate one outgoing Nimbus
workspace edge: `nimbus-core`. This direction lets upper layers consume one
portable network vocabulary without creating a dependency cycle.

The crate is also provider-effect-free. It does not import Axum, Pingora,
Netavark, nftables, gvproxy, Iroh, or a cloud SDK. Its crate-level contract in
`crates/nimbus-network/src/lib.rs` keeps these concrete effects with their
owners:

- `nimbus-server` and `nimbus-kv` own protocol listeners and socket effects.
- `nimbus-sandbox` owns namespaces, bridges, IPAM adapters, firewalls, and
  sandbox-network effects.
- `nimbus-machine`, `nimbus-proxy`, and `nimbus-node` own their provider and
  forwarding effects.
- `nimbus-services` owns logical names and service readiness.
- `nimbus-tenant` owns admission and endpoint policy.
- `nimbus-egress` decides egress policy. `nimbus-proxy` enforces it.

This is what **transport-free** means here. A `PublishedEndpoint` can describe
portable endpoint semantics, but a transport owner still binds and serves the
real socket. See [server and transport](/concepts/architecture/server-transport/)
for that effect boundary.

## Stable identity, not address identity

`crates/nimbus-network/src/identity.rs` defines separate IDs for each resource
kind. The kinds include plans, attachments, segments, and published endpoints.
They also include listeners, ingress routes, port leases, and providers.
`NetworkResourceId` in
`crates/nimbus-network/src/state.rs` preserves those domains instead of
flattening them into untyped strings.

An IP address or port number is never workload identity. Providers can move or
reuse addresses. Stable IDs remain meaningful across inspection, restart, and
cleanup. Every authoritative resource version also carries:

- the parent plan ID.
- the resource ID.
- the desired generation.
- the complete plan digest.
- the lease epoch.

`NetworkResourceVersion` compares all five values before a state change. A
stale generation, crossed resource, changed plan, or old epoch fails closed.

## Desired, durable, and observed state

The model keeps three forms of state separate:

| State | Meaning | Source |
| --- | --- | --- |
| Desired | What connectivity this workload generation needs. | `NetworkPlan` in `crates/nimbus-network/src/plan.rs` |
| Durable | Which resource identity, lease epoch, provider handle, and lifecycle phase Nimbus owns. | `DurableNetworkResourceState` in `crates/nimbus-network/src/state.rs` |
| Observed | What a provider currently reports about that exact fenced version. | `NetworkObservation` and `NetworkStatus` in `crates/nimbus-network/src/status.rs` |

`NetworkPlan` binds a stable plan ID and generation to a canonical content
digest, capability requirements, and readiness requirements. Equal-generation
content conflicts are errors. A later generation can advance the plan.

Durable state records authority. It can retain a redacted provider handle and
move through explicit lifecycle phases. It does not contain observed
readiness. `NetworkStatus` is rebuildable evidence. Losing an observed
projection does not release a lease or authorize resource reuse.

That separation prevents a common control-plane defect. A green status row
cannot become allocation authority, and a missing status row cannot erase a
resource that may still exist.

## One node-local durable authority

`LocalNetworkManager` in `crates/nimbus-network/src/manager.rs` composes one
process-owned network authority. The manager pairs the state store, attachment
authority, port authority, and one immutable capability registry. A second
independent composition in the same process fails instead of silently
selecting another host-global authority.

`LocalNetworkStateStore` in `crates/nimbus-network/src/state_store.rs` is the
node-local persistence boundary. It uses one checksum-protected state file,
one cross-process lock domain, and atomic replacement. Startup rejects
filesystems that cannot provide the required local locking and synchronization
contract.

The store partitions segment allocations, attachment state, tenant IPAM, and
host-global port leases. These partitions share one revision and commit point.
They are not separate allocation authorities.

## Port and segment allocation

The port authority models reservation, provider binding, active use, cleanup,
and release. A provider may adopt an already-bound socket or report a
provider-assigned port. The durable lease still carries stable ownership,
generation, and epoch fencing. Shell scan-close allocation is not part of this
model.

`NetworkSegmentAllocator` in `crates/nimbus-network/src/segment.rs` is a
portable allocation capability. Attachment holds use `NetworkAttachmentId`,
never an address or sandbox ID. Cleanup retains the hold until an exact proof
confirms that provider and namespace effects are absent. Failed cleanup
therefore prevents unsafe reuse.

The allocator does not define cluster transport. A future cluster can fence a
node's super-net lease and inject an allocator. Cluster membership, node
identity, routing, mesh, and lease sourcing remain a separate control plane.

## Capabilities and sovereignty

Providers report closed capability facts through
`crates/nimbus-network/src/capability.rs` and
`crates/nimbus-network/src/capability_registry.rs`. The dimensions include
attachment shape, isolation, address family, bind realm, exposure, port
assignment, ingress, forwarding, lifecycle operations, and TLS behavior.

Sovereignty is part of selection evidence. A requirement can constrain
control-plane locality, external dependencies, and offline restart. The
locality order distinguishes local-only, operator-local, and third-party
control planes. Selection fails when a provider cannot prove every required
dimension.

This supports different machine modes without pretending they are identical.
A host-managed provider can report Nimbus-owned namespace and forwarding
effects. A provider-managed environment can report its own virtual-network
boundary. The caller selects by evidence, not by a hard-coded provider name.

## Readiness and lifecycle ordering

`NetworkReadinessRequirement` names one provider condition for one stable
resource. `NetworkReadinessDependency` binds that requirement to an active
port lease and exact lifetime. `NetworkReadinessEvidence` accepts only matching
current-generation observations. These types live in
`crates/nimbus-network/src/readiness.rs`.

Compute uses that evidence while it coordinates the complete workload saga.
The safe provision order is:

```text
admit → reserve → prepare → attach → activate → publish → observe
```

The workload stays inert until the required attachment and enforcement
preconditions are ready. The safe teardown order is:

```text
withdraw → drain → stop → detach → release → record
```

The durable resource state machine in `crates/nimbus-network/src/state.rs`
makes skipped cleanup illegal. Ambiguous effects enter `CleanupPending`.
Release needs confirmed deletion evidence. Inspection and retry must use the
same stable identity and fence.

## How compute uses the seam

`WorkloadNetworkPlanCompiler` in
`crates/nimbus-compute/src/workload_network_plan.rs` lowers admitted workload
intent into portable network plans. `ComputeState` receives the one
`LocalNetworkManager` and gives its immutable capability reports to the sole
workload provisioner. The durable workload saga then coordinates network and
execution steps without moving provider effects into the network crate.

This preserves two distinct authorities:

- `nimbus-compute` decides when the workload lifecycle advances.
- `nimbus-network` decides whether a connectivity resource transition is
  valid for the exact identity, generation, digest, and epoch.

Service names remain in `nimbus-services`. Endpoint and attachment handles are
network resources. DNS, xDS, Consul, or another name provider would be an
optional effect provider, not a new source of logical service identity.

## What this seam does not promise

The implemented control plane is node-local. Multi-node clustering and
horizontal scale-out are not available today. In particular, cluster
transport is not part of `nimbus-network`, and the crate does not imply an
overlay network such as VXLAN or Geneve.

The seam does promise one durable local authority, explicit provider evidence,
fenced recovery, and truthful observations. Those contracts let Nimbus swap
network providers without giving up deterministic lifecycle tests or safe
cleanup.

## Where this connects

- [Sandboxes and machines](/concepts/architecture/sandbox-machines/) explains
  where namespace, Netavark, nftables, and gvproxy effects run.
- [Server and transport](/concepts/architecture/server-transport/) explains
  where routers, protocol listeners, and socket lifecycles run.
- [Engine and mutation path](/concepts/architecture/engine-mutation-path/)
  explains the durable application-data coordinator.
- [Storage](/concepts/architecture/storage/) explains the durable-data plane.
- [Scaling](/concepts/scaling/) states the current single-process boundary.
