# Node-Compatible Runtime Plan

Status: active

This plan owns the work required to make `crates/neovex-runtime` deliberately
Node-compatible on top of the existing `deno_core`/V8 backend, without giving
up Neovex's single-binary posture, `HostBridge` capability boundary, or
enterprise-trust security model.

Neovex is still pre-launch. Prefer clean breaking changes and a coherent final
runtime contract over compatibility shims that preserve the current
Node-external toolchain shape forever.

Desired end state:

- Neovex has **one canonical JavaScript runtime implementation**:
  `crates/neovex-runtime`.
- this is **not** a sidecar runtime adjacent to the current V8 runtime; it is
  an upgrade of the existing runtime into the product's primary JS execution
  contract.
- the runtime should expose multiple compatibility targets on top of that one
  backend where the product contract requires them:
  - a web-standard isolate target for ordinary Convex-compatible function code
    and other worker-like adapter flows
  - a versioned Node target for Node APIs, `"use node"` actions, Firebase /
    Cloud Functions, and local tooling
- the only enduring duality this plan should tolerate is **runtime profile**,
  not **runtime implementation**:
  - an application runtime profile with strict capability boundaries
  - a tooling runtime profile with broader local-only permissions when required
- `RuntimeProfile::Application` is the product runtime contract for
  adapter-facing user
  code, including Convex-compatible apps, Cloud Functions-compatible apps, and
  Firebase-facing JS flows that rely on the canonical runtime.
- `RuntimeProfile::Tooling` exists to support local authoring, package
  materialization, codegen, and Node-API/tooling needs without changing the
  adapter/runtime contract promised by `RuntimeProfile::Application`.
- `NeovexOnly` behavior and the external-Node runner are acceptable only as
  transitional migration states. They should not survive as long-term product
  commitments unless the plan records a durable JTBD that requires them.

## Status

- **Plan status:** `active`
- **Control item:** `—`
- **Status values:** `pending`, `in_progress`, `done`, `blocked`
- **Control item rule:** when a roadmap item is marked `in_progress`, mirror it
  here. Use `—` only when no item is currently active.
- **Primary source of truth:** this file plus the current git worktree.
- **Checkpoint rule:** every work session that changes implementation state
  must update the roadmap item status and the execution log before stopping.
- **Continuation rule:** after closing one roadmap item and recording its
  verification, immediately advance to the next eligible `pending` item in the
  same session unless the work is blocked or the plan itself is complete.

## Plan Ownership And Canonical Inputs

This is the active control plan for Neovex's Node-compatible runtime work. It
owns the coordinated runtime, codegen, adapter-compatibility, and external-Node
removal wave for this slice. Do not start a separate broad Node/runtime
compatibility plan unless this plan is first completed, blocked with a written
handoff, or explicitly superseded.

Implementation work must keep these source inputs open:

- Top-level repo references: `README.md`, `ARCHITECTURE.md`, `docs/README.md`,
  `docs/plans/README.md`.
- Adapter compatibility references:
  `docs/adapters/convex/compatibility.md`,
  `docs/adapters/cloud-functions/compatibility.md`,
  `docs/adapters/firebase/compatibility.md`.
- Runtime and boundary references:
  `docs/architecture/runtime/adapter-boundary.md`,
  `docs/architecture/server/auth-runtime-trust.md`,
  `docs/plans/research/runtime-file-storage-surface.md`.
- Current implementation roots:
  `crates/neovex-runtime/`, `crates/neovex-bin/`, `packages/codegen/`,
  `packages/convex/`.
- External truth sources recorded at the end of this file.

## Autonomous Execution Contract

This plan is designed to survive compaction and resume autonomously. Each
roadmap item must be actionable from the plan, the execution log, the current
git worktree, and the source files without relying on chat history.

An agent resuming this plan must:

- read the status section, roadmap tables, detailed phase entry for the active
  or next eligible item, and the latest execution-log entries
- run `git status --short` before choosing work
- resume the existing `in_progress` item if one exists
- otherwise pick the first `pending` item in roadmap order whose hard deps are
  `done`
- continue directly to the next eligible item after a closeout instead of
  stopping at a verification boundary

## Control Plan Rules

1. Read `AGENTS.md`, `README.md`, `ARCHITECTURE.md`, `docs/README.md`,
   `docs/plans/README.md`, and this plan before starting a roadmap item.
2. Run `git status --short` before choosing work. If the worktree is dirty,
   reconcile before editing.
3. If any roadmap item is `in_progress`, resume it. If none, pick the first
   `pending` item in roadmap order whose hard deps are `done`.
4. Mark exactly one roadmap item `in_progress` before implementation. Do not
   advance another item until the active item is `done` or `blocked`.
5. After an item reaches `done` and its verification is logged, immediately
   start the next eligible `pending` item in the same session unless the work
   is blocked or the whole plan is complete.
6. A roadmap item is not `done` until its verification, touched files, and
   resulting contract changes are recorded in the execution log.
7. If work stops mid-item, leave the item `in_progress` and record the exact
   remaining work so the next agent can resume without rediscovery.

## Verification Contract

Every completed roadmap item must leave durable evidence:

- the roadmap item status is updated
- the execution log records the date, item, status, files touched, and
  verification
- focused tests or fixtures cover the changed behavior
- `cargo fmt --all --check` and `make clippy` run after each implementation
  item unless the log records a concrete blocker
- broader verification runs when the item changes behavior or external claims
- compatibility matrices, docs, and plan claims are narrowed immediately if a
  verification lane fails

## Why This Plan Exists

Neovex currently has an awkward split:

- runtime execution is already V8-backed inside `neovex-runtime`
- runtime host calls already flow through a narrow capability seam
  (`HostBridge`)
- code generation and JS authoring tooling still depend on an external
  Node.js installation

That split is acceptable for the current shipped product, but it is not the
best long-term architecture if Neovex wants all of the following to be true at
once:

1. V8 isolates are meaningfully Node-compatible.
2. The onboarding path does not require a separately installed Node.js.
3. Runtime host access remains capability-scoped instead of becoming raw host
   filesystem or process access.
4. The system remains credible to enterprise buyers who care about
   determinism, permissions, auditability, and explicit trust boundaries.

The current codebase proves the starting point:

- `neovex-runtime` is built directly on `deno_core` and a custom bootstrap,
  not on the full Deno CLI stack.
- the runtime owns its own `RestrictedModuleLoader` and startup snapshot flow
- the runtime API is currently a Neovex-owned bootstrap surface, not a
  Node-compatible global environment
- `neovex codegen` still shells out to `node`
- `@neovex/codegen` currently imports `node:*` modules and depends on
  `esbuild` plus `typescript`

## Scope

- add first-class Node-compatible runtime profiles to `crates/neovex-runtime`
- choose and document the initial compatibility-target set for the runtime
  contract:
  - a preserved web-standard isolate target
  - one supported versioned Node target
- reuse official Deno crates where they provide the canonical implementation
  of Node compatibility
- preserve `HostBridge` as the stable Rust-side host contract
- add capability-scoped filesystem, env, process, network, timer, and module
  services that satisfy Node-compatible code without exposing arbitrary host
  access
- support `node:` imports, relevant globals, package.json semantics, and ESM /
  CommonJS interop
