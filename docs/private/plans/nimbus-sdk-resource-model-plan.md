# Plan: Nimbus SDK Resource Model

## Status

- **Status:** `done`
- **Primary goal:** implement the `@nimbus/nimbus` resource model described in
  [`docs/architecture/sandbox/service-sandbox-session-model.md`](../architecture/sandbox/service-sandbox-session-model.md).
  The landed SDK owns service lifecycle/status, service-definition CRUD,
  standalone sandbox resources, and scoped service/sandbox session leases through
  server-backed routes.
- **Activation prerequisites:**
  - `docs/plans/service-backend-and-sandbox-spec-refactor-plan.md` completed
    SBR0 through SBR6 on 2026-06-09. SRM0 and SRM1 may start from the current
    landed baseline instead of waiting for another naming/refactor wave.
  - `docs/plans/nimbus-capability-segregation-plan.md` completed CB3 for the
    top-level `@nimbus/nimbus` SDK, default credentials, and low-level transport
    split. Verify that baseline before implementing resource phases.
  - The same plan completed the SDK transport boundary: `new Nimbus()` selects
    authenticated control-plane transport by default, while any private
    Nimbus-managed isolate host transport is installed only for an allowed
    backend, invocation tier, principal class, and exact grant set. Bun/JSC or
    another isolate backend must fail closed until it proves equivalent gating.
  - The same plan completed CB8's principal-class route gate for service
    control-plane routes. SRM2 through SRM4 must extend that gate to new
    service-definition, sandbox, and session routes before exposing those SDK
    methods.
  - SRM2 through SRM4 must not expose new public SDK methods until their matching
    server routes, resource-shaped responses, authorization tests, audit records,
    and verifier conditions land in the same phase.
  - Session channels that depend on desktop/GPU/libkrun media plumbing wait for
    the corresponding band in `docs/plans/nimbus-sandbox-plan.md`.

This plan is the SDK/control-plane follow-on. It does not rename Compose
services and does not replace the sandbox backend plan.

## Current Baseline

Repo audit and implementation on 2026-06-09 completed this plan:

- `packages/nimbus/package.json` names the package `@nimbus/nimbus` and exports
  the root SDK plus `./transports/rest`.
- `packages/nimbus/src/index.ts` exposes `new Nimbus()` with default endpoint
  and credential discovery.
- `Nimbus.services` exposes `start`, `stop`, `restart`, `get`, and `wait`.
  `ensureRunning` is intentionally absent.
- `Nimbus.services` also exposes server-backed service-definition
  `create`/`update`/`delete`/`list` APIs for sandbox-backed, built-in, and
  external service definitions.
- `Nimbus.sandboxes` exposes server-backed `create`, `get`, `list`, and `stop`
  APIs for id-addressed standalone sandbox resources.
- `Nimbus.sessions` exposes server-backed `open`, `get`, `list`, and `close`
  APIs for scoped service/sandbox session leases with target snapshots, TTL
  expiration, and channel admission.
- `crates/nimbus-server` exposes canonical service routes:
  `GET /api/tenants/{tenant_id}/services/{service_name}` and
  `POST /api/tenants/{tenant_id}/services/{service_name}/{start|stop|restart}`.
- `docs/plans/service-backend-and-sandbox-spec-refactor-plan.md` has completed
  SBR0 through SBR6, so the Rust vocabulary this plan depends on is now
  `ServiceBackend`, `BuiltInServiceSpec`, `ExternalServiceSpec`,
  `SandboxSpec.root`, and `SandboxRootSpec`.

SRM0 through SRM6 are complete. Use the resource-model verifier as the primary
guard against regressions, future-looking SDK stubs, adapter context leaks, and
service/sandbox/session authority drift.

## Execution Order

1. SRM0/SRM1 registered the SDK resource-model verifier and closed out the
   service lifecycle/status SDK baseline.
2. SRM2 implemented service definition CRUD as server-backed resource code, not
   SDK-only stubs.
3. SRM3 implemented id-addressed standalone sandbox resources without creating
   a service-name side channel.
4. SRM4 implemented scoped session resources with server-side target reach,
   channel admission, TTL expiration, and audit.
5. SRM5/SRM6 added examples, verifier hardening, and final closeout.

## Resource Model

Follow the architecture vocabulary exactly:

- **Service:** named tenant-scoped app dependency or capability. Services are
  addressed by tenant plus service name and expose lifecycle, readiness,
  endpoints, and optional session channels. A service backend may be sandbox,
  built in, or external. Compose `services:` entries are the existing static
  sandbox-backed declaration source.
- **Sandbox:** isolated execution resource. Sandboxes are addressed by sandbox id
  or returned handle, never by name. Labels may support filtering and diagnostics
  but must not become authority-bearing names.
- **Session:** scoped lease/interactions channel over a target. A session target
  is either `{ service: { name } }` or `{ sandbox: { id } }`. Sessions are not a
  third name registry.
- **Runtime isolate:** invocation execution domain owned by `nimbus-runtime`.
  Runtime isolates are not SDK sandbox resources and are not session targets.
  If Nimbus later exposes isolate execution as a user-created sandbox resource,
  the reserved profile is `profile: "isolate"` and it must satisfy the normal
  sandbox contract.

The current MVP SDK shape is:

```ts
import { Nimbus } from "@nimbus/nimbus";

const nimbus = new Nimbus();

await nimbus.services.start({ name: "search", waitUntil: "ready" });
await nimbus.services.get({ name: "search" });
```

The landed SDK resource shape is:

```ts
const sandbox = await nimbus.sandboxes.create({ profile: "desktop" });

// Reserved future shape for explicit isolate-backed sandbox resources.
// This is not how ordinary function invocation isolates are addressed.
const worker = await nimbus.sandboxes.create({ profile: "isolate" });

const desktop = await nimbus.sessions.open({
  target: { sandbox: { id: sandbox.id } },
  channels: ["screen", "input", "files"],
});

const tools = await nimbus.sessions.open({
  target: { service: { name: "mcp-tools" } },
  channels: ["stdio", "events"],
});

const browser = await nimbus.sessions.open({
  target: { service: { name: "browser" } },
  channels: ["cdp", "page", "files"],
  profile: "research",
});
```

Transport selection stays internal to the SDK. Product APIs live on
`Nimbus.services`, `Nimbus.sandboxes`, and `Nimbus.sessions`; low-level
`@nimbus/nimbus/transports/*` entries are explicit protocol plumbing. This plan
must not add public adapter or runtime-context shortcuts for services,
sandboxes, sessions, browser, model, audio, video, or content capabilities.

## Non-Escape Rules

- Do not add `resolveSandboxByName`, `sandboxes.get({ name })`, or any
  authority-bearing sandbox-name lookup.
- Do not implicitly publish a service when creating a sandbox.
- Do not make sessions the only way to use simple service lifecycle/status.
  `services.start(...)`, `services.stop(...)`, `services.restart(...)`, and
  `services.get(...)` are the canonical service lifecycle/status APIs.
- Do not expose a public raw service-binding resolver in the MVP. Internal
  tenant-plus-service-name resolution remains available to runtime bindings,
  service-targeted sessions, autoscaling, and load-balancer implementations. A
  public resolver requires a later explicit product need, such as sandboxed nginx
  consuming generated upstreams.
- Do not expose services, sandboxes, or sessions through adapter ctx APIs.
  Adapter packages stay adapter-shaped.
- Do not expose built-in service/session capabilities through adapter ctx APIs
  or raw runtime host ops. Nimbus-managed isolate host transport is a private
  SDK implementation detail, not a public `ctx` surface or public
  `@nimbus/nimbus/transports/host` entry.
- Do not let a tenant/spawned principal resolve operator authority through
  default credentials, sessions, services, or sandboxes.
- Do not add wildcard service grants or broad "can use all services" booleans.
- Do not model runtime isolates as SDK sandboxes, service targets, or session
  targets.
- Do not introduce `profile: "js-isolate"` or another synonym. If isolate-backed
  execution becomes a user-created sandbox resource, the profile spelling is
  exactly `profile: "isolate"` and the resource must have id/handle lifecycle,
  policy, quotas, audit, tenant binding, and supported session-channel rules.

## Canonical Control-Plane Contracts

These contracts are implementation requirements, not illustrative examples. A
public SDK method may land only when the matching server route, authorization
tests, response type, audit record, verifier condition, and documentation land in
the same phase.

### Common Resource Contract

- Resource responses use stable `metadata`, `spec`, and `status` sections once a
  route owns declarative state. The already-landed service `GET` response may
  remain flat for SRM1, but SRM2 must add the resource-shaped projection without
  removing the stable top-level service status fields that existing tests rely
  on.
