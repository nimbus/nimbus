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
- [Backup and restore](/operators/backup-restore/) — `nimbus backup` for the
  embedded providers, plus what to capture per backend and how to restore it.
- [Object storage](/operators/object-storage/) — administer the object byte
  plane: placement policy, the master key, GC and erasure health, byte-plane
  backup and restore, and destructive tenant removal.

## Administration

- [Manage tenants](/operators/tenant-isolation/) — create and administer
  tenants on a running server.
- [Run Nimbus as a service](/operators/node-lifecycle/) — systemd and Quadlet
  service management with `nimbus node`.
- [Scale out](/operators/scale-out/) — grow past one machine by partitioning
  tenants across independent instances.
- [Inspect the server](/operators/observability/) — health, debug endpoints,
  logs, and the access audit log.
- [Security hardening](/operators/hardening/) — the checklist for exposed
  deployments.
- [Troubleshooting](/operators/troubleshooting/) — symptom, cause, fix.

When one machine runs out of headroom, [scale out](/operators/scale-out/) by
partitioning tenants across instances; the single-process growth model is
explained in [Concepts: scaling](/concepts/scaling/). The flag-by-flag
configuration tables live in the [Reference](/reference/configuration/)
section, and the isolation model itself is explained in
[Concepts: tenant isolation](/concepts/tenant-isolation/).