- define a package-resolution and `node_modules` strategy that is explicit
  enough for enterprise trust
- prove whether Neovex can run `@neovex/codegen` in-process without an
  external Node.js installation

## Non-Goals

- claiming full Node.js parity in one pass
- claiming unversioned "all Node versions" compatibility
- replacing `HostBridge` with Deno's `Deno.*` runtime API as the primary Neovex
  application contract
- allowing arbitrary runtime access to the host filesystem, unrestricted
  environment variables, subprocess spawning, or raw network access
- replacing the current `V8DenoCore` backend with Bun, workerd, or another
  backend in this plan
- removing the external Node.js prerequisite before embedded codegen parity is
  actually proven

## Current Local Baseline

The current local baseline is:

- `crates/neovex-runtime` uses `deno_core` directly and owns its own runtime
  bootstrap, loader, and host ops.
- `crates/neovex-runtime/src/runtime/bootstrap/source.rs` installs a Neovex
  bootstrap around `Deno.core.ops`; there is no Node compatibility layer today.
- `crates/neovex-runtime/src/module_loader.rs` enforces a restricted file-only
  bundle root.
- `crates/neovex-bin/src/codegen.rs` shells out to `node`.
- `packages/codegen/src/main.mjs` imports `node:fs/promises` and `node:path`.
- `packages/codegen/package.json` depends on `esbuild` and `typescript`.

This means the current runtime and current toolchain are still separate
architectural systems.

## JTBD Assumptions

This plan assumes Neovex is solving these concrete jobs:

1. **Run user application code inside the Neovex runtime without requiring a
   separate Node installation.**
2. **Support enough Node semantics that Node-targeted code and ecosystem tools
   can execute inside that same runtime.**
3. **Keep deployed application execution capability-scoped and auditable,
   rather than inheriting Node's unconstrained host model.**
4. **Run local authoring and codegen tasks that may need broader filesystem,
   package, or Node-API access, without turning those permissions on for
   deployed application execution.**
5. **Make the product runtime compatible enough for Neovex's adapter promise:
   Convex-compatible apps, Cloud Functions-compatible apps, and Firebase-facing
   JS flows should all target the same canonical Node-compatible Neovex
   runtime, not a separate compatibility engine.**

This plan does **not** assume a durable product need for two independent JS
runtime implementations. If that assumption changes, the plan must be amended
with a concrete JTBD for the second runtime.

## Runtime Taxonomy

To keep this work future-friendly without overcommitting the current plan,
Neovex should treat runtime design as three separate axes:

1. **Engine backend**
   The underlying JS engine and embedder stack.
   Examples:
   - `V8DenoCore` for the current `deno_core`/V8 implementation
   - a future alternative backend if a later plan justifies one

2. **Compatibility target**
   The language/runtime contract Neovex is trying to satisfy for user code.
   Examples:
   - a web-standard isolate target such as `WebStandardIsolate`
   - a versioned Node target such as `NodeLTSMajor`
   - a future workerd-like target if a product JTBD appears
   - a future Bun-like target if a product JTBD appears
   - an internal minimal JS/V8 target only if Neovex has a real reason to keep
     one

3. **Runtime profile**
   The capability envelope and local-artifact behavior under which the runtime
   executes.
   Current planned profiles:
   - `RuntimeProfile::Application`
   - `RuntimeProfile::Tooling`

Current delivery target for this plan:

- one engine backend: `V8DenoCore`
- two initial compatibility targets:
  - `CompatibilityTarget::WebStandardIsolate`
  - `CompatibilityTarget::Node22` as the primary named Node baseline
- runtime profiles layered on top:
  - `RuntimeProfile::Application`
  - `RuntimeProfile::Tooling`

Important constraints:

- this plan does **not** commit Neovex to shipping Bun, workerd, or another
  backend/runtime family now
- this plan **does** require Neovex to avoid baking Node assumptions into the
  runtime-profile layer so a future compatibility-target plan remains possible
- a future "V8 only" / minimal-JS mode should not survive by default just
  because it was historically present; it needs a concrete JTBD

Compatibility-target clarification:

- `CompatibilityTarget::WebStandardIsolate` remains a first-class delivery
  target in this plan, not a deprecated leftover
- `CompatibilityTarget::Node22` is the primary **named Node baseline** for
  Node APIs, package semantics, and tooling/runtime conformance
- Node compatibility work must not regress or erase the web-standard/default
  runtime semantics that current Convex-compatible ordinary functions rely on
- treat those Convex default-runtime semantics as a separate adapter-facing
  conformance slice that must remain verified while Node compatibility grows

## Node Versioning Policy

Neovex should not claim generic, unversioned "Node compatibility."

Instead:

- NCR0 must choose one supported Node compatibility baseline tied to the chosen
  Deno crate family
- any runtime path that claims Node compatibility must target the **same**
  supported Node baseline unless the plan is explicitly amended
- the compatibility lane, docs, and adapter claims must name that baseline
- support for an additional Node major should only be added if a concrete JTBD
  requires it

This keeps the product promise testable and prevents "Node-compatible" from
quietly becoming "whatever happened to work on one machine."

## Adapter Version Targets As Of 2026-04-28

The end state of this plan should be checked against the actual upstream
adapter ecosystems Neovex claims to be compatible with, not just a generic
Node story.

### Convex

- Official Convex docs say backend functions run in two runtimes:
  - the default Convex runtime, which is a custom V8 environment similar to
    Cloudflare Workers and oriented around web-standard APIs
  - an opt-in Node.js runtime for `"use node"` action files
- Official Convex docs say the Node.js runtime currently supports Node.js 20
  and 22, and defaults to Node.js 20 unless configured otherwise.
- The latest published `convex` npm package as of 2026-04-28 is `1.36.1`, and
  its package metadata declares `engines.node >=18.0.0`.

Implication for Neovex:

- this plan cannot treat "Convex compatibility" as only a Node-compat problem
- `RuntimeProfile::Application` must preserve the existing Convex-compatible
  default-runtime behavior for ordinary Convex functions while also supporting
  the Convex Node.js action contract
- Node compatibility work in this plan is necessary for Convex's `"use node"`
  path, but it is not sufficient by itself for full current Convex parity

### Firebase / Cloud Functions

- Official Firebase docs say Cloud Functions for Firebase fully supports
  Node.js 20 and 22 today, with Node.js 18 deprecated and Node.js 14 / 16
  already decommissioned.
- Official Google Cloud runtime docs list Cloud Run functions support for
  Node.js 22 and 20 as current stable runtimes, Node.js 18 as an older runtime
  still present in the table, and Node.js 24 as preview.
- The latest published `firebase-functions` npm package as of 2026-04-28 is
  `7.2.5`, with `engines.node >=18.0.0` and a peer dependency on
  `firebase-admin ^11.10.0 || ^12.0.0 || ^13.0.0`.
- The latest published `firebase-admin` npm package as of 2026-04-28 is
  `13.8.0`, with `engines.node >=18`.
- The latest published `@google-cloud/functions-framework` npm package as of
  2026-04-28 is `5.0.2`, with `engines.node >=10.0.0`.

Implication for Neovex:

- the platform/runtime contract Neovex should optimize for is not the broadest
  package-engine range; it is the currently supported serverless runtime set
