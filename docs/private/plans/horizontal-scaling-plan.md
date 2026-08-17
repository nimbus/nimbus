# Horizontal Scaling Plan

Preserve the deferred horizontal-scaling design as the executable plan for the
iroh + openraft **cluster substrate** that turns the
single-binary Nimbus into a self-forming multi-node cluster. This plan owns the
substrate; the stateful workloads that ride it (`secret-management`,
`service-identity-provider-auth`, `agent-browser-service`) are consumers, not
part of this plan (§16 Consumer Plans in the architecture doc).

The design is settled and source-stable: this plan is the band-by-band buildout,
verifier, and execution ledger for it. It does **not** re-derive the
architecture. The formerly referenced `docs/private/architecture/horizontal-scaling.md`
is not present in this checkout; this plan and its linked research are the
authoritative design, `docs/private/plans/research/iroh-cluster-substrate-2026.md`
is the iroh-v1 capability + decisions contract (version pins, API renames,
auth≠authz enrollment model, irpc-for-RaftNetwork, connectivity profiles,
resilience decisions — **authoritative for every iroh fact, do not re-derive from
memory**), and `docs/private/plans/research/horizontal-scaling-architecture-spec.md`
is the pattern-selection research (Patterns C + D + E). When a band needs a
contract decision, read those three first.

> **Architecture revision (2026-06-18) — coordinates with
> [`storage-seams-architecture.md`](storage-seams-architecture.md).**
> - HS6's iroh-blobs content distribution is the **`IrohBlobStore`**
>   implementation of **Seam A `BlobStore`**; the NOS byte plane rides it for
>   node↔node replication over the shared BLAKE3 address space. **Correction
>   (per spec §16a):** the cluster leg **adopts iroh's store as-is and layers
>   above it** — there is no pluggable Store trait; owning the durable format is
>   the deferred option (spec §17 D3), not the baseline. `LocalPackStore` (own
>   packs) stays the local-first default.
> - **Maturity flag:** `iroh-blobs` 0.103 self-declares *"not yet production
>   quality; use 0.35 for production."* The `BlobStore` seam is the hedge that
>   keeps it swappable; source-dive the `store` trait + `FsStore` on-disk layout
>   before committing the bridge.
> - Tenant durable-data migration **follows placement** (Seam E `migrate_in`):
>   cloud-placed → new owner reads from S3; local → replicate-then-handoff via
>   iroh-blobs.

This plan was refined (2026-06-17) against a first-principles iroh v1 deep dive.
The bands below adopt the deep-dive's recommended HS0..HS7 shape — with operator
DX, observability, and audit **front-loaded at HS3, before the data plane** —
and bake the deep-dive's failure-mode mitigations (ownership fencing lease,
joint-consensus pre-flight + force-recovery, snapshot GC protection, committed
tombstone set, rolling-upgrade barrier) into band requirements and verifier
conditions rather than leaving them as prose.

## Status

- **Status:** `deferred` (demand-gated substrate; single-node is the launch
  baseline — see §1 of the architecture doc).
- **Activation gate:** the first real multi-node deployment, **or** the first
  consumer plan whose own activation gate fires *and* genuinely requires
  cross-node behavior. Note that every named consumer (secret-management S7,
  service-identity, agent-browser) ships a single-node MVP and only *rides* this
  substrate in cluster mode, so the true trigger is a cluster deployment, not a
  consumer plan reaching its single-node MVP. Promote HS0 when that trigger is
  real; do not scaffold the iroh/openraft stack speculatively.
- **Primary owner:** this plan.
- **Primary goal:** a self-forming cluster substrate embedded in every `nimbus`
  binary — cryptographic node identity, NAT-traversing QUIC mesh, durable
  tenant→node placement mapping under linearizable consensus, reactive
  invalidation fanout, and content-addressed blob distribution — behind one
  `ClusterTransport` abstraction boundary so the binary is not structurally
  coupled to Iroh internals (§11).
- **Related plans and references:**
  - This plan is the current routing owner. The earlier architecture document
    is absent, so section references below are historical until a future
    horizontal-scaling activation deliberately restores or replaces that
    design. Do not infer implementation authority from the missing file.
  - `docs/private/plans/research/iroh-cluster-substrate-2026.md` — **iroh-v1
    capability + decisions contract.** Version pins, the 0.x→1.0 renames, the
    6-ALPN layout, the auth≠authz two-layer authz seam, connectivity profiles,
    and the resilience/rolling-upgrade/key-at-rest/FIPS decisions. Authoritative
    for every iroh fact; supersedes any pre-1.0 iroh assumption in the design doc
    until that doc is reconciled.
  - `docs/private/plans/research/horizontal-scaling-architecture-spec.md` —
    pattern-selection research (Patterns C + D + E) and the five-phase
    week-scoped staging the band order follows.
  - [`archive/nimbus-network-control-plane-plan.md`](archive/nimbus-network-control-plane-plan.md)
    — sole owner of the transport-free network resource lifecycle, including
    stable segment/attachment identity, the portable `NetworkSegmentAllocator`
    contract, local durable allocation state, and allocation fencing. This plan
    owns the future raft-issued node super-net lease. When HS activates, its
    composition root must adapt committed cluster state through a minimal
    dependency-safe lease-source seam (the prototype is currently the
    sandbox-local `ClusterLeaseProvider`); it does not redefine segment
    allocation, provider effects, or the network state store.
  - `docs/private/plans/archive/service-sandbox-node-reconciliation-plan.md` (NSR) —
    HS7 microVM/long-lived-workload placement binds **beneath** the NSR
    `WorkloadScheduler`/`WorkloadExecutor` (HS5 is request routing +
    fencing leases); cluster placement is the multi-node generalization of
    NSR's single-node scheduler. Needs the NSR2 executor spine.
  - `docs/private/plans/archive/nimbus-s3-object-storage-plan.md` (NOS) — HS6
    iroh-blobs content distribution is the transport the NOS Mirror placement
    mode rides.
  - `docs/private/plans/secret-management-plan.md`,
    `docs/private/plans/service-identity-provider-auth-plan.md`,
    `docs/private/plans/agent-browser-service-plan.md` — the three consumer plans
    that ride this substrate. `service-identity` carries a **hard** dependency on
    HS1 (cluster membership + node identity must exist before a node is trusted
    to mint or exchange provider-auth credentials).

