# Nimbus Capability Segregation Plan

Status: proposed (design complete; not yet adopted into the active roadmap)
Owner: TBD
Created: 2026-05-30
Research backing: docs/plans/research/capability-isolation-prior-art.md

---

## 0. Orientation for a fresh agent (read this first)

What this plan does. It makes the privileged "services" capability (start/stop/
restart sandbox microVMs) and the REST control plane reachable only by
AUTHORIZED PRINCIPALS, while keeping the Convex/Firebase/Mongo compatibility
surfaces pure. It also renames one overloaded type so the word "service" has a
single meaning repo-wide.

Two consumers of the `nimbus` package, two principal classes:

- Operators (the `nimbus-ui` admin console, which already depends on `nimbus`)
  act ACROSS tenants -- create/list/delete tenants, grant services, manage any
  tenant's machines and services.
- Apps / adapters / dynamic workers / dynamic sandboxes act WITHIN one tenant --
  they may reach services only for their own tenant, only when that tenant is
  granted.

Authorization is by PRINCIPAL CLASS resolved server-side, never by which package
was imported (see section 2a).

One-paragraph mental model. A tenant's JS function runs in a V8 isolate built by
a Rust host bridge. The function can only call capabilities the host registered
as V8 ops and placed on its `ctx`. Today every host call funnels through one
generic op into the `HostBridge` trait. We add a separate, grant-gated op for
`services`: it is added to a deployment's isolates only if that deployment holds
the `services` grant (off by default). Ungranted means the op is absent and
unreachable. Granted means the op is present and the server scopes every call to
that tenant's own services. Operators do not go through a tenant isolate at all
-- they call the control plane over HTTP as a cross-tenant principal, gated
server-side. The JS package split is for developer ergonomics, not security.

The three things called "service" (do not conflate):

| Term | What it is | This plan |
| --- | --- | --- |
| `nimbus_engine::Service` | engine coordinator (tenant registry, persistence, scheduler, triggers) | renamed to `Engine` (CB1); stays shared by all adapters |
| `nimbus_services` / `SandboxServiceManager` / `/services/*` / compose `services:` | sandbox/microVM service lifecycle | kept; the privileged, grant-gated capability |
| REST control plane (`/api/tenants/*` admin) | direct platform admin (tenants, schema, raw docs, machines) | kept; privileged, principal-gated |

Decisions are settled (section 7). Implement and verify -- no architecture
choices remain.

How to execute: phases CB0..CB9 with a dependency chain (section 6). Each phase
lists files to touch and an evidence-bearing gate. Do them in order; CB1 (a
mechanical rename) lands first and alone.

Pre-launch rules apply (root AGENTS.md/CLAUDE.md): breaking changes over shims,
no back-compat aliases, fix root causes, tests assert behavior.

---

## 1. Current state (As-Is)

JS packages (npm workspace):

- packages/nimbus -- canonical SDK; exports ./server ./values ./browser ./react
  ./rest; deps: [] (no upstream convex).
- packages/convex -- compat; exports ./server ./values ./browser ./react; deps:
  nimbus, @nimbus/codegen, esbuild. (Only adapter that depends on nimbus.)