- Node.js 24 should not be the initial Neovex compatibility promise because it
  is still preview in Google Cloud runtime docs
- Node.js 18 should not be the initial Neovex compatibility promise because it
  is already deprecated in Firebase docs

### Current Recommendation

Unless fresher upstream guidance changes this before implementation lands:

- choose `CompatibilityTarget::Node22` as the primary named product baseline
- add an explicit adapter verification lane for `Node20`
- do not make `Node18` part of the product compatibility promise
- do not make preview `Node24` part of this plan's completion criteria

This is the narrowest target set that still aligns with:

- current Convex Node-runtime support
- current Firebase Functions support
- current Google Cloud functions runtime support
- a prelaunch preference for one clean named baseline rather than many
  weakly-tested majors

## External Research Summary

### Deno Is The Closest Reusable Foundation

The best modern reusable foundation for a `deno_core`-based Node-compatible
runtime is Deno's own Node compatibility stack, not a fresh hand-rolled layer.

Key facts from official Deno sources:

- Deno officially supports `node:` built-ins, npm packages, `package.json`,
  CommonJS, and npm binaries.
- Deno exposes an official `deno_node` crate that initializes Node
  compatibility as a `deno_core` extension.
- `deno_node::init(...)` accepts injected services instead of hard-coding the
  host filesystem and resolver behavior.
- `deno_node` depends on a larger family of Deno crates, including
  filesystem, process, network, permissions, and npm / resolution support.
- `deno_runtime` is the slim Deno runtime library and reexports the Deno
  extension family, including `deno_node`, `deno_fs`, `deno_permissions`,
  `deno_process`, `deno_net`, `deno_napi`, and others.
- Deno documents that the `deno_runtime` crate API is subject to rapid and
  breaking changes.
- Deno's Node compatibility work is useful because it gives Neovex a canonical
  implementation path for a versioned Node target without requiring Neovex to
  hand-maintain every Node semantic from scratch.

### Deno Also Proves The Right Tooling Shape

Deno's official compatibility model shows that modern Node compatibility is not
just built-in module aliases. It includes:

- `node:` built-ins
- package.json-aware resolution
- CommonJS interop
- npm package resolution
- optional local `node_modules` creation
- Node-API addon support
- explicit permissions for filesystem, env, network, subprocess, and FFI

This matters for Neovex because `@neovex/codegen` uses `esbuild`, and Deno's
official docs explicitly call out `esbuild` as a Node-API addon scenario that
works when a local `node_modules/` is present and FFI is allowed.

### Cloudflare Provides The Best Rollout Pattern

Cloudflare Workers is not a reusable Rust embedder stack for Neovex, but its
product rollout pattern is instructive:

- Node compatibility is explicit and opt-in via a compatibility flag
- some Node APIs are native runtime implementations
- the rest are polyfill shims injected by tooling
- unsupported APIs may be importable before they are fully executable

That is a better rollout model for Neovex than an all-or-nothing "full Node"
claim.

### `unenv` Is Useful As A Shim Layer, Not The Runtime Core

`unenv` is useful for build-time aliasing and JS-level shims. It is already
used by Cloudflare's toolchain story. However, it is not the right primary
runtime substrate for Neovex because:

- it is not V8 / `deno_core` native
- many modules are polyfilled or mocked rather than backed by real capability
  implementations
- it does not solve Rust-side host policy, permissions, or trusted module
  loading

`unenv` is best treated as a targeted fallback for low-risk JS shims, not as
the source of truth for Neovex's Node host contract.

### Bun Is Useful Inspiration, Not A Reuse Path

Bun demonstrates that a single-binary Node-compatible developer experience is
possible. However, it is not a realistic reuse path for Neovex because Bun is
an all-in-one JavaScriptCore-based runtime and toolchain, not a Rust / V8 /
`deno_core` embedder layer.

Inference from the official Bun docs:

- Bun is useful as a product benchmark for DX
- Bun is not a practical drop-in implementation path for Neovex's current
  `deno_core`/V8 stack

## Recommendation

### Recommended Architecture

Adopt a **hybrid Deno-host approach**:

1. **Reuse official Deno crates for real Node compatibility.**
   Prefer `deno_node` plus the matching Deno extension family over
   hand-implementing Node built-ins from scratch.

2. **Do not adopt full `deno_runtime::MainWorker` as Neovex's runtime
   abstraction.**
   Neovex already has a runtime lifecycle, loader, bootstrap, snapshot model,
   and `HostBridge` contract. Replacing that with Deno's worker model would
   create unnecessary churn and ownership confusion.

3. **Cherry-pick or compose Deno runtime crates under Neovex-owned execution
   modes.**
   The runtime contract should stay Neovex-owned even when the module
   implementations are Deno-owned.

4. **Keep host access capability-scoped.**
   Node compatibility must not become a backdoor to raw host access. Any
   filesystem, env, process, network, or FFI capability must still flow through
   Neovex-owned policy and adapter seams.

5. **Separate "runtime Node compatibility" from "tooling Node replacement".**
   The first makes Node-style code executable in isolates. The second removes
   the external Node.js dependency from authoring and codegen. They are related,
   but they should not be conflated into one risky cutover.

Clarification:

- this separation is about **profiles and rollout order**, not about shipping
  two permanent runtime engines
- after the plan lands, the target is still one canonical Neovex runtime with
  product-justified runtime profiles

### What To Reuse

Neovex should plan to reuse:

- `deno_node`
  canonical implementation of Node built-ins, globals, and module semantics
- `deno_fs`
  injected filesystem trait and extension surface
- `deno_permissions`
  explicit capability gating model
- `node_resolver`
  package.json, exports, conditions, and Node resolution logic
- `deno_package_json`
  package.json handling
- `deno_napi`
  Node-API addon support, required for packages like `esbuild`
- selected `deno_runtime` composition patterns
  as implementation references and possibly narrow helpers, not as the
  top-level Neovex runtime abstraction

### What Not To Reuse Blindly

Neovex should not blindly reuse:

- full `MainWorker` / Deno CLI worker ownership
- unrestricted `RealFs`
- Deno's default "download on import" developer trust model for runtime
  invocation
- blanket Node API polyfills that silently noop in production-sensitive paths

## Decision Principles

1. **Capability before convenience.**
   A Node-compatible API is acceptable only if Neovex can express it through a
   bounded capability contract.

2. **Staged enablement beats blanket claims.**
   Support a deliberate subset first and expand from measured evidence.

3. **Runtime invocation must not fetch code or packages.**
   Package acquisition belongs to explicit CLI / staging flows, not to live
   request execution.

4. **Prefer canonical upstream implementations.**
   If Deno already ships the correct implementation for a Node module, do not
   fork that behavior into a Neovex-only reimplementation unless there is a
   concrete host-policy reason.

5. **Runtime profiles must be explicit.**
   Node compatibility should be enabled by runtime profile / adapter contract,
   not inferred accidentally.

6. **Compatibility targets must be versioned and named.**
   "Node-compatible" is not precise enough for an enterprise contract; Neovex
   should name the supported Node baseline it actually tests.

## Architecture Shape

### 1. Add Runtime Profiles, Not Parallel Runtime Implementations

Introduce explicit runtime profiles inside the single Neovex runtime:

