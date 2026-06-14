---
title: Runtime permissions
description: What server-side functions can and cannot touch — the default-deny grant model, permission modes, and how risky workloads are routed out of the shared engine.
sidebar:
  label: Runtime permissions
  order: 6
---

Every server-side function in Nimbus executes under an explicit
permission policy. Nothing is ambient: a function that has not been
granted a resource by name cannot reach it, no matter which runtime
surface the code targets. This page explains the model from two angles —
for a developer asking *"what can my function touch?"* and for an
operator asking *"what's the blast radius if tenant code misbehaves?"*

## Default deny

A function starts with no filesystem access, no network access, no
environment variables, no subprocesses, no native code, and no
background workers. Capability comes only from **grants**, and grants
are exact — a list of named resources, not an on/off switch per
category. A representative few:

| Grant family | What it names |
| --- | --- |
| `read` / `write` | Filesystem roots, individually — including symbolic roots like the function's bundle directory and temp/cache roots. |
| `net_connect` / `net_listen` | Network hosts, individually. Outbound access to one host does not imply another. |
| `env_read` / `env_write` | Environment variables, by exact name. |
| `service` | Managed services the function may look up, by exact name. |

Other families name subprocesses, system metadata, native libraries, and
worker concurrency on the same exact-naming principle. A handful of
families — covering secrets, identity, and tooling — are reserved: they
are accepted in a policy but expose nothing at runtime today.

Database access is deliberately *not* in this list. Reading and
writing documents goes through host calls bound to the invocation's
tenant and principal — the same engine path every API request uses (see
[the adapter boundary](/concepts/adapter-boundary/)) — so data access
is governed by tenant admission and auth, not by a filesystem or
network grant.

## Modes are ceilings, not access

Above the grants sits a **permission mode** — `Restricted`, `Standard`,
or `Privileged`. A mode never grants anything; it caps which grants a
policy may carry at all:

- `Restricted` permits no grants of any kind. It is the ceiling for
  generated or maximally sandboxed code.
- `Standard` is the normal application ceiling: bounded grants are
  allowed, native FFI never is.
- `Privileged` allows explicitly configured grants for trusted operator
  workloads — and production admission refuses to run privileged
  policies as ordinary tenant code at all.

So the answer to "what can my function touch?" is always the
intersection: what the mode permits, what the grants name, and — next —
what the invocation kind allows.

## Queries and mutations are sealed; actions carry the grants

Permissions are also profiled by *what kind of function* is running.
Queries and mutations execute with ambient authority denied outright —
no network, no filesystem, no subprocess, no FFI — regardless of what
the policy grants. Only **actions** receive the policy's configured
authority. This keeps queries and mutations deterministic and
side-effect-free by construction; the only world they see is the
database, through the engine.

## "use node" changes the API surface, not the permissions

Compatibility targets and permissions are independent axes. A module
that opts into the Node runtime with `"use node"` gets Node's built-in
APIs; it does not get Node's traditional ambient authority. The
production Node application profile grants the function's own bundle
directories and a few pieces of system metadata — no network connect or
listen, no worker threads, no inspector, no subprocesses.

The local-development profile used by the dev workflow adds loopback
networking, worker threads, and inspector support for debugging — and
a production server will not admit that grant shape into the shared
engine. [How the Node runtime works](/concepts/nodejs-runtime/)
explains the compatibility model; the
[Node runtime guides](/developers/runtimes/nodejs/) cover usage.

## Risky policies are routed out of the shared engine

Operators should read the model as a routing decision, not just a
filter. Tenant functions normally run in-process, in the engine's
default untrusted tier. Before any tenant JavaScript runs, a production
server checks the workload's whole policy against that tier, and a
policy the tier cannot contain is either rejected or routed to an
isolation boundary that can hold it:

- Policies wanting subprocesses, FFI, workers, network listeners,
  loopback or wildcard network access, or broad filesystem roots belong
  in an **isolated service sandbox** (its own microVM or container),
  not in the shared engine process.
- Policies wanting privileged mode, identity or tool grants,
  environment writes, or the inspector are **trusted-only**: operator
  territory, never ordinary tenant code.
- Non-JavaScript bundles route toward a **WASM capability sandbox**
  tier rather than the in-process engine.

Two consequences matter operationally. First, broad grants are never
silently honored in-process — there is no configuration that quietly
turns a tenant function into a process with host authority. Second,
loopback access is treated as cross-service authority: a tenant
function cannot scan localhost for services it was never granted,
because generic loopback grants don't survive production admission.
Production is the default posture; the relaxed mode exists only in the
single-developer dev workflow.

## Defense in depth at use time

Admission is checked again where it counts. Sensitive host entrypoints
re-verify their own grant family at the moment of use: a managed
service lookup requires that exact service grant, spawning a worker
thread requires the worker grant, and a denied call fails with a
diagnostic naming the missing grant rather than an undefined error.

Around the permission checks sit resource bounds that hold even when
code does only what it was allowed to do: per-invocation heap limits, a
wall-clock execution watchdog, and per-tenant budgets on active,
in-flight, and queued invocations so one tenant's load cannot starve
another's. Function bundles are integrity-checked against their
expected digest before every invocation, so what executes is exactly
what was deployed.

## How this composes with tenant isolation

The permission model and [tenant isolation](/concepts/tenant-isolation/)
answer different questions on purpose. Tenant admission decides *whose
data and services* an invocation may address; the runtime permission
policy decides *which host resources* the code may use while doing it.
A function with generous grants still cannot name another tenant, and a
function admitted to a tenant still cannot open a socket it was never
granted. For the operator-facing tasks — verifying a deployment's
posture, configuring isolation — see the
[tenant isolation operator guide](/operators/tenant-isolation/).

## Related pages

- [The adapter boundary](/concepts/adapter-boundary/) — why function
  host calls and API requests share one engine path.
- [How the Node runtime works](/concepts/nodejs-runtime/) — the
  compatibility contract behind `"use node"`.
- [Tenant isolation](/concepts/tenant-isolation/) — the boundary around
  every invocation.
