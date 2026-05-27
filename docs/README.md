# Documentation

Start with the root [README.md](../README.md) for what Nimbus is, how to
install it, and a quick start. See [ARCHITECTURE.md](../ARCHITECTURE.md) for
how the system is built.

## Getting Started

- [Getting started](getting-started.md) -- install, pick your adapter, start building
- [Current capabilities](current-capabilities.md) -- what works today
- [Design system](../DESIGN.md) -- UI product language, information architecture, and component rules

## Adapters

Each adapter speaks a different client protocol against the same engine.

- [Convex](adapters/convex/) -- Convex-compatible queries, mutations, React hooks
- [Firebase / Firestore](adapters/firebase/) -- Firestore REST, gRPC-Web, WebSocket Listen
- [Cloud Functions](adapters/cloud-functions/) -- Firebase v2 triggers and HTTP handlers
- [MongoDB](adapters/mongodb/) -- MongoDB wire protocol with stock drivers
- [Native HTTP/WS](adapters/native/) -- REST and WebSocket API with the `nimbus` SDK

## Operating

- [CLI reference](operating/cli.md) -- server flags, service/machine commands
- [Node lifecycle](operating/node-lifecycle.md) -- native systemd,
  containerized Quadlet, machine-os, transient tenant units, and export
  boundaries
- [Container image](operating/container-image.md) -- GHCR image contract,
  digest pinning, probes, and orchestrator examples
- [Tenant isolation runbook](operating/tenant-isolation.md) -- rejection,
  drift, conformance, and incident-response workflow
- [Storage backends](operating/storage-backends.md) -- SQLite, Postgres, MySQL, libSQL, redb
- [Encryption at rest](operating/encryption.md) -- key providers, migration, recovery
- [Deploy admin API](operating/deploy-admin-api.md) -- staging, diffing, activation

## Runtimes

- [Runtimes](runtimes/) -- developer-facing runtime families and support posture
- [Node.js runtime](runtimes/nodejs/) -- `"use node"`, Node20 / Node22 / Node24 selection,
  packages, bundling, compatibility evidence, and current limits

## Architecture

Internal docs mirroring the crate tree. See
[architecture/README.md](architecture/README.md).

- [server/](architecture/server/) -- adapter contracts,
  [local enforcement](architecture/server/local-enforcement-boundary.md),
  auth/runtime trust
- [runtime/](architecture/runtime/) -- V8 host capabilities, adapter boundary,
  [runtime engine seam](architecture/runtime/engine-seam.md),
  [new engine proof harness](architecture/runtime/new-engine-proof-harness.md),
  [generated Node LTS compatibility baseline](architecture/runtime/node-lts-compat/node-lts-compat-summary.md),
  and [runtime surface matrix](architecture/runtime/node-compat-surface-matrix.md)
- [storage/](architecture/storage/) -- encryption design, persistence engine, provider topologies
- [sandbox/](architecture/sandbox/) -- microVM baseline, macOS machine flow, krun validation
- [testing/](architecture/testing/) -- verification harness, reliability posture, CI investigation
- [Architecture quality ledger](architecture/repo-architecture-quality-ledger.tsv) --
  owned-source size thresholds, generated/vendor/test-corpus exclusions, and
  helper/common naming exceptions

## Other

- [Tenant isolation](tenant-isolation.md) -- threat model, isolation matrix,
  evidence, residual risks, and external review targets
- [Technical debt](technical-debt.md) -- cross-cutting, actionable backlog
  discovered during architecture and compatibility hardening
- [Plans](plans/README.md) -- active execution plans and archived history
- [Research](plans/research/) -- background research and north-star direction
- [Demos](../demos/README.md) -- working example applications