## Why this plan exists

The architecture doc has existed and matured (it is the newest of the three
horizontal-scaling artifacts), and three deferred plans already declare they
"ride" it. But **no plan builds the substrate.** Phase 5 of the plan portfolio
sequenced the consumers (secret-management → service-identity → agent-browser)
as if their cluster substrate were already owned by someone — it is not. This
plan closes that gap: it is the single owner of the iroh/openraft buildout, so
the consumer plans have a concrete predecessor to wait on instead of an
ownerless architecture document.

It is `deferred` rather than active because the single-node binary is the launch
baseline (§1) — Nimbus scales first by distributing *tenants across a cluster*,
and a cluster of one is the common case until a deployment outgrows a node.

## Architecture constraints (ratified, do not relitigate without a design pass)

These are lifted from the architecture doc and are the binding decisions every
band inherits. They exist so a band author does not re-open a settled question.

- **Patterns C + D + E, never Pattern A.** Database-per-tenant + deterministic
  log + edge replicas. No shared-nothing range sharding, no distributed
  transactions, no consensus on the hot mutation path (research §2, §3).
- **Two ecosystems, complete coverage (§11).** Iroh (all networking: mesh,
  identity, NAT traversal, relay, gossip pub/sub, content-addressed blobs) +
  openraft (all consensus). No Foca/SWIM, no Zenoh, no tonic/gRPC, no pingora —
  each is replaced by an Iroh-native primitive (§11 "What we no longer need").
