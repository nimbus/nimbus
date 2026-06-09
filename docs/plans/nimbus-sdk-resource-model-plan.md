# Plan: Nimbus SDK Resource Model

## Status

- **Status:** `proposed`
- **Primary goal:** implement the `@nimbus/nimbus` resource model described in
  [`docs/architecture/sandbox/service-sandbox-session-model.md`](../architecture/sandbox/service-sandbox-session-model.md).
  The current landed SDK slice owns service lifecycle/status; sandbox and session
  namespaces land only with server-backed routes in their owning phases.
- **Activation prerequisites:**
  - `docs/plans/nimbus-capability-segregation-plan.md` lands CB3 for the
    top-level `@nimbus/nimbus` SDK, default credentials, and low-level transport
    split.
  - The same plan lands the SDK transport boundary: `new Nimbus()` selects
    authenticated control-plane transport by default, while any private
    Nimbus-managed isolate host transport is installed only for an allowed
    backend, invocation tier, principal class, and exact grant set. Bun/JSC or
    another isolate backend must fail closed until it proves equivalent gating.
  - The same plan lands CB8 or an equivalent principal-class route gate for
    service/sandbox/session control-plane routes.
  - `docs/plans/service-backend-and-sandbox-spec-refactor-plan.md` lands SBR1
    through SBR5 before this plan implements dynamic service backend specs,
    sandbox resource APIs, or session resource APIs. SRM0 and SRM1 may only
    bootstrap/verify the existing service lifecycle/status SDK slice before the
    backend refactor is complete.
  - Session channels that depend on desktop/GPU/libkrun media plumbing wait for
    the corresponding band in `docs/plans/nimbus-sandbox-plan.md`.

This plan is the SDK/control-plane follow-on. It does not rename Compose
services and does not replace the sandbox backend plan.

## Current Baseline

Repo audit on 2026-06-09 found that part of this plan has already landed:

- `packages/nimbus/package.json` names the package `@nimbus/nimbus` and exports
  the root SDK plus `./transports/rest`.
- `packages/nimbus/src/index.ts` exposes `new Nimbus()` with default endpoint
  and credential discovery.
- `Nimbus.services` exposes `start`, `stop`, `restart`, `get`, and `wait`.
  `ensureRunning` is intentionally absent.
- `Nimbus.sandboxes` and `Nimbus.sessions` are intentionally absent until
  server-backed resource routes land.
- `crates/nimbus-server` exposes canonical service routes:
  `GET /api/tenants/{tenant_id}/services/{service_name}` and
  `POST /api/tenants/{tenant_id}/services/{service_name}/{start|stop|restart}`.

SRM0 still must create the verifier. SRM1 should reconcile, verify, and close
out this landed baseline rather than reimplement it from scratch.

## Execution Order

1. Complete `docs/plans/service-backend-and-sandbox-spec-refactor-plan.md`
   through SBR6. That plan stabilizes the Rust service/sandbox vocabulary this
   SDK plan builds on.
2. Run SRM0 and SRM1 to register the SDK resource-model verifier and close out
   the already-landed service lifecycle/status SDK baseline.
3. Run SRM2 only after the backend refactor and principal-class route gates are
   in place.
4. Run SRM3/SRM4 for sandbox and session resources after their server routes,
   policy, audit, and channel support exist.
5. Run SRM5/SRM6 for examples, verifier hardening, and final closeout.

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

The future full SDK shape, once server-backed sandbox and session routes land,
is:

```ts
// Future sandbox namespace once server-backed sandbox routes land.
const sandbox = await nimbus.sandboxes.create({ profile: "desktop" });

// Reserved future shape for explicit isolate-backed sandbox resources.
// This is not how ordinary function invocation isolates are addressed.
const worker = await nimbus.sandboxes.create({ profile: "isolate" });

// Future session namespace once server-backed session routes land.
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
`Nimbus.services`, `Nimbus.sandboxes`, and future `Nimbus.sessions`; low-level
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

## Control Rules

- Use this plan and
  `docs/architecture/sandbox/service-sandbox-session-model.md` as the source of
  truth. Do not rely on chat history for the resource vocabulary.
- Keep one phase `in_progress` at a time.
- Update the phase status ledger and execution log before handoff or closeout.
- A phase is done only when its gate has concrete command/test evidence.
- If activation prerequisites are not landed, complete SRM0 only and leave later
  phases `todo`.
- If the service-backend refactor is incomplete, SRM0 and SRM1 may verify the
  existing service SDK/server baseline, but SRM2-SRM5 must remain pending.

## Phase Status Ledger

| Phase | Status | Hard dependencies | Verifiable success signal |
| --- | --- | --- | --- |
| SRM0 | `todo` | none | Plan registered; architecture model linked; verifier bootstrap passes registration/model checks. |
| SRM1 | `partial` | capability-segregation CB3; current service SDK/server baseline | Existing `@nimbus/nimbus` service lifecycle/status APIs are verified, package selftests pass, stale `ensureRunning` and fake sandbox/session methods are rejected, and adapter export/type-surface guards prove no resource namespace leakage. |
| SRM2 | `todo` | SRM1; service-backend refactor SBR1-SBR5; capability-segregation CB8 | Static and dynamic services support sandbox-backed, built-in, and external implementations with exact-grant route coverage. |
| SRM3 | `todo` | SRM1; service-backend refactor SBR1-SBR5; capability-segregation CB8 | Sandbox APIs are id/handle-addressed only; labels cannot confer authority; sandbox creation does not publish a service. |
| SRM4 | `todo` | SRM2; SRM3; required sandbox-plan backend bands for channel types | Sessions open only against `{ service: { name } }` or `{ sandbox: { id } }`, enforce TTL/audit/channel policy, and fail closed on unsupported channels. |
| SRM5 | `todo` | SRM2-SRM4 | App/agent examples typecheck and demonstrate service, sandbox, and session usage without adapter ctx shortcuts. |
| SRM6 | `todo` | SRM1-SRM5 | Final verifier and required JS/Rust/docs gates pass with evidence. |

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
  server routes or typed fail-closed route support.
- Gate: `import { Nimbus } from "@nimbus/nimbus"` typechecks with service
  lifecycle/status APIs; package selftests reject stale `ensureRunning`,
  `sandboxes.create`, and `sessions.create`/`sessions.open` calls until their
  owning routes land; adapter package export/type-surface guards prove they do
  not re-export Nimbus resource namespaces.

### SRM2 - Service API And Backend Specs

- Goal: make named services usable by apps and agents without leaking sandbox
  backend details.
- Files: `packages/nimbus/src/**`, `crates/nimbus-server/src/http/services.rs`,
  `crates/nimbus-services/**`, route auth tests.
- Steps: implement:
  - `nimbus.services.start({ name })`
  - `nimbus.services.start({ name, waitUntil: "ready" })`
  - `nimbus.services.stop({ name })`
  - `nimbus.services.restart({ name })`
  - `nimbus.services.get({ name })`
  - service definitions with backend kinds:
    `sandbox`, `builtin`, and `external`
  - built-in service records for load balancer, service discovery, and browser
    service shapes where the route exists, even if some backends/providers remain
    not-yet-supported until their owning plan lands
  - dynamic service registration/update/delete APIs only when the caller supplies
    name, service backend kind, sandbox/built-in/external spec, readiness probe,
    endpoint policy, optional session/channel policy, owner, TTL/idle policy,
    and admission inputs. Sandbox service specs run rootfs inputs, OCI image
    reference inputs, or policy-gated OCI image build inputs. Local/dev build
    input is an explicit exception; production tenant isolation must keep
    failing closed unless an operator-owned build provenance/admission policy is
    configured.
- Dependency rule: do not implement this phase against `ServiceImplementation`,
  `SandboxBackedServiceImplementation`, `SandboxImageLaunchSpec`, or
  `SandboxBuildLaunchSpec`. Wait for the service-backend refactor's
  `ServiceBackend`, `BuiltInServiceSpec`, `ExternalServiceSpec`,
  `SandboxSpec.root`, and `SandboxRootSpec` vocabulary.
- Gate: Compose-declared sandbox-backed services, built-in service definitions,
  external service definitions, and dynamically registered services all use the
  same tenant-plus-service-name authority path; service-backed sandboxes remain
  hidden behind service state; the MVP SDK does not expose raw service binding
  resolution; exact grants and principal-class route tests cover allowed and
  denied cases.

### SRM3 - Sandbox API

- Goal: expose isolated execution resources without creating a name-resolution
  side channel.
- Files: `packages/nimbus/src/**`, sandbox HTTP routes, `nimbus-server`
  authorization, `nimbus-sandbox` handle serialization as needed.
- Steps: implement:
  - `nimbus.sandboxes.create({ profile, image, ttl, policy, labels })`
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
  authority, tenant A cannot inspect/stop tenant B's sandbox, sandbox create
  does not create a service name, and any implemented `profile: "isolate"`
  resource is user-created/id-addressed rather than an invocation isolate.

### SRM4 - Session API

- Goal: add scoped interaction leases over services or sandboxes.
- Files: `packages/nimbus/src/**`, server session routes/state, audit/telemetry,
  transport/channel helpers.
- Steps: implement `nimbus.sessions.open(...)`, `get`, `list`, `close`, and
  channel APIs. Target shape is a discriminated union:
  `{ service: { name: string } } | { sandbox: { id: string } }`.
  Service targets resolve through the service manager at open time and record a
  service generation, selected implementation, or explicit rebind policy.
  Sandbox targets require an exact sandbox id. Channels include only those the
  target and backend can actually support. Built-in services such as `browser`
  may expose sessions even when they do not expose a raw endpoint binding.
- Gate: tests prove service-target sessions do not bypass service grants,
  sandbox-target sessions cannot be opened by name, expired sessions fail closed,
  session audit records target, principal, channels, TTL, and close reason, and
  unsupported channels fail actionably.

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
  principal-class route coverage.
- Gate: verifier passes; `npm run typecheck`, `npm run test`,
  `npm run build`, `npm run docs:validate-refs:strict`, and
  `git diff --check` pass. Run broader Rust gates required by touched server or
  sandbox code before closeout.

## Verifiable Success Criteria

1. Services are addressed only by tenant plus service name and can be backed by
   Compose-declared sandbox, dynamic sandbox, built-in, or external service
   definitions.
2. Sandboxes are created, listed, inspected, and stopped only by id/handle; no
   sandbox name resolver exists.
3. Sessions target `{ service: { name } }` or `{ sandbox: { id } }`; service
   targets use the service manager and sandbox targets use exact ids.
4. The MVP SDK exposes service lifecycle/status, not raw service-binding
   resolution. Service-targeted sessions land only when SRM4 supplies server
   routes, TTL, audit, and channel-gating tests.
5. Dynamic service registration records owner, TTL/idle policy, readiness,
   endpoint policy, session/channel policy when applicable, service backend
   kind, sandbox/built-in/external spec, admission inputs, and exact-grant
   requirements. Dockerfile/context build input is represented as a
   policy-gated OCI image build input nested under the OCI image spec, not as a
   root-level build kind or separate sandbox lifecycle API.
6. Creating a sandbox never implicitly creates a service name.
7. Runtime isolates are not SDK sandbox resources, service targets, or session
   targets. If isolate-backed sandbox execution is added, its SDK spelling is
   `profile: "isolate"` and it obeys sandbox id/handle lifecycle and policy.
8. Adapter package APIs and generated declarations do not expose Nimbus resource
   namespaces.
9. Principal-class route tests prove operator, tenant, and spawned-workload
   behavior for services, sandboxes, and sessions.
10. Unsupported session channels fail actionably and do not create partial
   sessions.
11. SDK examples for apps and agents typecheck.
12. `scripts/verify-nimbus-sdk-resource-model.sh` passes and records evidence.

## Execution Log

| Date | Phase | Outcome | Verification | Next step |
| --- | --- | --- | --- | --- |
| 2026-06-07 | Plan creation | plan-only | `npm run docs:validate-refs:strict` pass (245 working-tree Markdown files); `git diff --check` pass for touched docs | Start SRM0 after activation prerequisites are satisfied or bootstrap SRM0 only |
| 2026-06-07 | Service backend refinement | plan-only | Added sandbox-backed, built-in, and external service backend kinds; removed MVP public raw service-binding resolver; added runtime-isolate non-resource rule, reserved future `profile: "isolate"` sandbox spelling, and built-in browser service sessions. Verification: `npm run docs:validate-refs:strict` pass (245 working-tree Markdown files); `git diff --check` pass for touched tracked docs; no-index check clean for new docs | Start SRM0 after activation prerequisites are satisfied or bootstrap SRM0 only |
| 2026-06-07 | Isolate profile refinement | plan-only | Clarified that ordinary runtime invocation isolates are not SDK sandboxes, while any future user-created isolate-backed sandbox must use `profile: "isolate"` and obey sandbox lifecycle/policy/audit/id-addressing rules. Verification: `npm run docs:validate-refs:strict` pass (245 working-tree Markdown files); `git diff --check` pass for touched tracked docs; no-index check clean for new docs | Start SRM0 after activation prerequisites are satisfied or bootstrap SRM0 only |
| 2026-06-08 | SDK transport boundary | plan-only | Clarified that transport selection stays internal to `new Nimbus()`: authenticated control-plane transport is the default, private Nimbus-managed isolate host transport is gated by backend/tier/principal/exact grants, no public `@nimbus/nimbus/transports/host` entry exists, and built-in services/sessions must not appear through adapter or runtime `ctx` shortcuts. Verification: `npm run docs:validate-refs:strict` pass (245 working-tree Markdown files); touched-doc `git diff --check` pass. | Start SRM0 after activation prerequisites are satisfied or bootstrap SRM0 only |
| 2026-06-08 | Service backend vocabulary alignment | plan-only | Aligned SRM2/SRM3 with `docs/plans/service-backend-and-sandbox-spec-refactor-plan.md`: service definitions use service backend specs, sandbox creation runs rootfs inputs or OCI image inputs, and Dockerfile/context build remains an OCI image materialization input rather than a root-level build kind or separate lifecycle API. Verification recorded in the refactor plan. | Start SRM0 after activation prerequisites are satisfied or bootstrap SRM0 only |
| 2026-06-09 | Baseline and ordering audit | plan-only | Reconciled the plan with the current repo baseline: root `@nimbus/nimbus` service lifecycle/status SDK and tenant service routes already exist; SRM1 is now baseline closeout, while SRM2-SRM5 wait for service-backend refactor SBR1-SBR5. Verification before closeout: `npm run docs:validate-refs:strict` pass (246 working-tree Markdown files); `npm run test --workspace @nimbus/nimbus` pass; `npm run typecheck --workspace @nimbus/nimbus` pass; `cargo check -p nimbus-server` pass. | Execute service-backend refactor SBR1-SBR6 first; then run SRM0/SRM1 closeout and continue SRM2+ |

## /goal Prompt

```text
/goal Complete docs/plans/nimbus-sdk-resource-model-plan.md autonomously.

Use the plan and docs/architecture/sandbox/service-sandbox-session-model.md as
the control plane. First inspect git status --short and reconcile existing dirty
work. Confirm activation prerequisites from nimbus-capability-segregation-plan.md
and docs/plans/service-backend-and-sandbox-spec-refactor-plan.md before
implementation. If capability prerequisites are not landed, complete only SRM0
registration and verifier bootstrap, then leave later phases pending with
explicit evidence. If the service-backend refactor is incomplete, SRM0/SRM1 may
only verify the existing service lifecycle/status SDK and server route baseline;
leave SRM2-SRM5 pending.

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

After each phase, update the phase state and execution log with exact commands
and pass/fail counts. Final closeout requires the resource-model verifier,
npm run typecheck, npm run test, npm run build,
npm run docs:validate-refs:strict, git diff --check, and every focused Rust gate
needed by touched server/sandbox code.
```
