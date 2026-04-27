# Cloud Functions Compatibility Plan

Canonical execution plan for Cloud Functions-compatible compute on Neovex:
durable document-trigger delivery sourced from committed writes, a
protocol-neutral trigger registry plus generalized runtime artifact contract,
CloudEvent-native event dispatch, exact-source-compatible authoring surfaces
for `firebase-functions/v2` and `@google-cloud/functions-framework`, and
Neovex-hosted HTTP dispatch for the covered handler shapes.

This plan is a **follow-on to the Firebase adapter data-layer plan**
(`docs/plans/archive/firebase-adapter-plan.md`). That plan deliberately scoped
Firestore as "a data API rather than a function runtime" and excluded compute.
This plan fills the compute gap using the same architecture principle: promote
shared behavior into a protocol-neutral Neovex primitive before adding
adapter-local copies.

The scope is intentionally broader than Firebase. `firebase-functions/v2` and
`@google-cloud/functions-framework` are independent packages that both consume
the same Firestore CloudEvent types
(`google.cloud.firestore.document.v1.{created,updated,deleted,written}`) and
the same `DocumentEventData` payload shape. A developer who uses Cloud
Functions directly — without Firebase — should be able to bring their covered
handler source to Neovex without rewriting imports or handler bodies. A
developer who uses Firebase Cloud Functions should get the same zero-breaking-
change authoring path through the Firebase v2 surface. This plan is about
Cloud Functions authoring compatibility on Neovex; it does not claim first-
release replacement of the standalone Functions Framework local web server and
runtime contract unless a section below explicitly says so.

## Motivation

Cloud Functions developers — whether using Firebase or standalone GCP — expect
server-side compute triggered by document changes and HTTP requests. Without
Cloud Functions compatibility, Neovex asks migrating developers to "figure out
your own server-side logic" — a significant DX gap for a product positioning
itself as a credible Firebase and GCP alternative.

There are two distinct migration audiences:

1. **Firebase developers** using `firebase-functions/v2` with
   `onDocumentCreated` etc. They expect the Firebase v2 authoring API to work.
2. **Standalone Cloud Functions developers** using
   `@google-cloud/functions-framework` with `functions.cloudEvent()` and
   `functions.http()`. They expect the framework handler contract to work.

Both audiences share the same underlying Firestore CloudEvent types and
`DocumentEventData` payload. The plan targets both by building a shared
CloudEvent dispatch and execution foundation, then layering two authoring
surfaces on top.

The implementation effort is still modest relative to the DX payoff, but the
missing pieces are broader than "just add a trigger registry." The core
building blocks already exist:

- **Committed write records plus the durable mutation journal**
  (`CommitEntry`, per-write previous/current state, and the engine-owned journal
  path): these are the authoritative source for document-trigger events.
- **Resource-path metadata** (from the Firebase adapter data-layer plan):
  this lets the engine resolve Firestore document resources without pushing
  adapter-local path semantics into trigger code.
- **V8 runtime** (`neovex-runtime`): executes JavaScript with host access via
  the `HostBridge` trait.
- **Atomic write batch** (F0.3): triggered functions can write back safely
  through the engine-owned mutation path.
- **Deploy admin API and generation activation**: Neovex already stages,
  validates, and atomically activates runtime artifacts, even though the
  current path is Convex-shaped.

The missing pieces are:

- a **durable trigger delivery model** that survives crash/restart and supports
  retries with a persisted cursor or invocation ledger,
- a **generalized runtime authoring/build/deploy contract** for non-Convex
  handlers,
- a **Firebase and Cloud Functions app-discovery contract** so existing
  `firebase.json` `functions.source` / `codebase` layouts, the conventional
  `functions/` package, and common standalone Functions Framework package
  roots are auto-detected from the working tree for most apps, with
  `--app-dir` retained as an explicit override instead of the default path,
- a **CloudEvent-native event model** matching the standard Firestore event
  types and `DocumentEventData` payload so both authoring surfaces consume the
  same events,
- a **covered `firebase-admin` compatibility surface** for the common
  `firebase-admin/app` and `firebase-admin/firestore` imports that modern
  Firebase function bodies use,
- and compatible **authoring surfaces** for both `firebase-functions/v2` and
  `@google-cloud/functions-framework` handler contracts.

## Context

### What Already Exists

| Component | Location | Relevance |
|-----------|----------|-----------|
| `CommitEntry` plus per-write previous/current documents | `neovex-core`, engine mutation path | Authoritative committed document change source |
| Durable mutation journal | `neovex-engine`, `neovex-storage` | Canonical replay/recovery substrate for at-least-once trigger delivery |
| Resource-path metadata | Firebase adapter F0.2/F3.2 work | Resolves Firestore document resources from committed writes |
| V8 runtime + `HostBridge` | `neovex-runtime` | Executes JS with host database access |
| `ConvexHostBridge` | `neovex-server/src/adapters/convex/host_bridge/` | Per-invocation V8 execution with mutation unit |
| Atomic write batch | `neovex-engine` (F0.3) | Triggered functions commit atomically |
| Deploy admin API + generation activation | server deploy path | Existing artifact staging/validation/activation seam that currently loads Convex bundles |
| Subscription diff helper | `neovex-core/src/subscription.rs` | Useful secondary helper for tests/comparisons, but not the authoritative trigger event source |

### What Does Not Exist Yet

- A protocol-neutral trigger registry (path pattern → function mapping).
- A durable per-tenant trigger cursor or invocation ledger with crash recovery.
- Event dispatch from committed writes/resource paths into trigger execution.
- A CloudEvent-native event model using standard Firestore event type strings
  (`google.cloud.firestore.document.v1.{created,updated,deleted,written}`) and
  `DocumentEventData` payload shape (`value`, `oldValue`, `updateMask`).
- A `functions-framework`-compatible handler contract
  (`functions.cloudEvent()` / `functions.http()`) for standalone Cloud
  Functions developers.
- A `firebase-functions/v2`-compatible authoring surface
  (`onDocumentCreated`, etc.) for Firebase developers.
- A generalized function build/deployment surface for trigger handlers.
- A shared app-root discovery contract that auto-detects existing Firebase
  project roots and common standalone Functions Framework package roots from
  the working tree, with `--app-dir` as an explicit override.
- A Neovex-owned internal artifact layout for Firebase apps, parallel to the
  Convex `.neovex/convex/` pattern, so generated outputs stay out of the user
  `functions/` source tree.
- An exact import-resolution or alias contract for
  `firebase-functions/v2/*` and `@google-cloud/functions-framework`.
- An exact import-resolution or alias contract for the covered
  `firebase-admin` surfaces used by existing modern Firebase apps.
- A deploy-time target/binding manifest that binds named framework handler
  targets to signature type, Firestore event type, database, document path
  pattern, and service-principal execution semantics.
- A published support matrix for `DocumentOptions`, `HttpsOptions`, and
  `CallableOptions`, including fail-fast behavior for unsupported fields.
- A published support matrix for covered `firebase-admin` modules and methods.

## Status

- **Plan status:** `done`
- **Control item:** `complete`
- **Activation gate:** met — `docs/plans/archive/firebase-adapter-plan.md` is `done`,
  so the write-batch and path-metadata infrastructure is available for the
  compute follow-on.
- **Status values:** `pending`, `in_progress`, `done`, `blocked`
- **Primary source of truth:** this file plus the current git worktree.

## Compatibility Promise

This plan keeps "compatibility" explicit. Do not blur these layers:

- **App-discovery and project-layout compatibility:** Neovex should follow
  the same low-friction posture it already uses for Convex-compatible apps:
  detect the app root from the current directory or its parents for the common
  Firebase and standalone Cloud Functions layouts, and use `--app-dir` only as
  an explicit override for unusual repos. For Firebase projects, Neovex must
  preserve `firebase.json` `functions.source` / `codebase` layout, the
  conventional `functions/` package, and monorepo source-package organization
  instead of requiring a Neovex-specific source-root rename.
- **Exact source compatibility:** for covered surfaces, existing handler
  modules keep the same import specifiers and handler bodies. Neovex must
  preserve `firebase-functions/v2`, `firebase-functions/v2/*`, and
  `@google-cloud/functions-framework` imports, plus the covered
  `firebase-admin` imports used by modern Firebase apps, through compatible
  packages or resolver aliases during build/deploy, or the plan must downgrade
  that surface to rewrite-required instead of claiming zero-breaking-change
  migration.
- **Deploy compatibility:** Neovex provides its own deploy and binding
  contract. Existing `firebase deploy`, `gcloud functions deploy`, or raw
  Eventarc configuration are not portable contracts for this plan.
- **Runtime compatibility:** Neovex hosts execution inside its own runtime and
  HTTP server. First release covers execution of covered handlers on Neovex,
  not full replacement of the standalone Functions Framework local server
  contract (`FUNCTION_TARGET`, `FUNCTION_SIGNATURE_TYPE`, generic HTTP
  CloudEvent unmarshalling, or root/all-path routing behavior) unless a later
  phase explicitly promotes that work.

## Non-Negotiable Design Constraints

- **Trigger correctness comes from committed writes and resource paths, not
  subscription diffs.** `diff_subscription_snapshots` is a useful helper for
  subscription-facing behavior, but document triggers must derive from the
  authoritative committed write record or durable journal replay.
- **At-least-once delivery requires durable state before execution.** A plain
  post-commit in-memory enqueue is not sufficient. The trigger dispatcher needs
  a persisted cursor or invocation ledger so crash/restart replay is defined.
- **Cloud Functions authoring and deploy are first-class work items.** The
  current runtime artifact and deploy path is Convex-specific. This plan must
  explicitly decide whether to generalize that path or add a sibling path with
  the same integrity and generation-activation guarantees before execution
  starts.
- **Deploy-time trigger bindings are first-class.**
  `functions.cloudEvent(name, handler)` declares a target name and handler
  shape, not a Firestore event filter. The deploy contract must bind target →
  signature type, event type, database, document path pattern, and service
  principal before framework-trigger parity is credible.
- **Firestore triggers execute in a trusted service context.** Base document
  triggers run under a system/service principal, not the calling end-user
  principal. Auth-context trigger variants are a separate compatibility slice.
- **The first slice matches Firebase document trigger scope.** Trigger patterns
  target documents, including wildcard document paths. Collection-group
  matching or other non-Firebase trigger shapes stay deferred unless Neovex
  intentionally adds them as a documented extension.
- **CloudEvent types and payload shapes match the GCP standard.** Neovex
  emits `google.cloud.firestore.document.v1.{created,updated,deleted,written}`
  type strings and `DocumentEventData` payloads (`value`, `oldValue`,
  `updateMask`) so both `firebase-functions/v2` handlers and
  `functions-framework` CloudEvent handlers consume the same events without
  translation. This is the compatibility contract that enables zero-breaking-
  change migration for both audiences.
- **Zero-breaking-change means unchanged imports as well as unchanged handler
  bodies.** A plan item cannot claim zero-breaking-change migration if it
  still requires developers to swap imports away from
  `firebase-functions/v2/*` or `@google-cloud/functions-framework`.
- **Functions Framework compatibility is authoring/deploy compatibility first,
  not full standalone server parity.** The first release targets Neovex-hosted
  execution of `functions.cloudEvent()` and `functions.http()` handlers. Full
  replacement of the standalone Functions Framework local web server contract
  remains deferred unless explicitly promoted.
- **HTTP and callable option parity is explicit, not implied.** `onRequest`
  and `onCall` opts overloads are only compatible for the fields listed in the
  published support matrix. Unsupported `DocumentOptions`, `HttpsOptions`, and
  `CallableOptions` fields must fail validation; Neovex must not silently
  ignore them. Callable protocol details such as auth context, App Check
  semantics, CORS defaults, and `HttpsError` mapping are first-class
  compatibility items.
- **Firebase root-level defaults are explicit.** Modules that import from
  `firebase-functions/v2` and call `setGlobalOptions()` before declaring
  Firestore or HTTPS handlers are common. This plan must either support
  root-level default option inheritance for the covered surfaces or defer it
  explicitly; it cannot remain unspecified while claiming exact source
  compatibility.
- **Existing app structure is a compatibility input.** The first slice must
  plug Firebase and Cloud Functions discovery into the same shared app-root
  resolver used by Convex-style workflows: auto-detect the nearest compatible
  app root from the current directory or its parents, and keep `--app-dir` as
  a deterministic override. Firebase apps must preserve conventional
  `firebase.json` plus `functions/` layout, including `functions.source` /
  `codebase` monorepo configurations, rather than asking developers to move
  code into a Neovex-specific source tree.
- **Most migrations should not need `--app-dir`.** If a modern Firebase app
  or a conventional standalone Functions Framework package already has the
  expected config and source layout, running Neovex from that repo or package
  root should resolve the app automatically. Requiring `--app-dir` for the
  common case is a compatibility miss, not an acceptable steady state.
