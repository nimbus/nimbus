# Network Control Plane

Status: landed architecture; NNC9 closeout is active.

`nimbus-network` owns the transport-free lifecycle of connectivity resources.
It is the network counterpart to `nimbus-compute` workload lifecycle and
`nimbus-storage` durable-data lifecycle.

## Dependency rule

`nimbus-network -> nimbus-core` is its only outgoing workspace edge. Upper
crates consume the network crate. The network crate must not depend on tenant,
sandbox, services, compute, server, system, cluster, Axum, Pingora, Iroh,
Netavark, or a cloud SDK.

This rule keeps the portable state machines reusable and prevents cycles with
the existing tenant, sandbox, service, and system graph.

## Ownership

| Concept | Owner |
| --- | --- |
| Stable attachment, segment, endpoint, ingress-route, listener, provider, plan, and port-lease identity | `nimbus-network` |
| Portable network plans, resource state, durable node-local lease/state stores, capability evidence, readiness composition, and reconciliation contracts | `nimbus-network` |
| Workload lifecycle order and durable workload saga coordination | `nimbus-compute` over `nimbus-workloads` vocabulary |
| Netavark, namespace, IPAM realization, nftables, gvproxy, and guest-network effects | `nimbus-sandbox` |
| HTTP/WebSocket and protocol listeners | `nimbus-server`; RESP remains in `nimbus-kv` |
| Logical service names and readiness publication | `nimbus-services` |
| Tenant policy and endpoint/egress admission | `nimbus-tenant` |
| Egress decision and enforcement | `nimbus-egress` PDP and `nimbus-proxy` PEP |
| Machine provider mode and provider-specific effects | `nimbus-machine` plus its CLI/server adapters |
| Routes, listeners, ports, attachments, and endpoints as observed status | `nimbus-system` projections |
| Future membership, node identity, mesh, routing, forwarding, and raft super-net lease source | future cluster owner under the deferred horizontal-scaling plan |

Concrete single-node and fenced cluster-lease segment allocators stay in
`nimbus-sandbox` because they realize OCI/provider state. They implement the
portable `NetworkSegmentAllocator` contract from `nimbus-network`. The
allocator consumes a fenced node super-net lease; it does not own membership,
routing, or cluster transport. The data path remains routed, not overlay:
node super-net → tenant segments, with no VXLAN or Geneve requirement.

## State and identity

Every resource keeps three facts distinct:

1. Desired identity and generation.
2. Durable lease, claim, and opaque provider handle.
3. Fresh observed provider or projection status.

Stable IDs are tenant-qualified where needed. Generation and epoch fencing
reject stale work. An IP address, port, PID, or provider handle is location or
evidence, never workload identity. Ambiguous effects require exact inspection
before retry. Cleanup failure prevents resource reuse.

## Lifecycle order

Provision:

```text
admit -> reserve -> prepare inert -> attach -> activation-ready -> activate
      -> workload-ready -> publish -> observe
```

Preparation can create a process or VMM envelope, but it cannot run tenant
instructions. Compute activates only after the same generation has an
authenticated attachment and every required policy-enforcement prerequisite
is ready.

Teardown:

```text
withdraw -> drain -> stop -> detach -> release -> record
```

Compute owns this saga order. Each effect remains behind its concept owner.
Network readiness composes authenticated state; it does not absorb policy,
service naming, socket effects, forwarding, or projection authority.

## Sovereignty

Provider capability selection uses explicit, source-authenticated evidence and
fails closed. Host-managed Netavark/gvproxy and provider-managed machine
networking are different modes. Optional DNS, xDS, certificate, tunnel, or
cloud load-balancer adapters must have a concrete consumer before promotion.
The ingress certificate provider remains separate from the proxy's ephemeral
interception CA.

The offline sovereignty proof and all task evidence are under
`docs/private/plans/proof/nimbus-network-control-plane/`. The active control
plane is `docs/private/plans/nimbus-network-control-plane-plan.md`.
