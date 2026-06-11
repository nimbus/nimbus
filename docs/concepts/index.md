---
title: Concepts
description: How Nimbus works — the engine, the data model, tenancy, and the architecture.
sidebar:
  order: 1
---

Explanation-oriented pages: how Nimbus works and why it's built the way it
is. Read these to build a mental model; reach for [Developers](/developers/)
or [Operators](/operators/) when you have a task in hand.

## The system

- [How Nimbus works](/concepts/how-nimbus-works/) — the one-engine,
  many-adapters model: server, adapters, engine, runtime, and storage in
  one binary.
- [Data and mutations](/concepts/data-and-mutations/) — documents, optional
  schemas, the single mutation path, and reactive subscriptions.
- [The adapter boundary](/concepts/adapter-boundary/) — how five protocol
  front doors share one engine without sharing each other's semantics.

## Trust and isolation

- [Tenant isolation](/concepts/tenant-isolation/) — the multi-tenant trust
  model: who a tenant is and what separates one from another.
- [Runtime permissions](/concepts/runtime-permissions/) — grants, mode
  ceilings, and why queries and mutations carry no ambient authority.

## Workloads

- [Services, sandboxes, and sessions](/concepts/resource-model/) — the
  resource model for long-running workloads beyond functions.
- [How the Node runtime works](/concepts/nodejs-runtime/) — why Node.js
  support is an in-process compatibility contract, not a Node process.

## Operations

- [Scaling](/concepts/scaling/) — what one Nimbus process gives you today
  and where the scaling boundaries are.

## Architecture

- [Architecture](/concepts/architecture/) — a system-by-system map of the
  binary: twelve pages covering the server, adapters, engine, storage,
  runtime, sandboxes, auth, tenancy, node lifecycle, CLI, SDK packages,
  and observability, each citing the crates that implement it.