- **Covered `firebase-admin` imports are first-class.** Modern Firebase
  functions commonly call `initializeApp()` and `getFirestore()` from
  `firebase-admin/*`. If those imports still require body rewrites, Neovex
  cannot honestly claim low-friction migration for existing apps.
- **Two authoring surfaces, one execution foundation.** The
  `firebase-functions/v2` surface (`onDocumentCreated` etc.) and the
  `functions-framework` surface (`functions.cloudEvent()` /
  `functions.http()`) are different registration APIs over the same shared
  trigger registry, event dispatch, and V8 execution path. Do not duplicate
  the execution path per authoring surface.

## Architecture Boundary Contract

### Neovex Core / Engine Owns (Shared Primitive)

The trigger registry and event dispatch are **not Firebase-adapter-local**. They
are shared Neovex primitives that both adapters could use:

- **Trigger registry:** maps document path patterns to registered function
  references. The first slice supports Firestore-style document path patterns
  such as `users/{userId}` and
  `users/{userId}/{messageCollectionId}/{messageId}`. The registry does not
  know which protocol surface registered the function.
- **Authoritative event source:** derives trigger candidates from committed
  writes plus resolved resource paths, either directly from the committed write
  record or from durable journal replay. Subscription snapshot diffs are not
  the correctness source for document triggers.
- **Durable delivery state:** owns the persisted cursor or invocation ledger
  that records which committed events have been materialized for execution,
  retry state, completion state, and crash-recovery resume position.
- **Trigger execution:** spins up a V8 isolate or generalized runtime
  invocation for the matched function, passes the durable event record as
  input, and commits any writes through the engine-owned mutation path. The
  delivery contract is at-least-once with documented retry semantics.
- **Trigger event model:** carries CloudEvent identity (`id`, `source`,
  `specversion`, `type`, `time`, `subject`) using the standard
  `google.cloud.firestore.document.v1.{created,updated,deleted,written}` type
  strings, plus `DocumentEventData` payload (`value`, `oldValue`,
  `updateMask`), captured path params, commit metadata, and service-principal
  execution context. Both authoring surfaces consume this same event model.
- **Trigger lifecycle:** registration, deregistration, enable/disable, and
  cleanup. Triggers are tenant-scoped and generation-bound; the executing
  function runs under a trusted service principal.

### Shared Server / Runtime Deploy Surface Owns

- **Shared app-root discovery:** one resolver for Convex, Firebase, and
  standalone Cloud Functions authoring modes. The resolver must honor an
  explicit `--app-dir` override, otherwise walk the current directory and its
  parents to find the nearest compatible app root. For Firebase projects, that
  means parsing `firebase.json` plus `functions.source` / `codebase` entries.
  For standalone Cloud Functions projects, that means recognizing a package
  root with the framework dependency and a conventional handler entrypoint
  shape. The user-facing source-package layout must stay intact.
- **Internal artifact layout:** a Neovex-owned generated output root for
  Firebase apps, analogous to `.neovex/convex/`, so codegen and deploy
  artifacts do not require renaming or polluting the user `functions/`
  source tree.
- **Runtime artifact contract:** the manifest and bundle format used for
  trigger handlers, including integrity validation and runtime handler
  addressing.
- **Import-resolution contract:** exact source compatibility for
  `firebase-functions/v2`, `firebase-functions/v2/*`,
  `@google-cloud/functions-framework`, and the covered `firebase-admin`
  imports, either by shipping compatible packages at those specifiers or by a
  Neovex-owned build or deploy alias layer that preserves source imports
  without rewrites.
- **Target discovery and binding manifest:** source-discovered handler targets,
  signature types, and a deploy-time manifest/config that binds those targets
  to Firestore event filters (`type`, `database`, `document`), HTTP exposure,
  and service-principal execution semantics.
- **Build/deploy activation:** staging, validation, generation activation, and
  rollback behavior for trigger handler artifacts. This plan must either
  generalize the current Convex-shaped deploy path or add a sibling path with
  the same operator and integrity guarantees.
- **Runtime compatibility boundary:** first release covers Neovex-hosted
  execution of covered handlers. A generic standalone Functions Framework web
  server, raw HTTP CloudEvent ingress unmarshalling, and environment-variable-
  driven target selection remain separate compatibility work.

### Adapters Own

- **Firebase adapter:** `onDocumentCreated`, `onDocumentUpdated`,
  `onDocumentDeleted`, `onDocumentWritten` registration surface. Maps
  Firebase-shaped path patterns to the shared trigger registry. Constructs
  Firebase-shaped `FirestoreEvent` / `Change<DocumentSnapshot>` objects from
  the shared trigger event model, including CloudEvent identity fields and
  Firestore metadata such as project, database, document path, and params.
- **Functions-framework adapter:** `functions.cloudEvent(name, handler)` and
  `functions.http(name, handler)` registration surface with exact source import
  compatibility. Source registration declares a target name and handler shape;
  deploy-time binding metadata associates that target with Firestore event
  filters or HTTP exposure. The adapter passes the standard CloudEvent object
  directly to handlers without Firebase-specific wrapping.
- **Convex adapter:** could wire its own scheduler/trigger system to the same
  shared primitive in the future (not in scope for this plan).
- **Authoring-surface-specific ergonomics:** package/import shape, build-time
  metadata extraction, and migration affordances for existing Firebase or
  Cloud Functions source.
- **Firebase server-SDK compatibility surface:** the covered `firebase-admin`
  modules and methods used inside existing function bodies, plus their mapping
  to the shared Firebase adapter and engine-owned mutation/read paths.

## Compatibility Scope

### In Scope — Project Layout And Server SDK (T0-T2)

| Surface | Source contract | Notes |
|---------|-----------------|-------|
| Auto-detected Firebase project root | `firebase.json` | Running Neovex from a Firebase repo or a child path should discover the nearest compatible Firebase app root without requiring `--app-dir` |
| Auto-detected standalone Cloud Functions package root | `package.json` + framework dependency | Running Neovex from a conventional Functions Framework package should discover that package root without requiring `--app-dir` |
| Explicit app-root override | `--app-dir` | Remains available for Firebase repos, standalone Functions Framework packages, monorepos, and ambiguous working directories, but is not required for the common migration path |
| Functions source package discovery | `functions.source` / default `functions/` | Preserve existing source-package naming and layout |
| Multi-codebase monorepo discovery | `firebase.json.functions[].codebase` | Preserve codebase segmentation and source-package boundaries |
| Standalone package entrypoint discovery | `package.json.main` / default `index.*` | Cover the common Functions Framework package-root patterns so most standalone apps migrate without layout changes |
| Neovex-generated artifact root | `.neovex/firebase/` (planned) | Generated manifests and bundles stay in a Neovex-owned internal directory, parallel to `.neovex/convex/` |
| `firebase-admin/app` covered subset | exact import compatibility | Minimum first slice must explicitly settle `initializeApp()` plus default-app lifecycle behavior used by covered handlers |
| `firebase-admin/firestore` covered subset | exact import compatibility | Minimum first slice must explicitly settle `getFirestore()` and the covered Firestore read/write APIs used by migration fixtures |

The `firebase-admin` surface is not blanket-compatible by default. The first
slice must publish an explicit support matrix and fail validation clearly for
unsupported admin namespaces or methods.

### In Scope — Document Triggers (T0-T2)

Document-trigger compatibility in this plan has two layers:

1. **Source authoring compatibility:** handler code keeps the same imports and
   registration calls for covered APIs.
2. **Deploy-time binding compatibility:** Neovex bind manifests/config attach
   those handlers to Firestore event filters, database, document path pattern,
   and service-principal behavior. Eventarc and `gcloud` deployment parity are
   not part of this contract.

| API | Source package | Notes |
|-----|---------------|-------|
| `onDocumentCreated(path, handler)` | `firebase-functions/v2/firestore` | Firebase v2 authoring surface |
| `onDocumentUpdated(path, handler)` | `firebase-functions/v2/firestore` | Firebase v2 authoring surface |
| `onDocumentDeleted(path, handler)` | `firebase-functions/v2/firestore` | Firebase v2 authoring surface |
| `onDocumentWritten(path, handler)` | `firebase-functions/v2/firestore` | Firebase v2 authoring surface |
| `functions.cloudEvent(name, handler)` | `@google-cloud/functions-framework` | Exact-source-compatible named CloudEvent target; deploy-time binding manifest supplies event filters |
| Document path wildcard matching | both | `users/{userId}` — trigger patterns must resolve to documents, not collections |
| `google.cloud.firestore.document.v1.*` CloudEvent types | GCP standard | `created`, `updated`, `deleted`, `written` type strings |
| `DocumentEventData` payload | GCP standard | `value` (after), `oldValue` (before), `updateMask` |
| Trusted execution context | both | Function code executes as a service principal through the standard engine mutation path |
| `setGlobalOptions()` root defaults | `firebase-functions/v2` | In scope only for published `GlobalOptions` fields that the covered surfaces inherit under Neovex |
| Covered `firebase-admin/firestore` usage inside handlers | `firebase-admin/firestore` | In scope only for methods listed in the published admin compatibility matrix |

Base `path, handler` overloads are required. Firebase `opts, handler`
document-trigger overloads are only in scope for fields listed in the
`DocumentOptions` support matrix; unsupported fields must fail validation.

### In Scope — HTTP Handlers (T3)

| API | Source package | Notes |
|-----|---------------|-------|
| `functions.http(name, handler)` | `@google-cloud/functions-framework` | Exact-source-compatible named HTTP target; served through Neovex HTTP routing, not the standalone framework dev server |
| `onRequest(handler)` | `firebase-functions/v2/https` | Required base overload |
| `onRequest(opts, handler)` | `firebase-functions/v2/https` | In scope only for fields listed in the `HttpsOptions` support matrix |
| `onCall(handler)` | `firebase-functions/v2/https` | Required base callable overload |
| `onCall(opts, handler)` | `firebase-functions/v2/https` | In scope only for fields listed in the `CallableOptions` support matrix |

HTTP handlers are a separate dispatch mechanism (no trigger registry, no
durable delivery) but share the same runtime artifact and deploy contract.
They are sequenced after document triggers because the execution foundation
from T1 and the deploy contract from T0.4 make them straightforward.

The first HTTP slice must publish an explicit compatibility matrix:

- `onRequest` and `onCall` base overloads are required.
- `setGlobalOptions()` inheritance for covered HTTPS and document-trigger
  surfaces is field-by-field, not implied by import compatibility alone.
- `HttpsOptions` and `CallableOptions` support is field-by-field, not
  all-or-nothing.
- `onCall` compatibility must explicitly cover request envelope shape, auth
  context extraction, App Check contract, default CORS behavior, and
  `HttpsError`/FunctionsErrorCode mapping.
- Unsupported option fields and unsupported callable features must fail
  validation clearly instead of degrading silently.

### Deferred

- **Collection-group triggers or other non-Firebase trigger matching:** not
  part of document trigger compatibility.
- **Full standalone Functions Framework runtime contract:** local server
  parity for `FUNCTION_TARGET`, `FUNCTION_SIGNATURE_TYPE`, generic HTTP
  CloudEvent unmarshalling, and root/all-path routing behavior.
- **Auth-context document triggers**
  (`onDocumentCreatedWithAuthContext`, etc.): separate slice after the base
  service-principal event model lands.
- **`DocumentOptions`, `HttpsOptions`, and `CallableOptions` fields outside the
  published support matrix:** defer until explicitly promoted.
- **`GlobalOptions` fields outside the published support matrix, and root-level
  APIs such as `onInit()` unless explicitly promoted:** defer until explicitly
  promoted.
- **`firebase-admin` namespaces or methods outside the published compatibility
  matrix:** defer until explicitly promoted.
- **Callable streaming responses and other advanced callable-only response
  features:** defer until the base callable protocol contract is stable.
- **Scheduled functions** (`onSchedule`): the engine already has a scheduler
  primitive; wiring it to Firebase/Cloud Functions-shaped cron syntax is a
  separate slice.
- **Auth triggers** (`onCreate`, `onDelete` for auth): requires auth event
  pipeline, not document changes.
- **Storage triggers** (`onObjectFinalized`, etc.): requires storage event
  pipeline.
- **Pub/Sub triggers:** requires event source outside Neovex's current scope.
- **Firebase Extensions:** out of scope.
- **Firebase Emulator Suite compatibility:** out of scope for first release.
- **`gcloud functions deploy` CLI compatibility:** Neovex provides its own
  deploy path; the function code is the portable contract, not the deployment
  CLI.

## Implementation Phases

### T0: Trigger Contract And Durable Foundation

Location: `neovex-core/src/trigger.rs`, `neovex-engine/src/triggers/`,
shared deploy/runtime support, and the chosen package or resolver-alias
surface for exact import compatibility.

