# Plans

This directory prefers a small-number-of-plans model with clear ownership.

## Execution Order

The plans below were authored at different times against different states of the
codebase. This section is the single source of truth for **what order to execute
them in**. It groups the portfolio into six phases by dependency, not by author
date. Plans inside the same phase are parallel-safe; a later phase names the
specific predecessor artifact it waits on (not the whole predecessor plan).

Four dependency lanes run through the portfolio:

- **L1 — storage / isolate substrate:** `nimbus-crypto-extraction`,
  `nimbus-isolate-filesystem` (NFS), `nimbus-egress-gateway-extraction` (NEG),
  `nimbus-s3-object-storage` (NOS).
- **L2 — sandbox / node control plane:** archived
  `service-sandbox-node-reconciliation` baseline (NSR), `nimbus-sandbox`
  (Bands B/S/D/G), `sandbox-disk-limit-enforcement`.
- **L3 — runtime / WASM:** `wasmtime-backend`, `wasi-agent-capabilities`,
  `layered-admission-control`, `native-transport-evolution`,
  `profile-aware-isolate-runtime` (PIR; deferred, gated on its own PIR0
  measurement; successor to the archived `v8-locker-fork` plan),
  `runtime-execution-classification` (REC; derived execution-plan seam that
  consumes PIR's benchmark/profiling foundation).
- **L4 — adapters / distribution / identity / cluster:** `cloudflare-adapters`,
  `distribution`, `horizontal-scaling` (the cluster substrate),
  `secret-management`, `service-identity-provider-auth`,
  `agent-browser-service`, `windows-machine-support`.

### Phase 0 — Cohesion repair (this exercise)

Reconcile cross-plan seams and path drift so every plan agrees on ownership.
Done as of 2026-06-16: path-prefix repair across the ten top-level plans;
`nimbus-init`→`nimbus-guest`, CFA0 status, and the wasmtime activation-gate fix;
and the four seam re-scopes applied to the respective plans (wasi-agent consumes
NFS+NEG; the `libkrun_session` backend sits beneath the NSR executor; NOS
depends on NFS2/NFS3/NFS6; the egress credential-injection PEP defers secret
*values* to secret-management). No code; this is the ordering contract itself.

Second pass, 2026-06-17: closed the one missing plan and reconciled six order
inconsistencies. (1) **Added `horizontal-scaling-plan.md`** — three deferred
consumer plans declared they "ride" the iroh+openraft cluster substrate in
`docs/private/architecture/horizontal-scaling.md`, but no plan built it; that
substrate is now owned and sequenced as the head of Phase 5. (2) Softened
NC→NOS from a hard predecessor to **preferred-before, fallback reuse**
`nimbus-storage/src/encryption/` (matching the NOS plan body). (3) Decoupled
`layered-admission-control` from wasmtime — it is demand-gated by any EO8
experiment report, not by the wasmtime admission surface, so it moves to Phase 5.
(4) Corrected the `windows-machine-support` rationale: its macOS prerequisite
(MAC1-MAC7) is **already satisfied** in the archive, so it is deferred only
pending self-promotion, not gated on MAC5+/NSR5. (5) Clarified the `wasi-agent`
A2 activation as a three-way join (wasmtime W3 + NFS5 + NEG6). (6) Confirmed the
`nimbus-sandbox` activation precondition is met (its cited boundary plan is
archived/complete). Still no code; this is the ordering contract itself.

Third pass, 2026-06-18: tightened the execution contract after a verifier audit.
(1) NC is now the required predecessor for NOS byte-plane crypto, while NOS
metadata/interface design remains unblocked. This prevents a duplicate encryption
stack from forming under `nimbus-blob`. (2) The NFS/NOS/NEG/NC and
horizontal-scaling verifier scripts are explicitly owned by their phase-0
scaffold bands; before those bands land, a missing script is expected, not a
green signal. (3) The NFS/NEG shared runtime-bootstrap edit is now sequenced
through a small extension-registry seam before both plans touch `extensions.rs`.
(4) Function Source Visibility moved to the completed local-baseline section,
with its 21/0 verifier result recorded here.

Fourth pass, 2026-06-18: authored the governing
**`storage-seams-architecture.md`** spec after a first-principles review of the
storage / filesystem / volume subsystem (the ZeroFS/SlateDB challenge). It
re-centers NOS / NFS / sandbox / HS on five pluggable seams (`BlobStore`,
`ObjectMetaStore`, `NimbusFsBackend`, `s3s::S3`, and a new `VolumeProvider`) so
local-binary, on-prem-cluster, and cloud-primary are one codebase with swapped
implementations. Re-scopes folded into the four plans as revision notes: the NOS
byte plane becomes the `BlobStore` seam (`PackChunkStore` is one impl; FastCDC
demotes to the bundle/source lane); the durable agent filesystem moves from
NOS6/NFS to the new `VolumeProvider` seam in the sandbox plan (deleting the F1
random-write band); ZeroFS is excluded (AGPL) with its capability rebuilt as a
pluggable `ObjectFs` volume backing (SlateDB candidate engine), and SlateDB is
scoped to that one opt-in backing — never the metadata or byte plane. Still the
ordering contract; the spec is the interface source of truth.

Fifth pass, 2026-06-18: propagated the seam-centric re-cut from NOS to the rest
of the portfolio and verified the design against real source. (1) **NFS, sandbox,
HS, and NC** each got a seam-centric section (NFS-C0..C7 around Seam C; a
cross-cutting Volume seam / `VolumeProvider` in sandbox; a Seam-A coordination
section in HS; a `TenantKeyring` + framed-AEAD contract in NC) — done by a
background `storage-seams-propagation` workflow (4 parallel agents + a
consistency critic). (2) **Three open decisions resolved** in spec §17: D1
encrypted `get_range` = segment-framed 64 KiB AEAD (keyed per-frame nonce,
dedup-preserving); D2 native trait upcasting (rustc 1.96 ≥ 1.86, no shim); D3
bundle lane = `LocalPackStore` packs + `HashSeq` (FastCDC at ~16 KiB, measured).
(3) **Verified findings folded in:** iroh-blobs v0.103 has no pluggable Store
trait → `IrohBlobStore` adopts-above-store (not own-format); CDC pays only at
~16 KiB (proof at `proof/storage-seams/dedup-bench/`); Seam C is `(?Send)` with
per-backend `deno_io::fs::File` impls. (4) The critic's findings were
reconciled (stale detail rows now carry supersession notes; the HS
"own-the-durable-format" self-contradiction fixed; NC added to the spec roster).
A compiling `nimbus-blob` trait skeleton + a cross-cutting
`verify-storage-seams.sh` close out this wave. Still the ordering contract; the
spec governs the interface.

Sixth pass, 2026-06-18: the seam reconciliation **reversed an L1 ordering edge**,
so the execution order changed. The old contract gated `NFS2/NFS3` **before**
NOS1 because `nimbus-blob` compiled against `nimbus-fs` (it implemented a
`ChunkRead` trait owned by `nimbus-fs`). The reconciliation made `ChunkRead` the
read side of `BlobStore::get_stream` (owned by `nimbus-blob`), which `nimbus-fs`
now **consumes** — so the edge is `nimbus-fs → nimbus-blob`, and `nimbus-blob` is
a leaf on `nimbus-core`. Current worktree note, 2026-06-19: the preserved
`nimbus-blob` scaffold has been applied from stash into this worktree, but it is
not a committed/CI-proven baseline until this branch lands it. Net: (1)
`nimbus-blob` (Seam A) is a **parallel foundation** alongside NFS's mount table,
not downstream of it; the NFS2/NFS3-before-NOS1 gate is dropped. (2) Surviving
NFS↔NOS joins: `nimbus-blob` before **NFS-C3** (CAS-RO consumes `get_stream`) and
**NFS6 before NOS-A7** (ObjectRwBackend fills the registration slot). (3) New
cross-lane edges from the new seams: **Seam E** (sandbox `VolumeProvider`
snapshot/fork) → **Seam A** (`nimbus-blob`), and **`BlobGc`** snapshot-root GC
spans L1+L2 (manifest-root GC needs only Seam B). (4) NC still precedes NOS's
*real* byte-plane AEAD; HS still gates only the NOS-A8 cluster leg (local-first
NOS is HS-independent). The spec §12 graph + the NOS/NFS bodies were corrected to
match.

Seventh pass, 2026-06-19: integrated the new **`profile-aware-isolate-runtime`**
(PIR) plan — the runtime-execution-optimization successor to the archived
`v8-locker-fork` plan — into the ordering contract. (1) Placed PIR in **Phase 5**
(demand-gated L3) alongside `layered-admission-control` and
`native-transport-evolution`; its hard predecessor (Locker fork Phase 5) is the
same met gate that opened wasmtime, so it is launch-independent. (2) Recorded PIR
as a **third consumer of the Phase-2 `extensions.rs` bootstrap registry seam**
(with NFS and NEG): PIR6's per-profile snapshot composition, node-snapshot
consumption, and lazy/tree-shaken extension init touch the same `extensions.rs` /
`snapshot_extensions` / `execution_extensions` lists, so it routes through the
registry rather than hand-editing — whichever of NFS/NEG/PIR touches `extensions.rs`
first lands the seam. (3) Recorded the cross-lane edge **PIR8 → `nimbus-sandbox`
Band S** (PIR8's strong-tier escalation rides the S-band snapshot/MAP_PRIVATE/fork
primitive; it consumes, never duplicates — and `nimbus-sandbox` Band S sits in
Phase 3, ahead of PIR's Phase 5, so the dependency is already satisfied by order).
(4) Noted PIR's adjacency to `layered-admission-control` (EIB6 already names
retained-runtime-pool pressure as an execution class): PIR owns the pool
*mechanism* + the pool-sizing/autoscaling equation; layered-admission owns whether
to add *gates* — neither blocks the other. Still the ordering contract; no code.

### Phase 1 — Launch closeout + strategic adapter wedge, parallel (L2 + L4)

Independent of the L1/L3 substrate work. Distribution remains launch-critical
unless product scope changes; NSR is now a completed archived baseline that
other L2 plans build beneath. Cloudflare is a strategic adapter expansion and
must not block launch unless the owner explicitly makes a Cloudflare surface a
launch criterion. The launch gate remains
`bash scripts/verify-launch-readiness.sh`, not the Cloudflare adapter verifier.

- **`distribution-plan.md`** — binary release, Homebrew/cask, Linux package
  mirror, and release-owned OCI image lanes (in flight). See the GA-sequencing
  section in that plan for the channel cutover order.
- **`nimbus-kv-foundation-plan.md`** (NKV0, bands F0..F5) — the KV primitive the
  Cloudflare KV wedge rides: a monolithic RESP-native store on `nimbus-storage`
  whose `TenantKvStore` persistence trait + redb impl land at **band F2**. Heads
  the NKV0..NKV6 program (collections / streams / transactions / Memcached +
  Workers-KV adapters / cluster are later phases). **NKV0 (through F2) is a hard
  predecessor of CFA3/CFA5.** Verifier `bash scripts/verify-nimbus-kv-foundation.sh`
  (9 conditions).
- **`archive/cloudflare-adapters-plan.md`** — completed CFA1..CFA9
  primitives-first baseline: KV primitive prerequisite, Workers KV REST +
  `env.NS` in a real Worker, minimal Workers runtime slice, single-node Durable
  Object substrate, operator docs, and verifier closeout. Future R2, D1,
  full-Workers API, production auth, or cluster-scale DO routing work needs a
  follow-on active plan rather than reopening the archived CFA baseline.
- **`archive/service-sandbox-node-reconciliation-plan.md`** (NSR) — completed
  workload-control spine (`WorkloadScheduler`/`WorkloadExecutor`, node agent,
  systemd transient units). Other plans bind *beneath* its executor; follow-on
  service/sandbox/node work needs a new active plan rather than reviving NSR.

### Phase 2 — Storage/isolate foundations, parallel (L1)

NC is complete and archived, but remains the predecessor artifact for NOS
byte-plane crypto. The remaining substrate extractions are independent crates
and run concurrently, with one ordering constraint on the shared `extensions.rs`
bootstrap edits. If
NFS and NEG are active in the same window, first land a small runtime bootstrap
extension registry/slotting seam, then route the NFS `RealFs` replacement
(`extensions.rs:71`) and the NEG `fetch` binding (`extensions.rs:82`) through it
so neither plan hand-edits the same bootstrap list independently.
**`profile-aware-isolate-runtime` (PIR, Phase 5) is a third consumer of this same
seam:** PIR6's per-profile snapshot composition, node-snapshot consumption, and
lazy/tree-shaken extension init touch the same `extensions.rs` /
`snapshot_extensions` / `execution_extensions` lists, so it routes through the
registry too. Whichever of NFS / NEG / PIR touches `extensions.rs` first lands the
registry seam; the others consume it.

- **`nimbus-isolate-filesystem-plan.md`** (NFS) — **foundational.** NFS1
  creates `crates/nimbus-fs/` with the passthrough shell; NFS2 extends it with
  the mount resolver and `MemFsBackend`; NFS3 adds `CasReadOnlyBackend` over
  `nimbus-blob::BlobStore::get_stream`; NFS6 defines the `NimbusFsBackend`
  registration slot that NOS6 fills. The plan now carries a 2026-06-20
  exemplar audit (cap-std, Wasmtime, Deno, Kubernetes, Mountpoint for S3,
  Convex, workerd, Node) and adopts the canonical default: capability roots,
  gated/preopened mount views, backend-specific semantics, and explicit failure
  for unsupported POSIX operations — not a universal POSIX emulation layer.
  The second-pass audit adds Capsicum/CloudABI object-capability literature,
  WASI filesystem, `openat2`/securejoin/runc/crun, OCI masks/readonly paths,
  Deno `ext/fs` bypass guards, Emscripten FS, Bazel sandboxing, and Nix/iroh as
  reference input; NFS0 must close the
  handle-rooted passthrough strategy, virtual `realpath`/`readlink` output,
  symlink policy, internal `FsCaps` rights matrix, and Landlock/process
  hardening posture before code starts.
- **`nimbus-egress-gateway-extraction-plan.md`** (NEG) — PDP `nimbus-egress` +
  PEP `nimbus-proxy`. NEG5 (credential injection) keeps a minimal self-contained
  secret store as MVP, later sourcing values from secret-management (Phase 5).

### Phase 3 — Built on the foundations, parallel (L1 + L2)

- **`nimbus-s3-object-storage-plan.md`** (NOS) — **REVISED 2026-06-19:** NOS-A1
  (`nimbus-blob`, Seam A) is a leaf on `nimbus-core`; in this worktree the
  scaffold is applied from the preserved blob stash but still must be landed and
  verified before other lanes treat it as baseline. It does NOT wait on NFS (the
  old `nimbus-blob → nimbus-fs` edge reversed — `nimbus-fs` now consumes
  `BlobStore::get_stream`). Joins: NC before NOS-A1's *real* byte-plane AEAD
  (the skeleton stubs it; name-plane design is unblocked); `nimbus-blob` before
  **NFS-C3** (CAS-RO); **NFS6 before NOS-A7** (ObjectRwBackend fills the
  registration slot). The byte-plane envelope chain must source `nimbus-crypto`,
  not duplicate `nimbus-storage/src/encryption/`. NOS4 cloud writes are NEG5's
  first consumer.
- **`nimbus-sandbox-plan.md`** — Band B (`libkrun_session` backend) lands
  *beneath* the NSR `WorkloadExecutor`: NSR's `krun` `RunnerKind` maps onto the
  backend's `SandboxBackendKind`. Then Bands S → D → G.
- **`sandbox-disk-limit-enforcement-plan.md`** — SDL0 + SDL1 (project-quota
  detect + enforce/refuse), unblocking the SBR C3 fail-closed decision.
- **`krun-microvm-egress-enforcement-plan.md`** (KME) — lifts the fail-closed
  Linux-production krun execute path by retiring TSI-outbound for **passt** and
  making the conmon/crun microVM the **fourth NEG enforcement point**. Consumes
  the Phase 2 NEG foundation (needs `nimbus-egress` PDP + `nimbus-proxy` PEP —
  KME enforcement phases start after NEG1/NEG4; KME0/KME1 scaffold + spike run
  in parallel). Builds the passt PEP + passt-mode bind-address hook **once**,
  shared with `nimbus-sandbox-plan.md` Band B/D. Lifts the execute gate only;
  scale-to-thousands still needs Band S.

### Phase 4 — WASM runtime, gated on wasmtime (L3)

- **`wasmtime-backend-plan.md`** — activation gate **MET** (Locker fork Phase 5,
  2026-04-06); runtime-engine seam hardening is the next Step 0. Opens this
  phase: W1..W7.
- **`wasi-agent-capabilities-plan.md`** — re-scoped (Phase 0): owns the
  **process** primitive only; `nimbus:agent/filesystem` is the WIT projection
  over NFS (binder NFS5) and `nimbus:agent/http-client` over NEG (binder NEG6).
  A2 activation is therefore a **three-way join** — it needs wasmtime W3 (the
  component linker), NFS5 (the filesystem binder), and NEG6 (the http-client
  binder) all live, not wasmtime alone.

`layered-admission-control-plan.md` is **not** part of this phase — it is
demand-gated independently and now lives in Phase 5.

### Phase 5 — Demand-gated (L3 + L4)

Promote each only when a concrete consumer needs it; sequence within the phase:

- **`horizontal-scaling-plan.md`** — **the cluster substrate the next three
  plans ride, so it leads the phase.** Promotes the iroh+openraft design in
  `docs/private/architecture/horizontal-scaling.md` into bands HS0..HS7 (node
  identity + discovery + membership, durable mapping replication, gossip
  invalidation, isolate placement, microVM placement beneath the NSR executor,
  iroh-blobs content distribution, multi-tenancy cluster integration). Gated on
  the first real multi-node deployment; each consumer below ships a single-node
  MVP and only rides this substrate in cluster mode. HS5 binds beneath the NSR
  `WorkloadExecutor`; HS6 is the transport the NOS Mirror mode rides.
- **`secret-management-plan.md`** and **`service-identity-provider-auth-plan.md`**
  → **`agent-browser-service-plan.md`** (browser B5 and wasi-agent A2 are the
  named secret consumers; service-identity consumes `WorkloadIdentity`, not
  secret values, and carries a **hard** dependency on horizontal-scaling HS1 —
  cluster membership + node identity must exist before a node is trusted to mint
  provider-auth credentials; secret-management becomes the durable owner the NEG
  PEP sources credential values from, riding HS2/HS3 in cluster mode).
- **`layered-admission-control-plan.md`** — decision/experiment plan,
  demand-gated by an EO8 experiment report that names a specific slice;
  independent of every other lane (it is **not** wasmtime-coupled). PIR (below) is
  its runtime-pool sibling: PIR owns the in-process isolate-pool *mechanism* + the
  pool-sizing/autoscaling equation; layered-admission owns whether to add *gates*
  (EIB6 already names retained-runtime-pool pressure as an execution class).
  Neither blocks the other.
- **`archive/profile-aware-isolate-runtime-plan.md`** (PIR) — archived complete
  runtime-pool baseline (`bash scripts/verify-profile-aware-isolate-runtime.sh`:
  `Summary: 108 passed, 0 failed` on 2026-06-27), not active work. It remains the
  benchmark-driven successor to the archived `v8-locker-fork` plan and the
  reference for future isolate-pool follow-ons.
  PIR7L live adaptive autoscaling is a named PIR follow-on: it consumes PIR7's
  replay/host-budget proof plus REC's derived execution-plan seam before any
  live adaptive default can ship.
  Execute PIR7L only after REC closeout if reopened for live defaults; it must
  carry captured traces, shadow mode, p99/fairness/pressure evidence, operator
  rollback, and keep live adaptive defaults disabled unless separately promoted.
  PIR8 (strong-tier microVM restore/fork) still rides `nimbus-sandbox` Band S and
  consumes that primitive rather than duplicating it. Final architecture decision
  record: `profile-aware-isolate-runtime-final-architecture-plan.md`.
- **`archive/runtime-execution-classification-plan.md`** (REC) — archived
  complete companion to PIR, not a second optimization lane. REC owns the
  derived internal execution-plan seam that
  separates semantic function kind, runtime profile, effect/capability class,
  cooperative eligibility, pool authority, scheduling class, and budgets. PIR
  supplies the numeric validation method: benchmark lanes, RSS/current-RSS
  evidence, synthetic-await and CPU-bound traces, host budget metrics, and
  verifier discipline. REC closed on 2026-06-27 after
  `bash scripts/verify-runtime-execution-classification.sh` passed
  `24 passed, 0 failed`; the final fix made the REC4 runtime/codegen
  `nestedCalls` matrix explicit. Future scheduler/pool/adaptive-default work
  consumes REC's richer function/effect classes instead of reopening this plan.
- **`node-full-substrate-realm-plan.md`** (NFR) — PIR/REC follow-on for NodeFull
  substrate work. NFR1 is closed: Node20/22/24/26 now share one
  target-invariant NodeFull startup snapshot without collapsing execution
  authority. NFR remains active until
  `bash scripts/verify-node-full-substrate-realm.sh` reaches 0 failed
  (2026-06-27 audit: `10 passed, 5 failed`). NFR2 recorded the current blocker:
  Node realm leases need realm-scoped Node extension-JS replay plus a
  single-active lease authority protocol with generation checks before NFR3-NFR6
  can proceed.
  Any future realm lease work must consume REC's `RuntimeExecutionPlan`,
  `RuntimePoolAuthorityKey`, host-effect classification, async-provenance
  posture, and PIR7 host-governance vocabulary instead of creating a parallel
  authority or scheduler model. Realm pooling remains owner/tenant-affine,
  single-active-lease, and non-public until NFR6 records adoption, deferral, or
  rejection with security and benchmark proof.

Runtime plan-family execution order:

1. Align the plan index and cross-plan ownership rules.
2. Finish PIR baseline stabilizers, including target-specific pointer-compression
   default policy work and the approved NFR1 profile-keyed NodeFull startup
   snapshot follow-on.
3. REC0-REC5 are complete: scheduling, effect, authority, and host-governance
   facts now have one canonical runtime-owned seam.
4. PIR7L is complete as an operator-gated implementation. The original gate was:
   Execute PIR7L only after REC closeout when live adaptive autoscaling is the
   active request; require captured traces, shadow mode, p99/fairness/pressure
   gates, and rollback before actuation. Any future default promotion must still
   satisfy those workload-evidence gates.
5. Execute TFA when changing tenant/developer autoscaling UX or operator quota
   vocabulary. TFA owns the resource-first tenant envelope and inferred
   autoscaling contract; do not revive PIR7M's public `activation_warm`,
   `live_scaling` naming, explicit tenant `autoscaling` knob, or pool-first
   operator happy path.
6. Execute NFR2-NFR6 only after REC closeout, ending in a measured adoption,
   deferral, rejection, or blocker for NodeFull realm pooling.
- **`native-transport-evolution-plan.md`** — benchmark-driven, independent.
- **`windows-machine-support-plan.md`** — its macOS prerequisite is **already
  satisfied** (MAC1-MAC7 archived); deferred only because the plan itself has not
  been promoted, **not** gated on MAC5+/NSR5.

## Exploratory Drafts (Not Ready)

These documents capture candidate UX and architecture questions, but they are
not execution control planes. Do not target them with a `/goal`, verifier, or
implementation agent until an owner promotes the draft into an active plan with
closed scope, success criteria, and proof obligations.

- `docs/private/plans/nimbus-run-exec-exploration-plan.md`
  - exploratory draft for the possible explicit raw argv surface
    `nimbus run exec -- ...`. It keeps command/process execution separate from
    `nimbus run functions <name> [jsonArgs]`, preserves the NSR raw-command
    safety boundary, and records open policy, runner, quota, audit, stdin, and
    local-vs-remote questions before implementation.
- `docs/private/plans/nimbus-run-jobs-exploration-plan.md`
  - exploratory draft for `nimbus run jobs <name>` as a named, app-declared,
    operator-admitted workload template. It is intentionally separate from
    `run exec`, recurring schedules, and PIR7M function invocation, with a
    candidate function-backed MVP but unresolved config, status, admission, and
    scheduler integration questions.

## Active execution plans

- `docs/private/plans/archive/cloudflare-adapters-plan.md`
  - completed control plane (CFA0..CFA9) for the inbound **Cloudflare** adapter
    baseline (Workers / Workers KV / Durable Objects) on the same seam as the
    MongoDB/DynamoDB/Firestore/Convex adapters. Owner-ratified scope
    (2026-06-16; **re-architected primitives-first 2026-06-22**): Cloudflare
    adapters are thin compat surfaces over first-class Nimbus primitives, not
    foreign-runtime embeds. Landed baseline: `TenantKvStore` prerequisite from
    NKV0, Workers KV REST, `env.NS` end-to-end inside a real Worker, minimal
    Workers runtime slice, and a single-node Durable Object substrate with
    per-instance storage, lease fencing, alarms, and hibernation records.
    Contract source of truth remains
    `docs/private/plans/research/cloudflare-adapters-2026.md` (do not re-derive
    Cloudflare API contracts from memory; §9b records the runtime decision).
    Future work needs a new active plan. Cross-lane deps: **R2 → NOS Phase 3**
    (object-storage primitive); cluster-scale single-instance DO routing →
    **HS5** (`horizontal-scaling`); D1 over the existing SQLite/libSQL family
    is an independent follow-on. Closeout verifier:
    `bash scripts/verify-cloudflare-adapters.sh` (12 conditions).
- `docs/private/plans/connection-broker-plan.md`
  - **CB0..CB8 (proposed)** — a **coordinating** plan unifying inbound WebSocket
    hibernation (CFA8) with the outbound egress PEP (NEG) into one host-owned
    **connection broker** (home `nimbus-services`, beside the landed
    `SessionResource`/`SessionBroker`): the broker holds every long-lived socket
    in Rust and per-frame-invokes the isolate, so the warm slot frees while the
    connection stays open. **Two session classes** — Hibernatable (evict the
    isolate between frames: DO/socket.io) vs Pinned (host holds the socket,
    isolate stays warm: CDP/agent-browser; a live Chrome cannot be evicted).
    AMENDS rather than forks: NEG (extend the NEG3 fork hook to bind
    WebSocket-out; extend `nimbus-proxy` to hold long-lived sockets), CFA8 (the
    "resume semantics gap" is this plan), KME (one shared PEP; microVMs do not
    hibernate — DNAT inbound stays separate), HS5 (3 day-one cluster seams, key
    on session/DO-id not tenant), NKV0 F2 (`TenantKvStore` = DO state),
    secret-mgmt/identity (PEP injects creds; 3 orthogonal checks), agent-browser
    (the loudest consumer → Pinned class). Egress = default-NONE + tenant-ALLOW
    (per-app manifest) + operator-DENY (deny-overrides, Cilium-tier); ingress +
    the isolate's reply default-ALLOW at a separate layer. CB1/CB2 are NEG- and
    NKV-independent (buildable now against the landed `SessionBroker`); CB3 needs
    NKV0≤F2, CB5 needs NEG1/3/4, CB8 needs HS5. Exemplar lifts (Apache/MIT):
    `cloudflare/workerd` hibernation, `openworkers/*` pool+reset+ws_commands+
    OperationsHandler, `cloudflare/pingora` PEP, `cilium` tiers,
    `denoland/clawpatrol` creds (Convex is FSL — shape only). Gated on
    `bash scripts/verify-connection-broker.sh` (14 conditions, CB0-complete 3/11;
    CB9 = standard-`ws`/socket.io zero-config DX is the Vercel-parity surface,
    CB10 = Active-CPU-vs-resident metering). LOCAL-ONLY plan: lives under
    untracked `docs/private/`, never committed.
- `docs/private/plans/crate-architecture-modularity-plan.md`
  - **CAM0..CAM7 (active)** — pre-launch Rust crate architecture cleanup from
    the 2026-06-30 full-crate audit, revalidated against clean `main` at
    `54eeb9c57` after the NOS/KME/dependency PRs. Removes the
    `nimbus-cloud-functions` -> `nimbus-firebase` adapter dependency by lifting
    shared Firestore semantics to a provider-family seam; narrows accidental
    public surfaces in `nimbus` and `nimbus-server`; decomposes overloaded
    `nimbus-storage` traits and Postgres helper roots without collapsing the
    landed `nimbus-object-storage` module tree; splits OCI/container sandbox
    networking/runtime roots by concept-owned seams while preserving KME
    netns/PEP routing; moves runtime bootstrap JavaScript into named assets; and
    preserves the `nimbus-core` zero-I/O plus `nimbus-runtime`
    zero-workspace-dep invariants. `/goal` control-plane verifier is planned at
    `bash scripts/verify-crate-architecture-modularity.sh` (10 conditions) and
    is created by CAM0. Until CAM0 lands, a missing verifier is expected, not a
    pass. CAM0 also records the dedicated worktree
    `/Users/jack/.codex/worktrees/crate-architecture-modularity/nimbus`, the
    current NOS/KME verifier baselines, and `git worktree list --porcelain`.
    Implementation must happen on branch `codex/crate-architecture-modularity`
    in that dedicated worktree. LOCAL-ONLY plan: lives under untracked
    `docs/private/`, never committed.
- `docs/private/plans/nimbus-kv-foundation-plan.md`
  - **NKV0 (Foundation)**, first phase of the `nimbus-kv` program: a monolithic,
    natively Redis/Valkey-compatible (RESP2/RESP3) data-structure store that
    delegates durable persistence to `nimbus-storage` and owns the in-memory
    cache/tiering (durable-by-default + configurable cache; no-disk pure-memory
    mode like Redis). Memcached + Cloudflare Workers KV become adapters over it
    (the CFA KV wedge rides the `TenantKvStore` trait landed at **NKV0 F2**;
    fuller CF Workers KV consolidation onto the RESP surface is NKV1 — owner:
    "consolidate now"). Bands F0..F5: crate + RESP server (`redis-protocol` crate)
    + flat `TenantKvStore` capability in `nimbus-storage` (redb default) +
    cache/tiering
    scaffold + the Valkey-conformance harness, proven by a GET/SET/DEL/EXPIRE/
    INCR round-trip under RESP2+RESP3. Verifiable per-phase criteria = the
    **Valkey TCL suite (BSD-3) in external mode** + a redis-rs harness (Kvrocks
    `gocase` precedent); never lift Redis 7.4+ (SSPL/AGPL). Full architecture +
    the NKV0..NKV6 roadmap + the iroh/cluster reality are in
    `docs/private/plans/research/nimbus-kv-architecture-2026.md` (§6–§10, the
    contract source of truth). `/goal` gated on
    `bash scripts/verify-nimbus-kv-foundation.sh` (9 conditions). LOCAL-ONLY
    plan: lives under untracked `docs/private/`, never committed.
- `docs/private/plans/nimbus-isolate-filesystem-plan.md`
  - scaffolded control plane (NFS0..NFS7) for **NimbusFS**: the in-process
    (isolate + wasm) filesystem subsystem — one prefix→backend mount table, two
    binders (V8 `deno_fs::FileSystem` + WASI preopen), tier-gated backends
    (passthrough/memfs/CAS-RO/object-store-slot), and a fail-closed `FsCaps`
    capability model. Replaces `deno_fs::RealFs` at
    `crates/nimbus-runtime/src/runtime/bootstrap/extensions.rs:71` with a
    Nimbus-owned `FileSystem` whose passthrough default is byte-identical (zero
    Node-compat regression); the mount table is additive. Honors the
    `nimbus-runtime` zero-workspace-dep invariant via an injected
    `NimbusFsBackend` trait (HostBridge pattern): `nimbus-runtime` owns the
    trait/injection slot, while `nimbus-fs` owns the shell, table, backends, and
    `FsCaps`. **Foundational** plan of the filesystem/object-storage/egress
    portfolio derived from the `/tmp/nimbus-isolate-architecture.html`
    deep-dive; NFS-C3 consumes `nimbus-blob::BlobStore::get_stream`, and NFS6
    defines the registration slot that `nimbus-s3-object-storage-plan.md` NOS6
    fills. Includes a 2026-06-20 canonical exemplar comparison (cap-std,
    Wasmtime, Deno, Kubernetes, Mountpoint for S3, Convex, workerd, Node)
    that binds implementation to capability roots/preopens, explicit
    backend-specific semantics, and fail-closed unsupported operations. A
    second-pass audit adds Capsicum/CloudABI object-capability literature, WASI
    filesystem, `openat2`/securejoin/runc/crun, OCI masks/readonly paths, Deno
    `ext/fs` bypass guards, Emscripten FS, Bazel sandboxing, and Nix/iroh;
    NFS0 must close the handle-rooted
    passthrough strategy, virtual `realpath`/`readlink` output, symlink policy,
    internal `FsCaps` rights matrix, and Landlock/process hardening posture
    before code starts. Also
    includes the 2026-06-20 architecture audit: `nimbus-runtime` is interface
    only, `crates/nimbus-fs/` is the deep module, `nimbus-server` is wiring,
    complexity pockets are named with containment rules, each NFS band has
    concrete proof criteria, and the paste-ready `/goal` prompt drives the
    entire NFS0-through-NFS7 implementation, final verification, green branch
    CI, and PR rather than a single band.
    Coordinates the WASI binder with `wasi-agent-capabilities-plan.md`. `/goal`
    control-plane verifier is planned at
    `bash scripts/verify-nimbus-isolate-filesystem.sh` (10 conditions) and is
    created by NFS0. Until NFS0 lands, a missing verifier is expected, not a
    pass. LOCAL-ONLY plan: lives under untracked `docs/private/`, never
    committed.
- `docs/private/plans/nimbus-s3-object-storage-plan.md`
  - scaffolded control plane (NOS0..NOS7) for the **object-storage subsystem**:
    a two-plane object model (small manifest rides the commit log atomically;
    large bytes as content-addressed BLAKE3 chunks, idempotent and
    non-transactional), split across two new crates — the **byte-plane engine
    `crates/nimbus-blob/`** (the `ChunkStore` trait + its single `PackChunkStore`
    impl — explicitly **NOT** a per-store trait) and the **S3 surface
    `crates/nimbus-s3/`** — with `ObjectMetaStore` (the name/metadata plane) the
    one object trait added across all five stores via the existing macro pattern.
    The byte-plane crate is named `nimbus-blob` (opaque content-addressed bytes),
    reserving "object" for the name plane (`ObjectMetaStore`/`nimbus-s3`). An
    `s3s` S3 protocol surface, lifecycle + grace-window GC, four placement modes (Local / Mirror
    via changefeed / Tier / Cloud-primary over `object_store`), config seams
    (control-DB placement store + `TenantProviderConfig` + CLI + `NIMBUS_*`
    env), and the HTML §21 filesystem binder ("an S3 becomes a mount") that
    fills the NFS6 slot. **Full-subsystem** plan; depends on
    `nimbus-isolate-filesystem-plan.md` (NOS6 needs NFS6). License posture:
    `s3s`/`object_store`/`mountpoint-s3`/`fuse3`/`fuser` freely incorporable;
    **Garage is AGPL-3.0 and excluded** (prior art only, never linked). The
    control-plane verifier is planned at
    `bash scripts/verify-nimbus-s3-object-storage.sh` (12 conditions) and is
    created by NOS0. Until NOS0 lands, a missing verifier is expected, not a
    pass. LOCAL-ONLY plan: lives under untracked `docs/private/`, never
    committed.
- `docs/private/plans/nimbus-egress-gateway-extraction-plan.md`
  - scaffolded control plane (NEG0..NEG7), titled **Nimbus Network Egress
    Gateway Plan**, extracting a **tier-neutral egress gateway** from the
    container-exclusive proxy that already exists. Grounded findings:
    `SandboxEgressProxy` (`egress_proxy.rs:68`) has one production caller
    (`backends/container/runtime.rs:731`); the isolate's `fetch` is
    `deno_fetch::deno_fetch::init(Default::default())` at `extensions.rs:82` —
    gated by the coarse `deno_permissions` `allow_net` allowlist
    (deny-by-default in prod, loopback in dev), so isolate egress is **NOT
    unmediated**; the real gap is that the rich `SandboxEgressPolicy` (already
    tier-neutral + tested, `verify-enterprise-policy-egress` green) never reaches
    it. **Egress is a node-wide network concern, not a sandbox feature, so NEG
    adds two new crates split along the PDP/PEP seam** (Phase-1's single
    `nimbus-egress` was refined into a two-crate split 2026-06-16 per the
    OpenShell / Cloudflare-QUIC / ClawPatrol agentic-chokepoint analysis):
    **`crates/nimbus-egress/`** (the **PDP** — deps `nimbus-core` only, zero-I/O,
    V8-free, socket-free) owns the rich policy + decision engine + verdict + DLP
    *rules* + credential-injection *policy*; **`crates/nimbus-proxy/`** (the
    **PEP** — deps `nimbus-egress` + `nimbus-core` + transport) owns the
    tier-neutral `EgressProxy` forwarder (lifted from `SandboxEgressProxy`), the
    credential **secret store** + on-wire injection, DLP body enforcement,
    SSRF/IP-pinning, and the future QUIC/MASQUE transport. The `EgressGateway`
    trait **plus its seam types `EgressRequest`/`EgressAuthorization`** live in
    `nimbus-runtime` (zero-workspace-dep, HostBridge-shaped — fixes the latent
    bug of placing seam types in `nimbus-core`); `nimbus-server` implements the
    trait, bridging the runtime seam to the PDP decision and routing forwarding
    through the PEP; `nimbus-sandbox` wires the PEP's `EgressProxy`. **NOT**
    `nimbus-gateway` as a crate name (overloaded, connotes ingress —
    `EgressGateway` stays the runtime trait only). NEG binds isolate `fetch`
    (NEG3) + wasm (NEG6) + container (NEG2) to the same decision and adds
    credential injection + L7 DLP with secrets held only in the PEP (the
    ClawPatrol property). **Independent** plan of the portfolio; NOS4 cloud
    writes are NEG5's first consumer. The control-plane verifier is planned at
    `bash scripts/verify-nimbus-egress-gateway-extraction.sh` (10 conditions)
    and is created by NEG0. Until NEG0 lands, a missing verifier is expected,
    not a pass. LOCAL-ONLY plan: lives under untracked `docs/private/`, never
    committed.
- `docs/private/plans/krun-microvm-egress-enforcement-plan.md`
  - scaffolded control plane (KME0..KME6), titled **Krun MicroVM Egress
    Enforcement Plan**, lifting the **fail-closed Linux-production krun execute
    path**. Grounded findings: `KrunSandboxBackend` execute mode is unconditionally
    rejected at `crates/nimbus-sandbox/src/backends/krun/vm/start.rs:155` ("krun
    execute-mode is fail-closed until Nimbus has a packet-level egress enforcement
    path for libkrun TSI") because **TSI** socket-impersonation gives the rich
    `SandboxEgressPolicy` no host-side packet path to attach to; the container
    backend (`SandboxEgressProxy` at `egress_proxy.rs:68`, sole caller
    `container/runtime.rs:731`) is the only enforceable process-capable lane today;
    the originating EPS4b2b plan deferred the PEP and is archived, NEG is the wrong
    layer (L7 fetch, three substrates, krun not among them), and
    `nimbus-sandbox-plan.md` explicitly disclaims the conmon/crun service-microVM
    ("not owned here"). **Owner-ratified 2026-06-22: retire TSI-outbound for
    passt**; enforce on the host-side passt packet path bound to NEG's PDP, making
    the microVM the **fourth NEG enforcement point** (container/isolate/wasm/microVM,
    one decision; HTTP(S) forwarded through `nimbus-proxy`). Keeps the TSI *inbound*
    bind-address patch; adds the passt-mode inbound hook (fork-inventory §8 item 1).
    **Consumer** of NEG (`nimbus-egress` + `nimbus-proxy`); coordinates the passt
    PEP with `nimbus-sandbox-plan.md` Band B/D (build once). Lifts the execute gate
    only — scale-to-thousands still needs Band S. Touches the `nimbus-crun` fork
    (passt selection in `handlers/krun.c`) and possibly `nimbus-libkrun`; a
    `nimbus-crun` fork-health budget row is a KME0 precondition. The verifier is
    planned at `bash scripts/verify-krun-microvm-egress-enforcement.sh` (10
    conditions) and is created by KME0; until then a missing verifier is expected,
    not a pass. LOCAL-ONLY plan: lives under untracked `docs/private/`, never
    committed.
- `docs/private/plans/archive/nimbus-crypto-extraction-plan.md`
  - archived complete local baseline (NC0..NC4; verifier
    `10 passed, 0 failed` on 2026-06-27) extracting Nimbus's
    envelope-encryption-at-rest model from `crates/nimbus-storage/src/encryption/`
    (11 files, ~4,324 lines) into a **near-foundational `crates/nimbus-crypto/`**
    leaf (deps `nimbus-core` only + external crypto crates). Verified clean leaf:
    zero outward storage coupling, `redb_rotation.rs` is redb-named but
    redb-type-free. NC1 = pure move + repoint of storage, engine, facade, CLI,
    bench, and test crypto consumers, no `nimbus-storage` shim; NC2 = clean
    envelope API + revocable **crypto-shred** primitive (the NOS `tenant rm`
    consumer, including stale-handle failure after shred); NC3 = deterministic
    framed-blob AEAD trait + pinned `aws-lc-rs` backend proof path under a hard
    SIV-determinism invariant (dedup), while existing database/page encryption
    remains byte-compatible and random-nonce; NC4 = closeout. Request/token
    signing stays in `nimbus-auth` (peer crate, named boundary). **Required
    predecessor of NOS byte-plane encryption** — `nimbus-blob` sources crypto
    from `nimbus-crypto` instead of `nimbus-storage`. Code plan → PR branch
    `codex/nimbus-crypto-extraction`; NC0 also records the AWS Encryption SDK,
    Tink, restic, kopia, MongoDB CSFLE, local Convex, and local iroh-blobs
    exemplar comparison so the implementation adopts crypto-materials/keyring
    trace, typed algorithm-suite/framed-session, provider-negative, and
    range-proof patterns instead of a shallow AEAD wrapper; the control-plane
    verifier is planned at
    `bash scripts/verify-nimbus-crypto-extraction.sh` (10 conditions) and is
    created by NC0. Until NC0 lands, a missing verifier is expected, not a pass;
    final readiness requires the NC branch/PR CI green against the current `main`
    base. LOCAL-ONLY plan: lives under untracked `docs/private/`, never
    committed.
- `docs/private/plans/nimbus-sandbox-plan.md`
  - proposed unified sandbox plan for every Nimbus workload on the
    `nimbus-libkrun` fork via capability profiles (lambda / desktop / gpu).
    Four bands ship semi-independently with per-band `/goal` prompts and
    per-band success criteria: **Band B** — shared `libkrun_session`
    backend skeleton, profile dispatch trait surface, and `nimbus-guest`
    PID-1 binary (B0..B2). **Band S** — Linux-KVM-only snapshot/restore
    + MAP_PRIVATE fork-on-demand primitive port into `nimbus-libkrun`
    (S0..S5, ~7,050 LoC net new + ~2,400 LoC tests = ~9,450 LoC across
    the fork; lifts Firecracker snapshot patterns and zeroboot's
    MAP_PRIVATE primitive, both Apache-2.0). **Band D** — desktop
    profile: tenant-isolated sessions with headless compositor + frame
    capture + virtio-input RPC; operator surface
    `nimbus sandbox session start/list/inspect/stop/screencast/input`
    (D1..D10). **Band G** — GPU profile: Venus default (Linux + macOS
    via krunkit + virglrenderer + MoltenVK; ~75-80% native Metal on
    Apple Silicon per libkrun #353/#377), virtio-gpu native-context
    opt-in for trusted AMD/Adreno tenants with strict ioctl filters,
    CUDA routed to a separate NVIDIA fleet, ROCm deferred (G1..G13).
    Subsumes archived FSI, CUS, and GAW plans plus the standalone
    snapshot-port plan (all archived 2026-05-27). Decision baseline
    (D1-D12) at `docs/private/plans/research/vmm-landscape-2026.md`. Per-host
    topology: Linux production runs direct libkrun-on-KVM microVMs;
    macOS dev runs one outer machine-os VM with crun containers per
    workload (D11/D12, no nested microVMs on macOS).
  - **Supporting research and decision baselines (context for this plan, not
    standalone execution plans):**
    - `docs/private/plans/research/vmm-landscape-2026.md` — VMM landscape
      decision baseline (D1-D10, ratified 2026-05-27): one nimbus-libkrun
      family carries every workload via capability profiles; Firecracker
      snapshot/restore patterns and the zeroboot MAP_PRIVATE fork primitive
      are lifted into the fork (Band S). Records the May 2026 state of
      Firecracker, libkrun, Cloud Hypervisor, and the zeroboot derivative.
    - `docs/private/plans/research/nimbus-libkrun-fork-inventory.md` — fork
      inventory satisfying CUS0 + GAW0: one functional patch (TSI bind-address
      hook) plus build/CI commits, an MIT-licensed per-component muvm
      Lift/Reference/Skip disposition table, the `krun_set_gpu_options2` call
      shape, an AI-agent capability map, and the anticipated-patch list.
    - `docs/private/plans/research/computer-use-capabilities-audit.md` —
      desktop-profile capability audit against Computer Use, Operator,
      browser-use, OSWorld, and AgentBench across six dimensions; identifies
      seven v0 must-haves wired into CUS1..CUS8 plus eleven follow-on phases.
- `docs/private/plans/archive/service-sandbox-node-reconciliation-plan.md`
  - archived completed control-plane baseline that bridges the completed SDK resource model, the
    completed systemd D-Bus binding, Compose-backed services, sessions, and
    machine-os execution into one node-reconciled service/sandbox architecture.
    Keeps `nimbus dev`, `nimbus start`, `nimbus compose`, and `nimbus node`
    distinct as CLI workflows while converging them on one desired-state,
    scheduler/executor, node-agent, systemd-transient-unit, Compose-lifecycle,
    leased Compose-template, and session-channel model without stranding the
    current local sandbox executor during the migration.
    Makes the macOS machine-os Linux guest a node for execution purposes:
    host Nimbus keeps server/storage/runtime/resource authority, while guest
    systemd supervises container workload units through an explicitly chosen
    machine-API compatibility shape. Also reserves the future Wasm/Wasmtime
    extension point without collapsing runtime invocation isolates into SDK
    sandbox resources or pre-choosing public Wasm sandbox profile spelling.
    Runtime isolates may lease only exact-granted, operator-admitted sandbox
    templates by tenant/name; raw Compose admission and host-published template
    ports stay outside the runtime surface. The template DX favors stock Compose
    plus app-owned `nimbus.yaml` admitted against operator-owned
    `nimbus.policy.yaml`: Compose carries image/build, healthcheck, resources,
    profiles, tty/stdin, secrets, and internal networks; `nimbus.yaml` owns app
    requests, template aliases, service uses, channel expectations, and SDK
    handles; policy owns tenant entitlements, quotas, allowed runtimes and
    profiles, image/provenance requirements, egress, secrets, service
    availability, grants, and risk acceptance.
    The verifier is `scripts/verify-service-sandbox-node-reconciliation.sh`
    and proof files live under
    `docs/private/plans/proof/service-sandbox-node-reconciliation/`.
    Final local baseline on 2026-06-27 is 21/21 green after the NSR1
    TargetContext closeout and NSR11 archive closeout; proofs are
    `docs/private/plans/proof/service-sandbox-node-reconciliation/nsr1-target-context-closeout.md`
    and
    `docs/private/plans/proof/service-sandbox-node-reconciliation/nsr11-final-closeout.md`.
    Remaining broad-doc drift is outside this plan: `bash scripts/check-docs.sh`
    still reports stale `docs/source-map.md` encryption paths, and
    `npm run docs:validate-refs:strict` still reports two pre-existing Node
    published-reference links.
- `docs/private/plans/distribution-plan.md`
  - canonical plan for distributing nimbus across all channels: install
    script, apt repo (Debian/Ubuntu), COPR (Fedora), Homebrew + machine VM
    (macOS via krunkit/libkrun), binary tarballs, container images, cloud
    VM images (AWS AMI, GCP). Channel 4 covers the macOS machine VM
    architecture (krunkit, guest image, control channel, virtiofs, gvproxy).
    Activation gate met on 2026-04-13 (microVM service baseline `done`);
    binary release, Homebrew/cask, Linux package mirror, and release-owned OCI
    image lanes are in flight under this plan.
## Current Reference Baselines

Completed execution plans usually live under `docs/private/plans/archive/`. Some
local-only completed baselines may remain under `docs/private/plans/` until an
explicit archive move. Use current architecture and operating docs first; open
archived plans only when you need historical execution detail.

- `docs/private/plans/archive/nimbus-function-source-visibility-plan.md`
  - completed local control-plane baseline (FSV0-FSV9, closed 2026-06-18).
    Shipped content-addressed source packages, module records, source-package
    CAS storage, deploy capture, hash-verified source reads, console Source tab
    rendering, structural code-navigation foundation, TS-compiler hover/type
    metadata, and deploy-time typecheck modes. Verified with
    `bash scripts/verify-nimbus-function-source-visibility.sh`: 21 passed,
    0 failed. Treat as current baseline, not active roadmap work; create a
    follow-on plan for stack-trace remap, called-by completion, or deeper
    cross-file type navigation.
- `docs/private/plans/archive/tenant-function-autoscaling-plan.md`
  - completed local control-plane baseline (TFA0-TFA6, archived 2026-06-27).
    Shipped inferred tenant function autoscaling from `preset` plus
    `min_warm`/`max_warm`, removed public `activation_warm`, `live_scaling`,
    explicit tenant `autoscaling`, and operator `allow_live_scaling`, kept live
    adaptive-controller actuation operator-only, and routed start/dev/node/
    explain/validate/run-functions through one admission seam. Verified with
    `bash scripts/verify-tenant-function-autoscaling.sh`: 32 passed, 0 failed.
    Future scaling UX changes need a follow-on plan.

- `docs/private/plans/archive/final-nimbus-release-readiness-plan.md`
  - completed release-readiness cycle (closed 2026-05-26; D6 OCI/GHCR
    follow-up 2026-05-27). Tied the standardized fork releases back into the
    `nimbus/nimbus` product release: fork health, released fork pins, clean
    release worktree, version/changelog/lockfile consistency, full local
    verification, release-binary proof, tag-driven GitHub release, downloaded
    artifact re-verification, and the published OCI image with digest/
    signature/provenance/SBOM/smoke evidence. A completed audit cycle, not a
    standing gate: promote a fresh launch-gate plan when real launch work
    begins.
- `docs/private/plans/archive/deno-rusty-v8-upstream-alignment-plan.md`
  - superseded upstream-alignment plan (archived 2026-06-16). Targeted deno
    `v2.8.1` / rusty_v8 `v149.2.0` as a prerequisite for the next NDS3 wave;
    overtaken by the merged NDS work (PR #10), which advanced the fork stack
    past those targets to deno `v2.8.3-nimbus.70` / rusty_v8
    `v149.4.0-nimbus.2`. Its fork-vs-upstream anti-duplication audit framing
    remains a sound principle for any future reconciliation wave.
- `docs/private/plans/archive/storage-engine-quality-and-mvcc-plan.md`
  - completed storage-engine MVCC quality baseline (SEQ0..SEQ14, closed
    2026-06-16 via merged PR #13). Typed MVCC semantics, versioned
    registry snapshots, historical point/scan/index reads, retention GC,
    PITR, changefeed/CDC, generated + deterministic cross-backend
    conformance, and operator diagnostics across redb/SQLite/Postgres/
    MySQL/libSQL. Verifier `scripts/verify-storage-engine-quality-and-mvcc.sh`
    20/0. Post-closeout challenge prompt at
    `docs/private/plans/prompts/storage-engine-quality-and-mvcc-post-closeout-architecture-review.md`.
- `docs/private/plans/archive/node-default-runtime-support-hardening-plan.md`
  - completed Node default-runtime hardening baseline (NDS, closed
    2026-06-16 via merged PR #10, Cycle 37). `v8_isolate_required` at
    0 gaps / 100% on node22 (2363/2363), node24 (2400/2400), node26
    (2092/2092); repinned to `nimbus/deno v2.8.3-nimbus.70` +
    `rusty_v8 v149.4.0-nimbus.2`; public shim/boundary inventory shipped.
    The local plan ledger is frozen on the pre-scrub divergent base, so
    PR #10 is the authoritative completion evidence; owner reconciliation
    of the local plan/proof corpus against the post-scrub base is a
    separate housekeeping task.
- `docs/private/plans/archive/nimbus-capability-segregation-plan.md`
  - completed capability-segregation baseline (CB0..CB9, closed 2026-06-12).
    Engine `Service` -> `Engine` rename, exact-grant service authority,
    adapter context shortcut removal, ungranted-isolate service-op/snapshot
    absence, Bun/JSC fail-closed parity, per-tier permission segmentation,
    `@nimbus/nimbus` + `/transports/rest` split, principal-class route
    policy. Verifier `scripts/verify-nimbus-capability-segregation.sh` 10/0.
- `docs/private/plans/archive/nimbus-sdk-resource-model-plan.md`
  - completed `@nimbus/nimbus` services/sandboxes/sessions/runtime-isolate
    resource-model baseline (SRM0..SRM6, closed 2026-06-09). Canonical
    vocabulary at
    `docs/private/architecture/sandbox/service-sandbox-session-model.md`.
    Verifier `scripts/verify-nimbus-sdk-resource-model.sh` 23/0.
- `docs/private/plans/archive/service-backend-and-sandbox-spec-refactor-plan.md`
  - completed pre-launch service/sandbox type-vocabulary refactor (SBR0..SBR6,
    closed 2026-06-09, re-verified post-drift 2026-06-15). `ServiceBackend`,
    `ServiceBackend::Sandbox(SandboxSpec)`, `SandboxSpec.root`, and a single
    `SandboxBackend::start(SandboxSpec)` lifecycle path.
- `docs/private/plans/archive/architecture-cleanup-plan.md`
  - completed June-12-2026 architecture cleanup baseline (AC0..AC6, closed
    2026-06-12). Private-plan route normalization, proof-root thinning,
    shared resource-control authorization seam for services/sandboxes/
    sessions, manifest-backed root SDK control-plane routes, concept-owned
    runtime-capability internals.
- `docs/private/plans/archive/docs-agents-group-and-value-ladder-plan.md`
  - completed first docs-site wave after `nimbus-docs-site` (DA0..DA6,
    closed 2026-06-11). Promoted Agents to the sixth public docs group,
    rewrote the landing value ladder, added the deploy tutorial and the
    title-uniqueness gate. Gated on `scripts/check-docs.sh` +
    `scripts/verify-nimbus-docs-site.sh`.
- `docs/private/plans/archive/nimbus-dev-adapter-autodetection-plan.md`
  - completed dev-experience baseline (DX bands A/F/W/L/D, DXA0..DXD2,
    closed 2026-06-12; PR #15 merged to main at `1c223ab69`). `nimbus dev`
    autodetects every adapter from declared project signals — Convex,
    Firebase (fail-closed import-coverage scan gates the only app-mutating
    path), MongoDB-driver, and DynamoDB-SDK apps — and both `nimbus dev`
    and `nimbus start` serve every adapter surface by default with
    `--no-firestore` / `--no-mongodb` / `--no-dynamodb` opt-outs (D6/D7/D8).
    Wire surfaces get generated persistent dev credentials under
    `.nimbus/dev/` (0600; never a no-auth listener; MongoDB listener
    loopback-only always) and Nimbus-owned-keys-only `.env.local` handoff;
    detection is live (manifest watch, enable-only listener spawn that
    survives the main listener and subscriptions). Decisions D1–D8
    recorded in the plan. Gated on
    `bash scripts/verify-nimbus-dev-autodetect.sh` (14 conditions, all
    green). Supersedes the never-executed
    `archive/firestore-client-app-dev-mode-plan.md`.
- `docs/private/plans/archive/launch-readiness-plan.md`
  - completed launch-readiness baseline (LR0..LR13, closed 2026-06-11).
    Closed the 13-item docs-truth gap list: deploy admin handshake (CLI +
    dev watch), api_key surface removal, explicit-once rotation gate with
    advisory age, configurable CORS, CLI wiring for Firestore/MongoDB/
    DynamoDB, rest.ts parity guard (3-sided manifest), in-server TLS for
    the main listener, nimbus backup on SEQ8 archives, shipped hardened
    systemd unit, live apt channel + release->distribution dispatch fix,
    hidden node workload executor (TSB14 lift), boot.rs naming cleanup. Also restored
    a 3-days-red main and executed the docs/private untrack + history
    scrub directive. `/goal` control plane gated on
    `bash scripts/verify-launch-readiness.sh` (14 conditions, all green).
- `docs/private/plans/archive/nimbus-docs-site-plan.md`
  - completed public documentation site baseline (DOC0..DOC13, closed
    2026-06-10). Shipped nimbusdocs.com: Astro 6 + Starlight on Cloudflare
    Workers (Static Assets) via `.github/workflows/docs.yml` with per-PR
    preview deployments, Markdown authored in the five public `docs/`
    groups (Get started · Developers · Operators · Concepts · Reference,
    hybrid persona×Diátaxis IA), llms.txt / llms-full.txt / llms-small.txt
    build artifacts, the full `docs/` restructure (all internal working
    state under `docs/private/`, the single never-published home), the
    public source-verified `concepts/architecture/` rewrite +
    `ARCHITECTURE.md` contributor map, `scripts/check-docs.sh` (dead links
    + source-map resolution + private fence), the DOC11 README/repo
    front-door refactor + repo metadata, the `.agents/skills/docs`
    authoring skill, and the DOC13 staging retirement sweep. `/goal`
    control plane gated on `bash scripts/verify-nimbus-docs-site.sh`
    (17 conditions). Deferred owner tasks: `www.` 301 → apex and the
    GitHub social-preview image (both web-UI/DNS-only).
- `docs/private/plans/archive/server-crate-extraction-completion-plan.md`
  - completed final server crate extraction wave (FCE0-FCE10, closed
    2026-05-28). Extracted `nimbus-artifacts`, `nimbus-provenance`,
    `nimbus-services`, `nimbus-operator`, `nimbus-mongodb`,
    `nimbus-firebase`, `nimbus-cloud-functions`, `nimbus-convex`, and the
    thin feature-gated `nimbus-adapters` facade while keeping route mounting,
    listener lifecycle, `AppState` construction, shutdown, and global
    composition in `nimbus-server`. `/goal` control plane gated on
    `bash scripts/verify-server-crate-extraction-completion.sh`.
- `docs/private/plans/archive/server-seam-extraction-readiness-plan.md`
  - completed readiness baseline after
    `docs/private/plans/archive/nimbus-system-bridge-adapters-extraction-plan.md`
    (SSE0-SSE7, closed 2026-05-28). Prepared the remaining server seams for
    honest extraction without creating decorative crates: per-adapter readiness
    in MongoDB → Firebase/provider-family → Cloud Functions → Convex order,
    artifact effects, provenance, services, and operator/local admin. `/goal`
    control plane gated on
    `bash scripts/verify-server-seam-extraction-readiness.sh`.
- `docs/private/plans/archive/nimbus-system-bridge-adapters-extraction-plan.md`
  - completed first server extraction wave (SBA0-SBA8, closed 2026-05-28).
    Extracted `nimbus-system`, `nimbus-bridge`, `nimbus-auth`, and
    `nimbus-license`; rejected an aggregate adapter crate until per-adapter
    readiness was proven; and recorded follow-on decisions for artifacts,
    provenance, operator, and services. `/goal` control plane gated on
    `bash scripts/verify-server-system-bridge-adapters-extraction.sh`.
- `docs/private/plans/archive/tenant-and-node-crate-extraction-readiness-plan.md`
  - completed tenant/node extraction readiness wave (TNE0-TNE5). Split
    artifact verifier host effects out of the tenant domain, extracted
    `nimbus-tenant`, added a production `NodeWorkloadReconciler`, extracted
    `nimbus-node`, and closed with
    `bash scripts/verify-tenant-node-extraction-readiness.sh`.
- `docs/private/plans/archive/tenant-domain-and-node-enforcement-boundary-plan.md`
  - completed tenant-domain and node-enforcement boundary baseline
    (TSB0-TSB14). Renamed the server tenant domain while keeping
    `TenantIsolation*` security names, added local enforcement and
    host-lifecycle seams, documented node lifecycle and Quadlet export
    roles, and recorded the predecessor deferrals that TNE later closed.
- `docs/private/plans/archive/node-lts-runtime-trust-plan.md`
  - completed execution record for the Node LTS runtime trust wave (NLRT0-NLRT11,
    closed 2026-05-28). Added a data-driven Node LTS lane registry, truthful
    per-lane runtime metadata, Deno fork provenance and upstream-policy gates,
    generated equal-lane public evidence, fixture provenance automation,
    harness timeout/watchpoint diagnostics, separated local-dev versus
    production Node permission profiles, and active-LTS canary/oracle evidence
    for Node22 and Node24. `/goal` control plane gated on
    `bash scripts/verify-node-lts-runtime-trust.sh`.
- `docs/private/plans/archive/node-faas-runtime-compatibility-plan.md`
  - completed execution record for the Node FaaS runtime compatibility wave
    (NFRC0-NFRC13, closed 2026-05-28). Made Node24 the product default,
    kept Node22 as supported Maintenance LTS, kept Node20 legacy-grace only,
    added Node26 Current/non-LTS lane-local evidence, vendored and classified
    current official fixture corpora, added real Convex app and SDK canaries,
    added host-heavy service/microVM diagnostic canaries, generated Deno-style
    public support docs, added release-train drift automation, and wired
    PR/nightly CI gates. `/goal` control plane gated on
    `bash scripts/verify-node-faas-runtime-compatibility.sh`.
- `docs/private/plans/archive/binary-embedded-package-distribution-plan.md`
  - completed binary-embedded package distribution wave (BPD0-BPD8, closed
    2026-05-31). Keeps every `packages/*` workspace package `"private": true`
    and distributes the JS surfaces through the `nimbus` binary instead of npm:
    `rust-embed` embeds dependency-closed `dist/` payloads (version-locked via
    `build.rs`, checksummed), provisioned into apps under a gitignored
    `.nimbus/packages/*` via `file:` specifiers; the whole default Convex
    codegen surface (schema/server/http/auth.config) runs in-binary and offline
    (auth.config via the compile-time AST interpreter, not esbuild), while
    Cloud Functions is the one out-of-contract surface on the external-Node
    runner; an explicit `nimbus packages provision` command serves existing
    client apps. `/goal` control plane gated on
    `BPD_FULL=1 bash scripts/verify-binary-embedded-package-distribution.sh`
    (27 conditions). Rides under `docs/private/plans/distribution-plan.md`.
- `docs/private/plans/archive/nimbus-assets-crate-plan.md`
  - completed asset-catalog extraction wave (AE0-AE6, closed 2026-06-05).
    Introduced the private `nimbus-assets` crate as the single owner for
    in-scope production JS package payload bytes, operator UI/auth static
    bytes, and init/machine templates while keeping provisioning, codegen,
    routing, auth/session, rendering, and release policy in their behavior
    owner crates.
- `docs/private/plans/archive/node-dbus-client-binding-plan.md`
  - completed Node-side systemd D-Bus binding wave (NDB0-NDB7, closed
    2026-05-29 via PR #3). Attached `lucab/zbus_systemd` (pin `=0.26000.0`)
    and direct `zbus` to the `SystemdDbusClient` trait with a live
    `ZbusSystemdClient`: `Manager.Subscribe` + signal-correlated `JobRemoved`
    completion (no polling), centralized `OwnedValue` property encoding,
    `nimbus_core::Error` `Transport`/`NotFound` taxonomy, Linux-gated
    `systemctl --user` integration tests proven green in the
    `node-dbus-integration` CI lane, and `systemd-dbus` default +
    `SystemdTransientUnitBackend::linux_systemd_default()`. Does not wire a
    production `NodeWorkloadReconciler` caller (TSB14 deferral stands).
    `/goal` control plane gated on
    `bash scripts/verify-node-dbus-binding.sh`.
- `docs/private/plans/archive/storage-architecture-trust-hardening-plan.md`
  - completed execution record for the storage architecture trust-hardening
    wave (SATH0-SATH11, closed 2026-05-27). Added the durable tenant event
    journal for document, schema, table lifecycle, index lifecycle, scheduler,
    and replay-affecting state; retention-gated hard delete; first-class read
    visibility; storage capability and health diagnostics; backend
    format/version gates; shared table lifecycle transition rules;
    generated/metamorphic cross-backend conformance; and operator evidence.
    `/goal` control plane gated on
    `bash scripts/verify-storage-architecture-trust-hardening.sh`.
- `docs/private/plans/archive/ci-wall-acceleration-plan.md`
  - completed execution record for the CI Wall Acceleration wave
    (CW0-CW5, closed 2026-05-23). Attacked the post-CA wall poles on
    `main` (23.6m on `32951ee7`): Server Verification Harness (12.7m),
    Rust Workspace Tests (15.7m), External Provider Integration Tests
    (14.6m), warm-sccache itself (10.2m). CW1 sharded the verification
    harness corpus per surface via `NIMBUS_HARNESS_SHARD=N/M` (server
    fans out 4 ways across its 7-case transport-liveness corpus,
    engine 2 ways, storage + runtime single-shard). CW2 sharded
    `Rust Workspace Tests` into a 3-way `cargo-nextest --partition
    hash:N/M` matrix backed by Makefile env-var forwarding. CW3 split
    `External Provider Integration Tests` into a per-provider matrix
    (postgres / mysql / libsql) with `NIMBUS_PROVIDER_FILTER`
    script-side dispatch and `docker run` per-fixture startup gated on
    `if: matrix.provider == ...`. CW4 dropped `--tests` from
    `warm-sccache`'s `cargo check --workspace` so test-binary compile
    pays inline in the already-sharded downstream test jobs; deferred
    a bespoke per-target cache layer because Swatinem v2 already
    caches `target/`. Canonical contract lives in
    `docs/private/operating/ci-modernization.md` ("PR critical-path
    acceleration" section). Promote a new active plan before another
    PR-wall acceleration wave.
- `docs/private/plans/archive/coverage-acceleration-plan.md`
  - completed execution record for the Coverage Acceleration wave
    (CA0-CA5, closed 2026-05-22). Cut PR CI Coverage's critical
    path from 24m27s `cargo llvm-cov -j 1 --workspace` to a 3-shard
    fan-out + `cargo llvm-cov report` reducer (lanes: `server`,
    `engine`, `rest`) on `-j 4` under mold. Installed `mold` in the
    `setup-rust-cached` composite via `CARGO_TARGET_*_RUSTFLAGS=
    -C link-arg=-fuse-ld=mold` (the `LINKER=mold` shape was tried
    first and failed with `fatal: unknown -m argument: 64`; the
    proof doc captures the failure mode). Migrated the 5 inline
    `dtolnay/rust-toolchain` + `Swatinem/rust-cache` sites in
    `release.yml` into the composite with a new `save-cache: always`
    input so release tag builds save per-target caches without
    violating CC9's `save-if: refs/heads/main` PR-poison invariant.
    The Windows release-build pole (70m12s of 76m35s release wall:
    vendored OpenSSL via Strawberry Perl, V8 prebuilt link via
    `link.exe`, cold-target `cargo build --release`) is documented
    as deferred scope for a future release-acceleration plan.
    Canonical contract lives in `docs/private/operating/ci-modernization.md`
    (the "Coverage and release acceleration" section). Promote a
    new active plan before another coverage / release acceleration
    wave.
- `docs/private/plans/archive/tenant-isolation-control-plane-plan.md`
  - completed execution record for making production tenant isolation explicit
    across compute, networking, storage, HostBridge/runtime grants, sandbox
    artifacts, volumes, images, secrets, cleanup, and resource quotas (closed
    2026-05-22). Added the server-owned `TenantIsolationContext`, tenant-owned
    sandbox roots and volumes, fail-closed network/image/secret admission,
    runtime grant/tier admission, storage/API tenant authorization, microVM
    resource quotas, and the two-tenant proof harness. Future tenant-isolation
    waves should start from current architecture docs plus this baseline and
    promote a new active plan before changing the contract.
- `docs/private/plans/archive/tenant-isolation-enterprise-hardening-plan.md`
  - completed follow-on execution record for turning the tenant-isolation
    control-plane baseline into an enterprise-grade trust surface (closed
    2026-05-22). Added typed admission decision records, PDP/PEP separation,
    reusable conformance, read-only drift/audit scanning, Linux hard-quota
    proof, admitted workload identity, image provenance/signature policy,
    structured redacted audit events, and customer/operator readiness docs.
    Current posture starts from `docs/private/tenant-isolation.md`,
    `docs/private/operating/tenant-isolation.md`, and the architecture docs linked
    there.
- `docs/private/plans/archive/local-dev-canonicalization-plan.md`
  - completed execution record for the local-dev canonicalization wave
    (LD0-LD7, closed 2026-05-21). Made `nimbus-server`'s cross-toolchain
    dependency on the `nimbus-ui` JS build explicit and self-healing
    via the Makefile's UI dependency graph: `UI_DIST_INDEX` /
    `UI_CODEGEN_SENTINEL` are prereqs on every workspace lane that
    compiles `nimbus-server` (`check`, `test`, `clippy`, `ci-required`,
    `verify-desktop-ui`, `test-rust-workspace`, `test-rust-docs`).
    `crates/nimbus-server/build.rs` no longer emits a stub HTML and
    errors actionably when `packages/nimbus-ui/dist/index.html` is
    absent in any profile. `.github/workflows/ci.yml` dropped all seven
    inlined `npm run codegen` / `npm run build -w packages/nimbus-ui`
    steps and routes through the Make targets instead. Build contract
    documented at `docs/private/operating/local-dev.md`. Fresh-clone proof at
    `docs/private/plans/proof/local-dev-canonicalization/`. `/goal` control
    plane gated on `bash scripts/verify-local-dev-canonicalization.sh`
    (ten conditions). Future local-dev / build-graph waves must
    promote a new active plan.
- `docs/private/plans/archive/ci-caching-canonicalization-plan.md`
  - completed execution record for the CI caching canonicalization wave
    (CC0-CC9, closed 2026-05-22). Layered sccache (per-rustc-call
    content-addressed cache via `mozilla-actions/sccache-action`) on
    top of the existing Swatinem floor across every Rust job in
    `.github/workflows/ci.yml`, `.github/workflows/desktop-ui.yml`, and
    `.github/workflows/node-compat-nightly.yml`. Rotated all Swatinem
    `shared-key` values `-v1 → -v2` to force a clean dep tree, gated
    `save-if` on `refs/heads/main` so PRs cannot poison the main-branch
    pool, and (CC9) raised the `sccache-action` pin floor to `v0.0.10`
    plus retracted CC3's wrong-headed `save-always: true` after the
    GHA cache v1 → v2 migration broke every Rust job on the original
    `v0.0.6` pin.
    Inserted two leader jobs: `ui-artifacts` (one canonical SPA build
    consumed by harness + coverage via `actions/download-artifact@v7`)
    and `warm-sccache` (serial cold-start that populates the sccache
    pool before harness/coverage fan out). Kept the `cargo llvm-cov
    -j 1` constraint from LD7 unchanged (sccache shortens compile
    time, not link time; rust-lld bus-error risk unchanged) and added
    `--no-doc-tests` to the Coverage `cargo llvm-cov` invocation
    (doc-tests still execute via `make test-rust-docs` in
    `rust-workspace-tests`; CC7 only drops coverage measurement).
    Canonical caching contract is `docs/private/operating/ci-caching.md`.
    `/goal` control plane gated on
    `bash scripts/verify-ci-caching-canonicalization.sh` (twelve
    conditions). Proof bundle at
    `docs/private/plans/proof/ci-caching-canonicalization/`. Future CI caching
    / sccache / Swatinem waves must promote a new active plan.
- `docs/private/plans/archive/ci-modernization-plan.md`
  - completed execution record for the CI modernization wave (CM0-CM8,
    closed 2026-05-22). Extracted the duplicated Rust toolchain +
    sccache + Swatinem cache + googlesource bootstrap into a single
    composite action at `.github/actions/setup-rust-cached/` so the
    next sccache-action pin bump becomes a 1-line edit instead of
    the 12-site sweep CC9 had to do. SHA-pinned every non-`actions/*`
    `uses:` reference (including the codeql-action sub-path refs) with
    a `# vX.Y.Z` version-name comment, pinned every `runs-on:
    ubuntu-latest` to `ubuntu-24.04`, dropped the
    `actions/create-github-app-token@v3.2.0` patch over-pin to `@v3`
    during the CI modernization wave. The release workflow later
    re-pinned those token steps to `@v3.2.0` because current
    `actionlint` validates `client-id` cleanly for that exact ref while
    still reporting stale legacy `app-id` metadata for the `@v3` major
    ref; keep `client-id` as the canonical input.
    instrumented four high-value jobs (deny, coverage,
    rust-gate-summary, desktop-ui) with structured
    `$GITHUB_STEP_SUMMARY` markdown, and shipped
    `.github/workflows/codeql.yml` for static analysis coverage of
    Rust + JavaScript/TypeScript. Canonical contract is
    `docs/private/operating/ci-modernization.md` alongside
    `docs/private/operating/ci-caching.md`. `/goal` control plane gated on
    `bash scripts/verify-ci-modernization.sh` (twelve conditions).
    Proof bundle at `docs/private/plans/proof/ci-modernization/`. Future CI
    infrastructure / SAST / composite-action waves must promote a
    new active plan.
- `docs/private/plans/archive/sandbox-microvm-hardening-plan.md`
  - completed execution record for closing the microVM service exposure
    blockers from `docs/private/plans/security/sandbox-isolation-audit.md`: krun OCI
    seccomp, explicit capabilities, `noNewPrivileges`, TSI bind-address
    carry-through, patched-crun port-map parser robustness, and Debian 13
    Linux localhost-only proof. Covered SMH0-SMH4, closed 2026-05-21. Future
    distribution packaging must pin and ship the validated patched
    crun/libkrun stack or rerun the same smoke proof for a replacement stack.
- `docs/private/plans/archive/nimbus-libkrun-runtime-stack-plan.md`
  - completed execution record for shipping the patched Linux krun service
    stack from Nimbus-owned release artifacts (closed 2026-05-21). Created
    `nimbus/nimbus-libkrun`, published `v1.18.1-nimbus.1`, paired
    `nimbus-crun` `v1.27.1-nimbus.2`, updated direct install/uninstall/verify,
    Linux package builders, apt/COPR mirror workflows, VMM helpers, and docs,
    and closed with Debian 13 root VMM localhost-only smoke plus Fedora 42
    package proof. Future krun stack bumps should promote a new active plan.
- `docs/private/plans/archive/runtime-engine-seam-plan.md`
  - completed Step 0 runtime-extension baseline (RS0-RS6, closed
    2026-05-21). Defined `WorkerLoop` / `RuntimeBackend` as the execution seam,
    split the JavaScript Nimbus context contract from Deno-op transport, made
    engine/content/compatibility/package metadata explicit, added the new
    engine proof-harness contract, and recorded Bun/JSC proof gates 1-10
    through generated-program timeout/cancel. Future execution-boundary work
    starts from
    `docs/private/plans/archive/execution-isolation-and-runtime-backends-plan.md`.
- `docs/private/plans/archive/execution-isolation-and-runtime-backends-plan.md`
  - completed execution-boundary control plane (EIB0-EIB7, closed
    2026-05-21). Built the ownership map across runtime, sandbox, WASM/WASI,
    admission, and security-audit plans; defined shared trust tiers; kept
    Bun/JSC proof-only with no fork yet; routed sandbox audit findings; aligned
    wasmtime and WASI agent plans; recorded resource/admission promotion rules;
    and closed with the runtime/backend go/no-go matrix. Future implementation
    waves should promote focused active plans from this baseline.
- `docs/private/plans/archive/cli-daemon-canonicalization-plan.md`
  - completed execution record for the CLI daemon canonicalization wave
    (CD1-CD9, closed 2026-05-19). Removed source-tree walk-up from
    `nimbus start`; bounded `dev`/`deploy`/compose walkers at the
    nearest `.git/` (directory or worktree-shaped file); added a
    CockroachDB-shaped `operator console:` banner line on both daemon
    commands; added `--open` to `nimbus dev` (only); removed the
    legacy explicit-spawn flag from `nimbus ui`, leaving it as an
    unflagged discover-and-open command. Regression tests in
    `nimbus-bin` cover the rebrand-trap shape,
    the worktree boundary, the banner, and the `ServerDiscoveryRecord`
    serde golden contract shared with Electron and the Playwright
    fixture.
- `docs/private/plans/archive/install-script-plan.md`
  - completed execution record for the nimbus install script (Channel 1):
    POSIX `curl | sh` quick start for Linux (Debian/Ubuntu, Fedora/RHEL) and
    macOS (Apple Silicon). Covered I1-I5: platform detection, dependency
    installation, GitHub Releases binary download with SHA256 verification,
    macOS Homebrew cask install/upgrade with bundled `libexec/gvproxy`,
    uninstall, and the canonical hosted URL
    `https://github.com/nimbus/nimbus/releases/latest/download/install.sh`.
    Fresh-host proofs landed for Ubuntu 24.04, Debian 13, Fedora 42, and
    Apple Silicon macOS at `.install-script-proofs/`. Closed 2026-05-17;
    future install-script work must promote a new active plan.
- `docs/private/plans/archive/desktop-mission.md`
  - completed mission-completion record for the autonomous-mode control
    plane that bound Phase 1 + Phase 2 desktop work into a single mission.
    Closed 2026-05-16; durable authorizations, resume procedure, and stop
    condition preserved as historical reference.
- `docs/private/plans/archive/desktop-ui-plan.md`
  - completed execution record for Phase 1 of the operator console:
    embedded React SPA at `/ui/*` via `rust-embed`, dashboard/machines/
    services/functions/data/logs/runs/settings tabs, dark mode, a11y,
    DU0–DU10 + DU11 hardening. Consumed the `_nimbus` system-tenant
    surface and current architecture references below. Successor:
    `docs/private/plans/archive/desktop-ui-shell-overhaul-plan.md` (two-view
    shell, primary drawer, sub-drawer, tenant selector, active-tenant
    store).
- `docs/private/plans/archive/desktop-ui-shell-overhaul-plan.md`
  - completed execution record for the two-view operator console
    shell: Developer console at `/app/*` (Overview / Compute /
    Schedules / Storage / Files / Observability / Settings, tenant-
    scoped) and Operator console at `/admin/*` (System / Tenants /
    Machines / Network / Services / Observability / Settings, server-
    wide), with top-nav view switcher + collapsible primary drawer +
    contextual sub-drawer (static menu | dynamic list) + tenant
    selector + active-tenant Zustand store + `?as=` bootstrap. Covered
    O0–O8. Closed 2026-05-17; verification artifacts under
    `docs/private/plans/proof/desktop-ui-shell-overhaul/`. Promote new active
    plans before implementing real feature content for the placeholder
    Services / Files / Schedules surfaces.
- `docs/private/plans/archive/desktop-ui-compute-services-redesign-plan.md`
  - completed execution record for correcting two IA mistakes in the
    shell-overhaul plan: (1) Services moves from Operator-only to
    dual-persona (Developer `/app/services*` + Operator
    `/admin/services*`, parallel to Observability), sharing
    `ServicesTable`/`ServiceDoc` with a `showTenantColumn` toggle;
    (2) Compute stops being a kitchen sink — inner tabs deleted, the
    page becomes a Functions-only landing with a Convex-style
    hierarchical function tree in the sub-drawer, a per-function
    detail page at `/app/compute_/$function` with Statistics/Source/
    Logs/Runs tabs, and a docked Input/Output runner; the standalone
    `/app/compute/runner` route was deleted (pre-launch breaking
    change). Scheduled/Cron migrated out of Compute into
    `/app/schedules` with `?section=scheduled|cron`. Covered CS0–CS10.
    Closed 2026-05-18; proof bundle under
    `docs/private/plans/proof/desktop-ui-compute-services-redesign/`. Promote
    a new active plan before implementing real Code-refs (Services)
    or Drift (Operator) backends, or before wiring a Monaco/Shiki
    source viewer.
- `docs/private/plans/archive/desktop-ui-design-review-fixes-plan.md`
  - completed execution record for the 2026-05-18 operator-console
    design review cleanup. Closed 12 of 14 in-scope findings (16
    total; F8 already-fixed and F16 folded into F1; F15 theme-matrix
    smoke and F6-restore of Restarts/Density/Drift deferred to owning
    plans): copy hygiene + canonical `EmptyState`
    (F1, F13), lens gating ⌘\ Developer-only (F2), Observability +
    Schedules section truth (F3, F4), tenant default + ScopeChip
    cleanup (F5, F12), real shells on `/admin` index +
    `/admin/observability` (F10, F11), service-detail tab pruning to
    Placement-only (F6), and polish for breadcrumb / tab casing /
    sub-drawer grouping (F7, F9, F14). Covered DR0–DR8. Closed
    2026-05-18; proof bundle under
    `docs/private/plans/proof/desktop-ui-design-review-fixes/` (before vs
    after walks at 1440×900, plus DR2 lens-blocked evidence). Out of
    scope: theme-matrix smoke (F15) and re-adding service-detail
    Restarts/Density/Drift tabs — both return when their owning
    plans land.
- `docs/private/plans/archive/desktop-ui-followup-hardening-plan.md`
  - completed execution record for the post-design-review correctness
    wave on the operator console. Consumed a 2026-05-18 four-reviewer
    pass against the predecessor's after-state (~30 follow-up items: 1
    BLOCKER, 8 MAJORs, 13 MINORs, 7 NITs). Ratified the shipped dual-
    persona Services IA into `DESIGN.md` (H1, BLOCKER cleared);
    introduced `TenantScope` + `SubDrawerItem<TId>` discriminated
    unions and dropped the `extractTenantId` regex helper (H2);
    wired `LoadingValue<T>` envelopes through Overview / System /
    `/admin/tenants` 404 (H3); landed casing + breadcrumb +
    `EmptyState` + `/admin/network` default-section polish (H4); and
    closed the cleanup + spec-backfill items including the shared
    `fetchTenants` helper and the AbortController unmount-path test
    (H5). Covered H0–H7. Closed 2026-05-18; proof bundle under
    `docs/private/plans/proof/desktop-ui-followup-hardening/` (predecessor's
    sealed `after/` serves as this wave's `before/`; this wave's
    `after/` closes the predecessor's `/admin/services/$service`
    evidence gap and adds 10 new `h7-*` captures). Out of scope:
    F15 theme-matrix smoke and the Restarts/Density/Drift restore —
    both still return when their owning plans land.
- `docs/private/plans/archive/desktop-ui-architecture-hardening-plan.md`
  - completed execution record for the architecture-debt wave on the
    operator console. Consumed a 2026-05-18 post-closure
    critical-reflection review of the Followup-Hardening wave that
    surfaced six architecture items the H-wave intentionally did not
    touch. Covered A0–A8: codegen now emits per-export typed return
    surfaces (`{ default, _typed }`) so every `useQuery` returns the
    handler's declared shape directly and every `as never` / `as
    ServiceDoc[] | undefined` cast was deleted in the same wave (A1);
    `ServiceDoc` lifted to `lib/types/services.ts` with zero
    cross-persona route-to-route type imports (A2);
    `routes/admin/settings.tsx` decomposed from 1608 LOC to 93 LOC
    plus four concept-owned siblings under `routes/admin/settings/`
    (A3); five data routes hoisted to TanStack Router `loader` +
    `Route.useLoaderData()` (A4); Storybook component catalog with
    11 stories under `packages/nimbus-ui/src/stories/` (A5);
    `verify-desktop-ui` CI lane wired via playwright-cli with a
    10-step smoke walk plus zero-error console hygiene gate, and a
    long-standing CSP violation on the inline FOUC theme-resolution
    script fixed at the root by pinning its SHA-256 hash in `UI_CSP`
    with a compile-time drift test (A6); `routes/app/observability.tsx`
    split 978 → 180 LOC root + three siblings under
    `routes/app/observability/`, with `routes/app/storage_.$table.tsx`
    (1154 LOC) and `routes/admin/machines.tsx` (715 LOC) kept under
    the CLAUDE.md under-1500 acceptable band with explicit
    justification (A7). Closed 2026-05-18; proof bundle under
    `docs/private/plans/proof/desktop-ui-architecture-hardening/`. Four grep
    gates clean at close: `as never` 0 hits, `as ServiceDoc[] | undefined`
    0 hits, cross-persona `ServiceDoc` import under `routes/admin` 0
    hits, `routes/admin/settings.tsx` 93 LOC (well under the 900-LOC
    cap). Visual baseline points to the sealed `h7-*` bundle from
    the followup-hardening wave since A1–A7 are code/architecture-only
    changes. Out of scope continues: F15 theme-matrix smoke and the
    F6 Restarts/Density/Drift restore — both return when their owning
    plans land.
- `docs/private/plans/archive/desktop-ui-architecture-residue-plan.md`
  - completed execution record for the architecture-residue wave on the
    operator console — the second pass after the Architecture-Hardening
    wave. Promoted 2026-05-19 from a four-stream post-closure code review
    that surfaced three blockers the A2/A4 grep gates missed (vanity-gate
    `as unknown as CountQuery` residue in `shell/nav-entries.ts`, partial
    loader migration on `_.$service.tsx` siblings, un-specced codegen
    inference layer) plus ~12 cleanup items and four nits. Covered R0–R12:
    producer-side typed `QueryEntry<TArgs, TReturn>` wrapper retired the
    nine remaining `as unknown as` casts (R1); the three sibling-`useQuery`
    sites on `_.$service.tsx`-class routes and the two page-level
    `useQuery` calls on `compute_.runs_.$runId.tsx` folded into
    `Route.loader` (R2 + R4); codegen `isTrivialValidator` now unwraps
    `union(v.any(), v.null())`, `inferMutationResultType` throws on missing
    `plan.table`, `helperCall` audits convention-inferred fallbacks, and
    `JsonValue` collapsed to a single source in `dataModel.d.ts` (R3);
    loader-error envelope (`Route.errorComponent` + `EmptyState` retry) +
    spec coverage added to the four A4 routes (R5); shared `_filters.tsx`
    + `Th`/`Td` primitives consolidate the observability + tenants
    duplication (R6); `routes/app/services{,_.$service}.tsx` switched
    from `useEffect → router.invalidate()` to `Route.loaderDeps` keying
    on `useUiStore.getState().activeTenant` (R7); A3 dead `dialogRef`
    declarations removed from `danger-zone.tsx` and `settings/sub-drawer.ts`
    adopted the typed-const `as const satisfies StaticSubDrawerSpec<…>`
    pattern (R8); `inline_fouc_script_hash_matches_csp` now asserts
    exactly one inline script and tolerates attribute-bearing tags, and
    the `desktop-ui.yml` workflow path filter widened to cover
    `Cargo.{toml,lock}` + `rust-toolchain*` (R9); smoke spec seeds one
    tenant + one service via `/api/tenants` + raw
    `/convex/_nimbus/mutation` (`Mutation::Insert`) so ScopeChip,
    services-table, and operator service-detail assertions fire
    unconditionally (R10); polish + nit pass (R11) collapsed the parallel
    `ObservabilityTab` unions, dropped debug-residue aria-live + unused
    re-exports, and added five Storybook variants
    (`copy-chip → ClipboardDenied`, `breadcrumb → LongPathTruncation`,
    `time → RelativeFutureSkew / RelativeFarPast / RelativeFarFuture`,
    `upgrade-popover → NotAvailable / StaleCheck / ErrorCheck`,
    `appearance-section → snapshot/restore on unmount`). Closed
    2026-05-19; proof bundle under
    `docs/private/plans/proof/desktop-ui-architecture-residue/` (`before.md`
    records R0 HEAD counts; `README.md` records R0–R12 →
    after-evidence mapping plus close-time grep-gate output). Eight
    grep gates clean at close: `as never` 0 hits, non-spec
    `as unknown as ` 0 hits, `as ServiceDoc … | undefined` 0 hits,
    cross-persona `ServiceDoc` import under `routes/admin` 0 hits,
    `type ServiceDoc` 1 hit (single canonical), `useQuery` in the three
    loaderized routes 0 hits, `router.invalidate` in `routes/app/services*.tsx`
    0 hits, `export type JsonValue` 1 hit in `_generated/`. Visual
    baseline inherits the H wave's sealed `h7-*` bundle since R1–R12
    are code/architecture-only changes.
- `docs/private/plans/archive/update-lifecycle-plan.md`
  - completed execution record for the operator-facing update lifecycle:
    server-side `/api/system/version-info` with stale-while-revalidate
    (UL1), SPA staleness UX in `packages/nimbus-ui/` (UL2),
    `nimbus-desktop` first-run "CLI not found" setup card + background
    `brew upgrade` runner + OS staleness notification (UL3), and
    operator-facing `docs/private/operating/updates.md` (UL4). Closed 2026-05-16.
    Current operator reference is `docs/private/operating/updates.md`; decision
    anchor is `docs/decisions/001-update-staleness-detection.md`.
- `docs/private/plans/archive/desktop-shell-plan.md`
  - completed execution record for Phase 2 of the operator console:
    signed, notarized, auto-updating Electron 42.x desktop shell in
    `nimbus/desktop` wrapping the embedded `/ui/*` SPA. Covered DS0A
    through DS10: external credentials, repo scaffold, server discovery/
    lifecycle, Electron Fuses + IPC security baseline, native chrome
    (tray/menu/window), auto-update, per-platform packaging, packaged
    E2E, code signing, release CI, and operator/security docs. DS7 / DS8
    / DS9 macOS re-verification deferred to first real `v0.x` release
    per the in-tree §"External feedback loops" disposition.
- `docs/private/plans/archive/brand-system-plan.md`
  - completed execution record for the two-tier brand system rollout
    across `nimbus/nimbus` and `nimbus/desktop`: 9-variant brand palette
    (Brand tier, gradients permitted, marketing surfaces) layered cleanly
    over the operator console's Industrial Precision Product tier (single
    teal accent, OKLCH 240° neutrals, no gradients). Covered L0–L9:
    canonical logo SVG + tight mark + 9 variants, DESIGN.md brand section,
    favicon, sidebar mark, desktop app icon, tray refresh,
    `cli-not-found.html` token migration, and idempotent
    `gen-variants.sh`. Closed 2026-05-16.
- `docs/private/plans/archive/bootc-machine-default-plan.md`
  - completed execution record for BMD0-BMD7: direct Fedora bootc machine-os
    recipe ownership, build artifact proof, bootc-native machine-config,
    macOS parity, bootc lifecycle, default promotion to
    `ghcr.io/nimbus/machine-os:v0.1.30@sha256:f565...`, and legacy
    Podman/FCOS demotion to explicit diagnostic/repair overrides. Closed
    2026-05-16; promote a new active plan for future machine OS work.
- `docs/private/plans/archive/system-tenant-api-plan.md`
  - completed execution record for ST1-ST4: `_nimbus` system tenant bootstrap,
    server-owned machine/service/network projections, local-admin lifecycle
    endpoints, packaged `_nimbus` Convex query bundle, read/write split, and
    verification evidence for the desktop UI prerequisite gate
- `docs/private/architecture/sandbox/microvm-service-baseline.md`
  - concise current baseline for the landed krun-backed microVM runtime,
    service activation, Compose-backed `nimbus compose ...` surface, and the
    Linux-versus-macOS platform model
- `docs/private/architecture/sandbox/macos-machine-flow.md`
  - concise current reference for the settled macOS developer-machine contract:
    pinned Nimbus bootc machine image digest, bootc-native machine-config,
    forwarded machine API, host-resident `nimbus start`, explicit legacy
    Podman/FCOS diagnostic overrides, and proof-helper entrypoints
- `docs/private/architecture/runtime/adapter-boundary.md`
  - current runtime and adapter ownership boundary
- `docs/private/architecture/runtime/permission-model.md`
  - current runtime permission-mode, grant, language, compatibility-target,
    and preset baseline
- `docs/private/architecture/server/auth-runtime-trust.md`
  - current server-owned auth and runtime trust boundary
- `docs/private/architecture/runtime/node-compat-surface-matrix.md`
  - current Node compatibility support matrix and evidence pointers
- `docs/private/adapters/convex/compatibility.md`
  - current Convex adapter compatibility contract
- `docs/private/plans/archive/machine-os-adoption-plan.md`
  - superseded evidence plan for MOS0-MOS2 and the abandoned MOS3A
    FCOS-derived candidate. Do not resume MOS3A from this plan; the
    bootc-native default lives in
    `docs/private/plans/archive/bootc-machine-default-plan.md` as completed
    implementation baseline. Future machine OS work must promote a new
    active plan.

## Deferred plans with defined scope

- `docs/private/plans/windows-machine-support-plan.md`
  - canonical execution plan for the Podman-aligned Windows developer-machine
    architecture, source-backed against the Podman WSL2 provider: Windows-native
    `nimbus.exe` with WSL2 machine provider, win-sshproxy named-pipe API
    forwarding, shell-script bootstrap (not ignition), WSL2-native networking
    (not gvproxy); activation gate is macOS MAC5+ stabilization
- `docs/private/plans/sandbox-disk-limit-enforcement-plan.md`
  - `proposed` host-layer follow-up to the SBR C3 fail-closed decision: both
    sandbox backends currently reject any `disk_limit_bytes` as `InvalidSpec`
    because the writable surface is a host bind-mount with no project-quota
    knob. Wire-now set is SDL0 + SDL1 (detect a project-quota-capable
    filesystem at the data dir, then enforce or refuse-to-start in production);
    SDL2/SDL3/SDL4 deferred. Activation gate is a real consumer needing
    enforced per-service disk caps.

## Deferred design and experiment plans

- `docs/private/plans/layered-admission-control-plan.md`
  - current owner of future layered admission-control and `EO8` promotion work;
    use it before promoting any new admission-control boundary
- `docs/private/plans/wasmtime-backend-plan.md`
  - canonical plan for adding a wasmtime-based WASM backend alongside the
    existing V8 backend (currently implemented via `deno_core`); covers
    backend abstraction refactor, WIT interface definitions, cooperative
    fuel-based scheduling, module caching, and bundle format extension;
    activation gate met (Locker fork Phase 5 completed 2026-04-06), but
    runtime-engine seam hardening now owns the next Step 0 before promotion
- `docs/private/plans/wasi-agent-capabilities-plan.md`
  - canonical plan for adding agent OS primitives (virtual filesystem, sandboxed
    process execution, HTTP client) via WASI Component Model interfaces; covers
    `nimbus:agent` WIT package, `AgentOsProvider` trait, capability-based tenant
    admission, and agent-os sidecar integration; activates after the wasmtime
    backend plan W3 completes
- `docs/private/plans/native-transport-evolution-plan.md`
  - proposed follow-on plan for Nimbus-native transport evolution: shared
    session and codec seams, benchmark-driven optional binary codec work, and
    optional WebTransport evaluation without re-owning the established
    WebSocket protocol or Firebase transport work.
- `docs/private/plans/agent-browser-service-plan.md`
  - canonical deferred plan for adding a built-in `browser` service that
    provides browser sessions: `BrowserProvider` trait, single-Chrome-
    many-BrowserContexts shape, sandboxed-Chrome production provider behind
    `nimbus-sandbox`, durable Playwright-compatible storage state/profile
    policy in `nimbus-storage`, warm pool, extraction cache, mutation-journal
    integration, and per-tenant capability admission. The SDK/session surface is
    canonical; any future `nimbus.browser` facade is an SDK facade over that
    service/session model, not an adapter or runtime `ctx` shortcut. North-star
    research at
    `docs/private/plans/research/agent-browser-service-prior-art.md`.
- `docs/private/plans/horizontal-scaling-plan.md`
  - canonical deferred plan for the **cluster substrate** — promotes the settled
    iroh+openraft design in `docs/private/architecture/horizontal-scaling.md`
    (Patterns C+D+E: database-per-tenant + deterministic log + edge replicas)
    into bands HS0..HS7: cryptographic node identity + NAT-traversing QUIC mesh
    + self-forming membership (HS1), openraft durable tenant→node placement
    mapping (HS2), iroh-gossip reactive invalidation (HS3), V8 isolate placement
    + cross-node routing (HS4), microVM placement beneath the NSR executor (HS5),
    iroh-blobs content distribution (HS6), and multi-tenancy cluster integration
    (HS7). All Iroh interaction rides one `ClusterTransport` abstraction boundary;
    a new `crates/nimbus-cluster/` honors the `nimbus-core` zero-I/O and
    `nimbus-runtime` zero-workspace-dep invariants. **This is the substrate the
    three plans below ride** (`secret-management` openraft+gossip,
    `service-identity-provider-auth` **hard**-deps HS1 membership/node-identity,
    `agent-browser-service` openraft+iroh-blobs+gossip+QUIC) — §16 of the
    architecture doc is the consumer-plan contract. `deferred`: single-node is
    the launch baseline; activation gate is the first real multi-node deployment.
    The control-plane verifier is planned at
    `bash scripts/verify-horizontal-scaling.sh` (18 conditions) and is created
    by HS0. Until HS0 lands, a missing verifier is expected, not a pass.
    LOCAL-ONLY plan: lives under untracked `docs/private/`, never committed.
- `docs/private/plans/secret-management-plan.md`
  - canonical deferred plan for a tenant-scoped secret-management
    surface in Nimbus: `SecretProvider` trait + URI-shaped references
    + capability-gated `ctx.secret.*` host bridge + `_nimbus.secrets`
    Nimbus-native storage with KMS-DEK-wrapped values + multi-backend
    routing via `_nimbus.secret_stores` + cache invalidation via
    iroh-gossip. MVP provider set: NimbusNative, File (SOPS-compatible),
    AWS Secrets Manager, GCP Secret Manager, Azure Key Vault, HashiCorp
    Vault, Kubernetes Secrets. Activates when any of the browser plan
    Phase B5, the wasi-agent Phase A2, or a first paying tenant needs
    stronger-than-env-var credentials. Prior-art research at
    `docs/private/plans/research/secret-management-prior-art.md`. Supersedes
    the gap note `docs/private/plans/research/secret-management-shape.md`.
- `docs/private/plans/service-identity-provider-auth-plan.md`
  - canonical deferred plan for promoting admitted `WorkloadIdentity` subjects
    into short-lived provider-auth credentials: enforced `identity` grants,
    OIDC/JWT minting, SPIFFE/SVID shape, mTLS/service-account tokens,
    node/machine identity binding, Vault/Kubernetes/AWS/GCP/Azure
    provider-auth adapters, lifecycle/revocation, and audit. Consumes
    `WorkloadIdentity`; does not own secret values. Dependency
    review at
    `docs/private/plans/research/service-identity-provenance-dependency-audit.md`.
- `docs/private/plans/archive/artifact-provenance-verification-plan.md`
  - canonical completed baseline for production artifact verification behind
    the landed `TenantImageVerificationProvider` seam: maintained OCI
    reference parsing, Cosign verification, SLSA provenance verification,
    SBOM evidence, offline/private-root verification, extension from service
    images to runtime/function bundles and machine images, runtime hook
    propagation, source-scoped provenance policy, and composite verifier
    chaining. Nimbus owns policy evaluation and evidence normalization, not
    cryptographic verification. Dependency review at
    `docs/private/plans/research/service-identity-provenance-dependency-audit.md`.
## Archive Policy

Completed plans are stored in `docs/private/plans/archive/` for historical review, but
this README intentionally does not catalog them. Use `rg` or `find` when you
need a specific historical record, and do not resume an archived plan unless
the work is explicitly a historical review.

## How To Use This Folder

- Start with the plan that owns your workstream.
- For broad maintainability, refactor, modularity, readability, canonical
  naming, idiomatic-Rust, or god-file cleanup work, start with
  `docs/private/architecture/testing/reliability-posture.md` and
  `docs/private/architecture/testing/ci-failure-investigation.md`, then promote a new
  active plan unless another active plan already owns the slice.
- For the landed krun-backed microVM and service-control architecture, start
  with `docs/private/architecture/sandbox/microvm-service-baseline.md` rather than
  opening the archived plans first.
- For current macOS developer-machine behavior, start with
  `docs/private/architecture/sandbox/microvm-service-baseline.md` and
  `docs/private/architecture/sandbox/macos-machine-flow.md`.
- Promote a new active plan before landing another machine/service CLI UX
  wave.
- Do not resume a plan from `docs/private/plans/archive/` unless you were explicitly
  asked to review historical work.
- If no active plan owns the work, promote or author a new active plan instead
  of reviving a completed archived one.
- For install-script follow-on work, start with the completed baseline at
  `docs/private/plans/archive/install-script-plan.md` and the active parent context
  in `docs/private/plans/distribution-plan.md`. Promote a new active plan before
  another install-script wave unless one already owns the slice.
- For Linux krun service stack packaging, `nimbus-libkrun`, or paired
  `nimbus-crun` release work, start with
  `docs/private/plans/archive/nimbus-libkrun-runtime-stack-plan.md`, then cross-check
  `docs/private/plans/distribution-plan.md`.
- For production tenant isolation across microVM services, in-process runtime
  grants, HostBridge access, storage namespaces, service networking, volumes,
  images, secrets, quotas, cleanup, enterprise auditability, admission
  decisions, conformance, or workload identity, start with
  `docs/private/tenant-isolation.md` and `docs/private/operating/tenant-isolation.md`, then
  cross-check
  `docs/private/plans/archive/tenant-isolation-enterprise-hardening-plan.md`,
  `docs/private/plans/archive/tenant-isolation-control-plane-plan.md`,
  `docs/private/plans/security/sandbox-isolation-audit.md`,
  `docs/private/architecture/sandbox/microvm-service-baseline.md`,
  `docs/private/architecture/runtime/permission-model.md`, and
  `docs/private/architecture/storage/provider-topologies.md`.
- For image signatures, SLSA provenance, SBOMs, OCI reference/referrer parsing,
  runtime bundle provenance, machine-image provenance, or any artifact
  verification code, start with
  `docs/private/plans/archive/artifact-provenance-verification-plan.md`. Do not hand-roll
  cryptographic verification in Nimbus.
- For service identity, provider-auth exchange, OIDC/JWT minting,
  SPIFFE/SVIDs, mTLS certificates, service-account tokens, or promotion of the
  `identity` grant, start with
  `docs/private/plans/service-identity-provider-auth-plan.md`.
- For Convex or Nimbus CLI/codegen workflow work (`packages/codegen/`,
  `packages/convex/`, `demos/convex/`, or the `nimbus start --app-dir`
  contract), start with `docs/private/adapters/convex/ai-guidelines.md`,
  `docs/private/operating/cli.md`, and `docs/private/adapters/convex/compatibility.md`.
  Promote a new active plan before another CLI/codegen/facade architecture wave
  unless one already owns the slice.
- For encryption at rest work, start with
  `docs/private/architecture/storage/encryption.md` and `docs/private/operating/encryption.md`.
  Use the archived execution plan only for historical closeout detail.
- For Compose-backed service lifecycle follow-on work, start with
  `docs/private/architecture/sandbox/microvm-service-baseline.md`, then promote or author a new
  active plan if the task is larger than a small focused change.
- For microVM service security hardening, start with
  `docs/private/plans/archive/sandbox-microvm-hardening-plan.md` and
  `docs/private/plans/security/sandbox-isolation-audit.md`.
- For sandbox profile work beyond the landed krun service baseline, start
  with `docs/private/plans/nimbus-sandbox-plan.md` — the single unified plan for
  every Nimbus sandbox workload via capability profiles on the
  `nimbus-libkrun` fork. Bands: B (shared backend skeleton + nimbus-guest),
  S (snapshot/fork mechanism, Linux-KVM-only per D11), D (desktop
  profile), G (GPU profile). The `libkrun_session` backend design lives
  in `docs/private/plans/research/libkrun-session-sandbox.md`; the GPU mediation
  policy matrix (Venus vs native-context vs CUDA-on-NVIDIA-fleet) lives
  at `docs/private/plans/research/gpu-sandbox-backends.md`; the decision baseline
  (D1-D12) and snapshot/fork mechanism research live at
  `docs/private/plans/research/vmm-landscape-2026.md`; the desktop product
  capability audit is at
  `docs/private/plans/research/computer-use-capabilities-audit.md`; the fork
  patch inventory is at
  `docs/private/plans/research/nimbus-libkrun-fork-inventory.md`; macOS host
  topology rationale is at
  `docs/private/plans/research/macos-host-vs-guest-control-plane-rationale.md`.
  Firecracker is a reference baseline only — do not propose it as a
  Nimbus VMM family. Archived predecessors at
  `docs/private/plans/archive/firecracker-snapshot-invocation-backend-plan.md`,
  `docs/private/plans/archive/nimbus-libkrun-snapshot-port-plan.md`,
  `docs/private/plans/archive/computer-use-sandbox-plan.md`, and
  `docs/private/plans/archive/gpu-accelerated-sandbox-plan.md` (all retained as
  baseline content for the unified plan's bands).
- For repo-wide reliability-proof posture or CI flake investigation, start
  with `docs/private/architecture/testing/reliability-posture.md` and
  `docs/private/architecture/testing/ci-failure-investigation.md`.
- For future cleanup or verification-hardening work that is not already owned
  by another active plan, author or promote a new active plan instead of
  reviving an archived one.
- For future execution-boundary work, including runtime engines, Bun/JSC,
  wasmtime, sandbox isolation, WASI agent capabilities, or admission/resource
  boundaries, start with
  `docs/private/plans/archive/execution-isolation-and-runtime-backends-plan.md`; then
  promote a focused active plan for the implementation slice or use an existing
  backend-specific deferred plan such as `wasmtime-backend-plan.md` when its
  activation gate is met.
- For future agent OS capabilities via WASI Component Model, start with
  `wasi-agent-capabilities-plan.md`.
- Resume any existing `in_progress` item and reconcile dirty worktree changes
  before starting a new roadmap item inside the owning plan.
- Use `docs/private/plans/research/` for north-star architecture and background research,
  not execution sequencing.