- **`ClusterTransport` is the abstraction boundary (§11).** All Iroh interaction
  goes through the `ClusterTransport` trait (`connect`/`accept`/
  `register_protocol`/`subscribe`/`broadcast`/`provide_blob`/`fetch_blob`) so an
  Iroh pivot is a single re-implementation, not a structural rewrite. The trait
  is keyed on **`EndpointId`** (iroh 1.0 rename of `NodeId`; it IS the ed25519
  public key) and **`EndpointAddr`**, and `connect`/`accept` carry the `Alpn` so
  **one `Endpoint` + one `protocol::Router` multiplexes every protocol** (the
  draft's per-protocol-endpoint implication is wrong). The blast radius is
  contained at this seam.
- **irpc for structured RPC; raw streams only for blobs (contract doc).** openraft
  `RaftNetwork` and all inter-node app RPC ride `irpc` + `irpc-iroh` 0.17
  (`IrohProtocol<S>` *is* a `ProtocolHandler`; postcard wire, 16 MiB cap). Only
  iroh-blobs bulk transfer uses raw QUIC streams. Snapshots use `full_snapshot`
  (NOT the deprecated `install_snapshot`) delivered over iroh-blobs. This retires
  the earlier "length-prefixed serde frames, no protobuf" framing as an overclaim.
- **Authentication ≠ authorization — membership is a committed raft fact
  (contract doc).** iroh's TLS authenticates the EndpointId but authorizes
  nothing (`ClientCertVerifier` accepts any key). A node becomes a member only
  when the leader commits `add_learner`. The two-layer authz seam is binding:
  (1) `EndpointHooks::after_handshake` checks committed membership and rejects an
  unknown EndpointId on every ALPN **except** `nimbus/enroll/1`; (2) auth-first
  per ALPN (valid token before any cluster RPC). Token = admission, not
  membership; revoking a token never evicts a joined node, and drain commits a
  **tombstone** so a drained node cannot re-enroll on a still-live token.
- **Stable-line library posture (§2, §11; pins per the iroh contract doc).**
  iroh 1.0 **shipped stable 2026-06-15** — the RC framing is retired. Product
  pins: `iroh` **1.0.0**, `iroh-gossip` **0.101.0**, `iroh-blobs` **0.103.0**
  (NOT 0.35 — that was the pre-1.0 generation), `irpc` + `irpc-iroh` **0.17**,
  `openraft` **0.9.24**. All are Apache-2.0/MIT (freely incorporable, no
  license-interplay gate). iroh-blobs 0.103 carries an upstream "not yet
  production quality" caveat handled at HS6 (maturity gate or raw-ALPN fallback).
- **Crate home honors the workspace invariants.** Cluster networking lives in a
  new `crates/nimbus-cluster/` workspace member that owns `ClusterTransport`,
  membership policy, and the openraft state machine. `nimbus-core` stays zero-I/O
  (no socket-touching cluster type lands there); any runtime-facing seam type
  rides `nimbus-runtime` (zero-workspace-dep) with the gateway implemented in
  `nimbus-server`, mirroring the egress (NEG) and filesystem (NFS) HostBridge
  pattern.
- **Durable state in openraft or iroh-blobs; gossip carries invalidation only.**
  Small metadata (the tenant→node mapping, membership, secret-store metadata) is
  openraft-replicated; large content-addressed payloads ride iroh-blobs. Gossip
  carries invalidation + liveness signals, **never** the canonical value of any
  stateful resource (§16 shared invariants).
- **Start-simple defaults for the open questions (§17, refined by the contract
  doc).** One global Raft group for the <20-node target (Q1/Q3 — but **document
  the split trigger**: tenant-count / placement-write-rate threshold at which
  membership and placement separate into two groups; resolve explicitly before
  HS7); carry both BLAKE3 and SHA-256 for bundles, **irreconcilable** — store
  both + a digest-translation table, never translate (Q2); one cluster
  invalidation topic with local filtering for the MVP, tenant-scoped
  `topic:<tenant_id>:<resource>` topics activated by profiling (Q4); custom
  cloud-tag `AddressLookup` auto-join (iroh 1.0 rename) as a named follow-on,
  AWS instance-tag first (Q5). Resolved this pass: **admission token =
  verifiable without contacting the signer and signed by the current leader**
  against a stable cluster root key
  (not a per-member CA-pin that rots), with the raft row as the
  revocation/consumption record (decide before HS2); **FIPS = non-FIPS by
  construction, documented**, optional `aws-lc-rs` TLS provider offered (resolve
  before HS2, not deferred); **rolling upgrade = forbid mixed-version
  state-machine schema, full-fleet barrier restart** (pre-launch posture). The
  canonical topic syntax is fixed even when the MVP uses a single topic.
- **Tenant ownership is a fencing-token lease, never a cached read (binding
  correctness invariant).** An owner may serve writes for tenant T only while it
  holds an unexpired, raft-committed ownership lease carrying a monotonic epoch;
  the lease TTL is provably shorter than the minimum reassignment delay; a
  minority-partitioned former owner fails closed on T once its lease expires; and
  every tenant write is epoch-stamped and rejected by storage if its epoch is
  stale. This is the antidote to split-ownership data loss and replaces any
  "lease-cached hot reads degrade gracefully" framing.
- **Tenant identifier is the first segment of every topic and the first column
  of every replicated row (§16).** This makes multi-Raft tenant partitioning
  (Q1) enable mechanically with no consumer-plan change.

## Seam A coordination (BlobStore / IrohBlobStore)

This section re-frames HS6's content-distribution work as the **`IrohBlobStore`
implementation of Seam A `BlobStore`** per
[`storage-seams-architecture.md`](storage-seams-architecture.md) (§5, §10, §13,
§16a, §17). It **supersedes the design intent** of the bare "iroh-blobs is the
transport NOS Mirror rides" framing in HS6 / §16 above; the HS0–HS7 bands and the
HS6 ↔ NOS seam note are retained beneath as the detailed task/test reference, with
their iroh-blobs work re-homing onto the seam contract here. Where this section and
the spec disagree with a band body, this section and the spec win. The
`ClusterTransport` boundary (§11) and the band buildout are unchanged; the change
is that the blob leg is now spoken about as **one impl behind a shared trait**, not
"the cluster's blob transport."

**HS6 builds `IrohBlobStore` — the cluster leg of Seam A, not a bespoke blob
plane.** The NOS byte plane (`BlobStore`, BLAKE3 content-addressed) rides
`IrohBlobStore` for node↔node replication, sharing the one BLAKE3 address space the
spec's `ReplicatingBlobStore` (`announce` → `BlobTicket`, `fetch_from`) defines.
`IrohBlobStore` implements `ReplicatingBlobStore: BlobStore`; the upcast
`Arc<dyn ReplicatingBlobStore>` → `Arc<dyn BlobStore>` is native (spec D2 —
toolchain rustc 1.96.0 ≥ 1.86, no shim).

**Byte-plane placement-policy resolution is owned by the shipped
`nimbus-object-storage` crate (`ObjectStorageResolver`), which composes
per-tenant placement policy into a `PlacementBlobStore`.** HS placement work
consumes that resolver rather than defining a parallel placement resolver.

**VERIFIED (spec §16a): iroh-blobs 0.103 has NO pluggable `Store` trait.** The
public `Store` is a concrete struct over an `irpc` actor; the only extension seam
is the unstable, doc-hidden `api::proto::Command` message set. `FsStore` is
**one-file-per-blob** (`<hash>.data`/`.obao4`/`.sizes4`/`.bitfield` + a redb index,
≤16 KiB blobs inlined) with no pack/segment concept. **Decision:** `IrohBlobStore`
**adopts iroh-blobs' own store (`FsStore`/`MemStore`) as-is and layers Nimbus's
manifests, encryption, and access-gating *above* it** — it does **NOT** "own the
format behind the seam." Owning the format means reimplementing the
`api::proto::Command` actor over a pack store, which is **deferred** (a documented
future option triggered only if the spec §11 ~16 KiB-CDC-chunk × one-file-per-blob
inode count is measured to hurt on the bundle/source lane — spec §17 D3). This
corrects the §16/HS6 "own the durable format" framing above: HS owns the durable
format **only** through `LocalPackStore`, never through iroh.

**`LocalPackStore` stays the local-first default; the `BlobStore` seam is the
maturity hedge.** Single-node and local-first placement use `LocalPackStore` (NOS's
own append-only encrypted pack format) and never touch iroh. iroh-blobs 0.103
**self-declares "not yet production quality; use 0.35 for production"** with a
fast-churning API — so the entire `IrohBlobStore` impl stays swappable behind Seam
A, and HS6's existing maturity gate (re-check OR a thin raw-ALPN blob-transfer
fallback, Q4) is what keeps the cluster blob leg from being upstream-blocked.