- Freeze the authoritative trigger source on committed writes plus resolved
  resource paths. Explicitly document that subscription diffs are not the
  primary source of truth.
- Define `TriggerPattern` for Firestore document paths with wildcard segments.
  Patterns must always resolve to documents, not collections.
- Define `TriggerRegistration` with pattern, function reference, event filter,
  tenant binding, generation binding, and enabled flag.
- Define `TriggerEvent` / envelope carrying CloudEvent identity,
  before/after snapshots, captured params, Firestore metadata, commit metadata,
  and service-principal execution context.
- Define the durable delivery state model: per-tenant cursor or invocation
  ledger, retry metadata, completion state, and crash-recovery rules.
- Decide the generalized runtime artifact, import-resolution, and deploy
  contract for non-Convex handlers before any execution work starts. This
  includes whether Neovex generalizes the existing deploy/admin/runtime
  registry path or adds a sibling path with the same integrity and activation
  guarantees, plus how unchanged imports for `firebase-functions/v2`,
  `firebase-functions/v2/*`, `@google-cloud/functions-framework`, and the
  covered `firebase-admin` surfaces resolve under Neovex.
- Define the shared app-root and internal-artifact contract: how an explicit
  `--app-dir` override and the default cwd/parent auto-discovery path resolve
  Firebase `firebase.json`, `functions.source`, and `codebase` layouts, plus
  common standalone Functions Framework package roots. Document precedence,
  ambiguity resolution, and where Neovex writes generated manifests or bundles
  (planned `.neovex/firebase/`) without requiring user source-tree renames.
- Define the deploy-time target/binding manifest for framework handlers:
  target name, signature type, Firestore event type, database, document path
  pattern, HTTP exposure, and service-principal binding. Document how
  Firebase path-based registration and framework name-based registration lower
  into the same shared trigger registry or HTTP routing surface.
- Publish the first explicit compatibility boundary for standalone Functions
  Framework runtime parity and the first support matrices for
  `DocumentOptions`, `HttpsOptions`, `CallableOptions`, and inherited
  `GlobalOptions`, plus the covered `firebase-admin` subset.
- Implement the registry and metadata types with focused tests.

#### T0.3 Durable Delivery Choice

- Use an **invocation-ledger model with a journal-backed materialization
  cursor**, not a cursor-only replay design.
- The per-tenant cursor records the highest committed journal sequence whose
  writes have already been expanded into durable trigger invocation records.
- The durable ledger stores one record per matched handler invocation, keyed
  by `(registration_id, cloud_event.id)`, with independent
  `Pending`/`Running`/`RetryPending`/`Completed`/`TerminalFailure` state.
- Cursor advancement happens when those invocation records are durably
  persisted, not when execution finishes; completion and retry semantics live
  on the invocation records themselves, and no-match commits still advance the
  cursor.

#### T0.4 Artifact And Import-Resolution Choice

- Keep the existing deploy-admin guarantees shared: authenticated staging,
  integrity validation, dry-run diffing, and atomic generation activation.
- Do **not** force the current Convex manifest into an artificial generic
  schema. Cloud Functions uses a sibling internal artifact family under
  `.neovex/firebase/`.
- Fix the first-slice `artifact.json` envelope to the Cloud Functions family,
  reserve `targets.json` as the deploy-time binding manifest, and keep the
  runtime bundle paired with a SHA-256 sidecar.
- Preserve unchanged source imports through a **Neovex-owned deploy/build alias
  layer**, not by rewriting user code or replacing upstream packages in the
  user's dependency graph.

#### T0.5 Target And Binding Choice

- Bind source-discovered handler targets through a typed `targets.json`
  manifest rather than inferring deploy bindings from runtime bundle exports.
- Every target records: authoring surface, target name, runtime entrypoint,
  signature type, binding kind, and execution identity semantics.
- Firestore document bindings must use `cloudevent` signature type, a
  Firestore CloudEvent type string, a database id, a document-terminal pattern,
  and trusted `service` execution.
- HTTP bindings must use `http` signature type, request-scoped execution, and
  explicit Neovex-hosted path exposure.
- Explicit first-slice rejections: legacy Functions Framework `event`
  signatures and Firestore `namespace` bindings.

#### T0.6 Root Defaults Choice

- Cover `firebase-functions/v2` root imports conservatively: support
  `setGlobalOptions()` for covered surfaces, but reject `onInit()` in the
  first slice.
- Inheritance order is explicit: per-handler option wins, then
  `setGlobalOptions()` default, then no value.
- The only inherited first-slice `GlobalOptions` field is `retry` for
  Firestore document triggers.
- HTTPS `onRequest()` / `onCall()` root-default inheritance remains deferred
  until the HTTP phase owns that runtime behavior.
- All other `GlobalOptions` fields fail validation explicitly instead of being
  silently ignored.

#### T0.7 Registry Choice

- Add a per-tenant, engine-owned `TriggerRegistry` rather than a server-local
  helper so later durable dispatch and both authoring surfaces reuse the same
  lookup seam.
- Registrations are keyed by stable string ids, carry Firestore event type plus
  `DocumentTriggerPattern`, and support register/deregister/enable/disable/list
  without coupling to any adapter-specific handler wrapper.
- Lookup filters on enabled state plus event type and returns the captured path
  params from the shared `DocumentTriggerPattern` matcher.

### T1: Durable Dispatch And Execution

Location: `neovex-engine/src/triggers/dispatch.rs`,
`neovex-engine/src/triggers/execution.rs`.

- Materialize trigger candidates from committed writes and resource paths,
  either directly after commit or via durable journal replay, without relying
  on subscription snapshot diffs.
- Persist matched invocation records before dispatch and advance the durable
  cursor only according to the documented at-least-once delivery rules.
- Enqueue execution with bounded concurrency and explicit backpressure, while
  keeping commit latency and subscription delivery isolated from trigger work.
- Execute trigger functions through the V8 runtime or generalized runtime
  registry with a trigger-scoped `HostBridge` that provides database
  read/write access through the standard engine mutation path.
- Track retryable failures, terminal failures, completion, and crash-recovery
  replay from persisted state.
- Ensure trigger writes go through the same engine-owned mutation path as all
  other writes (no bypass).

### T2: Authoring Surfaces And Deploy Integration

Location: `crates/neovex-server/src/adapters/firebase/triggers/`,
`crates/neovex-server/src/adapters/cloud_functions/`, the chosen generalized
deploy/runtime path, and the package or resolver-alias surfaces used to
preserve exact imports.

- Implement the trigger handler build/deploy contract: bundle upload, manifest
  validation, target discovery, binding validation, integrity checking,
  generation activation, runtime handler resolution, and exact import
  resolution or aliasing for `firebase-functions/v2`,
  `firebase-functions/v2/*`, `@google-cloud/functions-framework`, and the
  covered `firebase-admin` imports. The deploy contract must support both
  authoring surfaces without requiring separate artifact pipelines or a
  source-tree restructure.
- Add Firebase-shaped trigger registration API that maps
  `onDocumentCreated("users/{userId}", handler)` to the shared trigger
  registry. Construct Firebase-shaped `FirestoreEvent` /
  `Change<DocumentSnapshot>` objects from durable trigger events, including
  `before`/`after`, `params`, `project`, `database`, `document`, and
  CloudEvent identity fields.
- Add `functions-framework`-compatible handler registration that discovers
  named CloudEvent targets from `functions.cloudEvent(name, handler)`. Use the
  deploy-time binding manifest to associate those targets with Firestore event
  filters before lowering them into the shared trigger registry. Pass the
  standard CloudEvent object (with `google.cloud.firestore.document.v1.*`
  type and `DocumentEventData` payload) directly to handlers without
  Firebase-specific wrapping.
- Add exact-source-compatible authoring surfaces for both packages. For
  covered APIs, existing `firebase-functions/v2`,
  `firebase-functions/v2/*`, and
  `@google-cloud/functions-framework` imports must resolve unchanged through
  Neovex's chosen package or alias strategy. If a surface cannot preserve
  exact imports, it must be downgraded to documented rewrite-required
  compatibility instead of being marketed as zero-breaking-change.
- Add explicit `firebase-functions/v2` root-package behavior for covered
  surfaces: `setGlobalOptions()` must either inherit supported `GlobalOptions`
  fields into covered Firestore and HTTPS handlers or fail validation clearly
  for unsupported fields. If `onInit()` is not supported in the first slice,
  document and reject it explicitly.
- Add shared app-root discovery and codebase handling: auto-detect the nearest
  compatible Firebase project root or standalone Functions Framework package
  from the current directory or its parents, preserve existing
  `firebase.json` `functions.source` / `codebase` layouts plus conventional
  package-root entrypoints, keep user source in place, and emit generated
  artifacts under the Neovex-owned Firebase artifact root. `--app-dir`
  remains an explicit override for ambiguous or nonstandard repos.
- Add the covered `firebase-admin` compatibility surface used by existing
  modern Firebase function bodies. At minimum, settle the support contract for
  `firebase-admin/app` initialization and `firebase-admin/firestore`
  acquisition plus the Firestore APIs exercised by migration fixtures.

### T3: HTTP Handler Support

Location: shared server routing and the authoring packages from T2.

- Add HTTP handler dispatch for `functions.http(name, handler)` and
  `onRequest(handler)`. HTTP handlers share the runtime artifact and deploy
  contract from T0.4/T2 but use a different dispatch mechanism: route an
  incoming HTTP request to the registered handler function through V8, with
  Express-style `(req, res)` semantics. This is Neovex-hosted handler
  compatibility, not full replacement of the standalone Functions Framework
  local server/runtime contract.
- Add `onCall(handler)` support for the Firebase Callable protocol: JSON
  request/response envelope, auth context extraction, App Check contract,
  default CORS behavior, and `HttpsError`/FunctionsErrorCode mapping.
- Publish and enforce the field-level support matrix for `GlobalOptions`,
  `HttpsOptions`, and `CallableOptions`. Unsupported fields must fail
  validation clearly.
- HTTP handlers do not need trigger registration, durable delivery, or
  CloudEvent dispatch — they are synchronous request/response through the
  same V8 execution path.

### T4: Integration Tests And Documentation

- End-to-end tests: deploy/register trigger → write document → durable event
  recorded → trigger fires → trigger writes committed. Cover both Firebase
  and framework authoring surfaces.
- Failure and recovery tests: crash between commit and dispatch, replay after
  restart, retry behavior, poison-event handling, and chain depth limiting.
- HTTP handler tests: register → call → response, error handling, auth
  context for `onCall`.
- Edge cases: nested collection documents, no-op writes that should not emit
  update events, concurrent triggers, and loop-prevention guidance.
- Migration guide covering both audiences: Firebase Cloud Functions developers
  and standalone Cloud Functions developers. Document authoring/deploy
  differences, delivery semantics, CloudEvent compatibility, exact import
  strategy, supported `GlobalOptions`/`DocumentOptions`/`HttpsOptions`/
  `CallableOptions` matrices, the covered `firebase-admin` matrix, preserved
  project-layout expectations, standalone runtime non-goals, and known gaps.

## Context Window Budget

| Phase | Scope | Context windows |
|-------|-------|-----------------|
| T0 | Trigger contract, import/deploy, layout, and durable foundation | 6-8 |
| T1 | Durable dispatch and execution | 4-6 |
| T2 | Authoring surfaces, app discovery, admin SDK, and deploy | 8-11 |
| T3 | HTTP handler support and option matrices | 4-5 |
| T4 | Integration tests and docs | 3-4 |
| **Total** | | **26-35** |

## Risks

**R1: Durable delivery correctness (High, T0-T1).** A plain after-commit
in-memory enqueue would lose events across crashes and makes retries
non-deterministic. Mitigation: persist cursor or invocation state before
dispatch, replay from the durable mutation journal or equivalent persisted
ledger, and test crash windows explicitly.

**R2: Authoring/deploy contract drift (High, T0-T2).** The current runtime
artifact flow is Convex-specific, and standalone framework handlers need
deploy-time bindings beyond source registration. Trigger execution will
dead-end without a clear generalized runtime, import-resolution, and binding
contract. Mitigation: make the deploy/runtime decision plus target/binding
manifest a required T0 item before handler execution work begins.

**R3: Trigger loops and retry amplification (High, T1-T3).** Trigger A writes
document X → trigger B fires on X → repeated retries amplify the loop.
Mitigation: configurable depth limit, invocation ancestry metadata,
operator-visible terminal state, and migration guidance that emphasizes
idempotent handlers.

**R4: Trigger execution isolation (Medium, T1-T2).** Trigger functions must not
block the commit pipeline or degrade subscription delivery latency. Mitigation:
background dispatch with bounded concurrency, separate from the subscription
delivery path, plus focused latency and backpressure tests.

