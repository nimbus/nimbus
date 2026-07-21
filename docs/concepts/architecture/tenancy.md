---
title: Tenancy
description: A crate-level tour of how Nimbus admits tenants once, carries the decision everywhere, and keeps every layer fail-closed by default.
sidebar:
  label: Tenancy
  order: 9
---

Every tenant-facing action in Nimbus — running a function, launching a
sandbox, touching storage — happens under an explicit, previously admitted
tenant decision. This page tours the crates that implement that model:
`crates/nimbus-tenant` (the decision engine), `crates/nimbus-system` (the
operator-side system tenant), and the per-tenant runtime state in
`crates/nimbus-engine`.

For the user-facing trust model — what isolation tenants can rely on and why —
see [tenant isolation](/concepts/tenant-isolation/). This page is about the
machinery underneath it.

## Admit once, carry the decision everywhere

`crates/nimbus-tenant` is built around a single pattern: a tenant is
**admitted once**, producing a `TenantIsolationDecision`, and every downstream
layer consumes that decision instead of re-deriving policy. The decision is an
envelope that binds together:

- the tenant identity, the surface being used, and the authority class
  (operator, application, or system),
- the deployment generation, location, and workload being authorized,
- nine policy decisions covering runtime, storage, network, images, volumes,
  secrets, services, audit, and invocation behavior.

The envelope's id is a content fingerprint: a SHA-256 over the identity and
policy fields, rendered as `tid_<hex>` (`crates/nimbus-tenant/src/decision.rs`).
Two admissions with the same inputs produce the same id; any policy change
produces a new one, so a decision id in a log line pins down exactly which
policy was in force.

Downstream code does not trust itself to use the decision correctly — the
envelope carries `ensure_*` checks that re-verify the tenant id, the storage
backend, the deployment generation, and the tenant label on a runtime bundle
at the point of use. A decision admitted for one tenant cannot be silently
replayed for another.

## Fail-closed policy inputs

The policy inputs that feed admission (`crates/nimbus-tenant/src/policy_input.rs`)
default closed in every family:

- network: no public exposure, no generic loopback access, sandbox egress
  deny-all,
- volumes: no host bind mounts,
- secrets: no ambient secret materialization,
- audit: principal claims, bearer claims, secret handles, and raw credentials
  are redaction-listed by default.

Opening any of these is a deliberate policy act, recorded in the decision
envelope, not a configuration drift.

`TenantIsolationMode` (`crates/nimbus-tenant/src/authority.rs`) has two
values, `Production` and `LocalDevelopment`, and **`Production` is the
`#[default]`** — the server's router builder admits tenants under
`TenantIsolationMode::default()`. Local development is the explicit opt-out,
never the fallback.

## Runtime admission tiers

Admission classifies each workload into a `RuntimeIsolationTier`
(`crates/nimbus-tenant/src/runtime_admission.rs`):

- `InProcessUntrusted` — the default for ordinary application JavaScript: V8
  isolates in the server process under deny-by-default permissions (see
  [runtime isolates](/concepts/architecture/runtime-isolates/) and
  [runtime permissions](/concepts/runtime-permissions/)).
- `MicroVmService` — required for grant families an in-process isolate cannot
  contain: subprocess (`run`), FFI, and worker grants; loopback or wildcard
  `net_connect`; any `net_listen`; and broad filesystem grants such as `/`,
  `*`, or the application, cache, and temp roots.
- `InProcessTrustedOnly` — required for operator-trust grants: environment
  writes, identity and tool grants, privileged mode, non-application presets,
  the inspector grant, and reading `NODE_TLS_REJECT_UNAUTHORIZED` on Node
  targets.
- `WasmCapabilitySandbox` — the routing for non-JavaScript bundle kinds.

The shape is deliberate: risk is expressed in the grant request, and admission
routes the workload to the weakest tier that can actually contain that grant,
rather than trusting the workload to behave.

## Image admission: digest-pinned floor

Sandbox and service images pass through image admission
(`crates/nimbus-tenant/src/image_admission.rs`) before launch. The floor
policy is digest pinning: tag-only references are rejected because production
launches require an immutable sha256 digest reference. Above the floor, policy
can pin an exact reference, restrict registries to an allowlist, deny local
builds (denied unless explicitly allowed), and require signature, provenance,
and SBOM verification through a pluggable `TenantImageVerificationProvider`.

## The system tenant and reserved identity

Operator-facing state lives in its own tenant: `_nimbus`
(`crates/nimbus-system/src/identity.rs`). The underscore prefix is reserved —
user tenant ids beginning with `_` are rejected at validation, so no
application tenant can collide with or impersonate system identity.
`crates/nimbus-system` defines the system tenant's eighteen tables
(`crates/nimbus-system/src/schema.rs`): machines, services, bundles,
functions, modules, source packages, tables, events, runs, scheduled jobs,
cron jobs, routes, listeners, subscriptions, ports, adapter capabilities,
system status, and workload status. Operator observability is therefore ordinary tenant data — queried,
indexed, and subscribed to through the same engine paths as application data,
just under an identity applications can never hold.

## Storage: one namespace per tenant

The storage policy decision carries a namespace derived from the tenant id,
and the engine's persistence provider opens, creates, and deletes storage
per tenant id — there is no shared cross-tenant table space at the
storage seam. How that namespace maps onto each storage backend is the
[storage](/concepts/architecture/storage/) page's territory; the tenancy
contract is simply that a tenant's reads and writes are scoped to its own
namespace before any query logic runs.

## Per-tenant engine state and budgets

Inside the engine, each admitted tenant gets a `TenantRuntime`
(`crates/nimbus-engine/src/tenant.rs`): its own persistence handle, schema
snapshot, subscription registry and delivery queue, document cache,
materialized reads, and trigger queues. Mutation traffic is admitted through a
per-tenant `MutationAdmissionGate` and journaled through per-tenant queues
with bounded default capacities — one tenant's write burst backs up its own
queue, not its neighbors'.

Execution capacity is budgeted per tenant as well. `RuntimeTenantBudget`
(`crates/nimbus-runtime/src/limits/resources.rs`) caps active runtime slots,
in-flight and queued top-level invocations, worker thread slots, isolate heap
sizes, execution timeout, and nested invocation depth.

## Evidence without leakage

Tenancy decisions produce audit evidence, and the evidence pipeline
(`crates/nimbus-tenant/src/evidence.rs`) assumes inputs may contain secrets.
Reason codes are canonicalized to bounded snake_case identifiers, and
free-form evidence text matching sensitive shapes — bearer tokens, passwords,
private keys, query strings — is replaced with a redaction marker before the
event (scoped `nimbus.tenant_isolation`) is recorded.

## Where this connects

- [Tenant isolation](/concepts/tenant-isolation/) — the user-facing trust
  model this machinery implements.
- [Manage tenants](/operators/tenant-isolation/) — operating
  the isolation modes.
- [Sandboxes and machines](/concepts/architecture/sandbox-machines/) — the
  backends behind the `MicroVmService` and container launch paths.
- [Storage](/concepts/architecture/storage/) — how per-tenant namespaces map
  onto storage backends.
- [Runtime isolates](/concepts/architecture/runtime-isolates/) — the
  in-process tier that `InProcessUntrusted` workloads run in.
