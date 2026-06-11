---
title: Services, sandboxes, and sessions
description: Nimbus's resource model — named services, isolated sandboxes, and scoped sessions — and why each noun is addressed and trusted differently.
sidebar:
  label: Resource model
  order: 7
---

Nimbus models long-running and interactive workloads with three nouns:
**services** (named capabilities your app depends on), **sandboxes**
(isolated execution environments), and **sessions** (scoped, expiring
connections to a running service or sandbox). This page explains what each
noun means and why the model is shaped the way it is. For the hands-on
walkthrough — creating, starting, and connecting to each with the
JavaScript SDK — see
[Manage services, sandboxes, and sessions](/developers/sdk/resource-model/).

## Three nouns, three addressing rules

The model is intentionally asymmetric about how things are named:

- **Services are addressed by name**, scoped to a tenant. A name like
  `search` or `worker` is part of your application's dependency contract —
  stable across restarts, redeploys, and backend changes.
- **Sandboxes are addressed by id.** Creating a sandbox returns a handle;
  every later operation — get, list, stop, open a session — targets that
  id. Sandboxes can carry labels for filtering, but there is no
  resolve-sandbox-by-name API.
- **Sessions target exactly one of the other two.** A session opens
  against `{ service: { name } }` or `{ sandbox: { id } }` — never both,
  never neither — and is itself addressed by the id the server returns.

This asymmetry is the core of the model. A name is a promise that other
code can depend on; an id is a receipt for a resource you created. Keeping
them distinct means a throwaway execution environment never silently
becomes something other code can resolve, and a named dependency never
depends on which particular instance happens to be running behind it.

## Services: named capabilities with a chosen backend

A service is a tenant-scoped definition: a name, a backend, labels, and a
generation counter that guards concurrent edits. The backend tells Nimbus
how the capability is provided, and there are three kinds:

- **Sandbox-backed** — Nimbus launches and supervises a sandbox from the
  spec embedded in the definition. This is the workhorse backend: the
  lifecycle verbs (`start`, `stop`, `restart`, readiness waiting) operate
  on sandbox-backed services, and it is also how Compose files lower —
  each Compose `services:` entry becomes a sandbox-backed service
  definition in the tenant's catalog.
- **Built-in** — the capability is implemented by the Nimbus server
  itself. The accepted providers are `loadBalancer`, `serviceDiscovery`,
  `browser`, and `modelGateway`.
- **External** — an endpoint Nimbus does not run. The definition records
  an absolute `http(s)` URL (embedded credentials are rejected), an auth
  policy, and an HTTP health-check path. Nimbus owns the definition,
  validation, and authorization around the endpoint; the process behind
  it is yours.

Today only sandbox-backed services can actually be launched by the service
manager; built-in and external services exist as validated definitions and
report a `declared` lifecycle state rather than a running one.

Two distinctions worth internalizing:

- **A service is not its sandbox.** The service is the name, readiness
  contract, and policy surface; the sandbox is the isolation mechanism
  currently running it. When a sandbox runs on behalf of a service, its
  spec carries owner metadata naming that service, and launch is rejected
  if the owner metadata and the service name disagree — but the sandbox's
  id never becomes the way applications address the capability.
- **Definitions come from two sources.** Services declared statically
  (such as those lowered from Compose) and services created dynamically
  through the API are the same resource kind with the same verbs. Dynamic
  definitions additionally carry generation-checked update and delete
  semantics, so two writers cannot silently overwrite each other.

## Sandboxes: isolated execution environments

A sandbox is a single isolated world: a root filesystem, a process to run,
resource limits, mounts, and a network policy, created for one purpose and
addressed by id for its whole life. The spec answers two separate
questions:

- **What root material runs?** Either a prepared root filesystem or OCI
  image material — and image material is in turn obtained either by
  *reference* (a registry image) or by *build* (a Dockerfile and context).
  A build is a way to obtain an image, not a different kind of sandbox.
- **Who owns it?** A sandbox is either owned by a service (launched as
  that service's backend) or standalone (created directly, with an
  optional display name). Creating a standalone sandbox never implicitly
  registers a service name.

