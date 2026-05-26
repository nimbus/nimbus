# Architecture

This directory holds internal architecture docs for contributors. The
subdirectory tree mirrors the Rust crate structure.

For the stable top-level architecture overview, see
[ARCHITECTURE.md](../../ARCHITECTURE.md).

The repository-wide source ownership and architecture guardrail ledger lives at
[repo-architecture-quality-ledger.tsv](repo-architecture-quality-ledger.tsv).
Use it with `./scripts/verify-repo-architecture-quality.sh` before and after
large refactor waves.

## Crate mapping

| Directory | Crate | What's here |
|-----------|-------|-------------|
| [server/](server/) | `nimbus-server` | Adapter contracts, tenant isolation, auth/runtime trust boundary |
| [runtime/](runtime/) | `nimbus-runtime` | Runtime engine seam, V8 host capability ownership, adapter boundary |
| [storage/](storage/) | `nimbus-storage` | Encryption design, persistence engine, provider topologies |
| [sandbox/](sandbox/) | `nimbus-sandbox` | MicroVM baseline, macOS machine flow, krun validation |
| [testing/](testing/) | `nimbus-testing` | Verification harness, reliability posture, CI failure playbook |