- `metadata` includes `tenantId`, resource id or name, opaque
  `resourceVersion` or response `etag`, `generation`, `createdAt`, `updatedAt`,
  labels when supported, and owner/audit correlation when safe to reveal to the
  caller. `generation` tracks desired-state changes; `resourceVersion`/`etag`
  is the optimistic-concurrency token clients echo through `If-Match` or typed
  preconditions.
- `spec` is desired state. It is omitted or redacted unless the caller is
  authorized to inspect configuration.
- `status` is observed state. It may include lifecycle state, readiness, health,
  endpoints, selected backend, service generation, session channels, and close
  reason as appropriate for the resource. Every durable resource status includes
  machine-readable conditions with `type`, `status`, `reason`, `message`,
  `observedGeneration`, and `lastTransitionTime`; free-form state strings are
  shortcuts, not the only status contract.
- List responses are resource-shaped collections, not raw arrays. They include
  collection metadata with an opaque `resourceVersion`, optional continuation
  token, optional remaining count, and bounded `limit` handling. List filters
  must be explicit, index-backed where needed, and tenant-scoped; no global
  all-tenant list is added outside a separate audited operator-only route.
- Mutating resource-definition routes accept an idempotency key when the
  operation can be retried and use generation or `If-Match` style preconditions
  for replacement/deletion of existing resources. Blind overwrites are not
  allowed.
- All routes require either configured operator auth or authenticated
  tenant/spawned workload identity. If local server security is absent, operator
  auth is not silently synthesized; the route must fall through to tenant or
  spawned workload identity and fail closed when that identity is missing.
- Tenant route claims are mandatory for service, sandbox, and session
  control-plane routes. A route tenant, request tenant, constructor tenant, and
  authenticated principal tenant must match when more than one is present.
- Service lifecycle, status, endpoint, and service-target session access are
  authorized by exact service grants only. Wildcard service grants and broad
  "all services" booleans are invalid authorization sources.
- Service definition administration is separate authority. Creating, listing,
  inspecting configuration, updating, deleting, changing backend specs, changing
  endpoint/session/admission policy, or granting service access requires
  explicit service-definition permission scoped by tenant, service selector, and
  action, or configured operator auth. An exact service grant lets a principal
  reach the named service; it never lets that principal mutate the service
  definition or grant access to others.
- Service-definition list permission is inventory authority, not inspect
  authority. List-only callers receive metadata/status plus a redacted backend
  kind; full backend specs and external endpoint policy require inspect
  permission or configured operator auth.
- Sandbox and session control routes use their own explicit permissions and
  tenant ownership checks; they do not imply service reach or service-definition
  administration.
- Unsupported backend kinds, channels, profiles, health probes, build sources,
  or endpoint policies fail before creating partial resources.
- Route tenant, path resource name, and body `metadata` fields must either agree
  or be omitted from the body and filled by the server. A request body must not
  override the tenant or resource name carried by the route.
- Public JSON discriminators use the SDK/API spelling exactly:
  `kind: "sandbox"`, `kind: "builtIn"`, and `kind: "external"`. Rust enum names
  and log labels may differ, but serializers, SDK types, docs, and verifier
  guards must agree on this wire spelling.
- Long-running lifecycle or definition operations must not rely on one blocking
  HTTP request. If a start/stop/restart/create/update/delete operation may
  exceed the request budget, the route returns a current resource projection plus
  an operation id or condition, and the SDK waits by polling the canonical `GET`
  route.

### Service Routes And SDK Contract

The existing lifecycle/status routes remain canonical:

```http
GET  /api/tenants/{tenant_id}/services/{service_name}
POST /api/tenants/{tenant_id}/services/{service_name}/start
POST /api/tenants/{tenant_id}/services/{service_name}/stop
POST /api/tenants/{tenant_id}/services/{service_name}/restart
```

The matching stable SDK methods are:

```ts
await nimbus.services.start({ name: "db" });
await nimbus.services.start({ name: "db", waitUntil: "ready" });
await nimbus.services.stop({ name: "db" });
await nimbus.services.restart({ name: "db" });
await nimbus.services.get({ name: "db" });
await nimbus.services.wait({ name: "db", until: "healthy" });
```

SRM1 records the MVP status semantics:

- `waitUntil: "ready"` and `wait({ until: "ready" })` poll `GET service` until
  readiness is `ready`.
- `wait({ until: "stopped" })` succeeds when readiness or lifecycle state is
  `stopped`.
- `wait({ until: "healthy" })` succeeds only when health is `healthy`. In the
  MVP, sandbox-backed services may derive `healthy` from the backend ready state.
  Rich probe-backed health can refine the status later, but it must preserve the
  public wait contract.

SRM2 adds declarative service-definition routes for dynamic services. The
service resource remains tenant plus name; definition CRUD and lifecycle verbs
are separate concerns:

```http
GET    /api/tenants/{tenant_id}/services
POST   /api/tenants/{tenant_id}/services
PUT    /api/tenants/{tenant_id}/services/{service_name}
DELETE /api/tenants/{tenant_id}/services/{service_name}
```

Definitions use a resource-shaped body:

```ts
type NimbusServiceDefinition = {
  metadata: {
    tenantId?: string;
    name: string;
    generation?: number;
    labels?: Record<string, string>;
  };
  spec: {
    backend: NimbusServiceBackendSpec;
    readiness?: NimbusReadinessPolicy;
    endpointPolicy?: NimbusEndpointPolicy;
    sessions?: NimbusSessionPolicy;
    ttl?: NimbusTtlPolicy;
    idle?: NimbusIdlePolicy;
    admission?: NimbusAdmissionPolicy;
    access?: NimbusServiceAccessPolicy;
  };
};

type NimbusServiceBackendSpec =
  | { kind: "sandbox"; sandbox: NimbusSandboxSpec }
  | {
      kind: "builtIn";
      provider: NimbusBuiltInProviderId;
      policy?: NimbusBuiltInProviderPolicy;
    }
  | {
      kind: "external";
      endpoint: NimbusExternalEndpointPolicy;
      auth: NimbusExternalAuthPolicy;
      health: NimbusHealthCheckPolicy;
    };

type NimbusServiceAccessPolicy = {
  definitionPermissions?: NimbusServiceDefinitionPermission[];
  exactServiceGrants?: NimbusExactServiceGrant[];
};

type NimbusServiceDefinitionPermission = {
  principal: NimbusPrincipalSelector;
  actions: Array<"create" | "list" | "inspect" | "update" | "delete" | "grant">;
  scope: NimbusServiceDefinitionScope;
};

type NimbusServiceDefinitionScope =
  | { kind: "exactName"; name: string }
  | { kind: "namePrefix"; prefix: string };

type NimbusCondition = {
  type: string;
  status: "True" | "False" | "Unknown";
  reason: string;
  message: string;
  observedGeneration?: number;
  lastTransitionTime: string;
};
```

The SDK names for definition CRUD are explicit resource verbs:

```ts
await nimbus.services.create({ name: "worker", backend: { kind: "sandbox", sandbox } });
await nimbus.services.update({ name: "worker", ifMatchGeneration, backend });
await nimbus.services.delete({ name: "worker", ifMatchGeneration });
await nimbus.services.list();
```

`create` returns `409 Conflict` if the service already exists. `update` requires
a generation/resource-version precondition and returns `412 Precondition Failed`
when the caller's view is stale. `delete` refuses running services or live
sessions unless the request includes an explicit force policy that the server
authorizes and audits separately from normal definition administration; tenant
or spawned-workload force delete also requires exact service reach so definition
permissions cannot silently stop a service or close its sessions. Required
delete semantics must not depend on a `DELETE` request body that proxies may
drop; use `If-Match`, typed query parameters, or a separate audited action route
for rich force/grace policy.
Built-in and external definitions are not placeholders: a built-in provider or
external endpoint is accepted only when Nimbus can validate its provider,
policy, readiness, and auth behavior for that tenant. Otherwise the create or
update request fails actionably and creates no definition.

`NimbusBuiltInProviderPolicy`, `NimbusExternalEndpointPolicy`,
`NimbusExternalAuthPolicy`, and `NimbusHealthCheckPolicy` are closed,
discriminated policy types, not arbitrary JSON blobs. SRM2 must define the
minimal concrete variants needed by implemented providers before accepting
create/update requests. Built-in providers come from a server-side registry;
unknown providers fail before persistence. External endpoints require concrete
endpoint, auth, health/readiness, egress, and audit policy, and secrets are
stored through the approved secret path rather than echoed in resource
responses. External service variants that need secrets are blocked until the
approved secret-reference path exists; do not accept inline secret values as a
temporary bridge.

### Sandbox Routes And SDK Contract

SRM3 adds tenant-scoped sandbox resource routes:

```http
GET  /api/tenants/{tenant_id}/sandboxes
POST /api/tenants/{tenant_id}/sandboxes
GET  /api/tenants/{tenant_id}/sandboxes/{sandbox_id}
POST /api/tenants/{tenant_id}/sandboxes/{sandbox_id}/stop
```

