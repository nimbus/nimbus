# Plan: External SQL Storage Backends

Deferred follow-on plan for Postgres and MySQL as Neovex internal storage
backends after the SQLite migration is complete and stable.

---

## Status

- **Status:** `deferred`
- **Primary owner:** this plan
- **Activation gate:** SQLite storage migration complete, redb removed, and
  explicit product demand for external database deployments

## Purpose

This plan owns future work for Postgres and MySQL internal storage. It is
separate from the SQLite migration because external backends change the async
model, configuration model, tenant isolation model, pooling model, operational
story, and change-notification strategy.

Those concerns should not shape the SQLite migration seam prematurely.

## Scope

This plan will later cover:

- the stable engine-facing abstraction to use after SQLite settles
- external backend configuration (`database_url`, pools, schemas/databases per
  tenant)
- Postgres and MySQL transaction, journal, notification, and recovery design
- benchmarking and operational criteria for external deployments

This plan does not cover:

- the current SQLite migration and redb removal work
- user-facing `env.DB` / `env.HYPERDRIVE` bindings

## Initial Guardrails

- derive any future abstraction from the SQLite-backed engine contract, not a
  greenfield CRUD trait sketch
- do not assume `Path`-based tenant construction generalizes to external
  backends
- keep `CommitEntry` and durable journal semantics explicit unless they are
  intentionally redesigned in a dedicated later review
- require a fresh benchmark and operational plan per external backend

## Execution Log

| Date | Phase | Outcome | Summary | Verification | Next Step |
|------|-------|---------|---------|--------------|-----------|
| 2026-04-08 | meta | created | Split Postgres/MySQL out of the SQLite migration plan so external backends no longer constrain the current storage replacement seam. | doc review | activate only after SQLite migration stabilizes |
