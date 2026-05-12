# Architecture

This directory holds internal architecture docs for contributors. The
subdirectory tree mirrors the Rust crate structure.

For the stable top-level architecture overview, see
[ARCHITECTURE.md](../../ARCHITECTURE.md).

## Crate mapping

| Directory | Crate | What's here |
|-----------|-------|-------------|
| [server/](server/) | `nimbus-server` | Adapter contracts, auth/runtime trust boundary |
| [runtime/](runtime/) | `nimbus-runtime` | V8 host capability ownership, adapter boundary |
| [storage/](storage/) | `nimbus-storage` | Encryption design, persistence engine, provider topologies |
| [sandbox/](sandbox/) | `nimbus-sandbox` | MicroVM baseline, macOS machine flow, krun validation |
| [testing/](testing/) | `nimbus-testing` | Verification harness, reliability posture, CI failure playbook |