The matching SDK surface is:

```ts
const sandbox = await nimbus.sandboxes.create({
  profile: "desktop",
  root,
  ttl,
  policy,
  labels,
});

await nimbus.sandboxes.get({ id: sandbox.id });
await nimbus.sandboxes.list({ labels, status });
await nimbus.sandboxes.stop({ id: sandbox.id });
```

Sandbox create records `profile`, `root`, policy, TTL, labels, owner, backend,
and audit correlation. It returns a sandbox id/handle and never creates a
service name. Labels are query metadata only; they must not authorize `get`,
`stop`, session open, or endpoint access. A sandbox-backed service may record
`SandboxOwnerSpec::Service { name }` in the sandbox spec, but public sandbox
routes still target the returned sandbox id.

Sandbox route authorization requires tenant match plus explicit sandbox
permissions for create/list/get/stop. A spawned workload may only access
sandboxes owned by its tenant and permitted by policy. Operator access is
available only through configured operator auth and is audited as operator
access.

### Session Routes And SDK Contract

SRM4 adds resource-session routes:

```http
POST /api/sessions
GET  /api/sessions/{session_id}
GET  /api/sessions?tenantId={tenant_id}
POST /api/sessions/{session_id}/close
```

The SDK uses `open`, not `create`, because a session is a scoped interaction
lease rather than a named resource definition:

```ts
const browser = await nimbus.sessions.open({
  target: { service: { name: "browser" } },
  channels: ["cdp", "page"],
});

const shell = await nimbus.sessions.open({
  target: { sandbox: { id: sandbox.id } },
  channels: ["stdio", "files"],
});

await nimbus.sessions.get({ id: browser.id });
await nimbus.sessions.close({ id: browser.id });
```

`POST /api/sessions` accepts `tenantId?`, target, channels, requested TTL,
profile/config when supported by the target, and optional rebind policy. The
server resolves the tenant from the request, client options, or authenticated
principal and rejects ambiguity or mismatch. A service target requires an exact
service grant to the named service plus channel policy. A sandbox target
requires exact sandbox id ownership and sandbox session permission. Session ids
are opaque server ids; sessions are not name-addressable.

`GET /api/sessions?tenantId={tenant_id}` uses `tenantId` as the route tenant.
Tenant and spawned-workload callers must resolve to exactly that tenant and
cannot omit it. Application principals receive only sessions whose target they
can reach: exact service grants for service-target sessions and sandbox reach
for sandbox-target sessions. Operator callers may list another tenant's sessions
only through configured operator auth, with the target tenant explicitly
supplied and audited. This plan does not add a global all-tenant session listing
route; if one is ever needed, it must be a separate operator-only route with
pagination, filters, audit, and abuse controls.

The response records `id`, `tenantId`, target, resolved service generation or
sandbox id, granted channels, selected backend/provider, actual `expiresAt`,
status, close reason when closed, and connection descriptors only for channels
the caller may use. TTL and expiration are server-owned in this plan: clients may
request a TTL, but SRM4 does not add `renew` or `extend`. Unsupported channels,
expired targets, stopped sandboxes, denied service grants, or unavailable
built-in providers fail before a session row or channel allocation is created.

Default service-target rebind policy is `pin_generation`: the session records
the service generation selected at open time. A future explicit
`latest_on_reconnect` policy may be added only with route tests proving that
authorization and channel policy are rechecked on every rebind.

## Control Rules

- Use this plan and
  `docs/architecture/sandbox/service-sandbox-session-model.md` as the source of
  truth. Do not rely on chat history for the resource vocabulary.
- Keep one phase `in_progress` at a time.
- Update the phase status ledger and execution log before handoff or closeout.
- A phase is done only when its gate has concrete command/test evidence.
- If activation prerequisites cannot be verified in the current repo state,
  complete SRM0 only and leave the dependent phases `todo`.
- Do not add a public SDK namespace, method, type export, or example for a
  resource operation before the server route and verifier guard exist. This rule
  is the primary defense against future-looking SDK stubs.

## Phase Status Ledger

| Phase | Status | Hard dependencies | Verifiable success signal |
| --- | --- | --- | --- |
| SRM0 | `done` | none | Plan registered; architecture model linked; verifier bootstrap passes registration/model checks. |
| SRM1 | `done` | capability-segregation CB3; current service SDK/server baseline | Existing `@nimbus/nimbus` service lifecycle/status APIs are verified, package selftests pass, stale lifecycle/session verbs are rejected, and adapter export/type-surface guards prove no resource namespace leakage. |
| SRM2 | `done` | SRM1; capability-segregation CB8 | Static and dynamic services support sandbox-backed, built-in, and external definitions through the tenant service resource contract with exact-grant reach coverage and separate service-definition administration permissions. |
| SRM3 | `done` | SRM1; capability-segregation CB8 | Sandbox APIs are id/handle-addressed only; labels cannot confer authority; sandbox creation does not publish a service. |
| SRM4 | `done` | SRM2; SRM3; required sandbox-plan backend bands for channel types | Sessions open only against `{ service: { name } }` or `{ sandbox: { id } }`, enforce TTL/audit/channel policy, and fail closed on unsupported channels. |
| SRM5 | `done` | SRM2-SRM4 | App/agent examples typecheck and demonstrate service, sandbox, and session usage without adapter ctx shortcuts. |
| SRM6 | `done` | SRM1-SRM5 | Final resource-model verifier, capability-segregation verifier, and required JS/Rust/docs gates pass with evidence. |

## Phases

### SRM0 - Control Plane Bootstrap

- Goal: make this plan executable from disk.
- Files: this plan, `docs/plans/README.md`, `AGENTS.md`, new
  `scripts/verify-nimbus-sdk-resource-model.sh`.
- Steps: register the plan, add the verifier with PASS/FAIL-counter shape, and
  make it self-check the architecture model doc, plan registration, and
  AGENTS.md routing.
- Gate: docs refs pass; the SRM0 verifier contains only registration/model-doc
  conditions and passes. Later phases add their own verifier conditions before
  claiming the corresponding implementation done.

### SRM1 - SDK Service Baseline Closeout

- Goal: reconcile and verify the already-landed typed service lifecycle/status
  APIs on the top-level `Nimbus` client without widening adapter surfaces or
  exposing sandbox/session methods before server-backed routes exist.
- Files: `packages/nimbus/src/**`, `packages/nimbus/package.json`,
  package selftests/typecheck fixtures, SDK docs.
- Steps: inspect the current root SDK and preserve or finish stable TypeScript
  service request/response types for `start`, `stop`, `restart`, `get`, and
  `wait`. Confirm calls route to the tenant-scoped service endpoints. Do not
  expose `sandboxes` or `sessions` methods until SRM3/SRM4 implement real
  server-backed resource routes, response types, authorization tests, audit
  records, documentation, and verifier guards. Typed fail-closed errors belong
  inside implemented routes for unsupported profiles, channels, or providers;
  they are not a reason to ship public SDK methods ahead of real capability.
- Gate: `import { Nimbus } from "@nimbus/nimbus"` typechecks with service
  lifecycle/status APIs; package selftests reject stale `ensureRunning`,
  `/api/services/{name}/ensure-running`, `sessions.create`, `sessions.renew`,
  and `sessions.extend`; adapter package
  export/type-surface guards prove they do not re-export Nimbus resource
  namespaces. Tests cover `waitUntil: "ready"`, `wait({ until: "healthy" })`,
  and timeout/failure behavior against the real `GET service` status path.
  Lifecycle-specific type or runtime validation rejects nonsensical combinations
  such as `stop({ waitUntil: "ready" })`; `start`/`restart` may wait for
  `ready` or `healthy`, while `stop` waits for `stopped`.

### SRM2 - Service API And Backend Specs

- Goal: make named service resources usable by apps and agents without leaking
  sandbox backend details or conflating lifecycle/status with definition CRUD.
- Files: `packages/nimbus/src/**`, `crates/nimbus-server/src/http/services.rs`,
  `crates/nimbus-services/**`, route auth tests.