**R5: Service-principal versus auth-context semantics (Medium, T0-T2).**
Firebase base triggers run in a trusted environment, while auth-context
variants are separate APIs. Mitigation: codify service-principal execution in
T0, defer auth-context variants, and include CloudEvent identity in the event
model from day one.

**R6: Two authoring surfaces with one execution path (Medium, T2).** The
Firebase v2 surface and the functions-framework surface must produce the same
behavior for the same trigger. If they diverge, developers cannot reason about
which surface to use. Mitigation: both surfaces register into the same trigger
registry and consume the same CloudEvent model; authoring-surface-specific
logic is limited to handler registration and event object wrapping.

**R7: HTTP handler isolation from trigger path (Medium, T3).** HTTP handlers
are synchronous request/response, while document triggers are asynchronous
durable dispatch. Sharing the same V8 execution and deploy contract is correct,
but the dispatch paths must not entangle. Mitigation: HTTP handlers skip the
trigger registry and durable dispatch entirely; they route through the shared
HTTP server directly to V8 execution.

**R8: Compatibility promise overreach (Medium, T0-T4).** It is easy to say
"Functions Framework compatible" when only the source-level handler API is
compatible, or to say "Firebase app compatible" while silently dropping
option fields, root-level defaults, project-layout expectations, or the
`firebase-admin` imports used by real apps. Mitigation: keep explicit
compatibility matrices, preserve exact imports for covered surfaces, preserve
existing Firebase and Cloud Functions project structure under shared
auto-discovery plus `--app-dir` override semantics, fail validation on
unsupported options or admin methods, and defer full standalone runtime parity
until it has its own plan slice.

**R9: App-shape drift versus Convex-style ergonomics (Medium, T0-T2).** Neovex
already has a strong auto-discovery plus `--app-dir` override story for
Convex-compatible apps. If Firebase compatibility lands with a different
discovery model, requires explicit flags for common repos, or forces
source-root renames, teams will still face an avoidable migration tax.
Mitigation: codify cwd/parent auto-discovery, `firebase.json` project
discovery, standalone package-root discovery, `functions.source` /
`codebase` handling, and `.neovex/firebase/` internal artifact ownership as
first-class execution items before implementation starts.

## Dependencies

- **Hard dependency:** Firebase adapter plan F0.3 (atomic write batch),
  F0.2/F3.2 path metadata, and F3 phase completion for full resource-path
  parity.
- **Soft dependency:** Firebase adapter plan F4 client package work may share
  supporting code, but the server authoring surface in this plan must not be
  blocked on the Firestore client SDK package shape.
- **No hard dependency on:** Firebase adapter plan F0.6b
  (`diff_subscription_snapshots`) for correctness. That helper may still be
  reused for tests or secondary comparisons.
- **No dependency on:** Firebase adapter plan F5 (documentation) — compute docs
  would be part of this plan's T4 phase.

## Deferred Or Out Of Scope

- Firebase Realtime Database triggers.
- Firebase Auth event triggers.
- Firebase Storage event triggers.
- Pub/Sub triggers — requires event source outside Neovex's current scope.
- Scheduled functions (`onSchedule`) — natural follow-on using the existing
  engine scheduler primitive.
- Firebase Emulator Suite compatibility.
- `firebase deploy` / `gcloud functions deploy` CLI compatibility — Neovex
  provides its own deploy path; the function code is the portable contract.
- Multi-region trigger execution.
- Firebase Extensions framework.

## Phase Status Ledger

| Phase | Status | Context budget | Start condition | Done when |
|-------|--------|----------------|-----------------|-----------|
| T0: Trigger contract and durable foundation | `done` | 6-8 context windows | Firebase adapter F3 is `done` | Event source, durable delivery model, import/deploy/project-layout/admin contract, compatibility boundary, and trigger registry have focused tests or design-proof docs |
| T1: Durable dispatch and execution | `done` | 4-6 context windows | T0 is `done` | Committed writes produce durable trigger records and recovered replay can drive V8 execution through the shared mutation path |
| T2: Authoring surfaces and deploy integration | `done` | 8-11 context windows | T1 is `done`; relevant package/deploy scaffolding is chosen | Both `onDocumentCreated` (Firebase) and `functions.cloudEvent()` (framework) work against local Neovex through the shared trigger/deploy path with exact import compatibility for covered surfaces, shared auto-discovery plus `--app-dir` override behavior for common app layouts, preserved Firebase project structure, and the covered `firebase-admin` subset |
| T3: HTTP handler support | `done` | 4-5 context windows | T2 deploy contract is `done` | `functions.http()`, `onRequest()`, and `onCall()` work against local Neovex through the shared runtime/deploy path, and supported option fields are documented explicitly |
| T4: Integration tests and docs | `done` | 3-4 context windows | T2 and T3 are `done` | End-to-end tests pass for both surfaces, migration guides cover both Firebase and standalone Cloud Functions audiences |

## Roadmap Items

### T0 Work Queue: Trigger Contract And Durable Foundation

| Item | Status | Hard deps | Completion gate | Verification evidence |
|------|--------|-----------|-----------------|-----------------------|
| T0.1 Document-path trigger semantics and pattern model | `done` | none | Trigger patterns are defined over Firestore document paths, reject collection-only shapes, and capture wildcard params deterministically. | Focused `neovex-core` tests for exact match, nested wildcards, invalid collection-terminal patterns, and path param extraction. |
| T0.2 CloudEvent envelope, Firestore event types, and service-principal contract | `done` | T0.1 | `TriggerEvent` carries CloudEvent identity using standard `google.cloud.firestore.document.v1.*` type strings, `specversion: \"1.0\"`, `DocumentEventData` payload (`value`, `oldValue`, `updateMask`), Firestore metadata, captured params, and explicitly models service-principal execution. The event model is consumed identically by both authoring surfaces. | Type tests, CloudEvent type string verification, `specversion` verification, `DocumentEventData` serialization roundtrips, and service-principal contract tests. |
| T0.3 Durable cursor / invocation ledger model | `done` | T0.2 | The plan chooses and documents the persisted trigger cursor or invocation ledger, retry state, and completion semantics. | Design note plus focused persistence/model tests. |
| T0.4 Generalized runtime artifact, import-resolution, and deploy contract | `done` | T0.2 | The plan decides whether Neovex generalizes the current deploy/admin/runtime registry path or adds a sibling path, defines the manifest/bundle contract for trigger handlers, and settles exact import resolution for `firebase-functions/v2`, `firebase-functions/v2/*`, `@google-cloud/functions-framework`, and the covered `firebase-admin` imports. | Design proof, deploy contract doc update, import-resolution proof, and validation tests for the chosen artifact shape. |
| T0.5 Deploy-time target and binding contract | `done` | T0.4 | Source-discovered framework targets and Firebase registrations lower through a documented binding contract that captures signature type, Firestore event type, database, document path pattern, HTTP exposure, and service-principal semantics. Unsupported option fields and unsupported runtime-parity claims are documented explicitly. | Design proof plus validation tests for valid/invalid target bindings and unsupported-option rejection. |
| T0.6 Firebase root-package defaults contract | `done` | T0.4 | The plan explicitly decides how `firebase-functions/v2` root imports and `setGlobalOptions()` behave for covered Firestore and HTTPS handlers, including inheritance order, supported `GlobalOptions` fields, and clear rejection of unsupported root-level APIs such as `onInit()` if deferred. | Design proof plus validation tests for inherited defaults, unsupported global-option rejection, and deferred-root-API rejection. |
| T0.7 Trigger registry | `done` | T0.2 | Thread-safe, tenant-scoped registry with register/deregister/enable/disable/list and pattern-based lookup. | Concurrent registration tests, lookup tests, tenant isolation tests. |
| T0.8 App-root discovery, project-layout, and server-SDK contract | `done` | T0.4, T0.6 | The plan explicitly decides the shared app-root resolver contract: explicit `--app-dir` override, cwd/parent auto-discovery for existing Firebase project roots and common standalone Functions Framework package roots, `firebase.json` `functions.source` / `codebase` layouts, the `.neovex/firebase/` artifact root, and which `firebase-admin/app` and `firebase-admin/firestore` imports or methods are covered in the first slice. | Design proof plus validation tests for Firebase and standalone project discovery, ambiguity handling, codebase mapping, internal artifact layout, covered admin-import resolution, and unsupported-admin-method rejection. |

### T1 Work Queue: Durable Dispatch And Execution

| Item | Status | Hard deps | Completion gate | Verification evidence |
|------|--------|-----------|-----------------|-----------------------|
| T1.1a Trigger commit candidate derivation and durable cursor contract | `done` | T0.3, T0.7 | The engine has a shared builder that lowers committed writes plus resource-path metadata into deterministic trigger commit candidates, and every storage provider persists a `TriggerDeliveryCursor` for later replay/materialization progress. This slice does not yet wire the live background feed or advance the cursor after matching. | Focused engine/storage tests for insert/update/delete candidate derivation, resource-path lookup, deterministic candidate ids, and cross-provider trigger-delivery cursor roundtrips. |
| T1.1b Live candidate feed and replay bootstrap | `done` | T1.1a | Successful commits flow through the shared candidate-emission seam after commit or provider catch-up, and restart/bootstrap can rebuild the pending candidate stream from the durable journal plus persisted cursor without blocking commit completion. | Engine tests proving post-commit durability, crash-window replay/bootstrap behavior, and no commit-path latency regression beyond the documented boundary. |
| T1.2 Matching and invocation persistence | `done` | T1.1b | Committed events are matched against registered triggers and durable invocation records are stored with correct pattern resolution and parameter capture. | Dispatch tests for exact match, wildcard match, no match, and multiple matches. |
| T1.3a Engine-owned trigger invocation execution seam | `done` | T1.2 | The engine claims pending trigger invocation records, drives durable running/completed state transitions, and exposes a protocol-neutral execution seam so runtime execution stays pluggable instead of being baked into one adapter. | Engine tests for pending-to-running claim, successful completion, terminal failure, duplicate-claim rejection, and durable transition persistence across providers. |
| T1.3b Cloud Functions runtime registry and V8 trigger execution | `done` | T0.4, T1.3a | The Cloud Functions server/runtime surface loads the sibling artifact family, resolves handler targets from the manifest, and executes trigger JavaScript through the shared runtime seam with database access and atomic writes. | Execution tests for successful trigger, database read/write from trigger, invalid artifact rejection, and runtime handler lookup. |
| T1.4a Retryable failure classification and durable retry replay | `done` | T1.3b | Retryable failures are classified through the engine-owned execution seam, persisted as durable retry state, replayed after delay or restart, and promoted to terminal failure after bounded attempts. | Tests for retry scheduling, retry-to-completion, startup replay of due retries, and max-attempt terminal failure. |
| T1.4b Trigger ancestry metadata and chain-depth limiting | `done` | T1.4a | Trigger invocation ancestry/depth is materialized from parent-trigger writes, persisted with invocation records, and enforced so recursive trigger chains fail terminally once the configured depth budget is exceeded. | Tests for root trigger depth, nested chain depth N, and exceeded-depth terminal behavior. |
| T2.1 Trigger bundle build/deploy integration | `done` | T0.4, T0.5, T0.6, T0.8, T1.3 | Trigger functions can be built, uploaded, validated, and activated through the chosen generalized artifact path. Both authoring surfaces use the same deploy contract, exact import-resolution strategy, target/binding manifest, documented root-package defaults behavior for covered Firebase surfaces, and shared auto-discovery plus `--app-dir` override behavior for common app layouts. | Deployment tests for valid bundle, invalid bundle, manifest parsing, binding validation, inherited global-default behavior, auto-discovered and explicit app-root handling, integrity failure, and generation activation. |

### T2 Work Queue: Authoring Surfaces And Deploy Integration