**Blob access is open-by-hash by default — HS must build the membership gate.** The
iroh provide protocol serves any requested hash to any connecting peer. Gating is a
**custom `EventSender`** — `ConnectMode::Intercept` on the remote `EndpointId`
(reuse HS2's committed-membership check / `after_handshake` authz fact) plus
`RequestMode::Intercept` per request — so only cluster members fetch tenant blobs.
This gate composes with **per-tenant ciphertext** (the NOS `EncryptedBlobStore<S>`
decorator, spec D1 segment-framed AEAD): encrypt below placement so every leg
stores identical ciphertext under the same BLAKE3 address, and a leaked hash is not
a plaintext existence oracle. The membership gate and the ciphertext are
independent defenses — ship both.

**Migration follows placement (Seam E `migrate_in`).** HS7's tenant durable-data
handoff is the spec's `VolumeProvider::migrate_in` shape: a cloud-placed tenant's
new owner reads from S3 (the NOS cloud leg), a local tenant is moved
replicate-then-handoff via `IrohBlobStore` (`announce`/`fetch_from`), and the
rebalancer still waits out the old owner's fencing lease before reassigning (HS5/HS7
epoch handoff is unchanged).

**Dual-hash posture stands.** BLAKE3(content) is the `BlobStore`/iroh transfer
address; SHA-256 is bundle/snapshot provenance. They are irreconcilable — **store
both, never translate** (Q2 + spec §6) — and the HS6 digest-translation table
remains the join.

## Bands

`/goal` control-plane verifier is created by HS0 at
`scripts/verify-horizontal-scaling.sh` (18 conditions, mostly FAIL until later
bands flip them). Before HS0 lands, a missing verifier is expected, not a green
signal. Baseline proof at
`docs/private/plans/proof/horizontal-scaling/hs0-baseline.md`. This plan was
originally maintained only as an ignored private file. NNC2.8 force-tracks the
shared-seam ledger so allocator ownership and cluster activation state are
recoverable from branch history; later edits must preserve that durability.

**Band order is deliberate: operator DX, observability, and audit are
front-loaded at HS3 — before gossip, routing, and content distribution — because
the deployment story IS the product.** An operator must be able to form, inspect,
audit, and reason about a cluster's trust state before any throughput work
begins. The harden-pass correctness mitigations (fencing-lease, joint-consensus
pre-flight + force-recovery, snapshot GC protection, tombstone set,
rolling-upgrade barrier) are band requirements with verifier conditions, not
prose.