- Steps: implement:
  - the canonical service resource routes from "Service Routes And SDK
    Contract"
  - `nimbus.services.create({ name, backend, ... })`
  - `nimbus.services.update({ name, ifMatchGeneration, ... })`
  - `nimbus.services.delete({ name, ifMatchGeneration, force? })`
  - `nimbus.services.list(...)`
  - service definitions with backend kinds:
    `sandbox`, `builtIn`, and `external`
  - service-definition permission checks for create/list/inspect/update/delete
    and grant mutation, separate from exact service grants
  - resource-shaped list responses with bounded pagination, opaque
    `resourceVersion`/continuation tokens, redacted specs/endpoints when the
    caller lacks inspect/endpoint authority, and no global all-tenant listing
  - safe public service-definition response projections that never echo sandbox
    launch secrets, raw environment values, command arrays, host `rootfs` paths,
    local Dockerfile/context paths, or operator-only build inputs
  - service-definition delete/update coordination with the service lifecycle slot
    so definition mutation cannot race an in-flight activation into orphaned
    running backends or stale desired state
  - resource-shaped status conditions with `observedGeneration` so clients can
    distinguish stale controller observations from current desired state
  - route/body tenant and service-name agreement checks for create/update/delete
  - `If-Match` or typed preconditions for update/delete without depending on
    required `DELETE` request bodies
  - built-in provider admission for load balancer, service discovery, browser,
    and model/media gateway shapes only when the provider has real server-side
    validation and fail-closed route tests
  - external endpoint admission only when endpoint auth, health/readiness,
    egress, audit, and tenant policy are represented and tested
  - dynamic service create/update/delete only when the caller supplies name,
    service backend kind, sandbox/built-in/external spec, readiness probe,
    endpoint policy, optional session/channel policy, owner, TTL/idle policy,
    idempotency/precondition inputs, and admission inputs. Sandbox service specs
    run rootfs inputs, OCI image reference inputs, or policy-gated OCI image
    build inputs. Local/dev build input is an explicit exception; production
    tenant isolation must keep failing closed unless an operator-owned build
    provenance/admission policy is configured.
- Dependency rule: implement this phase against the canonical service-backend
  vocabulary: `ServiceBackend`, `BuiltInServiceSpec`, `ExternalServiceSpec`,
  `SandboxSpec.root`, and `SandboxRootSpec`. Do not reintroduce separate
  service implementation or launch-wrapper types.
- Gate: Compose-declared sandbox-backed services, built-in service definitions,
  external service definitions, and dynamically registered services all use the
  same tenant-plus-service-name authority path; `GET service` returns a stable
  resource/status projection; create/update/delete enforce generation or
  idempotency semantics; service-backed sandboxes remain hidden behind service
  state; the MVP SDK does not expose raw service binding resolution; exact
  service grants cover reach/lifecycle/session access, separate
  service-definition permissions cover create/list/inspect/update/delete/grant
  mutation, collection responses are bounded and opaque-versioned, and
  principal-class route tests cover allowed and denied cases for each backend
  kind. Tests prove public service-definition responses redact sandbox launch
  arrays, environment values, and operator-only host/build inputs even when the
  internal static catalog stores them.

### SRM3 - Sandbox API

- Goal: expose isolated execution resources without creating a name-resolution
  side channel.
- Files: `packages/nimbus/src/**`, sandbox HTTP routes, `nimbus-server`
  authorization, `nimbus-sandbox` handle serialization as needed.
- Steps: implement:
  - the canonical sandbox routes from "Sandbox Routes And SDK Contract"
  - `nimbus.sandboxes.create({ profile, root, ttl, policy, labels })`
  - `nimbus.sandboxes.get({ id })`
  - `nimbus.sandboxes.list({ labels?, status? })`
  - `nimbus.sandboxes.stop({ id })`
  Labels are filter metadata only. Reserve `profile: "isolate"` as the spelling
  for any future isolate-backed sandbox profile, but do not treat ordinary
  runtime invocation isolates as sandbox resources. Any user/app/agent
  interaction with a running sandbox goes through
  `sessions.open({ target: { sandbox: { id } } })` once the relevant session
  channels exist.
- Gate: tests prove sandbox operations require ids/handles, labels cannot confer
  authority, tenant A cannot inspect/stop tenant B's sandbox, operator access is
  unavailable without configured operator auth, sandbox create does not create a
  service name, failed admission creates no partial sandbox resource, and any
  implemented `profile: "isolate"` resource is user-created/id-addressed rather
  than an invocation isolate. Sandbox create/list/get/stop responses use the same
  safe projection rule as service definitions: they do not echo launch-time
  command arrays or environment values. Post-start backend validation failures
  such as mismatched returned tenant handles or duplicate backend ids must stop
  the returned handle before returning an error.

### SRM4 - Session API

- Goal: add scoped interaction leases over services or sandboxes.
- Files: `packages/nimbus/src/**`, server session routes/state, audit/telemetry,
  transport/channel helpers.
- Steps: implement the canonical session routes from "Session Routes And SDK
  Contract"; implement `nimbus.sessions.open(...)`, `get`, `list`, `close`, and
  channel APIs. Target shape is a discriminated union:
  `{ service: { name: string } } | { sandbox: { id: string } }`.
  Service targets resolve through the service manager at open time and record a
  service generation, selected backend/provider, and explicit rebind policy.
  Sandbox targets require an exact sandbox id. Channels include only those the
  target and backend can actually support. Built-in services such as `browser`
  may expose sessions even when they do not expose a raw endpoint binding. Do
  not add `renew` or `extend` in this plan; TTL/expiration remains server-owned.
  Streaming channels for models, audio, video, browser, or file transfer must
  define cancellation, backpressure/flow-control, message or frame limits,
  quota accounting, and close semantics before they are exposed as supported
  channels.
- Gate: tests prove service-target sessions do not bypass service grants,
  sandbox-target sessions cannot be opened by name, expired sessions fail closed,
  session audit records target, principal, channels, TTL, and close reason, and
  unsupported channels fail before session creation or channel allocation.
  Streaming-channel tests cover cancellation and bounded buffering for every
  channel family that SRM4 exposes.

### SRM5 - Agent/App Examples

- Goal: make the intended usage unmistakable.
- Files: docs/examples or demo fixtures, SDK selftests, relevant adapter docs.
- Steps: add examples for:
  - app starting/getting a Compose service by name
  - sandboxed nginx or load-balancer service consuming generated upstreams only
    through an explicit future/raw-resolver path, not the MVP app SDK
  - agent creating a task sandbox and opening a desktop/file/session channel
  - agent opening a browser session through the built-in `browser` service
  - agent registering a temporary named service, then opening a service-targeted
    session when that service offers channels
  - adapter app importing `Nimbus` directly while adapter APIs remain
    adapter-shaped
- Gate: examples typecheck through owned fixtures and no adapter public export
  grows Nimbus resource namespaces.

### SRM6 - Final Verifier And Closeout

- Goal: make the resource model durable.
- Files: verifier script, docs, tests, plan ledger.
- Steps: complete `scripts/verify-nimbus-sdk-resource-model.sh` with conditions
  for architecture links, SDK namespaces, service backend kinds, no MVP
  public raw service resolver, no sandbox-name resolution, no implicit service
  publication, session target semantics, runtime-isolate non-resource semantics,
  adapter export guards, private host-transport/non-ctx boundary, and
  principal-class route coverage. The verifier also guards the API details that
  prevent drift: `builtIn` wire discriminator spelling, resource-shaped list
  responses, condition objects, body/path tenant-name agreement checks,
  lifecycle-specific wait validation, no required `DELETE` body semantics, no
  inline external-service secrets, and no unbounded all-tenant list routes.
- Gate: verifier passes; `npm run typecheck`, `npm run test`,
  `npm run build`, `npm run docs:validate-refs:strict`, and
  `git diff --check` pass; `bash scripts/verify-nimbus-capability-segregation.sh`
  passes when the touched slice depends on adapter/resource namespace or host
  transport boundaries. Run broader Rust gates required by touched server or
  sandbox code before closeout.
- Modularity exception: `crates/nimbus-server/src/service_manager/tests.rs` is
  the service-manager route fixture root and may remain in the 1,500-1,999 line
  band while it owns shared HTTP fixture helpers, service lifecycle route tests,
  and principal-class route tests. Concept-owned slices that exceed shared
  fixture scope must live in children such as
  `service_manager/tests/definitions.rs`, `service_manager/tests/sandboxes.rs`,
  `service_manager/tests/sessions.rs`, and
  `service_manager/tests/redaction.rs`; the verifier keeps every module below
  2,000 lines.

## Verifiable Success Criteria

1. Services are addressed only by tenant plus service name and can be backed by
   Compose-declared sandbox, dynamic sandbox, built-in, or external service
   definitions. Service lifecycle/status and service definition CRUD use the
   canonical routes in this plan; no SDK method calls an undeclared or invented
   route.
2. Exact service grants authorize service reach, lifecycle/status, endpoint, and
   service-target session use only. Service definition create/list/inspect,
   update/delete, backend or policy mutation, and grant mutation require
   separate service-definition permissions or configured operator auth. Tests
   prove that exact service reach grants do not authorize definition mutation or
   regranting.
3. Sandboxes are created, listed, inspected, and stopped only by id/handle; no
   sandbox name resolver exists, labels are never authority-bearing, and sandbox
   create never publishes a service name.
4. Sessions target `{ service: { name } }` or `{ sandbox: { id } }`; service
   targets use the service manager with exact service grants and sandbox targets
   use exact ids plus sandbox session permissions.