- packages/firebase (@nimbus/firebase) -- deps: protobuf/connect; NOT on nimbus.
- packages/mongodb (@nimbus/mongodb) -- deps: mongodb; NOT on nimbus.
- packages/nimbus-ui -- the OPERATOR console; already depends on nimbus (imports
  nimbus/react, nimbus/browser); has routes/operator/* and routes/developer/*.
- NimbusRestClient lives in packages/nimbus/src/rest.ts. No JS services() API
  exists yet.

Rust (nimbus-server + runtime):

- HTTP -> router.rs -> adapter surface (convex/firebase/cloud_functions/mongodb)
  builds a per-deployment HOST BRIDGE.
- ALL bridges impl RuntimeCapabilityHost (capabilities.rs:23 fn service()).
- No SandboxCapabilityHost trait. No per-deployment "services" grant.
- Tenant fn runs in a V8 isolate (extensions.rs); its only host-call surface is
  one generic op -> HostBridge::call(...) (host.rs:673).
- Services routes exist and are UNGATED by capability: router.rs:627-635
  /api/tenants/{tenant}/services/{name}/start|stop|restart, backed by
  nimbus_services::SandboxServiceManager.
- deno_permissions (=0.107.0) profiles are built in runtime_capabilities.rs but
  not yet specialized per function tier.
- nimbus_engine::Service (service/mod.rs:58) -- the overloaded name.
- Identity primitives ALREADY EXIST (reuse, do not reinvent):
  - PrincipalContext / PrincipalClaimSource (nimbus-core/src/auth/mod.rs)
  - operator console sessions + local admin token (LocalServerSecurityState)
  - scoped agent sessions: per-tenant tenant_id + closed-set capabilities
  - TenantWorkloadStableIdentity from TenantIsolationDecision
  (all documented in docs/architecture/server/auth-runtime-trust.md)

Problems:

1. "services" the engine-coordinator and "services" the sandbox concept collide
   in one word.
2. The privileged services/control-plane surface has no capability gate -- any
   caller reaching the route, or any host bridge, can structurally drive
   microVMs. There is no per-deployment grant.
3. Authorization does not distinguish PRINCIPAL CLASSES. An operator
   (cross-tenant, via nimbus-ui) and a tenant deployment (own-tenant) are not
   separated at the gate, so one flat rule cannot serve both correctly.

## 2. End state (To-Be)

JS packages:

- nimbus = re-exports @nimbus/core PLUS the privileged nimbus/rest entry
  (tenants/schema/docs + startService/stopService/restartService).
- nimbus-ui (operator) depends on nimbus and uses nimbus/rest.
- @nimbus/core = unprivileged: v, query/mutation/defineSchema, function refs,
  function-call client. No control plane, no services.
- convex, @nimbus/firebase, @nimbus/mongodb depend on @nimbus/core ONLY; never on
  nimbus.
- Lint: compat PACKAGE SOURCE must not import nimbus or nimbus/rest (nimbus-ui
  and end-user app code are exempt).

Rust:

- nimbus_engine::Engine (renamed); RuntimeCapabilityHost::engine() is shared.
- NEW per-deployment `services` GRANT (off by default).
- NEW SandboxCapabilityHost trait (nimbus-bridge): nimbus_services + control
  plane. Implemented by a deployment's bridge IFF that deployment is granted.
- NEW grant-gated op extension nimbus_sandbox_ops: added to a deployment's V8
  isolates ONLY when granted. Ungranted isolate means the op is absent, so
  `import { sandbox } from "nimbus"` hits no op.
- Server gate (auth-runtime-trust) authorizes by PRINCIPAL CLASS: operator =
  cross-tenant (audited); tenant = own-tenant + grant; spawned resource =
  spawning-tenant scope. (See section 2a.)
- Per-tier deno_permissions: query/mutation = no net/fs; action = widened.
- microVM isolation (nimbus-libkrun) remains the escape backstop.

Reachability rule, end state:

- Operator (nimbus-ui): operator console session -> /api/tenants and /services/*
  across ANY tenant: ALLOWED, audited.
- Tenant app, NOT granted: ctx = {db,auth,scheduler}; `import {sandbox} from
  "nimbus"; sandbox().start()` FAILS because the op is absent.
- Tenant app, GRANTED: ctx = {db,auth,scheduler}; sandbox().start() SUCCEEDS, and
  the server scopes the call to THIS tenant.

## 2a. Principal and Authorization Model

Authorization is by PRINCIPAL CLASS, resolved server-side. The package a caller
imported, or which adapter served the request, never grants authority -- the
resolved principal does. Build on the existing primitives in
docs/architecture/server/auth-runtime-trust.md; do not invent a parallel
identity system.

| Principal | Authenticates via | Authority | Reaches services how |
| --- | --- | --- | --- |
| Operator | operator console session / local admin token (LocalServerSecurityState) | global cross-tenant: create/list/delete tenants, grant services, manage any tenant's machines/services; every action audited | control-plane HTTP (nimbus/rest from nimbus-ui); never through a tenant isolate |
| Tenant | deployment identity | own tenant only, services only if that tenant is granted | grant-gated nimbus_sandbox_ops op in its isolate (CB5); server re-checks own-tenant scope (CB8) |
| Spawned resource (dynamic worker / sandbox) | TenantWorkloadStableIdentity from its TenantIsolationDecision | inherits the spawning tenant's scope; never escalates, never wildcards | same grant-gated op path as its tenant; identity is decision-derived |

Hard rules:

- Operator credentials never resolve inside a tenant isolate -- the local admin
  token / operator session is not reachable from tenant JS (the auth doc already
  mandates this for agents). Enforced by CB7a.
- A tenant principal can never resolve to operator -- distinct credential
  classes; a tenant/deployment token is structurally not an operator session.
- Spawned-resource identity is decision-derived -- a worker/sandbox gets its
  tenant scope from the admitted TenantIsolationDecision, not from any string it
  passes, so it cannot claim another tenant.

Summary of reach:

- nimbus-ui (operator) -- operator session --> control plane --> ANY tenant
  (audited).
- tenant app / adapter -- deployment id --> own tenant only, grant-gated.
- dynamic worker/sandbox -- TenantWorkloadStableIdentity --> spawning-tenant
  scope.

## 3. Why this design

Full sourcing in docs/plans/research/capability-isolation-prior-art.md.

- An unregistered op is unreachable from JS. In deno_core, JS can only call Rust
  through registered ops; an op the host never adds to an isolate has no V8 entry
  point. Gating the services op by grant is a real boundary, not a bypassable
  runtime check. Convex (runtime tier), Cloudflare Workers (bindings on env), and
  Deno embedders use the same model.
- One thread is one privilege level. Code in one V8 realm shares one privilege
  level, so privileged services lives in the Rust host path and the grant-gated
  op is the only crossing. CB7a enforces this as a tested invariant.
- Three limits of op gating shape later phases: it does not stop a V8 engine
  escape (handled by microVM isolation, layer 5); it does not re-gate a
  registered op (a granted op is still principal-gated at CB8); it breaks if a
  shared op internally calls privileged code (CB6 guard).
- In-process JS hardening (SES/LavaMoat) is not needed. It only helps when
  privileged and untrusted JS share a realm, which CB7a forbids. Non-goal, with a
  documented re-open condition.
- Privileged routes use principal-class + identity gating -- the client is never
  trusted. Attenuated tokens (macaroon/Biscuit) are out of scope unless
  delegation is required.

## 4. Grounding (file references, verified 2026-05-30)

| Fact | Location |
| --- | --- |
| engine coordinator type | crates/nimbus-engine/src/service/mod.rs:58 `pub struct Service` |
| satellites | same file: ServiceBootstrapParts (:80), ServicePersistenceConfig, SERVICE_BACKGROUND_TASK (:93) |
| shared accessor | crates/nimbus-bridge/src/capabilities.rs:23 `fn service(&self) -> &Arc<Service>`; also lib.rs:212 |
| Service blast radius | ~1,453 standalone refs across ~302 .rs files |
| services routes (already exist) | crates/nimbus-server/src/router.rs:627-635 |
| control-plane routes (operator surface) | router.rs: /api/tenants (:615), /api/tenants/{id} (:617), /api/machines/* (:622-625) |
| sandbox manager | nimbus_services::SandboxServiceManager (construction.rs:15-16, service_manager.rs:5-6) |
| HostBridge trait (single op seam) | crates/nimbus-runtime/src/host.rs:673 |
| op extension assembly | crates/nimbus-runtime/src/runtime/bootstrap/extensions.rs (runtime_extension() from ops.rs:156) |
| permissions wiring | crates/nimbus-runtime/src/runtime_capabilities.rs (PermissionsContainer built :440-486); pin deno_permissions = "0.107.0" (Cargo.toml:69) |
| JS REST client | packages/nimbus/src/rest.ts (NimbusRestClient, NimbusSubscriptionClient) |
| pkg deps today | nimbus deps []; convex and nimbus-ui depend on nimbus; firebase/mongodb independent |
| operator console (consumes nimbus) | packages/nimbus-ui (nimbus/react, nimbus/browser); src/routes/operator/* + src/routes/developer/* |
| principal type | crates/nimbus-core/src/auth/mod.rs:19 PrincipalContext; :74 PrincipalClaimSource |
| operator/agent session model | docs/architecture/server/auth-runtime-trust.md (Agent Auth Contract); crates/nimbus-server/src/local_server/{access,audit,token}.rs |
| spawned-resource identity | TenantWorkloadStableIdentity from TenantIsolationDecision (auth-runtime-trust.md "Tenant Workload Identity") |
| de-brand: browser strings | packages/nimbus/src/browser.ts:375,398,526,663 |
| de-brand: react string | packages/nimbus/src/react.ts:529 |
| de-brand: runtime string | crates/nimbus-runtime/src/runtime/bootstrap/source.rs:476 |
| de-brand: .nimbus/convex path | 5 files (1 runtime + 4 test): runtime_capabilities.rs; runtime/bootstrap/ops/test_runtime/bundle.rs; runtime/tests/basic_invocation/{node_capabilities,support}.rs; runtime/tests/node/mod.rs |

## 5. Defense-in-depth layering

- Layer 1 PKG DEP HYGIENE (ergonomics): compat pkgs + @nimbus/core never import
  nimbus/rest.
- Layer 2 STATIC LINT (ergonomics): enforced in package source, not app code, in
  CI.
- Layer 3 GRANT-GATED OP (REAL boundary): services op added to a deployment's
  isolates ONLY when granted (opt-in, default off); compat ctx never carries
  services. Ungranted means op absent means unreachable.
- Layer 3' PERMISSION TIER: per-isolate deny-by-default deno_permissions profile
  (query/mutation = no net/fs; action = widened).
- Layer 4 SERVER GATE BY PRINCIPAL (REAL boundary; AWS-Lambda "client never
  trusted"): /api/.../services/* + control plane authorize by principal class --
  operator = cross-tenant (audited); tenant = own-tenant + grant; spawned
  resource = spawning-tenant scope.
- Layer 5 MICROVM ISOLATION: nimbus-libkrun -- a V8 escape cannot leave the VM.

Layers 1-2 are developer ergonomics. Layers 3 and 4 are the real boundaries.
Layer 5 bounds blast radius. The op-gating boundary (3) does not stop a V8 escape
(handled by 5), does not re-gate a registered op (still principal-gated at 4),
and breaks if a shared op internally calls privileged code (CB6 guard).

Rust capability split. SandboxCapabilityHost is implemented by a host bridge ONLY
for a granted deployment (any adapter type) -- not tied to "native vs compat."
The grant decides two things together: (a) the bridge implements
SandboxCapabilityHost, and (b) the services op extension is added to that
deployment's isolates.

- RuntimeCapabilityHost: shared -- engine() (coordinator), storage, principal;
  implemented by ALL host bridges (convex, firebase, cloud_functions, mongodb,
  native).
- SandboxCapabilityHost: privileged -- nimbus_services + control plane;
  implemented for any deployment GRANTED services (compat or native), NOT for
  ungranted deployments.

## 6. Phases

Each phase lists Goal, Files, Steps, Gate. Do not advance until the gate passes.

Dependency chain:

- CB0 then CB1 (CB1 lands alone, first).
- JS track: CB2 then CB3.
- Rust capability track: CB4 then CB5 then CB6.
- Runtime hardening: CB7, CB7a.
- CB8 (server gate) depends on CB4 (grant exists).
- CB9 (de-brand + guards) is last.

JS track (CB2/CB3) and Rust track (CB4..CB8) are independent after CB1 and may
run in parallel.

### CB0 - Baseline and frozen contract
- Goal: lock the consumer-visible compat API so later phases cannot regress it.
- Files: new test under packages/; scripts/verify-nimbus-capability-segregation.sh
  (stub).
- Steps: snapshot the public export surface of convex, @nimbus/firebase,
  @nimbus/mongodb, and the unprivileged surface of nimbus/(future) @nimbus/core
  as a contract test.
- Gate: contract test green on current main.

### CB1 - Rename engine coordinator Service to Engine (foundational; commit alone)
- Goal: free the word "service" for the sandbox concept only.
- Files: crates/nimbus-engine/src/service/** (Service to Engine,
  ServiceBootstrapParts to EngineBootstrapParts, ServicePersistenceConfig to
  EnginePersistenceConfig, SERVICE_BACKGROUND_TASK to ENGINE_BACKGROUND_TASK);
  nimbus-bridge (capabilities.rs:23, lib.rs:212 service() to engine()); every
  Arc<Service> / service: field across ~302 files. Optionally rename dir service/
  to engine/.
- Steps: prefer rust-analyzer rename; else scoped sed + cargo check loop. Do NOT
  touch nimbus_services, SandboxServiceManager, /services/*, service_manager.rs
  (the sandbox concept).
- Gate: cargo fmt --all --check, make check, make test green; no standalone
  engine-Service remains.

### CB2 - Extract @nimbus/core (JS Layer 1)
- Goal: an unprivileged shared package that compat adapters depend on.
- Files: new packages/core/ (@nimbus/core) holding values, server, browser,
  react, internal/shared, function-call client (re-homed from
  packages/nimbus/src); packages/nimbus/package.json (re-export core, keep
  ./rest); packages/convex/package.json (dep nimbus to @nimbus/core);
  @nimbus/codegen so generated _generated/* targets @nimbus/core. nimbus-ui keeps
  depending on nimbus (operator). firebase/mongodb only repoint if/when they
  consume core primitives.
- Gate: npm run typecheck && npm run test && npm run build green; no compat
  package.json depends on nimbus; CB0 contract test green.

### CB3 - nimbus/rest as the privileged JS entry + subpath lint (JS Layer 2)
- Goal: one privileged JS client; lint keeps it out of compat package source.
- Files: packages/nimbus/src/rest.ts (add startService/stopService/restartService
  wrapping /api/tenants/{t}/services/{name}/*); eslint + dependency-cruiser
  config; CI workflow.
- Steps: no-restricted-imports forbidding nimbus and nimbus/rest in compat package
  source (packages/convex, packages/firebase, packages/mongodb) and @nimbus/core.
  Scope note: target package source only; nimbus-ui (operator) and end-user app
  code importing nimbus are allowed.
- Gate: a planted import in compat package source fails CI; legitimate nimbus/rest
  use in nimbus, nimbus-ui, and an end-user app all pass.

### CB4 - SandboxCapabilityHost trait + per-deployment grant (Rust)
- Goal: a privileged capability trait, implemented only for granted deployments.
- Files: crates/nimbus-bridge/src/capabilities.rs (new trait); deployment
  config/state (nimbus-server construction.rs/state.rs); host-bridge impls.
- Steps: add SandboxCapabilityHost exposing nimbus_services
  (SandboxServiceManager/SandboxCatalog) + control-plane access. Keep
  RuntimeCapabilityHost::engine() shared. Add a per-deployment services grant
  (default off). A deployment's bridge provides SandboxCapabilityHost IFF granted
  -- independent of adapter type.
- Gate: ungranted deployment's bridge has no SandboxCapabilityHost; granted one
  does; make check green.

### CB5 - Grant-gated op registration (Rust Layer 3 - primary boundary)
- Goal: the services op exists in an isolate only when the deployment is granted.
- Files: crates/nimbus-runtime/src/runtime/bootstrap/extensions.rs + ops.rs (new
  nimbus_sandbox_ops extension); the isolate-construction site that knows the
  grant.
- Steps: put the services op(s) in a separate extension added to a deployment's
  isolate list only when granted (CB4). Do NOT route services through the generic
  op_nimbus_host_call -- give it its own grant-gated op so absence is the
  boundary. Defense in depth: even when present it dispatches via
  SandboxCapabilityHost (CB4) and is principal-gated (CB8). The compat ctx never
  carries services regardless of grant.
- Gate: ungranted deployment calling the services op fails because the op is
  absent; granted deployment succeeds and is principal/tenant-scoped.

### CB6 - Shared-op dispatch guard
- Goal: ensure no op reachable by ungranted/compat isolates reaches privileged
  code indirectly.
- Files: test/lint in nimbus-runtime or nimbus-server; review checklist.
- Steps: enumerate ops available to compat/shared bridges; assert none dispatches
  to nimbus_services/control-plane or returns an over-broad reference.
- Gate: test asserts no compat-reachable op calls a SandboxCapabilityHost path;
  checklist documented.

### CB7 - Per-tier deno_permissions profiles (Rust Layer 3')
- Goal: deny-by-default ambient authority by function tier.
- Files: crates/nimbus-runtime/src/runtime_capabilities.rs.
- Steps: profiles -- query/mutation: no Net/Read/Write/Run/Ffi; action: widened.
  Scope: per-isolate.
- Gate: a query-tier isolate is denied net/fs; an action-tier isolate gets only
  its configured set.

### CB7a - Realm-separation invariant (tested architecture rule)
- Goal: make "privileged JS never shares a realm with tenant code" and "operator
  credentials never reach a tenant isolate" enforceable.
- Files: a guard test; docs/architecture/runtime/adapter-boundary.md.
- Steps: assert (1) no privileged op or nimbus/rest symbol is reachable from a
  tenant isolate's realm; (2) operator session / local admin token material is
  not reachable from any tenant isolate. (1) keeps SES/LavaMoat out of scope; (2)
  is the operator-side analog of grant-gated op absence.
- Gate: guard test passes for both; doc updated.

### CB8 - Server-authoritative gate by principal class (Rust Layer 4 - real boundary)
- Goal: authorize the control plane and services routes by principal class,
  serving operators (cross-tenant) and tenants (own-tenant) correctly, even
  against a forged client.
- Files: auth-runtime-trust enforcement path; router.rs control-plane + services
  handlers; deployment grant state (CB4);
  docs/architecture/server/auth-runtime-trust.md,
  docs/architecture/runtime/adapter-boundary.md.
- Steps: resolve each request to a principal (operator / tenant / spawned
  resource) from the existing session + identity primitives, then authorize:
  - operator: cross-tenant allowed (create/list/delete tenants, grant services,
    manage any tenant's machines/services); emit an audit event per action.
  - tenant: must target its own tenant AND hold the services grant (for services
    routes).
  - spawned resource: scope from its TenantWorkloadStableIdentity; treated as its
    spawning tenant, never cross-tenant.
  Reject everything else, regardless of adapter type or client.
- Gate: integration tests -- (1) operator reaches another tenant's services:
  succeeds + audited; (2) tenant reaching another tenant: rejected; (3) ungranted
  tenant reaching its own services: rejected; (4) granted tenant reaching its own
  services: succeeds; (5) a tenant credential cannot resolve to operator.

### CB9 - De-brand + regression guards (last)
- Goal: remove residual Convex branding from neutral surfaces; lock it in.
- Files and exact targets:
  - packages/nimbus/src/browser.ts:375,398,663 (error strings) and :526
    (convex-${...} request-id prefix): neutral wording.
  - packages/nimbus/src/react.ts:529 ("convex paginated query failed").
  - crates/nimbus-runtime/src/runtime/bootstrap/source.rs:476 ("convex httpAction
    requires an authenticated identity").
  - .nimbus/convex bundle path: neutral bundle dir name, in 5 files:
    runtime_capabilities.rs; runtime/bootstrap/ops/test_runtime/bundle.rs;
    runtime/tests/basic_invocation/node_capabilities.rs;
    runtime/tests/basic_invocation/support.rs; runtime/tests/node/mod.rs.
- Steps: add a regression guard -- no new adapter-branded identifiers in
  nimbus/nimbus-core/nimbus-runtime; compat packages cannot reach privileged JS
  entries.
- Gate: lint/test green; full make ci + npm run build green.

## 7. Decisions

- Services capability model: per-deployment grant, off by default (opt-in); grant
  adds the nimbus_sandbox_ops extension + SandboxCapabilityHost impl; ungranted
  means op absent means unreachable.
- In-isolate guarantee: op present only when granted (strongest form).
- Authorization model: by principal class -- operator = global cross-tenant
  (audited); tenant = own-tenant + grant; spawned resource = spawning-tenant
  scope via TenantWorkloadStableIdentity. Reuses the existing operator-session +
  workload-identity machinery; no parallel identity system.
- Operator credentials: never resolvable inside a tenant isolate; a tenant
  credential can never resolve to operator (CB7a/CB8).
- Engine coordinator name: nimbus_engine::Engine.
- Unprivileged JS package: @nimbus/core, internal-only for now.
- Privileged Rust trait: SandboxCapabilityHost.
- Privileged JS entry: nimbus/rest (mirrors the server REST surface 1:1; services
  start/stop/restart included).
- Privileged-route auth: principal-class + identity gating now; attenuated tokens
  (macaroon/Biscuit) out of scope unless delegation appears.
- Per-tier permission scope: per-isolate.
- SES/LavaMoat: not adopted (see non-goals; re-open condition documented).

## 8. Non-goals

- Changing the shared Engine execution path -- all adapters keep routing through
  it (intentional invariant).
- Changing Convex/Firebase wire protocols or document/value model.
- Back-compat shims (pre-launch) -- including no Service type alias after CB1.
- Restricting scheduler/crons at the function layer (Convex parity preserved;
  only the REST control-plane admin routes are gated).
- In-process JS hardening (SES/LavaMoat) -- unnecessary while the realm-separation
  invariant (CB7a) holds; re-open only if privileged JS and untrusted JS ever
  share a realm.
- A JS capability token as a security mechanism -- the boundaries are the
  grant-gated op (CB5) + principal gate (CB8).
- A new operator identity system -- reuse the existing operator-session, local
  admin token, and TenantWorkloadStableIdentity machinery.

## 9. Control-plane verifier

scripts/verify-nimbus-capability-segregation.sh asserts:

1. no standalone engine-Service references remain (CB1);
2. compat package.json files depend only on @nimbus/core (CB2);
3. no nimbus or nimbus/rest import in compat package source or @nimbus/core
   (CB3 lint) -- nimbus-ui and app code exempt;
4. ungranted deployment's bridge lacks SandboxCapabilityHost; granted has it
   (CB4);
5. services op absent from an ungranted isolate; present when granted (CB5);
6. no compat/shared op dispatches to a privileged path (CB6);
7. per-isolate per-tier permission profile test passes (CB7);
8. realm-separation guard passes, incl. operator credentials unreachable from a
   tenant isolate (CB7a);
9. principal-class gating tests: operator cross-tenant succeeds+audited, tenant
   cross-tenant rejected, ungranted-own rejected, granted-own succeeds, tenant
   credential cannot resolve to operator (CB8);
10. de-brand regression guard passes (CB9).

## 10. Future trigger (not a pending decision)

A separate nimbus/services JS entry is added only if/when a live, streaming VMM
link is built (exec-into-VM with streamed output, port-forward, desktop attach)
-- long-lived sockets, not REST, so they need their own client. Until then
nimbus/rest is the sole privileged JS entry.