| Band | Scope | Status |
| --- | --- | --- |
| HS0 | **Crate scaffold & boundary.** Create greenfield `crates/nimbus-cluster/`; define the `ClusterTransport` trait refined to iroh 1.0 (`EndpointId`/`EndpointAddr`, `Alpn`-carrying `connect`/`accept`, first-class `register_protocol` for the one-Endpoint-one-Router model — NOT the stale `NodeId`/`NodeAddr` draft); add the `EndpointId`/`TenantId`/`Topic`/`BlobHash` newtypes to `nimbus-core` (zero-I/O preserved, NOT iroh-typed so the dep doesn't leak); wire the injected-trait seam into `nimbus-engine` (HostBridge pattern, mirroring NEG `EgressGateway` / NFS `NimbusFsBackend`); author `scripts/verify-horizontal-scaling.sh` (18 conditions) + `hs0-baseline.md`. No network yet. Routing in `AGENTS.md` + `docs/private/plans/README.md`. Tests/verifier: crate builds, boundary edges acyclic, `nimbus-core` stays zero-I/O, `nimbus-runtime` stays zero-workspace-dep. | todo |
| HS1 | **Identity & transport seam.** Stand up the single iroh `Endpoint` + `protocol::Router`; ed25519 key load/generate at `0600` **fail-closed** (refuse to boot a fresh identity on a missing/unreadable key — iroh silently `SecretKey::generate`s otherwise); version-suffixed ALPN registration; `irpc-iroh` `IrohProtocol` wired as a `ProtocolHandler` (verified pattern, `iroh-ping` is the skeleton). Two-node connect over a raw ALPN, no consensus yet. Tests: two endpoints holepunch + connect over `nimbus/cluster/1`; a node with bad key perms refuses to boot; one Endpoint demuxes ≥2 ALPNs through the Router. | todo |
| HS2 | **Enrollment ceremony & openraft.** openraft 0.9.24 instance (4 traits, `RaftNetwork` over `irpc`, redb-backed state machine, §7); `cluster init` guarded single `initialize()` (fail-closed if any persisted raft state exists — the split-brain hazard); the `nimbus/enroll/1` ALPN + token mint/validate committed to the state machine; `after_handshake` two-layer authz gate (unknown EndpointId reaches enroll ALPN only); `add_learner(blocking)` admission; **committed tombstone set** rejecting drained/revoked EndpointIds *before* token validation; **token whose current-leader signature can be checked offline** against a stable cluster root key (resolves Q7); **FIPS posture resolved** (non-FIPS documented, optional `aws-lc-rs` TLS path, resolves Q3). The auth≠authz seam made real: a keypair becomes a member ONLY at the `add_learner` commit. Tests: a committed mapping survives leader loss + re-election; a follower read is linearizable via `ensure_linearizable()`; a stranger's EndpointId is rejected on every ALPN except enroll; an invalid/expired/consumed token is rejected + audited; a tombstoned EndpointId cannot re-enroll on a valid token; `initialize()` refuses when raft state exists. | todo |
| HS3 | **Operator DX, observability & audit (FRONT-LOADED, before the data plane).** The full `nimbus cluster` verb group (`init` / `join-token create` / `join` / `promote` / `drain` / `members` / `status` / `join-token list,revoke`); the two-command join ceremony (mint-on-member → `join` on newcomer, learner-by-default, deliberate `promote`); **`drain`/`promote` pre-flight** that refuses with an actionable error if the resulting joint config is unsatisfiable with currently-healthy voters (the joint-consensus deadlock guard) + a **guarded, audited force-recovery verb** (split-brain-guarded single-survivor restore) as the documented stranded-quorum exit; `/health` (liveness) vs `/ready` (caught-up member) split; `RaftMetrics` + iroh `remote_info()`/`home_relay_status()` status surface (leader, term, quorum margin, per-peer lag, direct-vs-relay path); `after_handshake` accept/reject counters **labeled by reason**; the **raft-log-sourced append-only audit stream** (membership changes ARE committed entries → durable, ordered, identical on every node). **Operator & agent access plane (Tailscale-shaped, native):** a second committed **operator roster** distinct from the member roster (RBAC scopes `admin`/`read-only`/`forward`/time-boxed `break-glass`); the same `after_handshake` seam admits operator principals to `nimbus/admin/1` + `nimbus/op-forward/1` **only** — never the member-only ALPNs, and a member node never gets admin scope by default (separation of duties); `nimbus forward` opens an `op-forward` stream bridging a local TCP socket to a port on a target node (Tier-2 compose-under-existing-VPN inherits reachability, keeps roster authz on top); operator grants/revocations + admin actions commit on the same audit path (the three "nevers": operator key never a voter/learner, "on the VPN" never authorizes, member never auto-admin). **Bundled `nimbus relay` role:** an opt-in, **assigned-never-auto-on** relay role for the multi-cloud-over-public-internet profile, placed on a **non-voter** reachable ingress node (relay availability decoupled from member churn; same ingress doubles as the operator rendezvous) — transport wiring cross-refs HS1's `RelayMode::Custom`. Tests: the two-command join admits a learner end-to-end; `drain` of a voter while another voter is down refuses with a reason (not a blocking change); `promote` refuses a lagging learner; `/ready` reports false for a minority-partitioned / FENCED node; the audit stream replays every privileged action with actor/target/term/index; a removed-but-running voter self-fences; an operator key is admitted to `admin`/`op-forward` but rejected on raft/gossip/blobs/forward and cannot become a voter; a member node is denied admin scope absent an explicit grant; an `op-forward` stream reaches a target port and its open/close is audited. | todo |
| HS4 | **Gossip (invalidation & liveness hints).** iroh-gossip 0.101.0 on `topic:cluster:state` + canonical `topic:<tenant_id>:<resource>`; **Ed25519-signed** messages (gossip has no native auth and rides the unauthenticated-by-default substrate); `NeighborUp`/`NeighborDown` feeding the failure detector; `Event::Lagged` handled (subscriber close + resubscribe); single cluster-invalidation topic with local filtering for the MVP (Q4), tenant-scoped topics behind the same API. Gossip carries invalidation + best-effort liveness **hints only — never canonical state or authoritative membership** (that is openraft's). **The plane secret-management rotation invalidation and agent-browser session signals ride.** Tests: a mutation on the owner invalidates a subscriber on another node within the latency budget; an unsigned/forged gossip message is dropped; a gossip payload never carries a canonical resource value; local filtering drops off-tenant invalidations; `Event::Lagged` triggers resubscribe not a wedged subscriber. | todo |
| HS5 | **Request routing & V8 integration (fencing-lease).** `nimbus/forward/1` ALPN: route a tenant request to its owning node via the committed map (route-not-proxy, §16); the engine consults `ClusterTransport` via the injected trait; V8 host ops reach cross-node facts only through `HostBridge` (runtime stays zero-workspace-dep). **Tenant ownership is a fencing-token lease**: an owner serves writes for T only under an unexpired raft-committed ownership lease carrying a monotonic epoch (TTL provably < min reassignment delay); a minority-partitioned former owner **fails closed** on T once the lease expires (NOT stale-cached reads); every tenant write is epoch-stamped and the storage layer **rejects** a stale-epoch write. Also folds in the §4/§14 V8-isolate stateless-invocation distribution. Tests: a request for a remote-owned tenant reaches the owner and returns; routing reads the current committed map; a placement change re-routes the next request; **a partitioned former owner refuses tenant writes once its ownership lease expires**; a write carrying a stale epoch is rejected at the storage layer; the cross-node hop rides its own QUIC stream not a framed wrapper. **Durable-object consumer requirement (added 2026-06-22 from the CFA exemplar review):** this lease is **tenant-scoped**, but Cloudflare Durable Objects need **per-DO-id single-activation** — one tenant holds thousands of DOs that must scatter across nodes, so a tenant-only lease defeats DO horizontal scale. HS5 owns **per-DO-id placement/leasing beneath the tenant lease** (Akka two-level region=tenant→entity=DO / Orleans per-grain directory) — the cluster-scale generalization of the single-node per-DO-id write lane CFA7 commits to (owner decision 2026-06-22: per-DO-id lanes, DO-faithful), the same model at two scales, with the epoch fence enforced at **per-DO storage** (a stale former owner of DO *x* rejected at *x*'s storage) and per-DO alarms fired only by the lease holder. See `archive/cloudflare-adapters-plan.md` CFA6 + `research/cloudflare-adapters-2026.md` §11. | todo |
| HS6 | **Content distribution (iroh-blobs).** iroh-blobs 0.103.0 on `nimbus/blobs/1` for runtime bundles, OCI layers (as blobs, manifest as HashSeq/Collection), and **raft snapshots** (`full_snapshot` + `generic-snapshot-data` → blob ticket; NOT deprecated `install_snapshot`); bao-tree verified streaming. **Snapshot-transfer durability (must-fix):** `ProtectCb` protects every in-flight snapshot ticket via a **persistent Tag** (NOT TempTag — dies on leader restart) for the catch-up lifetime, released only when the learner reports installed; **raft log purge respects the slowest in-flight learner's needed range**; `add_learner(blocking)` enforces a **timeout** surfacing "join failed: snapshot unfetchable" to the operator (HS3 audit). Carry both BLAKE3 + SHA-256, **irreconcilable** → store both + digest-translation table (Q2). **Maturity gate:** iroh-blobs 0.103 is upstream-flagged "not yet production quality" → gate on a re-check OR ship a thin raw-ALPN blob-transfer fallback so bundle/snapshot distribution is not upstream-blocked (Q4). **Transport the NOS Mirror placement mode rides** (cross-seam with NOS4). Tests: a bundle provided on one node is fetched + verified on another; an incrementally-streamed blob fails fast on a corrupted range; a snapshot blob survives a leader-side GC sweep during a slow learner's catch-up; an unfetchable snapshot times out the join with an operator-surfaced error; a NOS mirror write is addressable as an iroh blob; the dual-hash (BLAKE3 transfer + SHA-256/Sigstore evidence) is preserved. | todo |
| HS7 | **Multi-tenancy, rebalancing, DR & consumer handoff (closeout).** Tenant-granular ownership + rebalancer on join/`drain` — **the rebalancer waits out the old owner's lease before reassigning** (the epoch handoff that makes HS5's fence safe); the microVM/long-lived-workload placement path **binds beneath the NSR `WorkloadScheduler`/`WorkloadExecutor`** (needs the NSR2 executor spine; cluster placement is the multi-node generalization of NSR's single-node scheduler); **rolling-upgrade barrier** (pre-launch: forbid mixed-version state-machine schema, full-fleet barrier restart); **backup/DR as a TESTED deliverable** (versioned snapshot over iroh-blobs; split-brain-guarded single-survivor `initialize()`-from-snapshot that refuses if any other original member is reachable; trust root rides the snapshot); resolve the **single-vs-split raft group** decision (one group for <20 nodes; document the split trigger, Q1/Q5); **consumer-plan handoff verification** — walk each §16 consumer (secret-management, service-identity, agent-browser) and confirm its declared primitive matches the topic/Raft/lease contract. Verifier 18/18 green. Tests: a node join rebalances tenants without dropping a subscription and without violating the lease handoff; the DR restore re-seeds a 1-voter cluster and refuses when another original member is live; each consumer plan's claimed primitive is exercised end-to-end. | todo |

## Dependencies and seams

- **Substrate for three demand-gated consumers (§16).** This plan is the
  predecessor of `secret-management` (openraft secret-store metadata + gossip
  rotation invalidation, S7), `service-identity-provider-auth` (**hard** dep on
  HS1 node identity + HS2 committed membership before credential minting — a node
  must be admitted to membership and bound to its `EndpointId` before it is
  trusted to mint or exchange provider-auth credentials), and
  `agent-browser-service` (openraft session-registry mapping + iroh-blobs
  storage-state + gossip session signals + cross-node CDP over QUIC streams).
- **HS7 ↔ NSR.** The microVM/long-lived-workload placement path binds beneath the
  NSR `WorkloadScheduler`/`WorkloadExecutor`; it needs the NSR2 executor spine.
  The cluster leader's tenant-ownership decision is the multi-node input to NSR's
  single-node desired-state reconciliation — they must agree, not fork.
- **HS6 ↔ NOS.** iroh-blobs is the transport the NOS Mirror placement mode rides;
  coordinate HS6 with NOS4 cloud/mirror writes so the blob address space is
  shared, not duplicated.
- **Workspace invariants.** `nimbus-cluster` keeps `nimbus-core` zero-I/O and
  `nimbus-runtime` zero-workspace-dep; the `ClusterTransport` boundary contains
  the Iroh blast radius (§11).

## Verifier

HS0 creates `scripts/verify-horizontal-scaling.sh` (18 conditions). At HS0 the
conditions are authored and mostly FAIL; each later band flips its conditions
green. Before HS0 lands, the absent script is not a pass. The verifier asserts,
at minimum:

**Boundary & posture (HS0–HS1):** the `nimbus-cluster` crate exists with the
declared acyclic edges and no `nimbus-core` I/O regression; `nimbus-runtime`
stays zero-workspace-dep; `ClusterTransport` is the single Iroh seam keyed on
`EndpointId`; the node key is `0600` fail-closed (refuses a fresh identity on a
missing key).

**Consensus & enrollment (HS2):** the openraft state machine is redb-backed and
single-group; `initialize()` refuses when raft state exists; `after_handshake`
rejects an unknown EndpointId on every ALPN except enroll; an invalid/expired
token is rejected + audited; a **tombstoned EndpointId cannot re-enroll** on a
valid token.

**Operator DX & safety (HS3):** `drain`/`promote` **pre-flight refuses** an
unsatisfiable joint-config change with an actionable error instead of blocking;
the guarded **force-recovery** verb exists; `/ready` reports false for a
minority-partitioned/FENCED node; the audit stream replays every privileged
action from the committed log; a removed-but-running voter self-fences; an
**operator-roster principal** is admitted only to `admin`/`op-forward` (rejected
on raft/gossip/blobs/forward, never a voter/learner), a member node is denied
admin scope without an explicit grant, and an `op-forward` stream's open/close is
audited.

**Data plane (HS4–HS6):** the gossip plane carries no canonical state and drops
unsigned messages; the topic syntax matches the canonical convention; **a
partitioned former owner refuses tenant writes once its ownership lease expires**
and a stale-epoch write is rejected at storage; HS6 rides iroh-blobs and a
snapshot blob survives a leader-side GC sweep during a slow learner's catch-up.

**Closeout (HS7):** the rebalancer honors the lease handoff on join/drain; the
**DR restore** re-seeds a 1-voter cluster and refuses when another original
member is live; each §16 consumer primitive is exercised end-to-end.

## Execution log

| Date | Band | Action | Detail | Verification | Disposition |
| --- | --- | --- | --- | --- | --- |
| 2026-06-17 | meta | authored | Promoted the existing `docs/private/architecture/horizontal-scaling.md` design + the Patterns-C+D+E research spec into a band-structured execution plan (HS0..HS7) during the plan-portfolio cohesion pass. Closes the ownership gap where three deferred consumer plans declared they "ride" the cluster substrate but no plan built it. Bands map to the architecture doc sections; constraints lifted verbatim from §3/§7/§11/§16/§17. Status `deferred` (demand-gated; single-node is the launch baseline). | reviewed against `docs/private/architecture/horizontal-scaling.md` §1-§17 and `research/horizontal-scaling-architecture-spec.md` §2-§5; cross-checked the §16 Consumer Plans table against `secret-management-plan.md`, `service-identity-provider-auth-plan.md`, and `agent-browser-service-plan.md`; no code changes | keep deferred until the activation gate (first real multi-node deployment) triggers |
| 2026-06-17 | HS3 | refined | Folded two operator-facing resolutions into the contract doc + architecture doc + HS3 scope/verifier. (1) **Bundled-relay decision (resolved open question #1):** bundle the relay as an opt-in **`nimbus relay` role**, assigned-never-auto-on, placed on a stable **non-voter** cross-network-reachable ingress node so relay availability is decoupled from member churn (the NATed node that most needs a relay cannot be one); added a 4th connectivity profile **"multi-cloud over the public internet (bundled relay on ingress)"** to both docs + the deployment-topology matrix. (2) **Operator & agent access plane (Tailscale-shaped, native):** ops/devs/devops/agents are a *second principal class* on the *same* `after_handshake` seam — a committed **operator roster** (RBAC scopes) admits them to `nimbus/admin/1` + new `nimbus/op-forward/1` only, never the member-only ALPNs and never a voter/learner; Tier-1 native (zero extra infra, reach any node by key NAT-traversed) + Tier-2 compose-under-existing-VPN; the three "nevers" (operator key never a voter, "on the VPN" never authorizes, member never auto-admin); audit rides the same raft apply path. Same ingress node = relay + operator rendezvous (the two decisions reinforce). Owned by HS3 (already owns the verb group + authz seam), not a new band. | doc-only fold-in of the prior workflow's design + this session's first-principles relay/operator reasoning; cross-checked the ALPN table, connectivity profiles, and matrix for consistency; no code changes | keep deferred; HS3 scope + contract doc now cover the operator/agent access plane and the bundled-relay role |
| 2026-06-17 | meta | refined | First-principles iroh **v1** deep dive (local `~/src/github.com/n0-computer/{iroh,iroh-gossip,iroh-blobs,iroh-ping,irpc}` + docs + v1 launch) feeding a multi-agent design/critic/harden workflow. Authored the contract doc `research/iroh-cluster-substrate-2026.md` (version pins, 0.x→1.0 renames, 6-ALPN layout, irpc-for-RaftNetwork, auth≠authz enrollment, connectivity profiles, resilience/rolling-upgrade/key-at-rest/FIPS decisions). Reframed bands to the deep-dive HS0..HS7 with **operator DX/observability/audit front-loaded at HS3**; baked in the harden-pass correctness mitigations as band requirements + verifier conditions: ownership **fencing-token lease** (split-ownership data-loss antidote), **joint-consensus pre-flight + force-recovery** (drain-deadlock exit), **snapshot GC persistent-Tag protection + log-purge interlock + add_learner timeout**, **committed tombstone set** (re-enrollment loop), **rolling-upgrade full-fleet barrier**. Added the `nimbus cluster` verb group. Re-pinned `iroh` 1.0.0 / `iroh-gossip` 0.101.0 / `iroh-blobs` 0.103.0 / `irpc` 0.17 / `openraft` 0.9.24; renamed `NodeId`→`EndpointId`. Resolved Q2 (irreconcilable dual-hash), Q3 (FIPS documented), Q7 (signed-token anchor); documented Q1/Q5 split-trigger. Verifier 12→18 conditions. | derived from the digested workflow synthesis (critic 11 corrections, harden 7 failure modes + 10 enterprise gaps + verdict, full first-principles design); cross-checked against the iroh 1.0 local sources; no code changes | keep deferred; the band scopes + contract doc are now iroh-v1-grounded and ready when the activation gate triggers |
| 2026-07-01 | HS2/HS5 | scoped, corrected by NNC2.8 | **Network-IPAM coordination** from the archived multi-tenant-per-node network lane (MTN, `archive/multi-tenant-node-network-plan.md`). Ratifies **routed-not-overlay**: per-node super-net → per-tenant segments, while cross-node east-west remains iroh-forwarded (`nimbus/forward/1`) — no VXLAN/Geneve. **Landed authority:** `nimbus-network` owns stable segment/attachment/lease identity and the portable `NetworkSegmentAllocator` lifecycle contract; `nimbus-sandbox` owns OCI realization plus its `SingleNodeSegmentAllocator` and transport-free `ClusterSegmentAllocator` adapters over the network contract. Durable state records the fenced super-net and `NetworkLeaseEpoch`; creation fails closed on stale or expired authority while restricted cleanup remains possible for durable old handles. **Deferred HS work:** (1) commit a disjoint node→super-net lease map in openraft, keyed on cluster `EndpointId`, with TTL shorter than reassignment delay; (2) promote the currently sandbox-local lease-source prototype into a minimal dependency-safe injected seam and adapt committed cluster state at the composition root, without moving allocation into `nimbus-cluster`; (3) admit mesh-joined workload creation only with a live fenced lease; (4) reclaim only after drain plus a committed epoch bump; and (5) publish the already-landed `NodeCapacity.remaining_segments` dimension into cluster-visible capacity. `ClusterTransport` continues to own membership, node identity, routing, forwarding, and mesh only. **Verifier invariant:** for every live pair of nodes, leased super-nets and all derived segments are disjoint; allocation uses stable IDs/epochs, never IP addresses as workload identity. | NNC2.8 links the canonical network owner and source-checks away the old sandbox-owned trait/install call claim; the portable contract and provider-neutral behavior are already proven by NNC2, while the raft lease source remains blocked on the intentionally unbuilt HS substrate | keep deferred; activate the cluster lease source with HS2/HS5 without creating a second allocator or coupling `nimbus-network` to Iroh/openraft |
| 2026-07-24 | NNC2.8 | shared-seam truth-up | Reconciled the completed network-control-plane extraction into this deferred owner. `nimbus-network` is the canonical connectivity-resource lifecycle contract; sandbox remains the Netavark/netns/OCI realization adapter; future `nimbus-cluster` remains the sole transport, membership, routing, and raft lease-source owner. The allocator consumes a fenced super-net lease and does not become cluster transport. | linked `archive/nimbus-network-control-plane-plan.md`; checked the landed trait/identity/epoch and sandbox adapter/lease-provider sources; scanned the durable plan set for stale sandbox-owned `NetworkSegmentAllocator` claims | keep HS deferred; consume the low-dependency network seam when the real multi-node activation gate fires |