5. The SDK exposes service lifecycle/status, not raw service-binding
   resolution. Service-targeted sessions are server-backed, TTL-bound, audited,
   and channel-gated.
6. Dynamic service registration records owner, TTL/idle policy, readiness,
   endpoint policy, session/channel policy when applicable, service backend
   kind, sandbox/built-in/external spec, admission inputs, and exact-grant
   requirements. Dockerfile/context build input is represented as a
   policy-gated OCI image build input nested under the OCI image spec, not as a
   root-level build kind or separate sandbox lifecycle API.
7. Built-in and external service policies use named closed policy types and
   provider registries. No accepted route stores unvalidated provider config,
   arbitrary endpoint/auth blobs, or response-echoed secrets.
8. Resource list APIs return bounded resource-shaped collections with opaque
   version/continuation tokens, redacted specs/endpoints when the caller lacks
   authority, and no all-tenant route outside a separate audited operator-only
   API.
9. Durable resource statuses include condition objects with
   `observedGeneration`, `reason`, `message`, and `lastTransitionTime`; clients
   are not forced to parse free-form state strings to understand readiness,
   health, admission, or channel availability.
10. Route tenant, path resource name, and body metadata cannot conflict.
    Update/delete preconditions use `If-Match`, opaque resource versions, or
    typed precondition fields, and required delete semantics do not rely on a
    `DELETE` request body.
11. `waitUntil` and `wait({ until })` semantics are backed by the real
   `GET service` status path. `healthy` either maps to the documented MVP
   ready-derived health state or to a richer probe-backed health state with the
   same public contract. Lifecycle-specific validation rejects nonsensical waits
   such as `stop({ waitUntil: "ready" })`.
12. Runtime isolates are not SDK sandbox resources, service targets, or session
   targets. If isolate-backed sandbox execution is added, its SDK spelling is
   `profile: "isolate"` and it obeys sandbox id/handle lifecycle and policy.
13. Adapter package APIs and generated declarations do not expose Nimbus
    resource namespaces.
14. Principal-class route tests prove operator, tenant, and spawned-workload
    behavior for services, sandboxes, and sessions.
15. Unsupported session channels fail actionably and do not create partial
    sessions.
16. Streaming session channels define and test cancellation, backpressure or
    flow-control, bounded buffering, quota accounting, and close semantics
    before they are advertised as supported.
17. Unsupported built-in/external providers, sandbox profiles, OCI build inputs,
    endpoint policies, or session channels fail before partial resource
    creation.
18. SDK examples for apps and agents typecheck.
19. `scripts/verify-nimbus-sdk-resource-model.sh` and
    `bash scripts/verify-nimbus-capability-segregation.sh` pass when the final
    slice touches adapter/resource namespace or host-transport boundaries, and
    both commands record evidence.

## Execution Log

