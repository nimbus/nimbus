# Architecture

This directory holds internal architecture docs for contributors. The
subdirectory tree mirrors the Rust crate structure.

For the stable top-level architecture overview, see
[ARCHITECTURE.md](../../ARCHITECTURE.md).

The repository-wide source ownership and architecture guardrail ledger lives at
[repo-architecture-quality-ledger.tsv](repo-architecture-quality-ledger.tsv).
Use it with `./scripts/verify-repo-architecture-quality.sh` before and after
large refactor waves.

## Routing

| Work type | Start with | Note |
| --- | --- | --- |
| Clock semantics, elapsed lifetimes, absolute scheduling, temporal validation, lease renewal, or durable ordering | [`time-and-ordering.md`](time-and-ordering.md) | Wall time, monotonic time, provider authority, and logical sequence are distinct domains. |
| Generic maintainability, refactor, modularity, reliability hardening, or canonical naming | [`testing/reliability-posture.md`](testing/reliability-posture.md), [`testing/ci-failure-investigation.md`](testing/ci-failure-investigation.md) | Large waves should update the architecture quality ledger. |
| Adapter/runtime/auth/trust cleanup | [`server/adapter-expectations.md`](server/adapter-expectations.md), [`runtime/adapter-boundary.md`](runtime/adapter-boundary.md), [`server/auth-runtime-trust.md`](server/auth-runtime-trust.md) | Keep adapter authority, runtime host calls, and server trust boundaries aligned. |
| Runtime capability segregation, service grants, private host-transport gating, Bun/JSC parity, principal-class service routes, or JS SDK authority boundaries | [`server/auth-runtime-trust.md`](server/auth-runtime-trust.md), [`runtime/adapter-boundary.md`](runtime/adapter-boundary.md), [`sandbox/service-sandbox-session-model.md`](sandbox/service-sandbox-session-model.md) | Verify with `bash scripts/verify-nimbus-capability-segregation.sh` when touching that lane. |
| SDK services, sandboxes, sessions, dynamic services, sandbox APIs, runtime-isolate non-resource semantics, or session target semantics | [`sandbox/service-sandbox-session-model.md`](sandbox/service-sandbox-session-model.md) | Runtime isolates are invocation execution domains unless a plan explicitly wraps them as resources. |
| Sandbox, machine lifecycle, macOS machine flow, microVM baseline, or desktop/GPU capability boundaries | [`sandbox/service-sandbox-session-model.md`](sandbox/service-sandbox-session-model.md), [`sandbox/microvm-service-baseline.md`](sandbox/microvm-service-baseline.md), [`sandbox/macos-machine-flow.md`](sandbox/macos-machine-flow.md) | Active sandbox sequencing lives in `../plans/README.md`. |
| Storage trust gaps, table lifecycle, table-aware document identity, index identity, consistency routing, typed keys, encryption, or provider topologies | [`storage/table-identity.md`](storage/table-identity.md), [`storage/consistency-routing.md`](storage/consistency-routing.md), [`storage/persistence-engine-baseline.md`](storage/persistence-engine-baseline.md), [`storage/encryption.md`](storage/encryption.md), [`storage/provider-topologies.md`](storage/provider-topologies.md) | Cross-substrate storage/filesystem/object/volume seams are governed by `../plans/storage-seams-architecture.md`. |
| Node-compatible runtime, `deno_core`, `rusty_v8`, embedded codegen, runtime profiles, or fork promotion decisions | [`runtime/adapter-boundary.md`](runtime/adapter-boundary.md), [`server/auth-runtime-trust.md`](server/auth-runtime-trust.md), [`runtime/deno-fork-bump-ledger.md`](runtime/deno-fork-bump-ledger.md) | Canonical local refs are `~/src/github.com/nimbus/deno`, `~/src/github.com/nimbus/rusty_v8`, `~/src/github.com/denoland/deno`, and `~/src/github.com/nodejs/node`. |
| Horizontal scaling, cluster transport, node identity, placement, gossip invalidation, or cluster-mode integration | [`horizontal-scaling.md`](horizontal-scaling.md), then [`../plans/horizontal-scaling-plan.md`](../plans/horizontal-scaling-plan.md) | Single-node remains the launch baseline until a concrete multi-node consumer promotes the lane. |

## Repository Quality Map

The current architecture hardening baseline keeps large composition roots thin
and moves behavior into concept-owned modules:

- `nimbus-server::tenant` owns admission context, authority, stable
  identity, policy inputs, runtime admission, artifact provenance, image
  admission, audit evidence, and operator policy.
- `nimbus-server::construction` and `nimbus-server::router` are the canonical
  public server construction seams: `ServeOptions::new(service)` plus
  `serve(listener, options)`, and `RouterOptions::new(service)` plus
  `build_router(options)`.
- `nimbus-server::service_manager` remains the sandbox-backed service facade while
  activation, launch materialization, handle refresh, catalog, registry,
  verification, and system-state recording live under `service_manager/`.
- `nimbus-services` currently owns the named lifecycle for sandbox-backed
  Compose services. The canonical service model also reserves built-in and
  external implementation kinds for future SDK/control-plane work; services are
  addressed by tenant plus service name, while sandboxes remain
  id/handle-addressed isolated execution resources. Runtime isolates are
  invocation execution domains, not SDK sandbox resources; future explicit
  isolate-backed sandbox resources reserve `profile: "isolate"`.
- `nimbus-runtime::limits` owns backend axes, grants, resource budgets,
  adapter diagnostics, and policy wrappers without workspace dependencies.
- `nimbus-bin::dev` and `nimbus-bin::machine::handlers` delegate workflow
  phases to child modules so CLI roots stay as dispatch surfaces.
- JavaScript compatibility selftests are grouped by capability under
  `packages/firebase/src/selftest/`, and public compatibility bridges stay
  typed and local to the SDK package that owns them.

When adding a new enterprise capability, update the owning architecture doc and
run the guardrail script. A new root above the AGENTS.md size threshold should
either split by product concept or be added to the ledger with a narrow
justification.

## Crate mapping

| Directory | Crate | What's here |
|-----------|-------|-------------|
| [server/](server/) | `nimbus-server` | Adapter contracts, tenant isolation, local enforcement, auth/runtime trust boundary |
| [runtime/](runtime/) | `nimbus-runtime` | Runtime engine seam, host capability ownership, adapter boundary |
| [storage/](storage/) | `nimbus-storage` | Encryption design, persistence engine, provider topologies |
| [sandbox/](sandbox/) | `nimbus-sandbox` / `nimbus-services` | Service/sandbox/session/runtime-isolate model, microVM baseline, macOS machine flow, krun validation |
| [testing/](testing/) | `nimbus-testing` | Verification harness, reliability posture, CI failure playbook |
