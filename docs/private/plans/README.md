# Plans

This file is the routing index for current Nimbus roadmap work. It answers two
questions:

1. Which plan owns the work?
2. In what order should the owning plans run?

It is intentionally not a history catalog. Keep past execution records out of
this index so the next agent can read it as the current control plane.

## First Principles

- Launch safety comes before breadth. Fail-closed egress, tenant separation,
  policy auditability, and release channels outrank new compatibility surfaces.
- Trust-boundary substrates come before consumers. Build the filesystem, proxy,
  runtime, sandbox, and network seams before features that rely on them.
- Single-node correctness comes before cluster mode. Cluster plans should consume
  single-node primitives rather than re-owning them.
- Runtime and sandbox ownership stays separated. `nimbus-runtime` owns injected
  traits and execution interfaces; behavior crates own policy, storage, proxy,
  filesystem, and sandbox implementation.
- Drafts are not execution targets. Promote a draft only after it has scope,
  verifier/proof obligations, dependencies, and a paste-ready goal.

## Status Vocabulary

- `active` or `in_progress`: ready to execute or resume.
- `proposed`: coherent enough to review and promote, but still needs an explicit
  owner decision before implementation.
- `deferred`: scoped, but gated on a named consumer or deployment trigger.
- `draft` or `exploratory`: records candidate work; do not target with an
  implementation goal.
- `map`, `spec`, or `decision record`: routing or architecture context, not an
  implementation control plane.

Plans in the same phase are parallel-safe unless a dependency is named in the
bullet. Later phases should consume earlier seams instead of re-deriving them.

## Execution Order

### Phase 1 - Launch Safety And Egress Trust

- `architecture-review-2026-07-plan.md` - `active`. Control plane for the
  2026-07 full-workspace architecture review: guarantee/fail-closed repairs
  (storage write-path unification, EgressGateway pairing, decision-log
  fail-closed), seam repairs, consolidations, decompositions, test-infra and
  UI hygiene, doc/spec truth-ups, and the `nimbus-compute` extraction plus
  workload-identity decision records. Review-driven refactor and cleanup work
  should route through this plan's band ledgers while it is active.
- `nimbus-runtime-tenant-isolation-plan.md` - `complete` in PR #227. Canonical
  runtime-owner identity, routing-versus-reuse-authority separation,
  owner-partitioned
  V8/Wasmtime retained state, tenant and deployment retirement, and a
  compute-owned Nimbus runtime manager consumed by Convex, Cloud Functions,
  and future adapters. It consumes Engine/storage's durable tenant incarnation
  and lifecycle authority rather than minting a parallel one. `IsolateGroup`
  remains a separate deferred density/VM-shape question, not the tenant
  boundary.
- `distribution-plan.md` - `in_progress`. Owns binary release, Homebrew/cask,
  Linux package mirror, release-owned OCI images, and channel cutover. It should
  consume launch safety decisions rather than define them.

### Phase 2 - Runtime, Filesystem, And WASM Substrates

- `wasi-agent-capabilities-plan.md` - `deferred`. Starts only after the Wasmtime
  component linker, NimbusFS binder, and HTTP-client binder exist. Owns the
  process primitive and WIT projection layer; it must not re-own filesystem or
  proxy behavior.

Phase 2 coordination: if future filesystem work, Wasmtime, or WAC need runtime
bootstrap changes in the same window, create or reuse a shared
extension-registry seam before the second concern edits `extensions.rs`.

### Phase 3 - Network, Sandbox, And Machine Execution

- `nimbus-network-control-plane-plan.md` - `active; NNC3.4 complete; NNC3.5
  server listener migration in progress`. Sole owner for the transport-free
  connectivity-resource control plane: portable network identities and plans,
  crash-safe segment and cross-process host-port lease authority,
  generation/epoch fencing, capability satisfaction, reconciliation, exact
  provision/teardown choreography, and sovereignty proof. It must stay below
  tenant, sandbox, services, machine, KV, compute, server, system, and future
  cluster. NNC0.0 first makes the force-tracked owner plan durable in branch
  history; the remaining NNC0 items capture the complete dependency/bind census,
  real-process proof harnesses, and executable failure baselines before
  extraction. Its workloads/compute, sandbox, egress/proxy, service, system,
  and horizontal-scaling integrations consume those owners rather than forking
  desired-workload state, policy, effects, projections, or cluster transport.

- `nimbus-sandbox-plan.md` - `proposed`. Owns the multi-backend sandbox
  architecture (`ADOPT_MULTI_BACKEND_SANDBOX_ARCHITECTURE`, 2026-07-08): the
  `SandboxBackend` router/dispatch seam with backend families
  (container/krun landed; libkrun_session, firecracker, isolate, wasm,
  gvisor, cloud_hypervisor, qemu named), the family/profile/capability
  vocabulary,
  and the libkrun-family session bands (shared backend skeleton, desktop
  profile, GPU profile). Band R (router) precedes any new backend family;
  Band B precedes the desktop/GPU bands.
