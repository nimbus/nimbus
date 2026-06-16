---
title: Scaling
description: How a single Nimbus binary scales today — the tenant as the unit of isolation and growth, sandbox-backed services for heavy compute, external storage backends, and the limits of horizontal scale.
sidebar:
  label: Scaling
  order: 9
---

A Nimbus deployment today is one process. The single binary contains the
transport layer, the function runtime, the engine that coordinates every
mutation, and the storage layer — there is no cluster mode, no leader
election, and no coordination protocol between instances. The scaling story
that exists right now has four parts: tenants isolate load from each other
inside one server, the server scales up with the machine it runs on,
heavier compute fans out into sandbox-backed services beside the engine,
and external storage backends move data onto infrastructure you can grow
independently.

This page describes only what ships today. For the full capability
snapshot, see [current capabilities](/reference/current-capabilities/).

## The tenant is the scaling unit

Nimbus does not spread one application across machines. It isolates many
applications — tenants — inside one server so that load on one cannot
degrade the others. Every tenant owns, separately from every other tenant:

- **A storage namespace.** One SQLite or redb file, one Postgres schema,
  one MySQL database, or one libSQL namespace per tenant, depending on the
  backend. Cross-tenant reads are impossible by construction, and deleting
  a tenant removes its namespace as a unit.
- **A mutation path.** Each tenant has its own admission queue with its own
  capacity, its own load-shedding, and its own durable mutation journal.
  A tenant that saturates its write queue gets backpressure; its neighbors
  do not.
- **Function execution budgets.** Active, in-flight, and queued function
  invocations are capped per tenant. When a tenant's queue overflows, that
  tenant's requests are rejected with `429` — other tenants keep running.
- **Subscriptions and schedules.** Live-query registrations, scheduled
  jobs, and cron jobs are tracked per tenant. One tenant's mutations never
  invalidate another tenant's queries.

This is the same boundary described in
[tenant isolation](/concepts/tenant-isolation/), doing double duty: the
isolation contract is also the unit you grow by. Adding load means adding
tenants, and tenants compose without contending on each other's hot paths.

## Scaling up one server

Within a single process, the design keeps the expensive paths per-tenant
and the read paths off the write path:

- Reads are served from per-tenant materialized serving snapshots, so
  query traffic does not queue behind writes.
- Explicit queries and subscriptions plan against single-field and
  composite indexes rather than scanning tables.
- Subscription re-evaluation is dependency-tracked: a commit only triggers
  the queries whose dependency sets it intersects, not every query on the
  table.
- Functions execute in V8 isolates inside the server process — no
  per-invocation process or container cost. Workloads too heavy for an
  isolate fan out into sandbox-backed services, covered next.

The practical consequence: a single Nimbus server scales with the machine
you give it, and the per-tenant diagnostics endpoint reports each tenant's
queue depths, journal lag, and serving health so you can see which tenant
is consuming the headroom. See [observability](/operators/observability/).

## Heavy compute fans out into services

Functions are for short, transactional work. When a tenant's workload
includes something heavier or longer-lived — a worker process, an indexing
pipeline, a headless browser — it moves into a
[service](/agents/services/): a named, tenant-scoped workload backed by a
container sandbox that the server launches and supervises beside the
engine. The service exposes readiness and health states other code can
wait on, generation checks guard concurrent edits to its definition, and
its process runs as its own supervised container rather than inside the
engine — so the heavy work lives next to the data without sitting on the
mutation path.

This is fan-out within the deployment, not across machines. Sandbox-backed
services run on the same Linux host as the server (on macOS and WSL2,
`nimbus machine` provides that host), and they scale with that machine the
way everything else in the process does. What services buy you is
separation and supervision, not a second box — when you do outgrow the
machine, the partitioning story below is unchanged.

## Where the data lives

Storage is pluggable, and the choice is the main scaling lever you have
today. See [storage backends](/operators/storage-backends/) for setup.

| Backend | Data location | What scales it |
| --- | --- | --- |
| SQLite (default), redb | Files on the server's disk | The machine: disk, memory, CPU |
| PostgreSQL, MySQL | A database server you operate | Your database's own replication, pooling, and backup tooling |
| libSQL / Turso | A remote libSQL primary | The managed service, plus local replica reads |

With the embedded backends, the server and its data share one machine, and
scaling is vertical. With Postgres or MySQL, each tenant's schema or
database lives in infrastructure that already has its own growth, failover,
and backup story — Nimbus remains the single coordinator for mutations,
but durability and capacity become your database's job.

The libSQL backend adds a read-locality path: writes go to the remote
primary, while reads are served from a per-tenant local replica cache on
the Nimbus server. Replica freshness is measured and exposed in the
per-tenant engine diagnostics, so staleness is observable rather than
assumed away.

## What is not horizontally scalable today

Nimbus instances do not know about each other. Concretely, today there is:

- no built-in clustering or node membership,
- no request forwarding between instances,
- no cross-instance subscription invalidation, and
- no support for two instances serving the same tenant data.

Each tenant must be served by exactly one Nimbus process. If you outgrow
one server, the supported pattern is partitioning: run independent Nimbus
instances, place different tenants on each, and route clients to the
instance that owns their tenant. Because every tenant is a self-contained
namespace, moving one between instances is a data move, not a schema
surgery — but the routing is yours to operate.

Multi-node operation is a direction we are designing toward, not a shipped
capability, so this page does not describe it. When that changes, the
machinery will be documented here and in
[current capabilities](/reference/current-capabilities/) — until then,
size deployments on the assumption that one binary, one machine, and one
storage backend carry the load.
