---
title: Operators
description: Self-host Nimbus for your team — deploy, operate, and administer a Nimbus server.
sidebar:
  order: 1
---

Guides for the people who run Nimbus: DevOps, platform, and admin teams
self-hosting a server for their organization — or developers hosting their
own. Start with the [self-host quickstart](/get-started/self-host/) if you
haven't run a server yet, then work through
[deploying to a Linux server](/operators/deploy-linux/).

## Install & deploy

- [Deploy to a Linux server](/operators/deploy-linux/) — the end-to-end
  tutorial: install, systemd unit, token, first request.
- [Container image](/operators/container-image/) — run the published OCI
  image with podman or docker.
- [Desktop install](/operators/desktop-install/) — the CLI and desktop app
  on a workstation.
- [Updates](/operators/updates/) — keep the binary current.

## Data

- [Storage backends](/operators/storage-backends/) — SQLite, redb,
  Postgres, MySQL, and libSQL topologies.
- [Encryption at rest](/operators/encryption/) — key providers, migration,
  and rotation.
- [Backup and restore](/operators/backup-restore/) — what to capture per
  backend and how to restore it.

## Administration

- [Tenant isolation](/operators/tenant-isolation/) — create and administer
  tenants on a running server.
- [Node lifecycle](/operators/node-lifecycle/) — systemd and Quadlet
  service management with `nimbus node`.
- [Observability](/operators/observability/) — health, debug endpoints,
  logs, and the access audit log.
- [Security hardening](/operators/hardening/) — the checklist for exposed
  deployments.
- [Troubleshooting](/operators/troubleshooting/) — symptom, cause, fix.

The flag-by-flag configuration tables live in the
[Reference](/reference/configuration/) section, and the isolation model
itself is explained in
[Concepts: tenant isolation](/concepts/tenant-isolation/).