| Item | Status | Hard deps | Completion gate | Verification evidence |
|------|--------|-----------|-----------------|-----------------------|
| T2.1 Trigger bundle build/deploy integration | `done` | T0.4, T0.5, T0.6, T0.8, T1.3 | Trigger functions can be built, uploaded, validated, and activated through the chosen generalized artifact path. Both authoring surfaces use the same deploy contract, exact import-resolution strategy, target/binding manifest, documented root-package defaults behavior for covered Firebase surfaces, and shared auto-discovery plus `--app-dir` override behavior for common app layouts. | Deployment tests for valid bundle, invalid bundle, manifest parsing, binding validation, inherited global-default behavior, auto-discovered and explicit app-root handling, integrity failure, and generation activation. |
| T2.2 Firebase trigger adapter and FirestoreEvent mapping | `done` | T2.1 | Firebase adapter maps `onDocumentCreated` etc. to the shared trigger registry and constructs Firebase-shaped `FirestoreEvent` / `Change<DocumentSnapshot>` objects with CloudEvent identity, `DocumentEventData` payload, and Firestore metadata. | Adapter tests for all four trigger types with correct before/after/params/project/database/document/id/time shape plus CloudEvent type string verification. |
| T2.3 Functions-framework CloudEvent handler adapter | `done` | T2.1 | Framework adapter discovers `functions.cloudEvent(name, handler)` targets, validates deploy-time bindings for those targets, lowers them into the shared trigger registry, and passes the standard CloudEvent object (with `google.cloud.firestore.document.v1.*` type and `DocumentEventData` payload) directly to handlers. | Adapter tests for target discovery, binding validation, event delivery with correct type/source/subject/data, and parity with T2.2 trigger behavior. |
| T2.4 Firebase import and authoring compatibility surface | `done` | T2.2 | Existing `firebase-functions/v2`, `firebase-functions/v2/firestore`, and `firebase-functions/v2/https` imports resolve without source edits through Neovex's chosen package/alias strategy, and covered APIs plus inherited `setGlobalOptions()` behavior match the published support matrix. | Package or resolver tests, typecheck, build, export-map verification, inherited-default tests, and local server smoke tests. |
| T2.5 Functions-framework import and authoring compatibility surface | `done` | T2.3 | Existing `@google-cloud/functions-framework` imports resolve without source edits through Neovex's chosen package/alias strategy. `functions.cloudEvent()` and `functions.http()` register named targets with the documented binding contract. HTTP dispatch lands in T3. | Package or resolver tests, typecheck, build, CloudEvent trigger smoke tests, and named-target discovery tests. |
| T2.6 Shared app-root discovery and codebase handling | `done` | T2.1 | The default path auto-detects the nearest compatible Firebase project root or standalone Functions Framework package from the current directory or its parents, while `--app-dir` remains an explicit override. Firebase layouts preserve `firebase.json` `functions.source` / `codebase` structure, and generated artifacts land under `.neovex/firebase/` without requiring source-tree renames. | Discovery tests for cwd-root Firebase apps, nested-child cwd Firebase discovery, default `functions/`, single-source `firebase.json`, multi-codebase monorepo arrays, standalone framework package discovery via `package.json`, ambiguity handling, explicit `--app-dir` override, and `.neovex/firebase/` artifact ownership. |
| T2.7a firebase-admin app lifecycle and Firestore handle acquisition | `done` | T2.1 | Covered `firebase-admin/app` imports and the first `firebase-admin/firestore getFirestore()` handle path resolve without source edits, preserve app-name/default-app semantics, and expose a documented handle contract instead of a fake package stub. Unsupported admin modules and methods still fail clearly. | Package or resolver tests, smoke tests for `initializeApp()` / `getApp()` / `getApps()` / `deleteApp()`, `getFirestore()` handle acquisition tests, and unsupported-admin-method rejection tests. |
| T2.7b Covered Firestore admin operations | `done` | T2.7a | The documented subset of Firestore admin reads/writes used by migration fixtures routes through the shared Firebase adapter or engine-owned data paths with explicit unsupported-method rejection outside the covered subset. | Focused smoke tests for the covered Firestore admin calls plus unsupported-operation rejection tests. |

### T3 Work Queue: HTTP Handler Support

| Item | Status | Hard deps | Completion gate | Verification evidence |
|------|--------|-----------|-----------------|-----------------------|
| T3.1 HTTP handler dispatch | `done` | T2.1, T2.5 | `functions.http(name, handler)` routes incoming HTTP requests to the registered handler through V8 with Express-style `(req, res)` semantics under Neovex-hosted routing. Full standalone Functions Framework local server parity remains explicitly deferred. | Handler tests for request routing, response writing, error handling, concurrent requests, unsupported runtime-parity features failing clearly, and generated bundle selftests for req/res materialization. |
| T3.2a Firebase `onRequest` base overload and path contract | `done` | T3.1, T2.4 | `onRequest(handler)` and the base `onRequest(opts, handler)` overload now lower onto the shared HTTP dispatch path from `T3.1`, with first-slice public paths derived from exported function names as `/<exportName>`. The first covered matrix keeps HTTPS root-default inheritance at `none` and accepts no explicit `HttpsOptions` fields yet; unsupported fields fail fast instead of being ignored. | Handler tests for `onRequest` parity with `functions.http()`, generated-target path coverage, root-default non-inheritance, and explicit unsupported-option rejection. |
| T3.2b Firebase `onCall` protocol and HTTPS matrices | `done` | T3.2a, T2.4, T2.7b | `onCall(handler)` adds the Firebase Callable protocol with JSON envelope, auth context extraction, App Check contract, default CORS behavior, and `HttpsError`/FunctionsErrorCode mapping. `onCall(opts, handler)` only supports fields listed in the published matrix, including inherited root-level defaults where covered. | Protocol tests for auth context, App Check, error codes, CORS defaults, JSON envelope, covered `firebase-admin` usage inside handler bodies, and unsupported-option rejection. |

### T4 Work Queue: Integration Tests And Documentation

| Item | Status | Hard deps | Completion gate | Verification evidence |
|------|--------|-----------|-----------------|-----------------------|
| T4.1a Common-layout trigger lifecycle tests | `done` | T2.2, T2.3, T2.7b | Full trigger lifecycle is tested through both authoring surfaces under active Cloud Functions registries: committed Firestore writes publish durable trigger candidates, matching registrations materialize invocation records, the background worker executes the generated bundle, and the handler's follow-up writes commit through the shared engine path. Covered `firebase-admin` Firestore operations are exercised inside both Firebase and standalone handler bodies. | Integration tests on the default provider for both Firebase and framework surfaces. |
| T4.1b App-root discovery and explicit override lifecycle variants | `done` | T4.1a, T2.6 | The same end-to-end trigger lifecycle lane also covers the app-discovery contract: common Firebase and standalone package layouts work from auto-detected roots, and ambiguous layouts still succeed when the explicit app override is provided. | Integration tests or start/deploy smoke covering nested-cwd discovery plus explicit override/ambiguity resolution. |
| T4.2 Failure, recovery, and loop-safety tests | `done` | T4.1b | Crash/restart replay, retry behavior, no-op write suppression, concurrent triggers, and chain depth limits are tested. | Targeted reliability tests. |
| T4.3 HTTP handler end-to-end tests | `done` | T3.2b | HTTP handlers work end-to-end through both authoring surfaces with deploy, routing, auth, and error behavior. | Integration tests for `functions.http()`, `onRequest()`, and `onCall()`. |
| T4.4 Migration guides and compatibility matrix | `done` | T4.1b, T4.3 | Documentation covers both audiences: Firebase Cloud Functions developers and standalone Cloud Functions developers. Includes default auto-discovery behavior, when to use `--app-dir` as an override, exact import strategy, preserved Firebase project-layout expectations, delivery semantics, CloudEvent compatibility, supported `firebase-admin` / `GlobalOptions` / `DocumentOptions` / `HttpsOptions` / `CallableOptions` matrices, standalone runtime non-goals, HTTP handler contract, and known gaps. | Docs review for both migration paths. |

## Execution Log

