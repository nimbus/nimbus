# Documentation

Start with the root [README.md](../README.md) for what Neovex is, how to
install it, and a quick start. See [ARCHITECTURE.md](../ARCHITECTURE.md) for
how the system is built.

## Getting Started

- [Getting started](getting-started.md) -- install, pick your adapter, start building
- [Current capabilities](current-capabilities.md) -- what works today

## Adapters

Each adapter speaks a different client protocol against the same engine.

- [Convex](adapters/convex/) -- Convex-compatible queries, mutations, React hooks
- [Firebase / Firestore](adapters/firebase/) -- Firestore REST, gRPC-Web, WebSocket Listen
- [Cloud Functions](adapters/cloud-functions/) -- Firebase v2 triggers and HTTP handlers
- [MongoDB](adapters/mongodb/) -- MongoDB wire protocol with stock drivers
- [Native HTTP/WS](adapters/native/) -- REST and WebSocket API with the `neovex` SDK

## Operating

- [CLI reference](operating/cli.md) -- server flags, service/machine commands
- [Storage backends](operating/storage-backends.md) -- SQLite, Postgres, MySQL, libSQL, redb
- [Encryption at rest](operating/encryption.md) -- key providers, migration, recovery
- [Deploy admin API](operating/deploy-admin-api.md) -- staging, diffing, activation

## Architecture

Internal docs mirroring the crate tree. See
[architecture/README.md](architecture/README.md).

- [server/](architecture/server/) -- adapter contracts, auth/runtime trust
- [runtime/](architecture/runtime/) -- V8 host capabilities, adapter boundary
- [storage/](architecture/storage/) -- encryption design, persistence engine, provider topologies
- [sandbox/](architecture/sandbox/) -- microVM baseline, macOS machine flow, krun validation
- [testing/](architecture/testing/) -- verification harness, reliability posture, CI investigation

## Other

- [Plans](plans/README.md) -- active execution plans and archived history
- [Research](plans/research/) -- background research and north-star direction
- [Demos](../demos/README.md) -- working example applications