| Date | Phase | Outcome | Verification | Next step |
| --- | --- | --- | --- | --- |
| 2026-06-07 | Plan creation | plan-only | `npm run docs:validate-refs:strict` pass (245 working-tree Markdown files); `git diff --check` pass for touched docs | Start SRM0 after activation prerequisites are satisfied or bootstrap SRM0 only |
| 2026-06-07 | Service backend refinement | plan-only | Added sandbox-backed, built-in, and external service backend kinds; removed MVP public raw service-binding resolver; added runtime-isolate non-resource rule, reserved future `profile: "isolate"` sandbox spelling, and built-in browser service sessions. Verification: `npm run docs:validate-refs:strict` pass (245 working-tree Markdown files); `git diff --check` pass for touched tracked docs; no-index check clean for new docs | Start SRM0 after activation prerequisites are satisfied or bootstrap SRM0 only |
| 2026-06-07 | Isolate profile refinement | plan-only | Clarified that ordinary runtime invocation isolates are not SDK sandboxes, while any future user-created isolate-backed sandbox must use `profile: "isolate"` and obey sandbox lifecycle/policy/audit/id-addressing rules. Verification: `npm run docs:validate-refs:strict` pass (245 working-tree Markdown files); `git diff --check` pass for touched tracked docs; no-index check clean for new docs | Start SRM0 after activation prerequisites are satisfied or bootstrap SRM0 only |
| 2026-06-08 | SDK transport boundary | plan-only | Clarified that transport selection stays internal to `new Nimbus()`: authenticated control-plane transport is the default, private Nimbus-managed isolate host transport is gated by backend/tier/principal/exact grants, no public `@nimbus/nimbus/transports/host` entry exists, and built-in services/sessions must not appear through adapter or runtime `ctx` shortcuts. Verification: `npm run docs:validate-refs:strict` pass (245 working-tree Markdown files); touched-doc `git diff --check` pass. | Start SRM0 after activation prerequisites are satisfied or bootstrap SRM0 only |
| 2026-06-08 | Service backend vocabulary alignment | plan-only | Aligned SRM2/SRM3 with `docs/plans/service-backend-and-sandbox-spec-refactor-plan.md`: service definitions use service backend specs, sandbox creation runs rootfs inputs or OCI image inputs, and Dockerfile/context build remains an OCI image materialization input rather than a root-level build kind or separate lifecycle API. Verification recorded in the refactor plan. | Start SRM0 after activation prerequisites are satisfied or bootstrap SRM0 only |
| 2026-06-09 | Baseline and ordering audit | plan-only | Reconciled the plan with the current repo baseline: root `@nimbus/nimbus` service lifecycle/status SDK and tenant service routes already exist; SRM1 is now baseline closeout, while SRM2-SRM5 must land server-backed routes before new SDK methods. Verification before closeout: `npm run docs:validate-refs:strict` pass (246 working-tree Markdown files); `npm run test --workspace @nimbus/nimbus` pass; `npm run typecheck --workspace @nimbus/nimbus` pass; `cargo check -p nimbus-server` pass. | Execute SRM0/SRM1 closeout, then continue SRM2+ with server-backed resource contracts |
| 2026-06-09 | Implementation-readiness audit cleanup | plan-only | Removed stale service-backend blocking language after SBR0-SBR6 completion; added canonical service definition, sandbox, and session route contracts; made health/wait semantics explicit; required server-backed routes, auth tests, audit, response types, and verifier guards before public SDK methods land; added capability-segregation verifier to closeout. Verification: `npm run docs:validate-refs:strict` pass (246 working-tree Markdown files); `git diff --check -- docs/plans/nimbus-sdk-resource-model-plan.md` pass. | Start SRM0/SRM1 |
| 2026-06-09 | Final readiness cleanup | plan-only | Removed the typed fail-closed SDK-method escape hatch; clarified that typed fail-closed errors belong inside implemented routes, not ahead of real capability; made session listing tenant-scoped and explicitly denied global all-tenant listing; refreshed capability prerequisite wording to completed-baseline-plus-verification language. Verification: `npm run docs:validate-refs:strict` pass (246 working-tree Markdown files); `git diff --check -- docs/plans/nimbus-sdk-resource-model-plan.md` pass. | Start SRM0/SRM1 |
| 2026-06-09 | Authority and policy readiness cleanup | plan-only | Split exact service reach grants from service-definition administration permissions; replaced built-in/external placeholder policy blobs with named closed policy types; made SRM2 own service-definition CRUD implementation instead of depending on itself; promoted the plan to ready status; added the exact resource-model verifier command to closeout expectations. Verification: `npm run docs:validate-refs:strict` pass (246 working-tree Markdown files); `git diff --check -- docs/plans/nimbus-sdk-resource-model-plan.md docs/plans/README.md` pass. | Start SRM0/SRM1 |
| 2026-06-09 | Multi-level architecture audit | plan-only | Compared the plan against local Kubernetes, Moby/Swarm, Firecracker, and current Nimbus code patterns; added resource-version/condition/list contracts, route/body agreement checks, stable `builtIn` wire spelling, lifecycle-specific wait validation, delete-without-body semantics, external-secret-reference gating, streaming-channel flow-control requirements, and verifier guards for each. Also corrected the completed service-backend refactor plan's stale top-level status. Verification: `npm run docs:validate-refs:strict` pass (246 working-tree Markdown files); `git diff --check -- docs/plans/nimbus-sdk-resource-model-plan.md docs/plans/service-backend-and-sandbox-spec-refactor-plan.md docs/plans/README.md` pass. | Start SRM0/SRM1 |
| 2026-06-09 | SRM0/SRM1 verifier and SDK wait closeout | done | Added `scripts/verify-nimbus-sdk-resource-model.sh` with registration/model, package surface, stale/future SDK method, server route, adapter namespace, and package typecheck conditions. Closed the service SDK wait contract by making `start`/`restart` waits activation-only (`ready`/`healthy`) and `stop` waits stopped-only, with runtime rejection before control-plane requests and type-level selftest coverage. Rebuilt `packages/nimbus/dist` and restaged embedded package artifacts. Verification: `bash scripts/verify-nimbus-sdk-resource-model.sh` pass (`7 passed, 0 failed`); `npm run typecheck --workspace @nimbus/nimbus` pass; `npm run test --workspace @nimbus/nimbus` pass; `npm run build --workspace @nimbus/nimbus` pass; `node scripts/stage-embedded-packages.mjs` pass (`8 packages`, `721 files`). | Start SRM2 |
| 2026-06-09 | SRM2 service definition resource API | done | Implemented dynamic service-definition CRUD through the existing tenant service manager path: `POST/GET collection/PUT/DELETE /api/tenants/{tenant_id}/services`, resource-shaped definition responses with metadata/spec/status/conditions, bounded list metadata, body tenant/name conflict rejection, generation preconditions, force-delete stop path, closed built-in provider IDs, external endpoint/auth/health policy admission, and separate service-definition permission claims. Expanded `@nimbus/nimbus` with `services.create/update/delete/list` and closed backend spec types while keeping sandbox/session namespaces absent. Expanded the verifier with SRM2 checks and refreshed dist/embedded package artifacts. Verification: `bash scripts/verify-nimbus-sdk-resource-model.sh` pass (`11 passed, 0 failed`); `npm run typecheck --workspace @nimbus/nimbus` pass; `npm run test --workspace @nimbus/nimbus` pass; `npm run build --workspace @nimbus/nimbus` pass; `node scripts/stage-embedded-packages.mjs` pass (`8 packages`, `721 files`); `cargo test -p nimbus-services` pass (`26 passed`); `cargo test -p nimbus-server service_manager -- --nocapture` pass (`12 passed`, `406 filtered out` plus filtered integration targets); `cargo check -p nimbus-services -p nimbus-server -p nimbus-bin` pass. | Start SRM3 |
| 2026-06-09 | SRM3 sandbox resource API | done | Implemented tenant-scoped standalone sandbox resources in the service manager and server: `GET/POST /api/tenants/{tenant_id}/sandboxes`, `GET /api/tenants/{tenant_id}/sandboxes/{sandbox_id}`, and `POST /api/tenants/{tenant_id}/sandboxes/{sandbox_id}/stop`; sandbox resources are id-addressed, require standalone sandbox owner metadata, preserve labels as filter metadata only, and do not publish services. Expanded `@nimbus/nimbus` with `sandboxes.create/get/list/stop`, updated package selftests and artifact policy to allow the implemented sandbox namespace while keeping sessions absent. Verification: `bash scripts/verify-nimbus-sdk-resource-model.sh` pass (`15 passed, 0 failed`); `cargo fmt --all --check` pass; `npm run typecheck --workspace @nimbus/nimbus` pass; `npm run test --workspace @nimbus/nimbus` pass; `cargo test -p nimbus-services` pass (`26 passed`); `cargo test -p nimbus-server service_manager -- --nocapture` pass (`14 passed`, `406 filtered out` plus filtered integration targets); `cargo check -p nimbus-services -p nimbus-server -p nimbus-bin` pass. | Start SRM4 |
| 2026-06-09 | SRM4 session resource API | done | Implemented server-backed session resources in `nimbus-services` and `nimbus-server`: `POST /api/sessions`, `GET /api/sessions?tenantId=...`, `GET /api/sessions/{session_id}`, and `POST /api/sessions/{session_id}/close`; sessions have opaque ids, target snapshots, server-owned TTL expiration, close reasons, and closed channel admission (`browser` service `cdp`/`page`; sandbox-backed targets `stdio`/`files`). Route policy requires session permission plus target reach: exact service grants for service-target sessions and sandbox reach for sandbox-target sessions. Expanded `@nimbus/nimbus` with `sessions.open/get/list/close`, rejected `sessions.create/renew/extend`, and refreshed dist/embedded package artifacts. Verification: `bash scripts/verify-nimbus-sdk-resource-model.sh` pass (`19 passed, 0 failed`); `npm run typecheck --workspace @nimbus/nimbus` pass; `npm run test --workspace @nimbus/nimbus` pass; `npm run build --workspace @nimbus/nimbus` pass; `node scripts/stage-embedded-packages.mjs` pass (`8 packages`, `721 files`); `cargo test -p nimbus-server service_manager -- --nocapture` pass (`17 passed`, `406 filtered out` plus filtered integration targets); `cargo check -p nimbus-server` pass. | Start SRM5 |
| 2026-06-09 | SRM5 app/agent examples | done | Added `docs/examples/nimbus-sdk-resource-model.md` and linked it from `docs/README.md`; examples demonstrate Compose service lifecycle/status, built-in service definition declaration without a raw resolver, standalone task sandbox creation, sandbox-target sessions, built-in browser service sessions, sandbox-backed service-target sessions, and adapter action code importing `Nimbus` directly while adapter contexts stay clean. Updated the package README and sandbox architecture model to reflect landed sandbox/session APIs and current channel support. Verification: `bash scripts/verify-nimbus-sdk-resource-model.sh` pass (`22 passed, 0 failed`); `npm run test --workspace @nimbus/nimbus` pass. | Start SRM6 |
| 2026-06-09 | SRM6 final closeout | done | Completed the SDK resource-model plan through SRM0-SRM6: service lifecycle/status, service definition CRUD, standalone sandbox resources, scoped session resources, examples, artifact guards, adapter-boundary checks, and final verifiers. Verification: `cargo fmt --all --check` pass; `npm run typecheck --workspace @nimbus/nimbus` pass; `npm run test --workspace @nimbus/nimbus` pass; `npm run build --workspace @nimbus/nimbus` pass; `node scripts/stage-embedded-packages.mjs` pass (`8 packages`, `721 files`); `npm run lint:capability-boundary` pass (`48 files`); `cargo test -p nimbus-services` pass (`26 passed`); `cargo test -p nimbus-server service_manager -- --nocapture` pass (`17 passed`, `406 filtered out` plus filtered integration targets); `cargo check -p nimbus-services -p nimbus-server -p nimbus-bin` pass; `npm run docs:validate-refs:strict` pass (`247 working-tree Markdown files`); `bash scripts/verify-nimbus-sdk-resource-model.sh` pass (`22 passed, 0 failed` before SRM6 closeout condition); `bash scripts/verify-nimbus-capability-segregation.sh` pass (`10 passed, 0 failed`); `git diff --check` pass. | Plan complete |
| 2026-06-09 | Post-review resource hardening | done | Closed the final audit findings after SRM6: external service definitions now preserve and return the admitted endpoint/auth/health policy shape; session ids use opaque ULID-backed ids instead of sequence-shaped ids; session `GET`/`close` pre-authorize session permission before manager lookup, enforce target reach after lookup, and mask cross-tenant application lookups as not found; closing an expired session preserves server-owned `expired` state; sandbox/session operator-auth failures now audit consistently with service routes; and `scripts/verify-nimbus-sdk-resource-model.sh` guards these contracts. Verification: `cargo fmt --all --check` pass; `cargo test -p nimbus-services` pass (`26 passed`); `cargo test -p nimbus-server service_manager -- --nocapture` pass (`17 passed`, `406 filtered out` plus filtered integration targets); `cargo check -p nimbus-services -p nimbus-server -p nimbus-bin` pass; `npm run typecheck --workspace @nimbus/nimbus` pass; `npm run test --workspace @nimbus/nimbus` pass; `npm run build --workspace @nimbus/nimbus` pass; `bash scripts/verify-nimbus-sdk-resource-model.sh` pass (`23 passed, 0 failed`); `npm run lint:capability-boundary` pass (`48 files`); `bash scripts/verify-nimbus-capability-segregation.sh` pass (`10 passed, 0 failed`); `npm run docs:validate-refs:strict` pass (`247 working-tree Markdown files`); `git diff --check` pass. | Plan complete |
| 2026-06-09 | Autoreview resource isolation closeout | done | Closed the structured autoreview findings after post-review hardening: tenant teardown now stops service-backed handles and standalone sandbox resources, then purges tenant-owned dynamic definitions, standalone sandbox resources, sessions, handles, and activations; service-definition updates reject active or activating backends before mutating desired state; and sandbox get/stop masks cross-tenant sandbox ids as not found. Verification: `cargo fmt --all --check` pass; `cargo check -p nimbus-services -p nimbus-server -p nimbus-bin` pass; `cargo test -p nimbus-services` pass (`26 passed`); `cargo test -p nimbus-server service_manager -- --nocapture` pass (`21 passed`, `406 filtered out` plus filtered integration targets); `bash scripts/verify-nimbus-sdk-resource-model.sh` pass (`23 passed, 0 failed`). | Rerun structured autoreview closeout |
| 2026-06-09 | Autoreview DTO/precondition/modularity closeout | done | Closed the second structured autoreview findings: sandbox-backed service and standalone sandbox HTTP routes now use an explicit public sandbox-spec DTO that accepts the SDK/docs wire shape, rejects mismatched `spec.tenantId`, and translates to internal `SandboxSpec` only at the route boundary; generation mismatches use `Error::PreconditionFailed` and HTTP 412 with adapter-specific protocol mappings; target-scoped session permissions now pass a two-phase `GET`/`close` check that requires session action authority before lookup and target reach after lookup; and server service-manager tests moved into `service_manager/tests.rs` so the production module stays below the repo line threshold. Verification: `cargo test -p nimbus-server service_manager -- --nocapture` pass (`21 passed`, `406 filtered out` plus filtered integration targets); `cargo check -p nimbus-services -p nimbus-server -p nimbus-bin` pass; `npm run typecheck --workspace @nimbus/nimbus` pass; `npm run test --workspace @nimbus/nimbus` pass; `npm run build --workspace @nimbus/nimbus` pass; `node scripts/stage-embedded-packages.mjs` pass (`8 packages`, `721 files`); `bash scripts/verify-nimbus-sdk-resource-model.sh` pass (`23 passed, 0 failed`); `cargo test -p nimbus-services` pass (`26 passed`); `cargo fmt --all --check` pass; `npm run lint:capability-boundary` pass (`48 files`); `npm run docs:validate-refs:strict` pass (`247 working-tree Markdown files`); `git diff --check` pass; `bash scripts/verify-nimbus-capability-segregation.sh` pass (`10 passed, 0 failed`). | Rerun structured autoreview closeout |
| 2026-06-09 | Autoreview public sandbox hardening closeout | done | Closed the final structured autoreview findings: public sandbox specs now reject host `rootfs` paths and local OCI build-context paths as operator-only internal inputs before backend launch; SDK sandbox root types expose only admitted OCI image references; sandbox spec responses serialize owner fields as `serviceName`/`displayName`; and service-manager tests are split into parent route tests plus `service_manager/tests/sessions.rs`, with verifier line-count guards for both modules. Verification: `cargo test -p nimbus-server service_manager -- --nocapture` pass (`22 passed`, `406 filtered out` plus filtered integration targets); `cargo check -p nimbus-services -p nimbus-server -p nimbus-bin` pass; `npm run build --workspace @nimbus/nimbus` pass; `node scripts/stage-embedded-packages.mjs` pass (`8 packages`, `721 files`); `npm run typecheck --workspace @nimbus/nimbus` pass; `npm run test --workspace @nimbus/nimbus` pass; `cargo fmt --all --check` pass; `cargo test -p nimbus-services` pass (`26 passed`); `npm run lint:capability-boundary` pass (`48 files`); `bash scripts/verify-nimbus-sdk-resource-model.sh` pass (`23 passed, 0 failed`); `npm run docs:validate-refs:strict` pass (`247 working-tree Markdown files`); `git diff --check` pass; `bash scripts/verify-nimbus-capability-segregation.sh` pass (`10 passed, 0 failed`). | Rerun structured autoreview closeout |
| 2026-06-09 | Autoreview response-redaction closeout | done | Closed the structured autoreview P1 on public sandbox/service responses: HTTP sandbox spec responses now use safe read projections that redact operator-only rootfs/build inputs and summarize launch argv/entrypoint/command/environment values instead of echoing secrets; SDK request and response types are split (`NimbusSandboxSpec` for launch input, `NimbusSandboxSpecResponse`/`NimbusServiceBackendResponse` for reads); service-definition and sandbox resource route tests assert dangerous launch strings and host paths are absent; and the verifier guards the redaction contract. | Rerun focused gates and structured autoreview closeout |
| 2026-06-09 | Autoreview sandbox cleanup closeout | done | Closed the structured autoreview P2 on standalone sandbox resource leaks: `create_sandbox_resource_for_decision_async` now stops a returned backend handle when post-start validation fails on a mismatched tenant or duplicate sandbox id; service-manager tests cover both cleanup paths; and the verifier guards the cleanup helper and regressions. | Rerun focused gates and structured autoreview closeout |
| 2026-06-09 | Autoreview definition lifecycle/projection closeout | done | Closed structured autoreview findings on service-definition race and list projection: service-definition delete now claims the same per-service lifecycle slot as activation, rejects non-force deletes while activation is in flight, waits for forced deletes to settle before stopping/removing active backends, and releases the slot on failure; service-definition collection responses now return a redacted backend-kind projection for list-only callers and require inspect permission or operator auth for full backend specs/endpoints. Tests and verifier guards cover both contracts. | Rerun focused gates and structured autoreview closeout |
| 2026-06-09 | Autoreview declared-volume/modularity closeout | done | Closed structured autoreview findings on service-backed tenant volumes and test-module threshold policy: sandbox-backed service launch now validates tenant volume mounts against the independently admitted service-definition/catalog volume policy, dynamic service definitions cannot self-authorize tenant volume mounts, and regression tests cover both rejected unadmitted volumes and accepted catalog-declared volumes; the active plan records the service-manager route test fixture as a justified 1,500-1,999 line exception while keeping concept-owned session/redaction tests split into child modules and verifier-guarded below 2,000 lines. | Rerun focused gates and structured autoreview closeout |
| 2026-06-09 | Autoreview service admission/session closeout | done | Closed structured autoreview findings on fail-closed service admission, session attachment, and lifecycle evidence: service-backed volume admission now comes from the independent catalog volume policy instead of the candidate launch spec; Compose catalogs expose admitted named-volume policy; dynamic definitions reject tenant-volume mounts; sandbox-targeted sessions require a ready sandbox; service-targeted sessions fail closed while a service lifecycle/delete slot is held and recheck dynamic definition generation before insertion; forced service-definition delete records a stopped endpoint-cleared service handle before removing state; external service endpoints use structural URL parsing with host and credential checks; and service-definition manager/route admission tests live in concept-owned child modules below line thresholds with no raw sleep in the in-flight delete regression. Verification: `cargo test -p nimbus-services` pass (`36 passed`); `cargo test -p nimbus-server service_manager -- --nocapture` pass (`24 passed`, `406 filtered out` plus filtered integration targets); `cargo test -p nimbus-bin compose::file` pass (`28 passed`, `542 filtered out` plus filtered integration targets); `cargo check -p nimbus-services -p nimbus-server -p nimbus-bin` pass; `cargo fmt --all --check` pass; `npm run typecheck --workspace @nimbus/nimbus` pass; `npm run test --workspace @nimbus/nimbus` pass; `npm run build --workspace @nimbus/nimbus` pass; `node scripts/stage-embedded-packages.mjs` pass (`8 packages`, `721 files`); `npm run lint:capability-boundary` pass (`48 files`); `npm run docs:validate-refs:strict` pass (`247 working-tree Markdown files`); `bash scripts/verify-nimbus-sdk-resource-model.sh` pass (`23 passed, 0 failed`); `bash scripts/verify-nimbus-capability-segregation.sh` pass (`10 passed, 0 failed`); `git diff --check` pass. | Rerun structured autoreview closeout |
| 2026-06-09 | Autoreview scoped-list authorization closeout | done | Closed structured autoreview findings on collection authorization: sandbox collection routes now authorize application principals with any valid sandbox `list` scope and then filter concrete resources by exact/id-prefix/tenant scope; session collection routes now authorize any valid session `list` scope and then filter each returned session by session scope plus target reach. Regression tests cover exact/id-prefix scoped sandbox inventory and service/sandbox-target scoped session inventory. Server route tests are split into `service_manager/tests/{definitions,sandboxes,sessions,redaction}.rs`, and the verifier enforces every module below 2,000 lines. Verification: `cargo fmt --all --check` pass; `cargo check -p nimbus-services -p nimbus-server -p nimbus-bin` pass; `cargo test -p nimbus-services` pass (`36 passed`); `cargo test -p nimbus-server service_manager -- --nocapture` pass (`24 passed`, `406 filtered out` plus filtered integration targets); `cargo test -p nimbus-bin compose::file` pass (`28 passed`, `542 filtered out` plus filtered integration targets); `npm run typecheck --workspace @nimbus/nimbus` pass; `npm run test --workspace @nimbus/nimbus` pass; `npm run build --workspace @nimbus/nimbus` pass; `node scripts/stage-embedded-packages.mjs` pass (`8 packages`, `721 files`); `npm run lint:capability-boundary` pass (`48 files`); `bash scripts/verify-nimbus-sdk-resource-model.sh` pass (`23 passed, 0 failed`); `bash scripts/verify-nimbus-capability-segregation.sh` pass (`10 passed, 0 failed`); `npm run docs:validate-refs:strict` pass (`247 working-tree Markdown files`); `git diff --check` pass. | Rerun structured autoreview closeout |
| 2026-06-09 | Autoreview exact-grant/reliability closeout | done | Closed structured autoreview findings on exact service grants and scheduler-sensitive tests: service and session routes now share one `http/service_grants.rs` exact-grant predicate that rejects wildcard aliases (`*`, `all`, `service:*`, `services:*`) before accepting exact service names; service-target session tests prove mixed exact-plus-wildcard grants are denied; and the in-flight force-delete regression now waits on a test-only lifecycle-slot wait observer instead of raw scheduler yields. The verifier guards the shared predicate, wildcard aliases, session regression, and absence of `yield_now` in the service-definition race test. Verification: `cargo test -p nimbus-services delete_service_definition_serializes_with_in_flight_activation -- --nocapture` pass (`1 passed`, `35 filtered out`); `cargo test -p nimbus-server service_manager -- --nocapture` pass (`24 passed`, `406 filtered out` plus filtered integration targets); `cargo fmt --all --check` pass; `cargo check -p nimbus-services -p nimbus-server -p nimbus-bin` pass; `cargo test -p nimbus-services` pass (`36 passed`); `bash scripts/verify-nimbus-sdk-resource-model.sh` pass (`23 passed, 0 failed`); `npm run typecheck --workspace @nimbus/nimbus` pass; `npm run test --workspace @nimbus/nimbus` pass; `npm run build --workspace @nimbus/nimbus` pass; `node scripts/stage-embedded-packages.mjs` pass (`8 packages`, `721 files`); `npm run lint:capability-boundary` pass (`48 files`); `bash scripts/verify-nimbus-capability-segregation.sh` pass (`10 passed, 0 failed`). | Rerun structured autoreview closeout |
| 2026-06-09 | Autoreview duplicate-id ownership closeout | done | Closed the structured autoreview P1 on duplicate sandbox ids after backend start: standalone sandbox creation now treats a duplicate returned backend id as a conflict without stopping by id, because that id may already belong to a tracked sandbox owned by an earlier create path. The regression proves the duplicate create returns conflict, records no backend stop calls, leaves one tracked sandbox resource, and preserves the existing backend handle. The verifier guards both the regression name and the duplicate-id no-stop assertion. Verification: `cargo test -p nimbus-services create_sandbox_resource_preserves_existing_backend_after_duplicate_started_id -- --nocapture` pass (`1 passed`, `35 filtered out`); `cargo fmt --all --check` pass; `cargo check -p nimbus-services -p nimbus-server -p nimbus-bin` pass; `cargo test -p nimbus-services` pass (`36 passed`); `cargo test -p nimbus-server service_manager -- --nocapture` pass (`24 passed`, `406 filtered out` plus filtered integration targets); `cargo test -p nimbus-bin compose::file` pass (`28 passed`, `542 filtered out` plus filtered integration targets); `npm run typecheck --workspace @nimbus/nimbus` pass; `npm run test --workspace @nimbus/nimbus` pass; `npm run build --workspace @nimbus/nimbus` pass; `node scripts/stage-embedded-packages.mjs` pass (`8 packages`, `721 files`); `npm run lint:capability-boundary` pass (`48 files`); `bash scripts/verify-nimbus-sdk-resource-model.sh` pass (`23 passed, 0 failed`); `bash scripts/verify-nimbus-capability-segregation.sh` pass (`10 passed, 0 failed`); `npm run docs:validate-refs:strict` pass (`247 working-tree Markdown files`); `git diff --check` pass. | Rerun structured autoreview closeout |
| 2026-06-09 | Autoreview session-target union closeout | done | Closed the structured autoreview P2 on ambiguous session targets: session open requests now parse `target` as a closed exact-one object and reject payloads that contain both `service` and `sandbox` instead of silently selecting one branch. Regression coverage sends an ambiguous target and expects `400 Bad Request`; the verifier guards the exact-one parser error and test anchor. Verification: `cargo test -p nimbus-server service_manager::tests::sessions::session_routes_reject_service_sessions_without_exact_grants_and_unsupported_channels -- --nocapture` pass (`1 passed`, `429 filtered out`); `cargo fmt --all --check` pass; `cargo check -p nimbus-services -p nimbus-server -p nimbus-bin` pass; `cargo test -p nimbus-server service_manager -- --nocapture` pass (`24 passed`, `406 filtered out` plus filtered integration targets); `cargo test -p nimbus-services` pass (`36 passed`); `cargo test -p nimbus-bin compose::file` pass (`28 passed`, `542 filtered out` plus filtered integration targets); `npm run typecheck --workspace @nimbus/nimbus` pass; `npm run test --workspace @nimbus/nimbus` pass; `npm run build --workspace @nimbus/nimbus` pass; `node scripts/stage-embedded-packages.mjs` pass (`8 packages`, `721 files`); `npm run lint:capability-boundary` pass (`48 files`); `bash scripts/verify-nimbus-sdk-resource-model.sh` pass (`23 passed, 0 failed`); `bash scripts/verify-nimbus-capability-segregation.sh` pass (`10 passed, 0 failed`); `npm run docs:validate-refs:strict` pass (`247 working-tree Markdown files`); `git diff --check` pass. | Rerun structured autoreview closeout |