- `RuntimeProfile::Application`
- `RuntimeProfile::Tooling`

Rules:

- both profiles must target the same Node-compatible language/runtime contract
- `RuntimeProfile::Tooling` must not become "the more Node-compatible runtime"
  while `RuntimeProfile::Application` remains a narrower JavaScript dialect
- the difference between profiles is capability envelope and local artifact
  behavior, not core Node semantics
- `NeovexOnly` may exist temporarily as migration scaffolding, but it is not an
  acceptable completed-plan end state

The runtime profile determines:

- which extensions are loaded
- which globals are installed
- which module-resolution rules are active
- which host permissions are available
- whether native Node-API addons and local `node_modules` are allowed

This should be a Neovex-owned runtime concept, not a free-form bag of flags.

### 1a. Keep Compatibility Target Separate From Runtime Profile

The plan should model compatibility target separately from runtime profile.

Recommended structure:

- `CompatibilityTarget`
  - initial targets:
    - `WebStandardIsolate`
    - one supported versioned Node baseline
  - future targets only when justified by a separate product plan
- `RuntimeProfile`
  - `Application`
  - `Tooling`

Rules:

- `RuntimeProfile::Application` may execute more than one compatibility target
  when the adapter contract requires it
- `RuntimeProfile::Tooling` must support the named Node baseline used for
  codegen and local tooling
- all Node-targeted runtime paths must use the same named Node baseline
- a future workerd-like or Bun-like target should plug in at the
  compatibility-target or backend boundary, not by forking the
  `RuntimeProfile::Application` / `RuntimeProfile::Tooling` model
- if Neovex keeps an internal minimal-JS or "V8 only" target, it should be
  explicitly marked non-product unless a JTBD justifies exposing it

### 1b. Keep Runtime Profile Separate From Runtime Execution Model

Neovex already has an execution-model axis in `neovex-runtime`
(`RunToCompletion`, `CooperativeLocker`, warm-pool behavior, and related
scheduler semantics). This plan must not blur that axis with runtime profile.

Rules:

- `RuntimeExecutionModel` remains the scheduler / isolation / reuse axis
- `RuntimeProfile` owns capability envelope, artifact policy, and
  compatibility-profile composition
- the implementation should compose these as independent axes rather than
  folding them into one combined enum or overloading `RuntimeLimits`
- plan and code language should avoid saying "execution mode" when the intent
  is actually runtime profile

### 2. Preserve The Existing Neovex Runtime Core

Keep the existing foundations:

- `HostBridge`
- typed host ABI envelope
- runtime-owned bootstrap sequencing
- startup snapshot lifecycle
- `RestrictedModuleLoader` ownership
- current runtime limits and cancellation model

Node compatibility should layer onto that base, not replace it.

### 3. Add A Neovex-Owned Node Compatibility Module Tree

Add a new ownership root under `crates/neovex-runtime/src/`, for example:

- `node_compat/mod.rs`
- `node_compat/profile.rs`
- `node_compat/extensions.rs`
- `node_compat/permissions.rs`
- `node_compat/fs.rs`
- `node_compat/resolution.rs`
- `node_compat/npm.rs`
- `node_compat/napi.rs`

These modules should own Neovex-side composition, while upstream Deno crates
continue to own the actual compatibility implementations where possible.

### 4. Use Injected Filesystem Services, Not Raw Host Filesystem Access

The presence of `deno_node::init(..., fs: FileSystemRc)` is strategically
important. Neovex should use that injection seam.

Recommended rule:

- do not wire Node compatibility straight to unrestricted host `RealFs`
- implement a Neovex-scoped filesystem adapter that exposes only approved
  paths and operations

Expected path families:

- runtime bundle root
- app directory / source root where explicitly allowed
- project-local `.neovex/` state
- explicit temp directories for codegen/build staging
- optional future XDG cache roots if separately documented

The host-facing design should follow the same principle already established in
the runtime-file-storage research: capability-backed filesystem semantics are
acceptable; raw host filesystem semantics are not.

### 5. Adopt An Explicit Permission Model

Mirror Deno's permission categories, but make Neovex own the policy:

- read
- write
- env
- net
- run
- ffi

Recommended baseline:

- `RuntimeProfile::Application`
  - allow read only inside approved bundle / app / generated-artifact roots
  - deny write by default except adapter-specific safe roots
  - deny env except explicit allowlist
  - deny net except adapter-owned allowlist
  - deny run
  - deny ffi
- `RuntimeProfile::Tooling`
  - allow read/write inside app root, generated artifact root, and local cache
  - allow env on a documented allowlist
  - allow ffi only when required for Node-API addons
  - still deny arbitrary subprocess execution by default

The permission model must be observable in logs, errors, and docs.

### 6. Keep Resolution And Acquisition Separate

Do not let runtime invocation auto-download npm packages from the network.

Instead:

- resolution logic may understand npm and package.json semantics
- acquisition and installation happen in an explicit CLI-managed staging step
- invocation runs against already-staged artifacts, caches, or `node_modules`

This separation is essential for enterprise trust:

- reproducibility
- auditability
- offline / air-gapped operation
- no surprise network fetches during request execution

### 6a. Make Package Acquisition A CLI-Owned Service

The plan should be explicit about ownership here so agents do not accidentally
move package-manager responsibilities into `neovex-runtime`.

Recommended ownership split:

- `crates/neovex-runtime`
  - runtime resolution semantics against already-staged packages
  - package-manifest consumption at invocation time
  - capability enforcement during module loading
- `crates/neovex-bin` or a newly introduced Neovex-owned staging component
  - npm package acquisition / download / verification
  - local `node_modules` materialization when the tooling runtime profile
    requires it
  - lock / manifest / cache invalidation policy
  - offline staging and replayable install behavior

Hard rule:

- no network fetch, package-manager mutation, or registry side effect may occur
  inside a live `neovex-runtime` invocation path

### 7. Prefer Local `node_modules` For Tooling Profiles

Deno's official model is useful here:

- default global cache is great for Deno-native apps
- local `node_modules` is recommended when tools expect it or when Node-API
  addons are involved

For Neovex tooling, especially codegen with `esbuild`, the recommended plan is:

- support a tooling runtime profile that can materialize a local
  `node_modules`
  directory or equivalent staged package tree
- do not require npm to do that once Neovex owns the package acquisition path

This profile is the likely bridge from external Node.js toward embedded codegen.

### 7a. Publish A Supported / Unsupported Surface Matrix

Because full Node parity is a non-goal, Neovex must make the boundary
executable and explicit rather than leaving it implicit in scattered tests.

Required artifact:

- a checked-in matrix or fixture map that states, for each claimed Node module
  or behavior slice:
  - supported
  - supported only in `RuntimeProfile::Tooling`
  - supported with caveats
  - explicitly unsupported / fail-fast

This matrix must cover not only the "happy path" modules Neovex enables, but
also privileged or host-coupled surfaces whose status would otherwise be
ambiguous, such as subprocess, inspector, worker, or watch-style APIs.

## Version Alignment Gate

Before implementation begins, Neovex must resolve the Deno version-family
alignment problem.

Current local state:

- Neovex pins `deno_core 0.395.0` via the workspace dependency and a locker
  fork patch.
- current Deno crates in the official ecosystem have already advanced past that
  release family.