| Date | Item | Status | Notes | Verification |
|------|------|--------|-------|--------------|
| 2026-04-25 | T4.4 Migration guides and compatibility matrix | `done` | Published the Cloud Functions closeout docs: `docs/reference/cloud-functions-compatibility.md` is now the public support matrix for Firebase v2 plus standalone Functions Framework surfaces, and `docs/reference/cloud-functions-migration-guide.md` is the practical adoption path covering auto-discovery, `--app-dir`, `.neovex/firebase/`, delivery semantics, HTTP path rules, covered `firebase-admin` usage, and current non-goals. Added both documents to `docs/README.md`. With the migration and compatibility docs in place, the Cloud Functions control plan is complete. | Manual docs review via `sed -n '1,260p' docs/reference/cloud-functions-{compatibility,migration-guide}.md`; link/index review via `rg -n "cloud-functions-(compatibility|migration-guide)" docs/README.md docs/plans/firebase-cloud-functions-plan.md docs/plans/README.md`. |
| 2026-04-25 | T4.4 Migration guides and compatibility matrix | `in_progress` | Closed `T4.3` after adding live generated-bundle HTTP smoke for both authoring surfaces instead of relying only on manual artifact/runtime tests. Standalone `functions.http()` now runs end to end through codegen, deploy-style artifact loading, routing, and response rendering; Firebase `onRequest()` does the same through its generated `/<exportName>` path contract; and Firebase `onCall()` now proves the generated callable envelope plus `HttpsError` mapping against the live server path. The remaining work is the public closeout: document the migration story, compatibility matrix, auto-discovery behavior, exact covered option/admin surfaces, and known non-goals/gaps for both Firebase and standalone Cloud Functions developers. | `cargo test -p neovex-server cloud_functions --lib`; `cargo fmt --all`; `cargo fmt --all --check`. |
| 2026-04-25 | T4.3 HTTP handler end-to-end tests | `done` | Added the missing live server coverage on top of the earlier HTTP unit tests: generated standalone `functions.http()`, Firebase `onRequest()`, and Firebase `onCall()` bundles now execute end to end through Neovex routing, the Cloud Functions registry, and the shared runtime path, with callable success/error envelopes verified on the real HTTP surface. | `cargo test -p neovex-server cloud_functions --lib`; `cargo fmt --all`; `cargo fmt --all --check`. |
| 2026-04-25 | T4.2 Failure, recovery, and loop-safety tests | `done` | Added the missing reliability coverage on top of the earlier trigger lifecycle lanes: generated Firebase bundles now prove that no-op overwrites do not emit `onDocumentUpdated()` events, concurrent committed writes both execute their handlers, and recursive write-back chains stop at the configured trigger-depth budget while still committing the last allowed child document. | `cargo test -p neovex-server cloud_functions --lib`; `cargo fmt --all`; `cargo fmt --all --check`. |
| 2026-04-25 | T4.1b App-root discovery and explicit override lifecycle variants | `done` | Added the missing lifecycle-side discovery coverage on top of the common trigger smoke. Start-side auto-detection now uses the same app-root resolver as deploy, nested Firebase layouts auto-resolve from child directories, generated Cloud Functions registries load from those auto-detected roots, and explicit `--app-dir` overrides can target alternate standalone framework packages without being filtered out by the auto-discovery heuristics. | `cargo test -p neovex-bin start::tests`; `cargo fmt --all`; `cargo fmt --all --check`. |
| 2026-04-25 | T4.1a Common-layout trigger lifecycle tests | `done` | Added the first true lifecycle integration coverage for Cloud Functions-compatible Firestore triggers. The server now installs Cloud Functions trigger registrations alongside the trigger executor, so deployed trigger targets participate in durable candidate matching rather than only supporting direct executor smoke tests. New generated-bundle integration tests prove the common-path lifecycle for both Firebase `onDocumentWritten()` and standalone `functions.cloudEvent()` handlers, including covered `firebase-admin/firestore` reads and follow-up writes. | `cargo test -p neovex-server cloud_functions --lib`; `cargo fmt --all`. |
| 2026-04-25 | T3.2b Firebase `onCall` protocol and HTTPS matrices | `done` | Landed the first callable HTTP slice across both the generated Firebase compatibility package and the Neovex-hosted HTTP runtime. `onCall(handler)` plus `onCall({}, handler)` now derive `/<exportName>` bindings, enforce the Firebase JSON request/response envelope, map `HttpsError` codes into callable error payloads, surface request auth when shared application auth is configured, preserve loopback-safe default CORS behavior, and fail fast for explicit `CallableOptions`, App Check verification, and streaming semantics outside the covered base slice. | `cargo fmt --all`; `cargo test -p neovex-server cloud_functions --lib`; `cargo fmt --all --check`; `npm run test --workspace @neovex/codegen`; `npm run typecheck --workspace @neovex/codegen`. |
| 2026-04-25 | T4.1 End-to-end trigger lifecycle tests | `in_progress` | Closed `T3.2b` after landing the first callable HTTP slice across the generated Firebase package surface and the Neovex-hosted HTTP runtime: `onCall(handler)` plus `onCall({}, handler)` now derive `/<exportName>` bindings, enforce the Firebase JSON request/response envelope, map `HttpsError` codes into callable error payloads, surface request auth when shared application auth is configured, preserve loopback-safe default CORS behavior, and fail fast for explicit `CallableOptions`, App Check verification, and streaming semantics outside the covered base slice. That broader row has since been split into `T4.1a` and `T4.1b` so common trigger lifecycle coverage and discovery/override lifecycle variants can advance independently. | `cargo fmt --all`; `cargo test -p neovex-server cloud_functions --lib`; `cargo fmt --all --check`; `npm run test --workspace @neovex/codegen`; `npm run typecheck --workspace @neovex/codegen`. |
| 2026-04-25 | T3.2b Firebase `onCall` protocol and HTTPS matrices | `in_progress` | Closed `T3.2a` after wiring the Firebase `onRequest()` surface onto the same Neovex-hosted HTTP execution path as `functions.http()`. A quick boundary review showed `T3.2b` is a real protocol slice, not a thin registration follow-up: `onCall()` still needs the Firebase callable JSON envelope, auth-context extraction, App Check semantics, default CORS behavior, and `HttpsError` / `FunctionsErrorCode` mapping before any option matrix can be claimed. | Source review of `packages/codegen/src/cloud_functions.mjs`, `docs/reference/cloud-functions-root-defaults-contract.md`, and the `T3` callable requirements in this plan. |
| 2026-04-25 | T3.2a Firebase `onRequest` base overload and path contract | `done` | Landed the Firebase HTTP registration layer on top of the shared `T3.1` request/response path instead of creating a Firebase-only runtime shim. `firebase-functions/v2/https onRequest(handler)` and `onRequest({}, handler)` now build and execute through the same Express-style `req` / `res` session used by `functions.http()`, Firebase-exported handlers derive exact public paths as `/<exportName>`, HTTPS root defaults remain intentionally empty, and every explicit `HttpsOptions` field still fails fast until a later slice owns the underlying behavior. | `npm run test --workspace @neovex/codegen`; `npm run typecheck --workspace @neovex/codegen`; `cargo test -p neovex-server cloud_functions_http_handler --lib`. |
| 2026-04-25 | T3.1 HTTP handler dispatch | `done` | Closed the first Neovex-hosted HTTP execution slice for Cloud Functions-compatible handlers. `functions.http(name, handler)` now routes exact public paths through router fallback into the shared V8 invocation path, generated bundles receive Express-style request/response objects, response envelopes round-trip back into Axum responses, and handler writes commit through the standard mutation execution path. Reserved public routes are rejected up front, and multi-tenant HTTP routing still fails clearly until an explicit tenant-binding contract lands. | `cargo test -p neovex-server cloud_functions_http_handler --lib`; `cargo test -p neovex-server cloud_functions --lib`; `npm run test --workspace @neovex/codegen`; `cargo fmt --all`; `cargo fmt --all --check`. |
| 2026-04-25 | T3.1 HTTP handler dispatch | `in_progress` | Started the post-T2 HTTP slice immediately after closing the covered admin Firestore work. Source review shows the right next seam is not a second runtime artifact path: the manifest already models request-scoped `https` targets, the registry already loads those targets, and the runtime invocation bridge already has a shared V8 entrypoint path. The remaining contract to settle is the Neovex-hosted HTTP route prefix plus the first request/response envelope for `functions.http(name, handler)` before layering Firebase `onRequest()` and callable protocol semantics on top. | Source review of `crates/neovex-server/src/router.rs`, `crates/neovex-server/src/adapters/cloud_functions/{mod,registry,execution}.rs`, `crates/neovex-server/src/adapters/convex/http_actions/*`, and `packages/codegen/src/cloud_functions.mjs`. |
| 2026-04-25 | T2.7b Covered Firestore admin operations | `done` | Closed the first real `firebase-admin/firestore` runtime slice instead of keeping `getFirestore()` as a hollow handle. Generated Cloud Functions bundles now cover explicit document refs plus `get()`, `set()`, `update()`, `delete()`, nested `collection(path)`, and `DocumentSnapshot.data()/get(fieldPath)` over the shared Firebase document-path and bound-write primitives, while deferred overloads such as auto-id `collection().doc()`, merge writes, and delete options still fail fast. The engine now has a non-finalizing `stage_atomic_write_batch()` seam so multiple admin Firestore writes can succeed inside one handler invocation without prematurely finalizing the live mutation execution unit. | `npm run test --workspace @neovex/codegen`; `cargo test -p neovex-engine staged_atomic_write_batch_keeps_execution_unit_reusable_until_commit --lib`; `cargo test -p neovex-server cloud_functions --lib`; `cargo fmt --all`; `cargo fmt --all --check`. |
| 2026-04-25 | T2.7b Covered Firestore admin operations | `in_progress` | Advanced to the actual Firestore admin-runtime gap after closing app lifecycle and `getFirestore()` handle acquisition. The current alias layer and shim now prove `firebase-admin/app` semantics plus `getFirestore()` creation, but `collection()` / `doc()` and any real admin reads or writes still fail fast by design, so the next step is to choose the first covered Firestore operation subset and thread it through the shared Firebase adapter or engine seams instead of extending the placeholder handle ad hoc. | Source review of `packages/codegen/src/cloud_functions.mjs`, `crates/neovex-server/src/adapters/firebase/mod.rs`, and the active Cloud Functions execution tests. |
| 2026-04-25 | T2.7a firebase-admin app lifecycle and Firestore handle acquisition | `done` | Closed the first `firebase-admin` slice without inflating it into a full Firestore admin-runtime wave. The deploy alias layer now covers `firebase-admin/app` plus `firebase-admin/firestore getFirestore()` end to end, app/default-app lifecycle semantics are exercised through generated bundles, and deferred Firestore handle methods still reject clearly until the next slice lands. | `npm run test --workspace @neovex/codegen`. |
| 2026-04-25 | T2.6 Shared app-root discovery and codebase handling | `done` | Closed the shared app-root and codebase handling row based on the same generalized artifact pass that landed `T2.1`. Firebase `firebase.json` roots, nested-child cwd discovery, multi-codebase source mapping, standalone Functions Framework package roots, ambiguity handling, explicit `--app-dir` override behavior, and `.neovex/firebase/` artifact ownership are now covered by the CLI/server contract and exercised by start/deploy/discovery tests instead of living as an undocumented heuristic. | `cargo test -p neovex-bin cloud_functions`; `cargo test -p neovex-server cloud_functions`; `cargo fmt --all --check`. |
| 2026-04-25 | T2.4 Firebase import and authoring compatibility surface | `done` | Closed the Firebase import-surface row on top of the deploy alias layer chosen in `T2.1`. Covered `firebase-functions/v2` root imports plus `firebase-functions/v2/firestore` document triggers now build and execute through generated bundles without source edits, inherited `setGlobalOptions({ retry })` stays intact, deferred `onInit()`, `onRequest()`, and `onCall()` fail immediately with explicit phase-boundary errors, and the new server-side smoke proves a generated Firebase-import bundle survives all the way into Cloud Functions trigger execution rather than only passing bundle-local tests. | `npm run test --workspace @neovex/codegen`; `cargo test -p neovex-server cloud_functions`; `cargo fmt --all`; `cargo fmt --all --check`. |
| 2026-04-25 | T2.5 Functions-framework import and authoring compatibility surface | `done` | Closed the standalone Functions Framework import surface on the same deploy alias layer instead of introducing a separate package strategy. Existing `@google-cloud/functions-framework` imports now build without source edits, `functions.cloudEvent()` and `functions.http()` register named targets through the documented `targets.json` binding contract, and CloudEvent handlers receive the standard event object under trigger execution while HTTP request dispatch remains explicitly deferred to T3. | `npm run test --workspace @neovex/codegen`. |
| 2026-04-25 | T2.3 Functions-framework CloudEvent handler adapter | `done` | Closed the framework CloudEvent slice on top of the same generated bundle path from `T2.1` instead of introducing a second runtime contract. `functions.cloudEvent(name, handler)` targets now keep their deploy-time binding manifest, lower through the shared target registry, and receive a standard CloudEvent object with `id`, `source`, `specversion`, `subject`, `type`, RFC3339 `time`, and raw `DocumentEventData` under `data` rather than the internal trigger envelope. | `npm run test --workspace @neovex/codegen`. |
| 2026-04-25 | T2.2 Firebase trigger adapter and FirestoreEvent mapping | `done` | Closed the Firebase trigger authoring slice in the shared Cloud Functions codegen/runtime shim rather than teaching the engine about Firebase event classes. Covered `onDocumentCreated`, `onDocumentDeleted`, `onDocumentUpdated`, and `onDocumentWritten` now materialize Firebase-shaped `FirestoreEvent` values with CloudEvent identity, `params`, project/database/document metadata, and the correct snapshot/change payloads for all four trigger types. | `npm run test --workspace @neovex/codegen`. |
| 2026-04-25 | T2.1 Trigger bundle build/deploy integration | `done` | Closed the generalized trigger bundle path by extending the existing deploy activation seam to a sibling Cloud Functions artifact family instead of forcing Cloud Functions through Convex-only manifests. `neovex codegen`, `neovex start`, and `neovex deploy` now detect Firebase projects plus standalone `@google-cloud/functions-framework` package roots, build `.neovex/firebase/{artifact.json,targets.json,bundle.mjs,bundle.sha256}`, preserve explicit framework target bindings, and activate Cloud Functions artifacts through the shared generation loader. | `cargo test -p neovex-bin cloud_functions`; `npm run test --workspace @neovex/codegen`; `cargo fmt --all --check`. |
| 2026-04-25 | T2.1 Trigger bundle build/deploy integration | `in_progress` | Closed `T1.4b` after teaching the trigger path to carry parent invocation identity and depth on committed writes, persist ancestry with durable invocation records, and fail recursive chains terminally once the configured depth budget is exceeded. The next slice moves to the broader authoring/deploy boundary: inspect the current Convex-shaped build and activation path together with the Cloud Functions artifact contract, then decide whether trigger bundle build/deploy integration can land as one generalized deployment pass or should split into artifact activation versus package-build/import-resolution work before code. | `cargo fmt --all`; `cargo test -p neovex-core trigger --lib`; `cargo test -p neovex-engine triggers::materialize --lib`; `cargo test -p neovex-engine mutation_execution_unit_persists_trigger_write_origin_on_committed_writes --lib`; `cargo test -p neovex-engine mutation_journal::triggers --lib`; `cargo check -p neovex-core -p neovex-engine -p neovex-storage -p neovex-server`; `cargo fmt --all --check`; `make clippy` (still fails only on pre-existing Firebase gRPC lint debt in `crates/neovex-server/src/adapters/firebase/grpc/{listen_stream,listen_websocket,unary,write_stream,mod}.rs`). |
| 2026-04-25 | T1.4b Trigger ancestry metadata and chain-depth limiting | `done` | Landed trigger ancestry and loop-safety in the shared mutation/materialization path instead of inferring recursion from runtime-local state. Trigger-scoped writes now carry parent invocation identity and depth through committed `WriteOp`s, durable invocation records persist ancestry metadata, materialization computes child depth deterministically, and over-budget recursive chains fail terminally before execution. The runtime host bridge now stamps trigger-origin metadata onto writes so recursive trigger chains stay visible to the shared engine rather than hidden inside the Cloud Functions adapter. | `cargo fmt --all`; `cargo test -p neovex-core trigger --lib`; `cargo test -p neovex-engine triggers::materialize --lib`; `cargo test -p neovex-engine mutation_execution_unit_persists_trigger_write_origin_on_committed_writes --lib`; `cargo test -p neovex-engine mutation_journal::triggers --lib`; `cargo check -p neovex-core -p neovex-engine -p neovex-storage -p neovex-server`; `cargo fmt --all --check`; `make clippy` (still fails only on pre-existing Firebase gRPC lint debt in `crates/neovex-server/src/adapters/firebase/grpc/{listen_stream,listen_websocket,unary,write_stream,mod}.rs`). |
| 2026-04-25 | T1.1b Live candidate feed and replay bootstrap | `in_progress` | Started the live-feed/bootstrap pass immediately after closing `T1.1a`. Source review shows the right ownership split is not to hide trigger replay inside adapter code or synchronous commit processing: the engine already has one post-commit seam for direct writes (`process_commit`) and one for applied journal/provider catch-up batches (`process_applied_commit_batch` / `catch_up_loaded_provider_tenant_async`). The working direction for this item is therefore a tenant-owned trigger-candidate worker fed from committed batches plus a separate bootstrap path that replays `read_commit_log_from(cursor + 1)` on tenant load, with startup catch-up kept distinct so the same commits are not double-enqueued before the cursor ever advances. | Source review of `crates/neovex-engine/src/service/mutations/{commit_processing,journal}.rs`, `crates/neovex-engine/src/service/{tenants,provider_hints}.rs`, `crates/neovex-engine/src/tenant/{subscription_delivery.rs,subscription_delivery/worker.rs}`, and `crates/neovex-engine/src/persistence/tenant/{provider_state,journal,trigger_delivery}.rs`. |
| 2026-04-25 | T1.1b Live candidate feed and replay bootstrap | `done` | Landed the tenant-owned trigger-candidate feed instead of extending synchronous commit work or adapter-local replay logic. Committed batches now enqueue `TriggerCommitCandidate` derivation after journal apply, provider catch-up paths can emit the same candidate stream, and tenant startup bootstraps pending candidates by replaying the durable journal from `TriggerDeliveryCursor + 1` without blocking mutation completion. I stopped before `T1.2` because the next slice is the separate durable-match boundary: registry-driven trigger matching and persisted invocation records need to be designed together so cursor advancement is tied to real invocation materialization instead of transient in-memory queue drain. Next action: inspect the trigger registry, candidate feed, and existing durable state seams, then add matching plus persisted invocation records with wildcard parameter capture before any runtime execution work. | `cargo fmt --all`; `cargo test -p neovex-engine mutation_journal::triggers --lib`; `cargo test -p neovex-engine triggers::dispatch --lib`; `cargo check -p neovex-core -p neovex-engine -p neovex-storage`; `cargo fmt --all --check`. |
| 2026-04-25 | T1.2 Matching and invocation persistence | `in_progress` | Started the durable matching/materialization pass immediately after closing `T1.1b`. The working direction for this item is to keep trigger pattern resolution in the shared engine registry, match durable `TriggerCommitCandidate`s against the per-tenant registry off the new candidate feed, and persist `TriggerInvocationRecord`s before advancing `TriggerDeliveryCursor`, so replay recovery is driven by stored invocation state rather than transient in-memory queue drain. | Source review of `crates/neovex-engine/src/{triggers.rs,triggers/registry.rs,tenant/trigger_candidates.rs}`, `crates/neovex-core/src/trigger.rs`, and existing persistence/metadata seams under `crates/neovex-engine/src/persistence/tenant/` plus `crates/neovex-storage/src/{store,sqlite,postgres,mysql,libsql}`. |
| 2026-04-25 | T1.2 Matching and invocation persistence | `done` | Landed the durable trigger-materialization step in the shared engine instead of deferring match state into later adapter/runtime code. Tenant runtimes now carry tenant identity plus a registry-readiness-gated trigger registry, committed trigger candidates materialize into deterministic `TriggerInvocationRecord`s with exact/wildcard parameter capture and service-principal CloudEvent metadata, and every storage provider persists those invocation records atomically with `TriggerDeliveryCursor` advancement. I stopped before `T1.3` because the next slice is the broader runtime-execution boundary: pending invocation claim/transition, runtime artifact lookup, and host-bridge execution need to land together so trigger JavaScript still reuses the shared mutation path. Next action: inspect the generalized artifact contract from `T0.4`, the current runtime/host-bridge seams, and the new invocation ledger to choose the narrowest execution worker that can claim pending invocations and run them through the shared runtime path. | `cargo fmt --all`; `cargo test -p neovex-engine mutation_journal::triggers --lib`; `cargo check -p neovex-core -p neovex-engine -p neovex-storage`; `cargo fmt --all --check`. |
| 2026-04-25 | T1.3a Engine-owned trigger invocation execution seam | `in_progress` | Split the old `T1.3` runtime-execution row before code. Source review showed two separate implementation risks: the engine still needs its own durable claiming/state-transition worker and a protocol-neutral execution seam, while the actual V8 bundle loading and host-bridge machinery still lives in `neovex-server` beside the sibling Cloud Functions artifact contract from `T0.4`. The working direction for `T1.3a` is therefore to land engine-owned invocation claiming plus a pluggable executor interface first, leaving Cloud Functions runtime artifact lookup and V8 host-bridge execution for `T1.3b`. | Source review of `crates/neovex-engine/src/{tenant.rs,triggers.rs,tenant/trigger_candidates.rs,persistence/tenant/trigger_invocations.rs}`, `crates/neovex-server/src/adapters/{cloud_functions,convex}/`, `crates/neovex-server/src/execution/invocations/`, and `docs/reference/cloud-functions-artifact-contract.md`. |
| 2026-04-25 | T1.3a Engine-owned trigger invocation execution seam | `done` | Landed the engine-owned durable trigger execution seam instead of tying invocation state transitions directly to a Cloud Functions-specific runtime path. Tenant runtimes now own a dedicated trigger execution worker, committed trigger materialization enqueues durable invocation keys, every storage provider can fetch and upsert individual invocation records, and the service can install a protocol-neutral `TriggerInvocationExecutor` that drives persisted `Pending -> Running -> Completed` or terminal-failure transitions while replay bootstrap drains already-persisted pending records after restart. I stopped before `T1.3b` because the remaining work is the separate server/runtime integration boundary: Cloud Functions still needs its own artifact-family loader, target registry, and host-bridge execution implementation on top of the new engine seam. Next action: add a sibling Cloud Functions runtime registry under `neovex-server`, resolve `targets.json` entries to runtime handler entrypoints, and implement a trigger-scoped host bridge that executes one CloudEvent target through the shared runtime invocation helpers. | `cargo fmt --all`; `cargo test -p neovex-engine mutation_journal::triggers --lib`; `cargo check -p neovex-core -p neovex-engine -p neovex-storage`; `cargo fmt --all --check`; `make clippy` (still fails only on pre-existing Firebase gRPC lint debt in `crates/neovex-server/src/adapters/firebase/grpc/{listen_stream,listen_websocket,unary,write_stream,mod}.rs`; this pass also fixed two missed `WriteOp.resource_path_binding` initializers in `crates/neovex-server/src/execution/read_tracking/tests.rs`). |
| 2026-04-25 | T1.4b Trigger ancestry metadata and chain-depth limiting | `in_progress` | Closed `T1.4a` after teaching the engine-owned execution seam to return retryable versus terminal dispositions, adding durable retry scheduling/replay in the tenant worker, bootstrapping both `Pending` and `RetryPending` invocations on executor install, and classifying Cloud Functions runtime/storage failures without baking retry timing into the adapter. I stopped before code on `T1.4b` because ancestry/depth limiting is a separate commit-materialization boundary: parent-trigger metadata has to be emitted with committed writes and durable invocation records rather than inferred from the retry worker after the fact. The active next step is to inspect the trigger candidate builder plus runtime write path together, choose where parent invocation identity/depth rides on committed writes, and then enforce the configured chain budget during invocation materialization or claim. | Source review of `crates/neovex-engine/src/triggers/{dispatch.rs,materialize.rs}` and `crates/neovex-server/src/adapters/cloud_functions/execution.rs`; `cargo fmt --all`; `cargo test -p neovex-engine mutation_journal::triggers --lib`; `cargo test -p neovex-server cloud_functions --lib`; `cargo check -p neovex-engine -p neovex-server`; `cargo fmt --all --check`; `make clippy` (still fails on pre-existing Firebase gRPC lint debt in `crates/neovex-server/src/adapters/firebase/grpc/{listen_stream,listen_websocket,unary,write_stream,mod}.rs`, and this pass also fixed one new `clippy::redundant_guards` finding in `crates/neovex-server/src/adapters/cloud_functions/execution.rs`). |
| 2026-04-25 | T1.4a Retryable failure classification and durable retry replay | `done` | Landed durable retry classification and replay in the engine instead of smuggling retry policy into the Cloud Functions runtime adapter. `TriggerInvocationExecutor` now returns explicit retryable versus terminal dispositions, the tenant worker persists `RetryPending` with bounded backoff/max-attempt handling, executor bootstrap requeues both `Pending` and `RetryPending` records, and focused tests cover retry-to-completion, due-retry replay after restart, and promotion to terminal failure after exhausting the attempt budget. | `cargo fmt --all`; `cargo test -p neovex-engine mutation_journal::triggers --lib`; `cargo test -p neovex-server cloud_functions --lib`; `cargo check -p neovex-engine -p neovex-server`; `cargo fmt --all --check`; `make clippy` (still fails on pre-existing Firebase gRPC lint debt in `crates/neovex-server/src/adapters/firebase/grpc/{listen_stream,listen_websocket,unary,write_stream,mod}.rs`, and this pass also fixed one new `clippy::redundant_guards` finding in `crates/neovex-server/src/adapters/cloud_functions/execution.rs`). |
| 2026-04-25 | T1.4a Retryable failure classification and durable retry replay | `in_progress` | Closed `T1.3b` after landing the Cloud Functions runtime registry and trigger execution bridge in `neovex-server`: the sibling artifact family now validates and loads through `CloudFunctionsRegistry`, resolved Firestore trigger targets execute through the shared runtime seam, and focused tests cover successful database reads/writes plus missing runtime handlers. I split the original `T1.4` row before continuing because retry/replay and chain-depth limiting are separate engine risks. The active next step is `T1.4a`: teach the engine-owned execution seam to distinguish retryable versus terminal failures, persist due retry state, and replay scheduled retries after delay or restart before adding ancestry/depth metadata in `T1.4b`. | `cargo test -p neovex-server cloud_functions --lib`; `cargo fmt --all`; `cargo check -p neovex-server`; `cargo fmt --all --check`; `make clippy` (still fails on pre-existing Firebase gRPC lint debt in `crates/neovex-server/src/adapters/firebase/grpc/{listen_stream,listen_websocket,unary,write_stream,mod}.rs`, not on the new Cloud Functions runtime work). |
| 2026-04-25 | T1.3b Cloud Functions runtime registry and V8 trigger execution | `done` | Added the server-owned Cloud Functions runtime registry and trigger executor on top of the shared engine seam. `CloudFunctionsRegistry` now validates and loads the sibling `.neovex/firebase/{artifact.json,targets.json,bundle.mjs,bundle.sha256}` family, `CloudFunctionsTriggerExecutor` resolves Firestore trigger bindings from that registry, and trigger JavaScript executes through the shared V8 runtime path with trigger-scoped database access and atomic writes. | `cargo test -p neovex-server cloud_functions --lib`; `cargo fmt --all`; `cargo check -p neovex-server`; `cargo fmt --all --check`. |
| 2026-04-25 | T1.3b Cloud Functions runtime registry and V8 trigger execution | `in_progress` | Started the server/runtime half immediately after closing the engine seam. The working direction for this item is to keep the new engine execution worker generic, then add a sibling Cloud Functions runtime registry in `neovex-server` that loads `.neovex/firebase/{artifact.json,targets.json,bundle.mjs,bundle.sha256}`, resolves Firestore document trigger targets to runtime entrypoints, and executes them through the existing runtime bundle invocation helpers plus a trigger-scoped host bridge instead of cloning Convex runtime code wholesale. | Follow-on source review queued for `crates/neovex-server/src/adapters/cloud_functions/`, `crates/neovex-server/src/execution/invocations/`, `crates/neovex-server/src/adapters/convex/host_bridge/`, and the Cloud Functions artifact/target-binding reference docs. |
| 2026-04-25 | T1.1a Trigger commit candidate derivation and durable cursor contract | `done` | Landed the shared trigger-candidate and cursor foundation instead of deferring durability into adapter-local dispatch code. `WriteOp` now optionally carries the committed `ResourcePathBinding` needed to preserve delete-path identity, the engine has a shared candidate builder that emits deterministic Firestore CloudEvent ids plus nested update masks, and every storage provider exposes `TriggerDeliveryCursor` persistence through its existing metadata path. I stopped before `T1.1b` because the next slice is a separate execution-risk boundary: live post-commit candidate emission and restart/bootstrap replay from the durable journal plus cursor without regressing commit latency. Next action: inspect the existing commit-processing and durable-journal recovery seams in `neovex-engine`, choose the narrowest shared post-commit emission hook, and thread replay bootstrap through the new cursor contract. | `cargo fmt --all`; `cargo check -p neovex-core -p neovex-engine -p neovex-storage`; `cargo test -p neovex-engine triggers::dispatch --lib`; `cargo test -p neovex-storage trigger_delivery_cursor --lib`; `cargo fmt --all --check`. |
| 2026-04-25 | T1.1a Trigger commit candidate derivation and durable cursor contract | `in_progress` | Split the old `T1.1 Durable trigger candidate emission` row into two execution slices before code. Source review showed two separate implementation risks: the provider-wide durable cursor contract and deterministic commit/resource-path candidate derivation can land cleanly ahead of the broader live-feed and restart-bootstrap wiring. The working direction for `T1.1a` is therefore to add a shared trigger commit candidate builder plus cross-provider `TriggerDeliveryCursor` persistence, leaving live post-commit feed and replay bootstrap for `T1.1b`. | Startup refresh of `AGENTS.md`, `README.md`, `ARCHITECTURE.md`, `docs/README.md`, `docs/plans/README.md`, and `docs/plans/firebase-cloud-functions-plan.md`; `git status --short`; source review of `crates/neovex-core/src/trigger.rs`, `crates/neovex-engine/src/service/mutations/{journal,commit_processing}.rs`, `crates/neovex-engine/src/persistence/tenant/{provider_state,reads}.rs`, `crates/neovex-storage/src/{store/sqlite/postgres/mysql/libsql}` journal/resource-path metadata seams. |
| 2026-04-25 | T0.8 App-root discovery, project-layout, and server-SDK contract | `done` | Added a shared Cloud Functions app-root contract in `crates/neovex-server/src/adapters/cloud_functions/app_contract.rs` instead of baking more Firebase-specific heuristics into `neovex-bin`. The contract now defines auto-discovery for Firebase `firebase.json` roots and standalone `@google-cloud/functions-framework` package roots, explicit `--app-dir` override behavior, Firebase codebase/source normalization, ambiguity handling for mixed repos, fixed `.neovex/firebase/` artifact ownership, and a fail-fast first-slice `firebase-admin/app` plus `firebase-admin/firestore` method matrix. I stopped at the completed T0 phase boundary so the next pass can start `T1` from the new durable-dispatch foundation rather than mixing design-contract work with engine dispatch wiring. | `cargo test -p neovex-server cloud_functions --lib`; `cargo fmt --all`; `cargo fmt --all --check`; `make clippy` (still fails on pre-existing Firebase gRPC lint debt in `crates/neovex-server/src/adapters/firebase/grpc/{listen_stream,listen_websocket,unary,write_stream,mod}.rs`, not on the new Cloud Functions app-root contract). |
| 2026-04-25 | T0.7 Trigger registry | `done` | Added a tenant-scoped, engine-owned `TriggerRegistry` instead of parking trigger lookup in the server layer. Registrations now use stable string ids plus Firestore event type and shared `DocumentTriggerPattern`, support register/deregister/enable/disable/list, and return captured wildcard params on lookup. I stopped before `T0.8` because app-root discovery, Firebase project-layout compatibility, and the covered `firebase-admin` matrix are a broader cross-surface design slice than the finished T0 contract batch. | `cargo test -p neovex-engine triggers::registry --lib`; `cargo test -p neovex-server cloud_functions::tests --lib`; `cargo fmt --all`; `cargo fmt --all --check`; `make clippy` (still fails on pre-existing Firebase gRPC lint debt in `crates/neovex-server/src/adapters/firebase/grpc/{listen_stream,listen_websocket,unary,write_stream,mod}.rs`, not on the new trigger registry or Cloud Functions contract types). |
| 2026-04-25 | T0.6 Firebase root-package defaults contract | `done` | Chose a narrow, fail-fast `firebase-functions/v2` root-defaults contract: `setGlobalOptions()` is covered, explicit per-handler options override inherited defaults, document triggers inherit only `retry`, HTTPS root-default inheritance stays deferred, and root-level `onInit()` is explicitly rejected for the first slice. The decision is recorded in a dedicated reference doc and backed by validation helpers in the new Cloud Functions contract module. | `cargo test -p neovex-server cloud_functions::tests --lib`; `cargo fmt --all`; `cargo fmt --all --check`; `npm run docs:validate-refs:strict` (unavailable in this checkout: missing script). |
| 2026-04-25 | T0.5 Deploy-time target and binding contract | `done` | Fixed the `targets.json` contract as a typed manifest rather than leaving event and HTTP bindings implicit. Each target now records authoring surface, target name, runtime entrypoint, signature type, binding kind, and execution identity semantics, with validation that rejects mismatched signature/binding pairs, duplicate target names, legacy Functions Framework `event` signatures, and Firestore namespace claims beyond the first slice. | `cargo test -p neovex-server cloud_functions::tests --lib`; `cargo fmt --all`; `cargo fmt --all --check`; `npm run docs:validate-refs:strict` (unavailable in this checkout: missing script). |
| 2026-04-25 | T0.4 Generalized runtime artifact, import-resolution, and deploy contract | `done` | Chose a sibling `.neovex/firebase/` artifact family instead of forcing the current Convex deploy manifest into a fake generic schema. The first-slice Cloud Functions contract now fixes `artifact.json`, `bundle.mjs`, `bundle.sha256`, and `targets.json` under that internal root, and records `deploy_alias_layer` as the exact import-resolution strategy for covered `firebase-functions/v2`, `@google-cloud/functions-framework`, and `firebase-admin` specifiers. | `cargo test -p neovex-server cloud_functions::tests --lib`; `cargo fmt --all`; `cargo fmt --all --check`; `npm run docs:validate-refs:strict` (unavailable in this checkout: missing script). |
| 2026-04-25 | T0.4 Generalized runtime artifact, import-resolution, and deploy contract | `in_progress` | Started the deploy/runtime contract pass. Source review confirmed the current staging and activation path is still deeply Convex-shaped: deploy uploads stage only `.neovex/convex/*`, the loader is `ConvexRegistry::from_app_dir(...)`, and the manifest schema is specific to Convex function plans and HTTP routes. The working direction for this item is to keep the deploy/admin guarantees shared (staging, integrity validation, dry-run diffing, atomic generation activation) but add a sibling Cloud Functions artifact family under `.neovex/firebase/` rather than force the current `ConvexRegistry` schema into a fake generic shape. Next action: codify that sibling artifact contract, the exact covered import-resolution strategy, and the future target-manifest hook in code and docs. | Source review of `crates/neovex-server/src/http/deploy.rs`, `crates/neovex-server/src/adapters/convex/{mod.rs,manifest.rs,registry/{mod.rs,loading.rs,deploy_summary.rs}}`, `packages/codegen/src/main.mjs`, and `docs/reference/deploy-admin-api.md`. |
| 2026-04-25 | T0.3 Durable cursor / invocation ledger model | `done` | Chose a hybrid durable-delivery contract that treats the invocation ledger as authoritative and the journal sequence cursor as materialization progress only. The shared trigger primitive now models a monotonic per-tenant `TriggerDeliveryCursor` plus durable `TriggerInvocationRecord` entries keyed by `(registration_id, cloud_event.id)` with explicit `Pending`, `Running`, `RetryPending`, `Completed`, and `TerminalFailure` states. This lets future dispatch work advance the journal cursor once matched invocations are durably recorded, while retries and completion survive restart independently per handler invocation. I stopped before `T0.4` because the next item is a broader cross-surface design boundary covering deploy artifacts, import resolution, and generalized runtime activation. Next action: inspect the current Convex-shaped deploy/admin/runtime registry path, package alias surfaces, and handler discovery contract to decide whether Neovex should generalize that path or add a sibling Cloud Functions artifact pipeline. | `cargo test -p neovex-core trigger --lib`; `cargo fmt --all`; `cargo fmt --all --check`; `make clippy` (still fails on pre-existing Firebase gRPC lint debt in `crates/neovex-server/src/adapters/firebase/grpc/{listen_stream,unary,write_stream,mod,listen_websocket}.rs`, not on the new trigger primitives). |
| 2026-04-25 | T0.3 Durable cursor / invocation ledger model | `in_progress` | Started the durable-delivery design pass. Initial engine/storage review shows the existing durable journal already provides the authoritative per-tenant sequence cursor, but cursor-only replay would not capture independent retry/completion state once a single commit fans out to multiple trigger matches. The working direction for this item is therefore an invocation-ledger model with a journal-backed materialization cursor: persist one durable record per matched handler invocation, keep retry/completion state on those records, and use the cursor only to track how far committed writes have been expanded into the ledger. Next action: land the shared ledger state machine and persistence-roundtrip tests in `neovex-core`, then record the choice in this plan before touching dispatch/storage code. | Startup refresh of `AGENTS.md`, `README.md`, `ARCHITECTURE.md`, `docs/README.md`, `docs/plans/README.md`, and `docs/plans/firebase-cloud-functions-plan.md`; `git status --short`; source review of `crates/neovex-engine/src/persistence/tenant/{provider_state,journal}.rs`, `crates/neovex-engine/src/service/mutations/journal.rs`, and `crates/neovex-storage/src/store/{read,journal,journal_snapshot}.rs`. |
| 2026-04-25 | T0.2 CloudEvent envelope, Firestore event types, and service-principal contract | `done` | Added a shared trigger-event model in `neovex-core` instead of letting Firebase or Functions Framework adapters invent their own event payloads first. The new `trigger.rs` module now defines standard Firestore CloudEvent type strings, fixed `specversion: "1.0"`, protocol-neutral `DocumentEventData` / update-mask shapes, Firestore metadata with captured params, commit metadata, and an explicit service-principal execution contract. I stopped before `T0.3` because the durable cursor versus invocation-ledger choice is the next broader engine/storage design boundary. Next action: inspect the existing durable journal progress and invocation-state seams in `neovex-engine` / `neovex-storage`, then choose and document the persisted trigger cursor or invocation-ledger model before touching dispatch code. | `cargo test -p neovex-core trigger --lib`; `cargo fmt --all`; `cargo fmt --all --check`; `make clippy` (failed on pre-existing Firebase gRPC lint debt in `crates/neovex-server/src/adapters/firebase/grpc/{listen_stream,unary,write_stream,mod}.rs`, not in the new core trigger primitives). |
| 2026-04-25 | T0.2 CloudEvent envelope, Firestore event types, and service-principal contract | `in_progress` | Closed `T0.1` after landing a shared `DocumentTriggerPattern` in `neovex-core` on top of the existing Firestore `DocumentPath` model. The new primitive matches only document-terminal paths, supports wildcard captures on alternating collection/document segments, rejects collection-terminal shapes, and produces deterministic wildcard params for later trigger dispatch. The next slice is the shared CloudEvent/event-payload model that both Firebase and Functions Framework adapters will consume. | `cargo test -p neovex-core resource_path --lib`; `cargo fmt --all`; `cargo fmt --all --check`. |
| 2026-04-25 | T0.1 Document-path trigger semantics and pattern model | `done` | Added a protocol-neutral document trigger path primitive to `neovex-core` by extending the path identity module rather than building adapter-local matching. `DocumentTriggerPattern` now parses Firestore-style document-terminal trigger patterns, supports wildcard captures on both collection and document segments, rejects collection-terminal shapes and duplicate wildcard names, and matches against the shared `DocumentPath` resource model with deterministic captured params. | `cargo test -p neovex-core resource_path --lib`; `cargo fmt --all`; `cargo fmt --all --check`. |
| 2026-04-25 | T0.1 Document-path trigger semantics and pattern model | `in_progress` | Activated the Cloud Functions follow-on plan immediately after closing the Firebase adapter plan. The first slice is the shared document-trigger path primitive in `neovex-core`: define Firestore-style document-terminal trigger patterns, support wildcard captures on alternating collection/document segments, reject collection-terminal shapes, and expose deterministic path-param extraction without pushing any protocol concerns into the type. | Startup refresh of `AGENTS.md`, `README.md`, `ARCHITECTURE.md`, `docs/README.md`, `docs/plans/README.md`, and `docs/plans/firebase-cloud-functions-plan.md`; `git status --short`; source review of `crates/neovex-core/src/resource_path.rs` and the Cloud Functions roadmap/status ledger. |
| 2026-04-25 | Plan creation | `pending` | Initial plan authored. Activation gate: firebase-adapter-plan F3 phase completion. The codex agent is currently executing F3.2b2 on the main worktree; this plan must not interfere with that work. | N/A |
| 2026-04-25 | Architecture review follow-up | `pending` | Revised the plan to resolve review findings: document triggers now source from committed writes/resource paths rather than subscription diffs, at-least-once delivery requires durable cursor or invocation state, authoring/build/deploy is a first-class design slice, and trigger events now model CloudEvent identity plus service-principal execution semantics. | Docs review |
| 2026-04-25 | Scope widening: Cloud Functions framework compatibility | `pending` | Broadened the plan from Firebase-only to cover both `firebase-functions/v2` and `@google-cloud/functions-framework` authoring surfaces. Added standard Firestore CloudEvent type strings (`google.cloud.firestore.document.v1.*`) and `DocumentEventData` payload as the shared event contract. Added HTTP handler support (`functions.http()`, `onRequest()`, `onCall()`) as T3. Renamed plan from "Firebase Cloud Functions" to "Cloud Functions" to reflect the broader scope. Total budget increased from 15-22 to 20-29 context windows. | Docs review |
| 2026-04-25 | Compatibility-boundary hardening | `pending` | Tightened the broadened plan after a second architecture review: added deploy-time framework target/binding metadata, exact import-resolution as a first-class requirement, explicit separation between authoring compatibility and full standalone Functions Framework runtime parity, and explicit HTTP/callable support matrices covering options, auth context, App Check, CORS defaults, and error mapping. Total budget increased from 20-29 to 23-32 context windows. | Docs review |
| 2026-04-25 | Existing-app compatibility hardening | `pending` | Extended the plan from handler-level compatibility to whole-app compatibility for modern Firebase projects: added `firebase.json` / `functions.source` / `codebase` discovery, a planned `.neovex/firebase/` internal artifact root, and a covered `firebase-admin/app` plus `firebase-admin/firestore` compatibility slice so existing function bodies and project structure do not require major refactors. Total budget increased from 23-32 to 26-35 context windows. | Docs review |
| 2026-04-25 | Auto-discovery parity with Convex | `pending` | Tightened the app-layout contract so Firebase and standalone Cloud Functions apps follow the same ergonomics as Convex-compatible apps: the common case should auto-detect the nearest compatible app root from the current directory or its parents, with `--app-dir` retained only as an explicit override for ambiguous or nonstandard repos. Added standalone Functions Framework package-root discovery to T0/T2 and clarified that requiring `--app-dir` for common migrations would be a compatibility miss. | Docs review |