## /goal Prompt

```text
/goal Complete docs/plans/nimbus-sdk-resource-model-plan.md autonomously.

Use the plan and docs/architecture/sandbox/service-sandbox-session-model.md as
the control plane. First inspect git status --short and reconcile existing dirty
work. Verify the completed capability-segregation baseline from
nimbus-capability-segregation-plan.md before implementation, especially CB3,
CB8, and the SDK host-transport boundary. Treat
docs/plans/service-backend-and-sandbox-spec-refactor-plan.md SBR0-SBR6 as the
completed vocabulary baseline unless repo evidence proves it regressed. If
required prerequisites cannot be verified, complete only SRM0 registration and
verifier bootstrap, then leave dependent phases pending with explicit evidence.

Execute SRM0-SRM6 in order. Preserve the resource model: services are addressed
by tenant plus service name and may be sandbox-backed, built-in, or external;
sandboxes are addressed by id/handle; sessions are scoped leases targeting
either a service name or a sandbox id; runtime isolates are not SDK sandbox
resources. If isolate-backed execution becomes a user-created sandbox resource,
the SDK profile spelling is exactly profile: "isolate" and the resource must
obey sandbox lifecycle/policy rules. Do not add MVP public raw service-binding
resolution, sandbox-name resolution, implicit service publication from sandbox
creation, adapter ctx shortcuts, wildcard service grants, or operator credential
reach into tenant/spawned workloads. Keep transport selection internal to
`new Nimbus()`: authenticated control-plane transport is the default, and any
private Nimbus-managed isolate host transport requires the capability-segregation
backend/tier/principal/exact-grant gate. Do not add public
`@nimbus/nimbus/transports/host` or runtime `ctx` capability shortcuts.

Do not add SDK-only futures. A public SDK namespace, method, exported type, or
example may land only with the matching server route, resource-shaped response,
authorization tests, audit records, docs, and verifier conditions in the same
phase. Implement the canonical contracts in this plan:
GET/POST/PUT/DELETE tenant service resources plus explicit start/stop/restart
lifecycle routes; tenant-scoped sandbox create/list/get/stop by id; and
POST/GET/list/close session routes with service-name or sandbox-id targets.
`sessions.open(...)` is the public session verb; do not add `sessions.create`,
`renew`, or `extend`.

Keep service authorization split cleanly: exact service grants authorize service
reach, lifecycle/status, endpoint, and service-target session access; separate
service-definition permissions authorize create/list/inspect/update/delete,
backend or policy mutation, and grant mutation. A principal that can use a
service cannot update, delete, or regrant it unless service-definition policy
also permits that action. Built-in and external service policies must be closed,
named, server-validated policy shapes, not arbitrary config blobs.

Implement control-plane details to modern resource API quality: list routes
return bounded resource-shaped collections with opaque version/continuation
tokens; durable status uses condition objects with observedGeneration; route
tenant/path name/body metadata conflicts are rejected; update/delete use
If-Match or typed preconditions; required delete semantics do not depend on a
DELETE request body; public JSON discriminators use `sandbox`, `builtIn`, and
`external`; lifecycle wait options are verb-aware; external service credentials
use secret references only; streaming session channels define and test
cancellation, backpressure, bounded buffering, quota accounting, and close
semantics before they are advertised.

After each phase, update the phase state and execution log with exact commands
and pass/fail counts. Final closeout requires
bash scripts/verify-nimbus-sdk-resource-model.sh,
npm run typecheck, npm run test, npm run build,
npm run docs:validate-refs:strict, git diff --check,
bash scripts/verify-nimbus-capability-segregation.sh when the touched slice
depends on adapter/resource namespace or host-transport boundaries, and every
focused Rust gate needed by touched server/sandbox code.
```