- current `deno_node` docs show a newer `deno_core` dependency than Neovex's
  pinned fork.

Therefore the first implementation gate is a version spike:

### Gate VC1: Choose The Compatibility Base

Options:

1. **Move the locker fork forward to a Deno release family that matches the
   current `deno_node` / `deno_runtime` ecosystem.**
   This is the recommended path.

2. **Backport `deno_node` and its dependency family down to the current
   `deno_core 0.395` fork.**
   This is high-maintenance and should be rejected unless the forward rebase
   fails for concrete reasons.

3. **Vendor a frozen subset of Deno compatibility crates into Neovex.**
   Only use this if both upstream alignment paths fail and the required surface
   is small enough to justify ownership.

Recommendation: choose option 1 unless a focused spike proves the locker fork
cannot be rebased cleanly.

## Phase Status Ledger

| Phase | Status | Items | Done when |
| --- | --- | --- | --- |
| P0: Runtime base alignment | `pending` | NCR0 | Deno family chosen, fork carry-forward decisions recorded, selected family boots with minimal Node smoke |
| P1: Runtime profile foundation | `pending` | NCR1, NCR2, NCR3 | Runtime profiles exist, core Node surface runs, capability-scoped fs/env/resolution path is verified |
| P2: Package and addon enablement | `pending` | NCR4, NCR5 | Staged package flow is documented and verified, addon policy is explicit, `esbuild` works in tooling profile |
| P3: Tooling cutover and product closeout | `pending` | NCR6, NCR7 | Embedded codegen parity is proven or rejected truthfully, docs/product contract match verified reality |

## Roadmap Items

| Item | Status | Hard deps | Completion gate |
| --- | --- | --- | --- |
| NCR0 Version-family alignment spike | `pending` | none | One Deno/v8 family chosen, Node baseline selected, fork carry-forward decisions written, minimal boot smoke passes, rebase/backport/vendor decision recorded |
| NCR1 Compatibility profile and extension wiring | `pending` | NCR0 | `RuntimeProfile` exists as a distinct axis, application profile can be composed with both initial compatibility targets, tooling profile composes with the Node baseline, `NeovexOnly` removal path is explicit, profile vs execution-model ownership is test-covered |
| NCR2 Minimal Node-compatible runtime surface | `pending` | NCR1 | Target built-ins/globals run, unsupported modules fail clearly, supported/unsupported surface matrix exists with fixture-backed claims, existing web-standard application fixtures still pass |
| NCR3 Capability-scoped filesystem, env, and module resolution | `pending` | NCR2 | Capability-scoped fs/env/resolution behavior passes, raw host traversal is impossible, runtime vs staging ownership is documented clearly |
| NCR4 CommonJS, package resolution, and staged npm support | `pending` | NCR3 | ESM/CommonJS/package.json fixtures pass, staged acquisition flow is explicit, application/tooling profile semantics stay aligned, artifact ownership is recorded |
| NCR5 Node-API addons and `esbuild` | `pending` | NCR4 | `esbuild` runs in tooling profile, FFI policy is auditable, unsupported addon classes are recorded explicitly |
| NCR6 Embedded codegen pilot | `pending` | NCR5 | Embedded codegen passes fixture suite, Convex/Cloud Functions codegen parity is evidenced, fallback path remains only as a pilot bridge |
| NCR7 External Node deprecation decision | `pending` | NCR6 | End-to-end onboarding flows are verified, latest upstream versions are rechecked, adapter claims are truthful, docs are updated only if embedded tooling quality is proven |

## Detailed Phases

### NCR0: Version-Family Alignment Spike

Goal: prove the minimum viable upstream Deno dependency family that Neovex can
adopt without destabilizing the locker runtime.

Current facts that must constrain this spike:

- Neovex pins `deno_core 0.395.0` and `v8 147.0.0` at the workspace root via
  patched forks.
- the local `agentstation/deno_core` fork was imported from the published
  `deno_core 0.395.0` crate because the old `denoland/deno_core` repository had
  already been archived.
- newer `deno_core` releases now come from the `denoland/deno` monorepo, while
  `rusty_v8` continues to move independently in `denoland/rusty_v8`.
- this means version-family alignment is not just a `deno_core` question; it is
  a coordinated `deno_core` + `rusty_v8` + Deno-extension-family decision.
- NCR0 also needs to choose:
  - the preserved web-standard isolate compatibility target name and boundary
  - the first supported versioned Node compatibility baseline that Neovex will
    claim and test

Recent `rusty_v8` fork audit findings that should change how NCR0 is executed:

- the local `agentstation/rusty_v8` fork is not just a packaging fork; it
  carries unique locker semantics (`Locker`, `UnenteredIsolate`, compile-fail
  safety tests, and explicit `Enter`/`Exit` ordering) that upstream still does
  not provide.
- however, upstream `rusty_v8` has materially advanced around the same isolate,
  scope, and binding seams that the locker fork modifies. Between upstream
  `v147.0.0` and `v147.4.0`, the most relevant changes are:
  - `#1891` store `IsolateHandle` data in its own allocation instead of keeping
    the older annex-backed model
  - `#1968` cache `IsolateAnnex` pointers in scope structs to avoid repeated
    FFI on slot access
  - `#1960` allow `EscapableHandleScope` construction directly from
    `&mut Isolate`
  - `#1911` enable Linux shared-library-safe V8 TLS mode by default
  - `#1902` fix a memory leak in `Function::get_script_origin`
  - `#1942` add `FunctionTemplate::SetAccessorProperty`
  - `#1959` add external two-byte string constructors
  - `#1965` add `Module::EvaluateForImportDefer` and phased namespace access
  - `#1967` improve string-conversion hot paths with `simdutf`
- this means the uplift plan should **not** preserve the older fork baseline
  mechanically. In particular, the pre-`#1891` `IsolateHandle` design should be
  treated as stale when rebasing forward.
- the current fork diff therefore has two categories:
  - **must-carry product behavior:** locker entry model, `UnenteredIsolate`,
    lock/unlock safety invariants, compile-fail safety tests, and the explicit
    `Enter`-before-use / `Exit`-before-unlock contract
  - **must-reconcile with upstream first:** `IsolateHandle` internals, annex
    and scope caching, FFI/binding fixes, Linux TLS defaults, and other
    non-locker improvements

Recent `deno_core` fork audit findings that should change how NCR0 is executed:

- the local `agentstation/deno_core` fork is a runtime-behavior fork, not just
  a vendored import. Its unique changes are centered on:
  - locker-aware `ManagedIsolate` support
  - public `JsRuntime` locker handoff APIs
  - `reset_main_realm` safety fixes
  - warm-reuse APIs such as `is_warm_reuse_safe()` and
    `reset_request_state()`
- the true upstream source of modern `deno_core` is the `denoland/deno`
  monorepo under `libs/core`, not the archived `denoland/deno_core`
  repository.
- the published `deno_core 0.399.0` crate already moved the dependency family
  forward from the local `0.395.0` import:
  - `v8` moved from `147.0.0` to `147.2.1`
  - `serde_v8` moved from `0.304.0` to `0.308.0`
  - `deno_ops` moved from `0.271.0` to `0.275.0`
  - `sys_traits` and Windows-specific `windows-sys` wiring were added