Sandboxes are created under a **profile** — currently `worker` or
`desktop` — which states the intended workload shape, and run on one of
two isolation backends: an OCI container backend (the process-capable
default, driven through `crun`) and a libkrun-based microVM backend. The
microVM backend currently fails closed for process execution until its
network egress enforcement reaches parity with the container backend, so
container isolation is what runs workloads today. The important design
point is that the backend is a property of the deployment, not of the
resource: the same spec, id, lifecycle, and session semantics apply
regardless of which isolation mechanism is underneath.

Whatever the backend, sandboxes inherit the tenant-isolation posture:
launches are validated against the admitted tenant decision, outbound
network access is deny-by-default, and the API redacts launch inputs
(command lines, environment values, build paths) when it returns a
sandbox, reporting only value counts. See
[Tenant isolation](/concepts/tenant-isolation/) for the full model.

## Sessions: scoped, expiring connections

Simple service use needs no session — start the service, let your app use
the named dependency. A session exists for the *interactive* cases: a
shell or stdio stream into a running workload, file exchange, or a
browser-control channel. It is a lease, not a registry entry:

- **Every session expires.** Opening one takes an optional requested TTL;
  the server applies a default of fifteen minutes and caps every session
  at one hour. Sessions move through `open`, `closed` (explicitly closed,
  with a recorded reason), and `expired` states.
- **Channels are declared up front** and validated against what the
  target can actually offer: sandbox targets and sandbox-backed services
  support `stdio` and `files`; the built-in `browser` service supports
  `cdp` and `page`; other built-in providers and external services expose
  no session channels at all.
- **The target is pinned at open time.** The response carries a target
  snapshot — the service name or sandbox id, its generation, and its
  backend — so you can always tell exactly what a session attached to,
  even if the definition changes later. Opening against a dynamic service
  whose definition changed mid-open is rejected as a conflict rather than
  silently attaching to something else, and a sandbox target must be in
  the `ready` state.
- **Opening is an authorization event.** An application caller opening a
  service-targeted session needs an exact grant for that service name; a
  sandbox-targeted session needs reach to that specific sandbox id.
  Session opens, lookups, and closes are recorded as audit events.

## How the pieces compose

The flow for a Compose-style dependency ties all three together:

```text
service name ("db")
  -> service definition (sandbox-backed)
  -> sandbox launch (owner: service "db")
  -> ready service, addressed by name
  -> optional session (stdio/files), addressed by session id
```

For agentic and long-running workloads, the model gives you a default
shape: reach for a **standalone sandbox** when an agent needs an isolated
world for one task — it has an id, a lifecycle, and session access, and it
disappears without leaving a name behind. Promote work to a **service**
only when other code should be able to depend on it by name. Open a
**session** when something needs to interact with a running resource —
stream output, exchange files, drive a browser — under a lease that
expires on its own instead of an ambient connection that outlives its
purpose.

## What is not a resource

Ordinary Nimbus functions run in V8 isolates inside the server process —
see [How the Node runtime works](/concepts/nodejs-runtime/). Those
invocation isolates are execution machinery, not resources: they have no
name, do not appear in sandbox listings, and cannot be a session target,
even though isolate execution is an isolation mechanism in the general
sense. The resource model covers things you create and address; function
invocation stays an internal concern of the engine.

The same boundary applies to compatibility surfaces: adapter contexts
(Convex, Cloud Functions, and the other compatibility APIs) do not grow
`ctx.services`-style shortcuts. Code running on those surfaces that needs
Nimbus resources imports the `@nimbus/nimbus` SDK explicitly, and the same
authorization applies regardless of where the call originates.

## Related pages

- [Manage services, sandboxes, and sessions](/developers/sdk/resource-model/)
  — the SDK walkthrough for every verb described here.
- [Tenant isolation](/concepts/tenant-isolation/) — the admission and
  enforcement model these resources run inside.
- [Tenant isolation operator guide](/operators/tenant-isolation/) —
  operating the isolation boundary.
- [HTTP API reference](/reference/native/http-api/) and
  [error reference](/reference/native/errors/) — the wire surface and
  error catalog beneath the SDK.
