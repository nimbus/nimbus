---
title: Tenant isolation
description: The isolation model between Nimbus tenants — storage scoping, runtime isolation, sandbox boundaries, and auth — and the reasoning behind it.
sidebar:
  label: Tenant isolation
  order: 3
---

A single Nimbus server hosts many tenants. Each tenant is a complete,
self-contained data and execution space: its own tables, documents,
functions, services, and storage namespace. This page explains what keeps
those spaces apart and why the design looks the way it does. For the
hands-on tasks — creating tenants, picking a storage provider, verifying a
deployment — see the
[tenant isolation operator guide](/operators/tenant-isolation/).

## Admit once, enforce everywhere

Nimbus treats tenant isolation as a control-plane contract, not as a side
effect of any single mechanism like a JavaScript isolate, a container, or
a microVM. The model has one rule: tenant-controlled intent never reaches
host paths, database namespaces, network listeners, runtime grants, or
secret material directly. It is first admitted — rewritten or rejected —
at the server boundary.

Admission turns the request's claimed identity and intent into a single
decision artifact that records the tenant, the authority class of the
caller, the workload identity, and the exact storage, service, network,
volume, image, secret, and quota authority granted. Every lower
enforcement point — the runtime, the sandbox launcher, the storage layer,
the network surface, the host-call bridge — consumes that admitted
decision or a narrow projection of it. None of them re-derives tenant
identity from anything the request supplied.

This shape exists for a reason: with one decision point, there is one
place where a tenant-swapping bug can live, and every layer below it
fails closed. A path segment, header, or token claim that names a
different tenant than the admitted one is a rejection, not a routing
choice.

## Storage scoping

Every tenant owns a separate storage namespace, and the namespace
boundary is structural rather than a `WHERE tenant_id = ?` filter:

- With the embedded providers, each tenant is a separate database file on
  disk.
- With Postgres, each tenant is a separate schema.
- With MySQL, each tenant is a separate database.
- With libSQL, each tenant is a separate namespace.

Queries, indexes, transactions, and subscriptions are all scoped to the
namespace selected by the admitted decision. There is no query shape that
spans two tenants, so cross-tenant reads aren't merely forbidden — they
are inexpressible at the storage layer.

Nimbus keeps its own control-plane state (service registrations, ports,
routes) in a reserved system tenant, `_nimbus`. Every tenant ID beginning
with `_` is reserved: application surfaces cannot create, list, or
address such tenants, and reaching system data requires operator
authority. The same boundary that separates two customers also separates
every customer from the server's own bookkeeping.

Deleting a tenant removes the whole namespace — runtime first, then
services, then the file, schema, database, or namespace itself — so
deletion cannot leave another tenant holding a dangling reference into
the removed data.

## Runtime isolation

Tenant functions execute in-process in V8 isolates, and the isolation
guarantee does not rest on V8 alone. Every host call a function makes —
reading a table, writing a document, scheduling work — flows through the
host bridge under a session bound to the admitted tenant. The function
has no API through which it can name a different tenant; the tenant is an
ambient fact of the invocation, fixed before any JavaScript runs.

Production servers also gate what a workload may ask of the host. A
workload's requested capabilities are checked against a tiered execution
model:

- **In-process untrusted** is the default tier for tenant code: no
  filesystem, subprocess, FFI, raw network listener, or environment-write
  authority.
- Workloads requesting privileged or trusted-only capabilities are kept
  out of the shared in-process tier.
- Workloads requesting process-like authority — subprocesses, FFI,
  workers, listeners, broad filesystem or loopback network access — must
  run as an isolated service in its own sandbox rather than in the shared
  engine.
- Non-JavaScript bundles route to a WASM capability sandbox.

The admission either routes the workload to a tier that can actually
contain it, or rejects it. Broad grants are never silently honored
in-process. Alongside the capability gate, per-tenant runtime budgets
(active, in-flight, and queued invocations, heap, and wall-clock time)
keep one tenant's load from starving another's.

## Sandbox and service isolation

Tenant services that do run as separate processes — container or microVM
sandboxes — stay inside the same admitted-decision model:

- A sandbox launch is validated against the decision's tenant, service,
  and backend before anything starts; a catalog entry pointing at the
  wrong tenant is treated as a state-integrity failure, not launched.
- Network exposure is private by default. Service ports bind to
  loopback-or-private addresses, and public exposure is not an admissible
  request.
- Outbound traffic is deny-by-default: a sandbox can reach only the
  egress endpoints its operator policy explicitly allows.
- Storage mounts are Nimbus-owned named volumes under the tenant's
  volume tree. Host bind mounts are denied.
- Production images must be pinned by immutable digest as the floor;
  tag-only references are rejected. Signature, provenance, and SBOM
  verification can be layered on top through operator-supplied tooling
  and trust roots.

## Auth boundaries

Three distinct authorities interact with tenants, and Nimbus keeps them
separate:

- **The operator** holds the local admin token and administers tenants
  through the native API. This token is server-wide authority: it can
  create, read, and delete any tenant. The server binds to loopback by
  default, and binding a non-loopback interface requires both an explicit
  opt-in flag and a recently rotated admin token.
- **Applications** authenticate with bearer tokens on application-facing
  surfaces. When an application principal carries a tenant claim
  (`tenant_id`, `tenantId`, `nimbus_tenant_id`, or `nimbusTenantId`), the
  claim must match the tenant the route addresses; a mismatch is a
  rejection. A token minted for one tenant cannot be replayed against
  another.
- **The system tenant** (`_nimbus`) accepts only operator authority.
  Application and unauthenticated callers cannot reach it on any
  surface.

Servers started with `nimbus start` always run in production isolation
mode; the relaxed local-development mode exists only in the
single-developer `nimbus dev` workflow and cannot be selected for a
production server.

Isolation-relevant events are recorded as structured audit events whose
schema redacts bearer claims, raw credentials, and secret handles by
design, so the audit trail itself cannot become a cross-tenant leak.

## What tenant isolation does not cover

The boundary is deliberate about its edges:

- **Authorization inside a tenant is the application's job.** Nimbus
  prevents cross-tenant widening; it does not replace your application's
  ACLs or row-level rules among its own users.
- **External storage configuration is the operator's job.** When tenant
  data lives in your Postgres, MySQL, or libSQL deployment, Nimbus scopes
  every tenant into its own schema, database, or namespace — but the
  database credentials, network placement, and prefix hygiene of that
  deployment are yours to manage.
- **Image verification beyond the digest floor uses your tooling.**
  Nimbus owns the policy and admission decisions; concrete signature,
  provenance, and SBOM verification rely on the tools and trust roots
  you configure.
- **Manual host edits are outside the contract.** State changed on the
  host behind Nimbus's back — files moved under a data directory,
  schemas altered directly in the database — is an operator
  responsibility.
- **A compromised host root account** is out of scope, as are physical
  attacks on the machine.

## Related pages

- [Manage tenants](/operators/tenant-isolation/) —
  create and administer tenants, configure storage scoping, verify
  isolation.
- [MongoDB tenant isolation](/reference/mongodb/tenant-isolation/) — how
  MongoDB database names map onto tenants on that surface.
- [Self-host quickstart](/get-started/self-host/) — run a server and
  create your first tenant.