- `firecracker-fast-invocation-backend-plan.md` - `proposed`. Owns the
  `firecracker` backend family: snapshot-backed fast invocation on stock
  Firecracker (sidecar + jailer, snapshot/restore, template fork) behind the
  Band R dispatch seam. Replaces the removed in-fork snapshot port (former
  sandbox-plan Band S) and precedes density work that assumes snapshot/fork
  scale. Promote when the `fast_invocation` product surface is scheduled and
  Band R is landed.
- `sandbox-disk-limit-enforcement-plan.md` - `proposed`. Owns host-layer
  project-quota detection and enforce-or-refuse startup semantics for per-service
  disk caps. Promote when a concrete service needs production disk limits.
- `windows-machine-support-plan.md` - `deferred`. Owns Windows developer-machine
  support using a Windows-native CLI plus WSL2 machine provider. Promote when
  Windows development support is scheduled.

### Phase 4 - Connection Residency, Secrets, And Service Identity

- `secret-management-plan.md` - `deferred`. Owns tenant-scoped secret references,
  providers, storage, cache invalidation, host bridge, and audit model. Promote
  when a named consumer needs stronger-than-env-var credentials.
- `service-identity-provider-auth-plan.md` - `in_progress`. Owns workload identity
  minting into provider credentials. SI0 landed (PR #126: `nimbus-workload-identity`
  crate — provider-auth policy, admission-anchored mint authorization, claim set,
  audit schema); SI1+ remain gated (production minting on HS1, provider adapters
  on a concrete provider need). It consumes workload identity and cluster
  node identity; it does not own secret values.
- `agent-browser-service-plan.md` - `deferred`. Owns the built-in browser
  service, session/profile storage, warm pool, Playwright-compatible surface,
  and sandboxed-Chrome production provider. Its credentialed flows should consume
  secret management and service identity rather than inventing a parallel model.

### Phase 5 - Cluster And Cross-Node Work

- `horizontal-scaling-plan.md` - `deferred`. Leads all multi-node work. Owns node
  identity, discovery, membership, placement replication, gossip invalidation,
  isolate placement, microVM placement, content distribution, and the
  cluster-mode integration layer, including HS5's per-Durable-Object placement,
  shared lease authority, and epoch-fenced protected writes. It supplies
  raft-committed fenced node super-net leases to the canonical
  `nimbus-network` allocation contract; it does not own a second segment
  allocator, provider effects, or local network state store.
- `nimbus-fips-iroh-ed25519-retrofit-plan.md` - `draft`. Owns a future
  aws-lc-rs/PQ TLS posture, NodeSigner seam, and CMVP-triggered identity-key
  retrofit. Promote only after the current FIPS and iroh identity facts are
  refreshed from primary sources.
- `nimbus-private-registry-cache-isolation-plan.md` - `draft`. Owns tenant
  isolation for authenticated/private registry blob-cache hits. Promote when
  authenticated registry pulls become real product scope.

### Phase 6 - Demand-Gated Policy, Admission, Transport, And Density