- the current `denoland/deno` `origin/main` branch has already advanced past
  that published crate family and currently declares:
  - `deno_core 0.400.0`
  - `v8 147.4.0`
  - `deno_runtime 0.255.0`
  - `serde_v8 0.309.0`
- importantly, the overlap between Neovex's fork diff and the upstream
  `0.395.0 -> 0.399.0` delta is narrower than it first appears. The direct
  overlap is concentrated in:
  - `Cargo.toml` / `Cargo.toml.orig`
  - `runtime/jsrealm.rs`
  - `runtime/jsruntime.rs`
  - lockfile churn
  This suggests the deno-core uplift risk is focused in runtime/event-loop
  seams rather than spread across the whole crate.
- the most relevant upstream `deno_core` changes after `0.395.0` are:
  - libuv-compat expansion, especially native `uv_pipe_t` support,
    `NativePipe`, `FdTable`, and new `uv_compat/pipe.rs` and
    `uv_compat/waker.rs`
  - event-loop close-phase support for V8/JS close callbacks
  - non-blocking stdio handling in `op_print`
  - event-loop scheduling changes: a single `run_io()` call per tick, explicit
    `nextTick` draining before close phase, and better microtask/close ordering
  - `uv_loop_t` drop cleanup to prevent worker-memory leaks
  - follow-on monorepo work that continues refining timers and libuv-compatible
    I/O semantics
- this means the uplift plan should **not** preserve the older `0.395.0`
  libuv/event-loop behavior mechanically. The forward port should keep the
  newer upstream uv/event-loop semantics and then reapply Neovex's
  locker/warm-reuse lifecycle hooks on top.

Tasks:

- identify the true upstream source for the target `deno_core` family
  (`denoland/deno`, not the archived `denoland/deno_core` repository)
- inventory the exact Deno crate family that matches the desired
  `deno_core` baseline
- inventory the matching `rusty_v8` family and the exact fork delta Neovex
  would need to carry forward
- choose the first supported versioned Node compatibility baseline and record
  why that baseline matches the selected Deno family and the current adapter
  ecosystem:
  - primary recommendation: `CompatibilityTarget::Node22`
  - explicit secondary adapter verification lane: `Node20`
  - explicitly reject `Node18` and preview `Node24` for the initial product
    claim unless upstream support facts change
- choose and document the first-class non-Node compatibility target name for
  the preserved worker/web-standard application contract:
  - recommended name: `CompatibilityTarget::WebStandardIsolate`
  - record which existing Convex-compatible behaviors define that contract
- decide whether NCR0 should target:
  - the latest published release family (`deno_core 0.399.x` /
    `v8 147.2.x`)
  - or the already-advanced monorepo family (`deno_core 0.400.0` /
    `v8 147.4.0`)
  and record why
- classify the current `rusty_v8` fork diff into:
  - upstream fixes/perf/features we should adopt as-is
  - unique locker behavior we still need to reapply
  - stale fork internals that should be dropped in favor of upstream
- classify the current `deno_core` fork diff into:
  - upstream uv/event-loop/runtime changes we should adopt as-is
  - unique Neovex runtime lifecycle behavior we still need to reapply
  - crate-level changes that only exist to keep the fork pinned to the older
    dependency family
- evaluate a forward rebase of the locker fork onto a release family that has
  matching `deno_node`, `deno_fs`, `deno_permissions`, `node_resolver`, and
  `deno_napi`
- create a throwaway runtime that can instantiate a `JsRuntime` with
  `deno_node` loaded
- prove a minimal `node:path` / `node:url` / `node:buffer` / `node:process`
  smoke runs in that runtime
- record whether the fork uplift should happen before or during NCR1

Exit gate:

- one chosen version family
- one chosen first-class non-Node compatibility target name and contract
- one chosen supported versioned Node compatibility baseline
- written source-of-truth decision for both `deno_core` and `rusty_v8`
- written source-of-truth decision for released-family versus monorepo-main
  targeting on the `deno_core` side
- written carry-forward decision for `rusty_v8` covering:
  - which upstream post-`v147.0.0` fixes are mandatory
  - which locker semantics remain fork-owned
  - which stale fork internals should be dropped instead of preserved
- written carry-forward decision for `deno_core` covering:
  - which upstream uv/event-loop/runtime changes are mandatory
  - which Neovex warm-reuse / locker-handoff behavior remains fork-owned
  - which old `0.395.0` assumptions should be dropped instead of preserved
- written decision on forward rebase vs backport vs vendor
- minimal smoke proving the selected family actually boots

Verification expectations for the `rusty_v8` side of NCR0:

- rerun the locker compile-fail suite after every rebase attempt
- rerun multithreaded locker / `Enter` / `Exit` tests to confirm the ordering
  contract still holds
- rerun cross-thread termination / interrupt tests because upstream
  `IsolateHandle` internals have changed
- rerun Linux shared-library or addon-hosting smoke when evaluating Node-API
  viability because upstream now injects `V8_TLS_USED_IN_LIBRARY` by default

Verification expectations for the `deno_core` side of NCR0:

- rerun realm reset / warm-reuse tests after every rebase attempt because
  `runtime/jsruntime.rs` is the highest-overlap file
- rerun libuv-compat and event-loop smoke because upstream core changed:
  - `run_io` scheduling
  - close-phase behavior
  - non-blocking stdio handling
  - uv-pipe / fd-table semantics
- rerun snapshot/bootstrap smoke because the fork still owns
  `ManagedIsolate`-aware runtime lifecycle behavior

### NCR1: Compatibility Profile And Extension Wiring

Goal: make Node compatibility an explicit runtime profile instead of an ad hoc
extension experiment.

Tasks:

- add a runtime-profile enum and configuration path
- isolate existing Neovex-only bootstrap behavior from profile-specific wiring
- add a Neovex-owned extension composition root for Node compatibility
- define which upstream Deno extensions are loaded in each runtime profile
- keep the existing snapshot and loader lifecycle intact
- record how `NeovexOnly` is removed once `RuntimeProfile::Application` and
  `RuntimeProfile::Tooling` are in place
- record the invariant that `RuntimeProfile::Application` and
  `RuntimeProfile::Tooling` share the same Node-compat semantics and differ
  only in capability envelope / local artifact policy

Exit gate:

- runtime can be constructed in `RuntimeProfile::Application` and
  `RuntimeProfile::Tooling`
- `RuntimeProfile::Application` can be composed with both
  `CompatibilityTarget::WebStandardIsolate` and the named Node baseline
- `RuntimeProfile::Tooling` can be composed with the named Node baseline
- `NeovexOnly` is either deleted or explicitly marked as temporary scaffolding
  with a dated removal owner before NCR2 begins
- the code-level ownership boundary between `RuntimeProfile` and the existing
  `RuntimeExecutionModel` axis is documented and test-covered

### NCR2: Minimal Node-Compatible Runtime Surface

Goal: make common Node-targeted userland code executable inside Neovex without
yet solving the full toolchain problem.

Target surface:

- `node:path`
- `node:url`
- `node:buffer`
- `node:process`
- `node:events`
- `node:util`
- `node:timers`
- relevant globals and host-defined options for Node-mode execution

Tasks:

- wire `deno_node` into the runtime
- define how Node-mode globals are exposed
- add fixture coverage for ESM imports and required globals
- explicitly document what is still unsupported

Exit gate:

- runtime fixtures can import and execute the target modules successfully
- unsupported modules fail clearly rather than silently no-oping
- the verified behavior is recorded against the chosen versioned Node baseline,
  not as a generic unversioned "Node-compatible" claim
- the first checked-in supported/unsupported surface matrix exists and points
  at the exact fixture coverage that justifies each claim
- web-standard application fixtures for the preserved non-Node target remain
  green while the Node surface is added

### NCR3: Capability-Scoped Filesystem, Env, And Module Resolution

Goal: support the first real Node-style host capabilities without breaking the
runtime security model.

Tasks:

- implement a Neovex-scoped `FileSystem` adapter for Deno's fs layer
- define read/write allowlists per runtime profile
- define env allowlists and error semantics
- integrate package.json lookup and Node-style resolution through
  `node_resolver` and related services
- decide the stable path contract for app roots, generated roots, temp roots,
  and any cache roots

Exit gate:

- runtime code can read approved files and resolve package metadata
- denied reads/writes/env access fail with explicit capability errors
- no raw host-root traversal is possible
- the runtime/package-staging ownership split is documented concretely enough
  that acquisition side effects cannot be mistaken for runtime behavior

### NCR4: CommonJS, Package Resolution, And Staged Npm Support

Goal: support realistic Node-targeted packages while keeping acquisition
explicit and reproducible.

Tasks:

- support CommonJS entrypoints where required
- support package exports / conditions / `type` rules
- define a staged package acquisition flow for Neovex CLI commands
- define where staged packages live and how they are invalidated
- decide whether Neovex uses Deno-style cache-only resolution, local
  `node_modules`, or both depending on runtime profile

Recommendation:

- `RuntimeProfile::Application` should execute only staged packages
- `RuntimeProfile::Tooling` may materialize local `node_modules` for
  compatibility with Node-API addons and tool expectations

Exit gate:

- fixture packages covering ESM, CommonJS, `exports`, and `package.json` modes
  resolve correctly
- the acquisition story is documented and explicit
- `RuntimeProfile::Application` resolves the same supported package semantics
  as `RuntimeProfile::Tooling`, even when `RuntimeProfile::Tooling` is allowed
  to materialize a richer local package tree
- the staging flow identifies its source-of-truth artifact set
  (lock/manifest/cache layout) and records which crate owns each piece

### NCR5: Node-API Addons And `esbuild`

Goal: prove whether embedded codegen can run required native addons safely.

Tasks:

- integrate `deno_napi` and the required Deno extension family
- define the FFI permission policy for the tooling runtime profile
- run focused `esbuild` smoke tests in the embedded runtime
- test any required npm lifecycle / install behavior for addon packages
- decide whether some addons remain unsupported in
  `RuntimeProfile::Application` but are allowed in
  `RuntimeProfile::Tooling`

Exit gate:

- `esbuild` executes successfully in `RuntimeProfile::Tooling`
- FFI access remains explicitly gated and auditable
- any `RuntimeProfile::Application` versus `RuntimeProfile::Tooling` addon
  difference is framed as a permission distinction, not a different
  Node-compat language/runtime contract
- unsupported addon classes are recorded explicitly in the checked-in surface
  matrix instead of being left implicit

### NCR6: Embedded Codegen Pilot

Goal: replace the external `node` subprocess for `@neovex/codegen` behind a
guarded pilot path.

Tasks:

- add an alternate codegen runner path in `neovex-bin`
- run `@neovex/codegen` inside the embedded Node-compatible runtime
- preserve the current external-Node runner as a fallback during the pilot
- add fixture coverage for Convex and Cloud Functions codegen
- measure performance, startup cost, cache behavior, and failure ergonomics

Recommended rollout:

- hidden env or unstable flag first
- then explicit `--js-runtime node|embedded`
- then default flip only after parity evidence is recorded

Exit gate:

- embedded codegen passes the existing fixture suite
- failure messages and performance are acceptable
- fallback path remains available only until the default-position decision is
  made
- Convex and Cloud Functions codegen parity evidence is written down with exact
  fixture names or commands rather than a prose-only claim

### NCR7: External Node Deprecation Decision

Goal: decide truthfully whether Neovex can remove Node.js as an onboarding
prerequisite.

Tasks:

- compare embedded codegen reliability against external Node
- verify `neovex init`, `neovex dev`, and `neovex codegen` flows end-to-end
- document any remaining cases that still need an external Node installation
- verify that adapter-facing application execution uses
  `RuntimeProfile::Application` rather than a separate compatibility runtime:
  - Convex-compatible apps
  - Cloud Functions-compatible apps
  - Firebase-facing JS flows that rely on the canonical runtime
- rerun upstream version checks before closeout and record the exact package and
  platform versions verified at that time:
  - latest `convex`
  - latest `firebase-functions`
  - latest `firebase-admin`
  - latest `@google-cloud/functions-framework`
  - latest officially supported Firebase / Cloud Run functions Node runtime
    majors
- only then update README / getting-started / adapter docs

Exit gate:

- if embedded tooling is production-quality, remove the global Node
  prerequisite from the happy path
- if not, keep the docs truthful and retain the external-Node contract
- the final product contract is explicit:
- one canonical Node-compatible Neovex runtime
- `RuntimeProfile::Application` for deployed adapter/app execution
- `RuntimeProfile::Tooling` for local codegen / authoring flows
- no surviving `NeovexOnly` profile
- one named supported Node compatibility baseline documented in product docs
- the final closeout records the exact upstream versions, commands, and fixture
  outcomes used to justify the documentation update

## Verification Strategy

### Runtime Verification

- built-in module fixture tests for the adopted Node subset
- global / CommonJS / package.json behavior tests
- permission-denial tests for fs/env/net/run/ffi
- `HostBridge` regression tests proving Node compatibility did not bypass
  capability enforcement

### Conformance Verification

Neovex should not invent its own Node semantics where upstream suites already
exist.

Add a curated compatibility lane that runs selected upstream-style tests for:

- `node:path`
- `node:url`
- `node:buffer`
- `node:process`
- `node:fs`
- CommonJS / resolver behavior

The goal is not to vendor the entire Node or Deno test universe on day one.
The goal is to adopt focused, high-signal slices that keep claims honest.

### Adapter Verification

The plan also needs adapter-facing evidence, not just raw Node conformance.

Minimum required lanes:

- Convex ordinary-function fixtures that verify the default Convex-runtime /
  web-standard behavior Neovex already claims
- Convex `"use node"` fixtures on the supported Node majors in scope
- Firebase / Cloud Functions HTTPS fixtures on `Node22`, plus the secondary
  `Node20` verification lane
- Firebase / Cloud Functions Firestore-trigger fixtures on `Node22`, plus the
  secondary `Node20` verification lane
- standalone Functions Framework HTTP and CloudEvent fixtures on `Node22`, plus
  the secondary `Node20` verification lane

If any adapter lane fails, the docs and plan closeout must narrow the claim
instead of silently carrying the broader compatibility promise forward.

### Phase Completion Evidence

Every NCR closeout should leave a durable evidence trail in the plan execution
record or linked PR summary.

Minimum evidence per phase:

