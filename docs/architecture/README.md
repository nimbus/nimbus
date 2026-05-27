# Architecture

This directory holds internal architecture docs for contributors. The
subdirectory tree mirrors the Rust crate structure.

For the stable top-level architecture overview, see
[ARCHITECTURE.md](../../ARCHITECTURE.md).

The repository-wide source ownership and architecture guardrail ledger lives at
[repo-architecture-quality-ledger.tsv](repo-architecture-quality-ledger.tsv).
Use it with `./scripts/verify-repo-architecture-quality.sh` before and after
large refactor waves.

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
- `nimbus-server::service_manager` remains the sandbox service facade while
  activation, launch materialization, handle refresh, catalog, registry,
  verification, and system-state recording live under `service_manager/`.
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
| [runtime/](runtime/) | `nimbus-runtime` | Runtime engine seam, V8 host capability ownership, adapter boundary |
| [storage/](storage/) | `nimbus-storage` | Encryption design, persistence engine, provider topologies |
| [sandbox/](sandbox/) | `nimbus-sandbox` | MicroVM baseline, macOS machine flow, krun validation |
| [testing/](testing/) | `nimbus-testing` | Verification harness, reliability posture, CI failure playbook |