- `archive/parallel-prepare-serial-commit-plan.md` - `complete, archived`
  (2026-07-23; PRs #188–#236). Delivered the per-tenant Convex-parity
  committer: parallel prepare, serial assignment, ordered publication,
  bounded conflict retry, provider lease fencing/pipelining, deterministic
  crash/replay and differential verification, and pinned external Elle
  evidence. FINAL recorded all readiness dimensions `PASS` and all closeout
  verdicts `SAFE`.
- `archive/storage-unification-and-carryover-plan.md` - `complete, archived`
  (2026-07-30; PRs #248–#261; decisions U1–U10). Delivered
  the 2026-07-29 storage review's ranked wins (CommitTransaction witness,
  objects/KV/scheduler onto a real commit path, provider facade extraction,
  DynamoDB principal) plus every carry-over: main full-CI triage (RED at
  plan creation; gates all phases), the July 21 HIGH-bug triad, the
  open-loop latency companion, the resource-binding decision, and the
  formal hot-key/OCC closure.
- `archive/storage-follow-ups-plan.md` - `complete, archived` (2026-07-31;
  PRs #262–#270). Closed every executable follow-up from the
  storage-unification campaign: the PPSC arm-theft fault-interface fix
  (`DurableApplyKind`), the MySQL scheduler outlier, DynamoDB batch stream
  staleness plus stream/scan read-policy bypasses and the GetRecords
  amplification ceiling, the nimbus-core lifecycle-predicate planner bug,
  BatchWriteItem validation ordering and rejection rules, the nimbus-fs
  object-write seam, schema-redeclaration write-amplification, three
  root-caused flake families, and the SUC6.2 literal measurement (U7
  override; measured REJECT). FU13 (400 KiB ceiling on the remaining
  DynamoDB write paths) and FU14 (extenddb nested-collection sizing,
  upstream) remain scoped open tickets recorded in the archived plan.
- `archive/sqlite-write-throughput-optimization-plan.md` - `complete, archived`
  (2026-07-28). Campaign PASS in one day: prepared-statement reuse +
  batch-invariant apply context (PR #244), resident writer (PR #245),
  attribution-rejected forward apply (PR #246), final acceptance ratio
  1.836 over `B_ref` with `F_ref` 51,399 N=256 mut/s (PR #247) — 2.4×
  the pre-campaign 21,433 observation. Hot-key +163%, N=1 ≈3.7×,
  storage lane +185%. Full A/B, fail-before, and rejected-run evidence
  under `proof/sqlite-write-throughput/`.
- `archive/clock-architecture-reliability-plan.md` - `complete, archived`
  (2026-07-21; independent Opus 4.8 review clean). Delivered canonical wall and
  monotonic clock seams, Engine-owned absolute scheduling, monotonic local
  duration policies, explicit temporal-validation observations, clock-source
  guards, and structural gates that leave unproved distributed clock authority
  disabled under the horizontal-scaling plan.
- `layered-admission-control-plan.md` - `deferred`. Owns future layered
  admission experiments and EO8-style promotion work. Consumes the
  retry-amplification admission signal from
  `archive/parallel-prepare-serial-commit-plan.md`.
- `native-transport-evolution-plan.md` - `proposed`. Owns benchmark-driven
  Nimbus-native transport evolution without replacing the established WebSocket
  protocol by default.
- `enterprise-crate-adoption-plan.md` - `proposed`. Owns the cross-workspace
  screen for mature Rust crates at commodity substrate seams: Sigstore artifact
  verification, DNS, OIDC/JWKS, OCI spec types, local-socket HTTP parsing,
  policy engines, telemetry, object-storage breadth, QUIC/H3, crypto/TLS
  provider posture, and path-capability primitives. It is a routing and
  promotion control plane; individual rows execute in their owning substrate
  plans after current-source and dependency-graph proof.
- `nimbus-tenant-admission-audit-plan.md` - `draft`. Owns aggregate admission,
  OCSF spool, H-gate ratification, and admitted quota enforcement. Consumes the
  node-scoped `EgressEngine` seams landed by the completed
  `archive/nimbus-egress-engine-plan.md` (decision-event fan-out sink, per-tenant
  fairness budget values).
- `nimbus-proxy-policy-hardening-plan.md` - `draft`. Owns proxy policy
  hardening adjacent to K11P without moving transport dependencies into runtime,
  sandbox, or policy crates.
- `nimbus-sandbox-egress-regression-and-seams-plan.md` - `draft`. Owns KVM proof
  lanes, path-filtered checks, readiness parity, port-pool hardening,
  netavark/firewall determinism, and tenancy hardening.
- `nimbus-proxy-density-and-datapath-plan.md` - `draft`. Owns shared-listener
  and possible eBPF/Aya datapath work. Promote only after K11P, sandbox
  snapshot/fork scale, and source-IP spoofing proof exist.
- `nimbus-masque-h3-egress-plan.md` - `draft`. Owns future QUIC/H3/MASQUE
  egress support. Until promoted and proven, proxy-required rules must deny
  QUIC/UDP bypass.
- `nimbus-libkrun-2-migration-plan.md` - `draft`. Owns the future libkrun/crun
  2.x fork migration after tuple validation and ABI-doctor baselines are ready.
- `nimbus-run-exec-exploration-plan.md` - `exploratory`. Records the possible
  raw argv surface `nimbus run exec -- ...`.
- `nimbus-run-jobs-exploration-plan.md` - `exploratory`. Records the possible
  named workload template surface `nimbus run jobs <name>`.

## Routing Aids And Specs

These documents may inform plan work, but they are not implementation targets:

- `nimbus-modernization-roadmap-plan-map.md` - row-to-owner map for the
  modernization roadmap. Use it before creating another plan.
- `storage-seams-architecture.md` - governing storage/filesystem/object/volume
  seam spec.
- `profile-aware-isolate-runtime-final-architecture-plan.md` - runtime decision
  record for profile-aware isolate pools, snapshots, code cache, pointer
  compression posture, and adaptive-autoscaling constraints.

## How To Use This Folder

- Start with the plan that owns the concrete workstream.
- Resume `in_progress` work before promoting a new plan in the same lane.
- If a draft owns the right topic, promote it before implementation by adding
  scope, sequencing, verifier/proof obligations, and a paste-ready goal.
- If no plan owns the topic, add or promote exactly one owner plan and update
  `nimbus-modernization-roadmap-plan-map.md` if the work came from the roadmap.
- Keep past execution records out of this README; this file should remain
  readable as the current execution order.