- exact files or modules changed
- exact commands run
- explicit test counts or named fixture outcomes
- exact upstream versions rechecked when the phase depends on external version
  truth
- explicit unsupported / deferred items discovered during the phase
- the next critical-path question or owner if the phase does not fully close

### Tooling Verification

- embedded `@neovex/codegen` smoke on repo fixtures
- Cloud Functions codegen smoke
- `esbuild` / Node-API addon smoke
- repeated cold / warm startup measurements
- offline / air-gapped reproduction test using staged dependencies only

### Standard Repo Verification

Before any implementation PR opens:

- `cargo fmt --all --check`
- `make check`
- `make clippy`
- focused `cargo test -p neovex-runtime`
- focused `cargo test -p neovex-bin`
- relevant JS workspace verification for codegen-facing changes

## Risks

### R1: Version Drift Between Neovex's Fork And Deno's Compatibility Crates

This is the highest immediate risk. Neovex currently depends on a locker fork of
`deno_core`, while the official compatibility crates move quickly.

Mitigation:

- make NCR0 mandatory
- prefer rebasing the fork to a coherent upstream family over maintaining
  cross-version glue

### R2: Silent Expansion Of Host Privileges

Node compatibility can accidentally become "raw host access with a familiar
API".

Mitigation:

- explicit runtime profiles
- allowlists
- permission-denial tests
- no direct unrestricted `RealFs` or process spawning in a runtime profile

### R3: Tooling-Only Needs Bleed Into Runtime Invocation

`esbuild`, npm package resolution, and native addons may tempt the runtime
surface to absorb dev-tooling concerns.

Mitigation:

- keep `RuntimeProfile::Application` and `RuntimeProfile::Tooling` distinct as
  capability profiles
- keep package acquisition outside invocation
- require both profiles to share the same supported Node-compat semantics so
  the split does not become a second language/runtime surface

### R4: API Churn In `deno_runtime`

Official Deno docs explicitly state that `deno_runtime` is subject to rapid and
breaking API changes.

Mitigation:

- treat `deno_runtime` as a source of extensions and patterns, not as the new
  Neovex runtime owner
- prefer narrower Deno crates where possible

### R5: Claiming More Compatibility Than We Have

This is a trust risk, not just a technical risk.

Mitigation:

- explicit runtime profiles
- published supported subset
- docs updated only after verification

## Exit Criteria

This plan is complete when all of the following are true:

- Neovex has one canonical Node-compatible runtime implementation on top of the
  existing V8 / `deno_core` backend.
- `RuntimeProfile::Application` and `RuntimeProfile::Tooling` are both
  implemented as runtime profiles of that runtime, not as separate runtime
  engines.
- `NeovexOnly` no longer exists as a supported end-state profile.
- one supported versioned Node compatibility baseline is named, tested, and
  documented.
- the named baseline matches current adapter reality: Node 22 as the primary
  product baseline, with explicit adapter verification evidence for Node 20
  where Convex and Firebase / Cloud Functions still support it.
- The implementation reuses upstream Deno compatibility crates for canonical
  behavior where feasible.
- Filesystem, env, network, subprocess, and FFI access remain capability-scoped
  and test-covered.
- A documented staged package-resolution story exists for application and
  tooling runtime profiles.
- the supported Node-compat semantics for `RuntimeProfile::Application` are
  sufficient for Neovex's adapter-facing product contract:
  - Convex-compatible apps
  - Cloud Functions-compatible apps
  - Firebase-facing JS flows that rely on the canonical runtime
- the Convex claim is truthful about both halves of the official Convex
  contract:
  - default Convex-runtime semantics for ordinary functions remain covered
  - Convex `"use node"` action semantics are covered for the supported Node
    majors in scope
- Neovex has evidence for whether embedded codegen can replace the external
  Node.js subprocess.
- Public docs make only claims that the verified profile actually satisfies.

## Execution Log

| Date | Item | Status | Files | Description | Verification |
| --- | --- | --- | --- | --- | --- |
| 2026-04-28 | — | `done` | `docs/plans/node-compatible-runtime-plan.md` | Converted this plan into an explicit control plane modeled after `mongodb-adapter-hardening-plan.md`: added status/ownership/resume rules, phase ledger, roadmap item table, continuation rule, execution-log contract, package-acquisition ownership seam, supported-surface matrix requirement, and adapter verification lanes. No implementation items started yet. | Documentation-only change; no code verification run. |

## Sources

Local inputs:

- `README.md`
- `ARCHITECTURE.md`
- `docs/README.md`
- `docs/plans/README.md`
- `docs/architecture/runtime/adapter-boundary.md`
- `docs/plans/runtime-provider-boundary-hardening-plan.md`
- `docs/plans/research/runtime-file-storage-surface.md`
- `crates/neovex-runtime/src/{host.rs,module_loader.rs}`
- `crates/neovex-runtime/src/runtime/{bootstrap/source.rs,driver/construction.rs}`
- `crates/neovex-bin/src/codegen.rs`
- `packages/codegen/{package.json,src/main.mjs}`

External primary sources reviewed on 2026-04-28:

- Deno Node and npm compatibility:
  https://docs.deno.com/runtime/fundamentals/node/
- Deno Node built-in API coverage:
  https://docs.deno.com/runtime/reference/node_apis/
- Deno permissions model:
  https://docs.deno.com/runtime/manual/getting_started/permissions
- `deno_runtime` crate docs / README:
  https://docs.rs/deno_runtime/latest/deno_runtime/
  https://docs.rs/crate/deno_runtime/latest/source/README.md
- `deno_node` crate docs:
  https://docs.rs/deno_node/latest/deno_node/
- `deno_fs` crate docs:
  https://docs.rs/deno_fs/latest/deno_fs/
- `node_resolver` crate docs:
  https://docs.rs/node_resolver/latest/node_resolver/
- Cloudflare Workers Node compatibility:
  https://developers.cloudflare.com/workers/runtime-apis/nodejs/
- Cloudflare Wrangler install contract:
  https://developers.cloudflare.com/workers/wrangler/install-and-update/
- `unenv` project:
  https://github.com/unjs/unenv
- Bun Node compatibility and runtime positioning:
  https://bun.sh/docs/runtime/nodejs-compat
  https://bun.sh/docs
- Convex runtimes and Node-version support:
  https://docs.convex.dev/functions/runtimes
- Convex latest npm metadata:
  https://registry.npmjs.org/convex/latest
- Firebase Functions get-started Node support statement:
  https://firebase.google.com/docs/functions/get-started
- Firebase Functions runtime-version management:
  https://firebase.google.com/docs/functions/manage-functions
- Firebase latest npm metadata:
  https://registry.npmjs.org/firebase-functions/latest
- Firebase Admin supported environments:
  https://github.com/firebase/firebase-admin-node
- Firebase Admin latest npm metadata:
  https://registry.npmjs.org/firebase-admin/latest
- Google Cloud Run functions runtime matrix:
  https://cloud.google.com/run/docs/runtimes/function-runtimes
- Google Cloud Node.js runtime doc for functions:
  https://cloud.google.com/functions/docs/concepts/nodejs-runtime
- Functions Framework latest npm metadata:
  https://registry.npmjs.org/@google-cloud/functions-framework/latest
