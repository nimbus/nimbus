# Node-Compatible Runtime Plan

Status: done

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

- **Plan status:** `done`
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

This plan is the completed baseline for Neovex's Node-compatible runtime work.
It records the coordinated runtime, codegen, adapter-compatibility, and
external-Node decision for this slice. Do not start a separate broad
Node/runtime compatibility wave without promoting a new active plan that cites
this one as the last completed baseline.

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
- Local fork worktrees for NCR0 and later runtime-family work:
  - canonical Deno-family fork checkout:
    `~/src/github.com/agentstation/deno`
  - matching V8 fork checkout:
    `~/src/github.com/agentstation/rusty_v8`
  - historical delta reference only:
    `~/src/github.com/agentstation/deno_core`
- Local upstream comparison worktree for Deno-family diffing when needed:
  `~/src/github.com/denoland/deno`.
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

### Fork Utilization Model

Moving upstream source-of-truth from the archived `denoland/deno_core`
repository to the `denoland/deno` monorepo does **not** mean Neovex should stop
using its forks. It changes **how those forks are sourced and maintained**.

Recommended model for this plan:

- long-term, prefer an `agentstation/deno` fork of the Deno monorepo as the
  **canonical home** for Neovex's `deno_core`-family changes
- treat the current `agentstation/deno_core` repository as a **historical
  reference source** for discovering the validated Neovex-specific deltas, not
  as the preferred long-term maintenance home
- keep using an `agentstation/rusty_v8` fork as the matching V8 crate patched
  in at the workspace root
- make a clean source-repository break onto an `agentstation/deno` fork at a
  coherent Deno-tested family from the monorepo (currently `v2.7.14`), then
  selectively re-implement only the validated Neovex-specific deltas that
  still matter instead of mechanically transplanting the standalone fork
  history
- treat the Deno monorepo tag as the source-of-truth family for:
  - `deno_core`
  - `deno_runtime`
  - `deno_node`
  - `deno_fs`
  - `deno_permissions`
  - `node_resolver`
  - `deno_package_json`
  - `deno_napi`
  - and the matching `v8` version

Implications:

- `deno_core`-family changes still remain Neovex-owned because Neovex carries
  unique locker and warm-reuse behavior that upstream does not provide
- the plan should **not** preserve old fork history, old patch scaffolding, or
  broad diff shape for its own sake; only the validated must-carry behavior
  should survive the uplift
- `rusty_v8` remains a Neovex-owned fork because Neovex still carries unique
  locker semantics there as well
- the Deno monorepo fork is preferable because future Node-compat work may need
  coordinated visibility into `libs/core`, `runtime`, and `ext/*` even if
  Neovex ultimately patches only a subset of those crates
- the newer Node-compat crates should **not** be forked by default
  unless the plan finds a concrete defect or policy reason that requires it
- instead, Neovex should first consume those crates at the exact versions that
  belong to the selected Deno family and let the `deno_core` / `v8` forks
  provide the custom runtime substrate underneath them
- moving to an `agentstation/deno` fork does **not** mean Neovex must adopt
  `deno_runtime` as its top-level runtime abstraction; it only changes the
  source repository that owns the patched crate family
- moving to an `agentstation/deno` fork also does **not** make Cargo honor
  Deno's `Cargo.lock` automatically. Neovex still needs to preserve the tested
  transitive graph explicitly through its own lockfile and, if necessary,
  tighter version pins or patches during the uplift

How to think about crate roles:

- `deno_core`
  - still a forked core execution crate that Neovex patches in globally
  - but preferably sourced from the `agentstation/deno` monorepo fork rather
    than a standalone `agentstation/deno_core` repo
- `v8`
  - still a forked execution dependency that Neovex patches in globally
- `deno_node`, `deno_fs`, `deno_permissions`, `node_resolver`,
  `deno_package_json`, `deno_napi`
  - preferred as upstream crates from the chosen Deno family, consumed as
    capability/runtime building blocks
- `deno_runtime`
  - useful as a composition reference and as a source of matching extension
    versions
  - **not** the top-level runtime abstraction Neovex should adopt wholesale

Operationally, that means the next uplift should look like:

1. ensure the local canonical Deno-family fork checkout exists at
   `~/src/github.com/agentstation/deno`
   - if absent, create/fetch the `agentstation/deno` fork before doing any
     uplift work
   - if present, refresh it and verify the `v2.7.14` tag/source family
2. start from the clean Deno tag `v2.7.14` in `agentstation/deno`
3. use `~/src/github.com/agentstation/deno_core` only as a historical delta
   reference while auditing what must survive
4. re-implement only the validated must-carry Neovex deltas in that monorepo
   fork:
   - locker-aware isolate ownership and lock handoff
   - warm-reuse lifecycle behavior
   - the regression tests that prove those invariants
5. intentionally drop obsolete standalone-fork maintenance scaffolding and any
   historical workaround behavior that is no longer part of the desired product
   runtime contract
6. update `agentstation/rusty_v8` to the matching V8 family
7. update Neovex workspace dependency versions and `[patch.crates-io]` to point
   `deno_core` (and only any other Deno-family crates we intentionally patch)
   at the monorepo fork tags, while continuing to patch `v8` to
   `agentstation/rusty_v8`
8. preserve the tested transitive graph in Neovex's own `Cargo.lock`, and add
   tighter pins or patches only if the clean uplift still drifts away from the
   Deno-tested family
9. add direct dependencies on the needed Node-compat crates from the same Deno
   family
10. compose those crates inside `neovex-runtime` under Neovex-owned profiles and
   capability policy

That is a different posture from the old standalone-fork mental model, but it
still absolutely uses Neovex's forks.

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

Current checked-in NCR4 contract:

- `RuntimeProfile::Tooling`
  - materializes and uses local `node_modules/` inside the app install root
    (`APP_DIR/` for Convex, `APP_DIR/functions/` for Cloud Functions)
  - records a Neovex-owned dependency fingerprint at
    `.neovex/cache/node/dependency-state.json`
  - invalidates that fingerprint when `package.json`, `package-lock.json`, or
    `npm-shrinkwrap.json` changes, or when declared package manifests are
    missing under `node_modules/`
- `RuntimeProfile::Application`
  - reads only generated artifacts and optional pre-staged sibling
    `node_modules/` trees beneath the generated bundle root
  - never materializes or mutates packages at invocation time
- `neovex-runtime` remains package-acquisition blind; it only consumes
  pre-existing staged artifacts under its path policy

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

1. **Start from a clean Deno release/tag family and re-implement the validated
   Neovex-specific deltas there.**
   This is the recommended path.

2. **Backport `deno_node` and its dependency family down to the current
   `deno_core 0.395` fork.**
   This is high-maintenance and should be rejected unless the forward rebase
   fails for concrete reasons.

3. **Vendor a frozen subset of Deno compatibility crates into Neovex.**
   Only use this if both upstream alignment paths fail and the required surface
   is small enough to justify ownership.

Recommendation: choose option 1 unless a focused spike proves the clean
re-implementation path is infeasible.

## Phase Status Ledger

| Phase | Status | Items | Done when |
| --- | --- | --- | --- |
| P0: Runtime base alignment | `done` | NCR0 | Deno family chosen, fork carry-forward decisions recorded, selected family boots with minimal Node smoke |
| P1: Runtime profile foundation | `done` | NCR1, NCR2, NCR3 | Runtime profiles exist, core Node surface runs, capability-scoped fs/env/resolution path is verified |
| P2: Package and addon enablement | `done` | NCR4, NCR5 | Staged package flow is documented and verified, addon policy is explicit, `esbuild` works in tooling profile |
| P3: Tooling cutover and product closeout | `done` | NCR6, NCR7 | Embedded codegen parity is proven or rejected truthfully, docs/product contract match verified reality |

## Roadmap Items

| Item | Status | Hard deps | Completion gate |
| --- | --- | --- | --- |
| NCR0 Version-family alignment spike | `done` | none | One Deno/v8 family chosen, Node baseline selected, fork carry-forward decisions written, minimal boot smoke passes, rebase/backport/vendor decision recorded |
| NCR1 Compatibility profile and extension wiring | `done` | NCR0 | `RuntimeProfile` exists as a distinct axis, application profile can be composed with both initial compatibility targets, tooling profile composes with the Node baseline, `NeovexOnly` removal path is explicit, profile vs execution-model ownership is test-covered |
| NCR2 Minimal Node-compatible runtime surface | `done` | NCR1 | Target built-ins/globals run, unsupported modules fail clearly, supported/unsupported surface matrix exists with fixture-backed claims, existing web-standard application fixtures still pass |
| NCR3 Capability-scoped filesystem, env, and module resolution | `done` | NCR2 | Capability-scoped fs/env/resolution behavior passes, raw host traversal is impossible, runtime vs staging ownership is documented clearly |
| NCR4 CommonJS, package resolution, and staged npm support | `done` | NCR3 | ESM/CommonJS/package.json fixtures pass, staged acquisition flow is explicit, application/tooling profile semantics stay aligned, artifact ownership is recorded |
| NCR5 Node-API addons and `esbuild` | `done` | NCR4 | `esbuild` runs in tooling profile, FFI policy is auditable, unsupported addon classes are recorded explicitly |
| NCR6 Embedded codegen pilot | `done` | NCR5 | Embedded codegen passes fixture suite, Convex/Cloud Functions codegen parity is evidenced, fallback path remains only as a pilot bridge |
| NCR7 External Node deprecation decision | `done` | NCR6 | End-to-end onboarding flows are verified, latest upstream versions are rechecked, adapter claims are truthful, docs are updated only if embedded tooling quality is proven |

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
- the local `agentstation/deno_core` fork still records
  `upstream = denoland/deno_core`, so NCR0 has to distinguish **historical fork
  provenance** from the **living upstream source of truth**.
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
- the concrete changed-file shape confirms that concentration. The fork's
  22-file / 1559-line diff is dominated by:
  - `runtime/managed_isolate.rs`
  - `runtime/jsruntime.rs`
  - `runtime/setup.rs`
  - `runtime/tests/jsrealm.rs`
  - `tests/locker_runtime.rs`
  - `tests/locker_spike.rs`
  while most remaining files are small wiring or packaging/test-registration
  changes
- current carry-forward classification for the fork should therefore be:
  - **must-carry runtime behavior**
    - `ManagedIsolate` / `LockerIsolate` ownership model
    - `RuntimeOptions::use_locker`
    - public `JsRuntime::{is_v8_lock_held,release_v8_lock,acquire_v8_lock}`
    - the internal `ensure_v8_lock_held()` guard pattern on runtime entrypoints
    - warm-reuse lifecycle APIs (`is_warm_reuse_safe()`,
      `reset_request_state()`) plus their stricter
      `EventLoopPendingState` accounting
  - **must-carry regression proof**
    - warm-reuse regression tests in `runtime/tests/jsrealm.rs`
    - locker interleave/runtime tests in `tests/locker_runtime.rs`
    - early feasibility spike coverage in `tests/locker_spike.rs`
  - **likely drop or rewrite during uplift**
    - `.cargo/config.toml` fork-local environment pinning
    - `Cargo.lock` churn and crate-level patch wiring that only exists to bind
      the older `v147.0.0-locker.2` family
    - any lingering references to the removed `reset_main_realm` /
      `destroy_for_reset` strategy rather than the current warm-reuse model
- NCR0 should interpret that classification as a **selective re-implementation
  brief**, not as a request to preserve or replay the old fork mechanically.
- the true upstream source of modern `deno_core` is the `denoland/deno`
  monorepo under `libs/core`, not the archived `denoland/deno_core`
  repository.
- current crate metadata also points there: published `deno_core` releases now
  link to `denoland/deno`, which means the archived `denoland/deno_core`
  repository is only useful for older-fork archaeology, not as the living
  upstream for forward-port decisions.
- the published `deno_core 0.399.0` crate already moved the dependency family
  forward from the local `0.395.0` import:
  - `v8` moved from `147.0.0` to `147.2.1`
  - `serde_v8` moved from `0.304.0` to `0.308.0`
  - `deno_ops` moved from `0.271.0` to `0.275.0`
  - `sys_traits` and Windows-specific `windows-sys` wiring were added
- as of 2026-04-28, the **current latest published** release family is newer
  still:
  - `deno_core 0.400.0` published at `2026-04-28T12:46:04Z`
  - `deno_node 0.185.0` published at `2026-04-28T12:54:43Z`
  - `deno_runtime 0.255.0` published at `2026-04-28T12:59:22Z`
  - `deno_core 0.400.0` now depends on `v8 147.4.0`,
    `serde_v8 0.309.0`, and `deno_ops 0.276.0`
  - `deno_runtime 0.255.0` depends on `deno_core 0.400.0` and
    `deno_node 0.185.0`
  - `deno_node 0.185.0` also depends on `deno_core 0.400.0`
- docs.rs had not fully caught up to that family during this audit, so NCR0
  should treat crates.io metadata as the release-family source of truth when
  same-day crate publishes disagree with docs.rs `latest` pages.
- the current **checked-out** local `~/src/github.com/denoland/deno` worktree is
  on `main` and is behind `origin/main`, which is why reading `HEAD` in that
  clone showed the older:
  - `deno_core 0.397.0`
  - `deno_runtime 0.252.0`
  - `v8 147.0.0`
- however, the actual upstream git source tag `v2.7.14` at commit
  `2d674b25625bcc367853d00fe86f6e84390f88cb` already matches the newest
  published family:
  - `deno_core 0.400.0`
  - `deno_runtime 0.255.0`
  - `deno_node 0.185.0`
  - `v8 147.4.0`
  - and Deno's own locked RustCrypto prerelease graph
- this means NCR0 **does** have an obvious tested upstream source family now:
  use the Deno tag `v2.7.14` as the canonical `deno_core`/`deno_runtime`/
  `deno_node`/`v8` source-of-truth family rather than treating crates.io alone
  as the source-of-truth.
- this means NCR0 must not casually equate "local Deno monorepo main" with
  "latest reusable upstream family." The local checkout is still valuable for
  code archaeology and composition references, but the family-selection
  decision must explicitly choose between:
  - the latest published crates family (`0.400.0` / `0.255.0` / `0.185.0`)
  - the currently mirrored monorepo checkout
  - a refreshed monorepo checkout or specific source tag if we decide to move
    beyond what is already available locally
- focused downstream boot smoke has now shown that **published-family recency
  is not enough by itself**:
  - a fresh scratch-crate resolve against the latest published family
    (`deno_runtime 0.255.0`, `deno_core 0.400.0`, `deno_node 0.185.0`) fails
    before runtime boot because Cargo resolves `ed448-goldilocks 0.14.0-pre.12`
    and `pkcs8 0.11.0`, and that combination does not compile
  - a comparison scratch-crate resolve against the previous published family
    (`deno_runtime 0.254.0`, `deno_core 0.399.0`, `deno_node 0.184.0`) fails
    the same way for the same `ed448-goldilocks 0.14.0-pre.12` /
    `pkcs8 0.11.0` reason
  - Deno's own workspace still carries exact crypto versions that avoid this
    drift (`ed448-goldilocks 0.14.0-pre.10`, `ed448 0.5.0-rc.5`,
    `pkcs8 0.11.0-rc.11`, and related prerelease companions in the local
    `Cargo.lock`)
  - this makes a **bare crates.io fresh resolve** an untrustworthy NCR0 target
    even when the published family is the newest one
- the published `deno_crypto 0.261.0` and `deno_node_crypto 0.17.0` manifests
  still depend on `ed448-goldilocks = "0.14.0-pre.10"`, which allows Cargo to
  float to newer RustCrypto prereleases during downstream resolution. NCR0 must
  therefore treat the Deno workspace lock / exact transitive graph as part of
  the effective compatibility family, not as incidental implementation detail.
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
- focused downstream smoke has now disproved the simpler "just use the latest
  published crates" recommendation. The forward target should still move
  toward the newest `v8 147.4.x` / `deno_core 0.400.x` family, but **only**
  through one of these reproducible paths:
  - a refreshed Deno monorepo source family whose exact workspace lock and
    source tree can be mirrored into Neovex's fork uplift
  - or a published-crate uplift paired with an explicit Neovex-owned lock /
    patch strategy that pins the same crypto prerelease graph Deno itself
    requires
- until one of those two paths is proven, the stale local `0.397.0` checkout
  remains useful for code archaeology, but neither the bare `0.399` family nor
  the bare `0.400` family is a trustworthy implementation baseline.
- current NCR0 recommendation after the reproducibility spike:
  - prefer the upstream Deno tag `v2.7.14` plus its exact lock graph as the
    canonical uplift path
  - treat published crates plus explicit Neovex transitive overrides as a
    fallback only if Neovex intentionally chooses to diverge from the tested
    Deno tag family
- a first NCR0 smoke attempt run from the local `denoland/deno` workspace root
  on this machine did not reach `deno_node` execution because the repo-local
  `.cargo/config.toml` injects `-fuse-ld=lld` for `aarch64-apple-darwin`, and
  this machine currently does not have `lld` installed. Any remaining NCR0
  boot smoke should therefore either:
  - run from a scratch crate outside the Deno workspace config
  - install `lld`
  - or use another environment where the monorepo's linker contract already
    holds

Current focus:

- NCR0 is now complete.
- the selected uplift baseline is the upstream Deno tag `v2.7.14`, not a bare
  crates.io fresh resolve.
- the successful minimal Node smoke only passed when the runtime was built
  against the exact Deno workspace lock plus the upstream bootstrap shape.
  Fresh scratch-crate resolves outside that lock continued to drift into the
  RustCrypto prerelease incompatibilities already recorded above.
- the local `~/src/github.com/agentstation/deno` checkout should therefore be
  treated as the canonical Deno-family fork worktree for the uplift, but all
  NCR1+ implementation work must continue to preserve the "exact tested family
  first, deliberate divergence only with evidence" rule.

Tasks:

Completed NCR0 outcomes:

- canonical upstream source: `denoland/deno`
- canonical uplift family: Deno tag `v2.7.14`
- canonical Node baseline: `CompatibilityTarget::Node22`
- required secondary adapter verification lane: `Node20`
- preserved non-Node application contract:
  `CompatibilityTarget::WebStandardIsolate`
- fork strategy: clean break onto `agentstation/deno` plus selective
  re-implementation of validated Neovex deltas; keep `agentstation/rusty_v8`
  aligned separately
- reproducibility rule: use the Deno-tested workspace lock as the default
  family contract; treat published crates plus Neovex-owned overrides as a
  fallback only if Neovex intentionally diverges
  - or published crates plus an explicit Neovex-owned override/pinning policy
    for RustCrypto prerelease drift
  and record why a bare crates.io fresh resolve is or is not acceptable
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
- if the chosen family depends on exact transitive pins beyond what a fresh
  crates.io resolve produces, the lock/override mechanism is written down as
  part of the NCR0 decision and the smoke uses that mechanism explicitly

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

Landed runtime-path contract from NCR3:

- `RuntimeProfile::Application` uses the generated bundle directory as
  `process.cwd()` and may only read files under that generated root.
- `RuntimeProfile::Application` may not write local files at runtime.
- `RuntimeProfile::Tooling` uses the app root as `process.cwd()`.
- `RuntimeProfile::Tooling` may read from `app_root`, `generated_root`,
  `.neovex/tmp`, and `.neovex/cache`.
- `RuntimeProfile::Tooling` may write only to `generated_root`,
  `.neovex/tmp`, and `.neovex/cache`.
- runtime invocation may read pre-existing staged package artifacts from those
  roots, but package acquisition/materialization remains CLI-owned behavior.

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

Chosen execution route:

- NCR4 should adopt the Deno-family `node_resolver`, `deno_package_json`, and
  CJS analysis machinery from `agentstation/deno` rather than growing the
  bespoke `RestrictedModuleLoader` into a second resolution stack.
- NCR3 intentionally landed only the minimal local ESM package path needed to
  prove scoped roots, `package.json` reading, and runtime-vs-staging
  separation.
- The remaining NCR4 work (`exports` conditions, CommonJS/package-type
  semantics, and a canonical staged-acquisition story) would duplicate Deno's
  already-maintained resolver family if implemented ad hoc on top of the old
  `deno_core 0.395` loader.
- The chosen unblock path is to move Neovex's dependency family forward to the
  selected `agentstation/deno` monorepo baseline first.
- Critical-path implementation order:
  1. re-land the validated locker semantics onto
     `~/src/github.com/agentstation/rusty_v8` at the matching `v147.4.0`
     family. Status: done via `locker-v147.4.0` and release
     `v147.4.0-locker.1`.
  2. re-implement the validated `deno_core` runtime deltas in
     `~/src/github.com/agentstation/deno` at `v2.7.14`. Status: done on
     `origin/locker-v2.7.14` at release `v2.7.14-locker.4` (`a2cc5bfdc`),
     with local `deno_core` compile proof plus focused locker and warm-reuse
     regression lanes green.
  3. update Neovex workspace dependency patches to the new family. Status:
     done locally in `neovex`; the workspace now points at `deno_core 0.400`
     via `agentstation/deno` release `v2.7.14-locker.4`, and
     `neovex-runtime` compile plus the focused NCR2/NCR3 runtime lanes are
     green on that family.
  4. resume NCR4 resolver/package work on top of the uplifted family. Status:
     active next step.
- Historical-fork audit note: the old `agentstation/deno_core`
  `locker-v0.395` line was 11 commits ahead of upstream, but that delta is now
  accounted for cleanly in the monorepo fork:
  - validated locker/warm-reuse runtime behavior was re-implemented in
    `agentstation/deno`
  - the old `reset_main_realm` / `destroy_for_reset` path had already been
    intentionally retired in the historical fork and stays retired
  - import-history and retag plumbing from the standalone fork were not
    mechanically carried forward
  This makes `agentstation/deno` the canonical released `deno_core`-family
  fork going forward.

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

Working note:

- treat the checked-in `esbuild` package as a broader Node-extension target,
  not just a Node-API target. Its current entrypoint loads CommonJS and then
  spawns a staged platform binary via `child_process`, which also pulls in
  `node:path`, `node:fs`, `node:os`, `node:crypto`, `node:tty`, and optional
  `node:worker_threads` behavior.
- the bootstrap/resolver substrate is now materially ahead of the original
  NCR5 starting point: `node:path` imports, staged CommonJS package loading,
  scoped `node:fs/promises` reads, and tooling-profile writes inside approved
  roots are all verified. The current `esbuild` blocker is narrower and more
  honest: Node's subprocess path still inherits env/run expectations that the
  tooling profile does not yet satisfy, so the remaining work is around
  subprocess/env/ffi policy rather than generic builtin imports.
- expect the next canonical implementation step to touch snapshot/context
  setup, not just module resolution. Proper `deno_node` adoption in the
  Deno family uses a dedicated Node VM context
  (`deno_node::init_global_template`, `deno_node::create_v8_context`,
  `deno_node::VM_CONTEXT_INDEX`) while the current Neovex snapshot path only
  snapshots `runtime_extension()` into the default context.
- keep the Deno-family provenance coherent while doing this work. `deno_core`,
  `deno_permissions`, `deno_resolver`, `node_resolver`, and
  `deno_package_json` should ride the same `agentstation/deno`
  `v2.7.14-locker.4` release line so the embedded-runtime contract is not
  assembled from mixed upstream families.

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
| 2026-04-28 | — | `done` | `AGENTS.md`, `docs/plans/node-compatible-runtime-plan.md` | Declared `node-compatible-runtime-plan.md` as the active runtime-compatibility control plan in `AGENTS.md`, routed future agents to the local `~/src/github.com/agentstation/deno_core`, `~/src/github.com/agentstation/rusty_v8`, and `~/src/github.com/denoland/deno` worktrees, and added those worktrees to the plan's canonical inputs. | Documentation-only change; no code verification run. |
| 2026-04-28 | NCR0 | `in_progress` | `docs/plans/node-compatible-runtime-plan.md` | Started the version-family alignment spike. Audited local fork provenance and refreshed upstream-family facts: `agentstation/deno_core` still has a historical `denoland/deno_core` remote, but its `0.395.0` import commit came from the published crate because living upstream moved to `denoland/deno`. Recorded that the local `~/src/github.com/denoland/deno` checkout is currently `origin/main` / `v2.7.14` (`deno_core 0.397.0`, `deno_runtime 0.252.0`, `v8 147.0.0`) while the latest published crates are newer (`deno_core 0.399.0`, `deno_runtime 0.254.0`, `v8 147.4.0`, with `deno_core 0.399.0` depending on `v8 147.2.1`). Also recorded the first monorepo-root smoke blocker: local Deno builds on this machine currently fail before `deno_node` execution because the repo-local macOS config injects `-fuse-ld=lld`, and `lld` is not installed. | `git status --short` in `neovex`, `deno_core`, `rusty_v8`, and `deno`; `git remote -v` and `git show --stat --summary 4aae4d9` in `agentstation/deno_core`; `git fetch --tags origin` plus `git tag -l 'v2.*' | tail -n 20`, `git branch -r --contains $(git rev-list -n 1 v2.7.14)`, `git log --oneline --decorate -n 8 -- libs/core/Cargo.toml`, and `rg` version checks in `denoland/deno`; docs.rs metadata for `deno_core`, `deno_runtime`, and `v8`; GitHub archive banner for `denoland/deno_core`; attempted `cargo +1.92.0-aarch64-apple-darwin test -p deno_node --lib -- --list` in `denoland/deno` failed with `clang: error: invalid linker name in argument '-fuse-ld=lld'`; verified `~/src/github.com/denoland/deno/.cargo/config.toml`, `which ld.lld`, and `which lld`. |
| 2026-04-28 | NCR0 | `in_progress` | `docs/plans/node-compatible-runtime-plan.md` | Refreshed the release-family recommendation again after same-day crates.io publishes landed. Verified that the coordinated family now available on crates.io is `deno_core 0.400.0`, `deno_node 0.185.0`, and `deno_runtime 0.255.0`, all published within minutes of each other and all pointing to `https://github.com/denoland/deno`. Recorded that `deno_core 0.400.0` now depends on `v8 147.4.0`, which makes the latest published family a better default NCR0 target than the older `0.399/0.254/0.184` set when choosing the forward-port baseline. | `curl -i -sS https://crates.io/api/v1/crates/deno_core/0.400.0`; `curl -i -sS https://crates.io/api/v1/crates/deno_core/0.400.0/dependencies`; `curl -i -sS https://crates.io/api/v1/crates/deno_runtime`; `curl -i -sS https://crates.io/api/v1/crates/deno_runtime/0.255.0/dependencies`; `curl -i -sS https://crates.io/api/v1/crates/deno_node`; `curl -i -sS https://crates.io/api/v1/crates/deno_node/0.185.0/dependencies`. |
| 2026-04-28 | NCR0 | `in_progress` | `docs/plans/node-compatible-runtime-plan.md` | Tightened NCR0 from "latest family exists" to "latest family is reproducible." A clean scratch-crate smoke against the latest published family (`deno_runtime 0.255.0`, `deno_core 0.400.0`, `deno_node 0.185.0`) fails before runtime boot because Cargo resolves newer RustCrypto prereleases than Deno's own workspace lock (`ed448-goldilocks 0.14.0-pre.12` plus `pkcs8 0.11.0` instead of Deno's locked `ed448-goldilocks 0.14.0-pre.10` plus `pkcs8 0.11.0-rc.11`). A comparison scratch-crate smoke against the previous published family (`deno_runtime 0.254.0`, `deno_core 0.399.0`, `deno_node 0.184.0`) fails the same way, so the real NCR0 decision is now "refreshed Deno source family / exact lock graph" versus "published crates plus explicit Neovex override strategy," not just "0.399 vs 0.400" in the abstract. | Scratch crate `/private/tmp/neovex-deno400-smoke.EM98dB`: `cargo +1.92.0-aarch64-apple-darwin tree -i ed448-goldilocks`; `cargo +1.92.0-aarch64-apple-darwin tree -i pkcs8@0.11.0`; `cargo +1.92.0-aarch64-apple-darwin run` failed in `ed448-goldilocks 0.14.0-pre.12` with `pkcs8::Error::KeyMalformed` constructor/type errors; inspected `/Users/jack/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/ed448-goldilocks-0.14.0-pre.12/Cargo.toml` and `src/sign/signing_key.rs`; compared with `~/src/github.com/denoland/deno/Cargo.lock` and `Cargo.toml`, plus downloaded `/Users/jack/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/deno_crypto-0.261.0/Cargo.toml` and `deno_node_crypto-0.17.0/Cargo.toml`; comparison crate `/private/tmp/neovex-deno399-smoke.DqQRPY`: `cargo +1.92.0-aarch64-apple-darwin run` resolved `deno_runtime 0.254.0` / `deno_core 0.399.0` / `deno_node 0.184.0` and failed with the same `ed448-goldilocks 0.14.0-pre.12` compile error. |
| 2026-04-28 | NCR0 | `in_progress` | `docs/plans/node-compatible-runtime-plan.md` | Refined the `deno_core` carry-forward map so the uplift stays surgical. The fork is now recorded as a runtime-behavior fork concentrated in locker-aware isolate ownership (`runtime/managed_isolate.rs`, `runtime/setup.rs`, `runtime/jsruntime.rs`) and warm-reuse lifecycle behavior (`is_warm_reuse_safe()`, `reset_request_state()`) backed by explicit regression suites, while local cargo-env pinning and old-family patch wiring are treated as disposable uplift scaffolding rather than behavior to preserve. | `git diff --stat 4aae4d9..HEAD`; `git diff --name-only 4aae4d9..HEAD`; `git log --oneline --decorate 4aae4d9..HEAD`; `rg -n "reset_request_state|is_warm_reuse_safe|ManagedIsolate|locker|destroy_for_reset|reset_main_realm" runtime tests`; `sed -n '1,260p' runtime/managed_isolate.rs`; `sed -n '200,280p' runtime/setup.rs`; `sed -n '1788,1855p' runtime/jsruntime.rs`; `sed -n '3250,3385p' runtime/jsruntime.rs`; `sed -n '1,260p' tests/locker_runtime.rs`; `sed -n '1,260p' tests/locker_spike.rs`; `git diff 4aae4d9..HEAD -- Cargo.toml Cargo.toml.orig .cargo/config.toml runtime/setup.rs runtime/managed_isolate.rs runtime/jsruntime.rs runtime/tests/jsrealm.rs tests/locker_runtime.rs tests/locker_spike.rs` in `~/src/github.com/agentstation/deno_core`. |
| 2026-04-28 | NCR0 | `in_progress` | `docs/plans/node-compatible-runtime-plan.md` | Corrected the NCR0 source-of-truth story after reconciling the Deno git tags with the local checkout state. The local `denoland/deno` worktree was simply behind `origin`, which is why reading `HEAD` showed `deno_core 0.397.0`; inspecting the actual upstream tag `v2.7.14` at commit `2d674b25625bcc367853d00fe86f6e84390f88cb` shows the coherent tested family Neovex wants: `deno_core 0.400.0`, `deno_runtime 0.255.0`, `deno_node 0.185.0`, `v8 147.4.0`, and Deno's own locked RustCrypto prerelease graph. This means the canonical recommendation is now sharper: rebase the forks onto the Deno tag `v2.7.14`, not onto a bare crates.io fresh resolve. | `git rev-parse v2.7.14`; `git show v2.7.12:libs/core/Cargo.toml`; `git show v2.7.12:runtime/Cargo.toml`; `git show v2.7.12:Cargo.toml`; `git show v2.7.12:Cargo.lock`; `git show v2.7.14:libs/core/Cargo.toml`; `git show v2.7.14:runtime/Cargo.toml`; `git show v2.7.14:Cargo.toml`; `git show v2.7.14:Cargo.lock`; `git show 2d674b25625bcc367853d00fe86f6e84390f88cb:libs/core/Cargo.toml`; `git show HEAD:libs/core/Cargo.toml`; `git status --short --branch` in `~/src/github.com/denoland/deno`; GitHub releases page for `denoland/deno` confirming the stable release train. |
| 2026-04-28 | NCR0 | `in_progress` | `docs/plans/node-compatible-runtime-plan.md` | Clarified the fork utilization model so future agents do not misread the monorepo move as "stop using our fork." The plan now explicitly says Neovex should continue to patch in its `agentstation/deno_core` and `agentstation/rusty_v8` forks, but source and rebase them from the coherent Deno monorepo family (`v2.7.14`) while consuming the newer Node-compat crates (`deno_node`, `deno_fs`, `deno_permissions`, `node_resolver`, `deno_package_json`, `deno_napi`) from that same family as upstream building blocks. It also makes explicit that `deno_runtime` stays a parts/composition source, not the new top-level runtime owner. | Read `Cargo.toml` in `neovex` to confirm current `deno_core`/`v8` `[patch.crates-io]` usage; reviewed and updated the plan sections covering reuse, fork strategy, and NCR0 source-of-truth guidance. |
| 2026-04-28 | NCR0 | `in_progress` | `docs/plans/node-compatible-runtime-plan.md` | Refined the recommendation further: the long-term canonical home for Neovex's `deno_core`-family changes should be an `agentstation/deno` monorepo fork, not the standalone `agentstation/deno_core` repo. The standalone fork should now be treated as the historical porting source. The plan also records the critical caveat that moving to an `agentstation/deno` fork does not make Cargo honor Deno's `Cargo.lock`; Neovex still needs to preserve the tested transitive graph explicitly in its own lockfile and pins during uplift. | Reviewed the updated fork-utilization section against Neovex's current `[patch.crates-io]` usage in `Cargo.toml` and the NCR0 evidence already captured for the `v2.7.14` family. |
| 2026-04-28 | NCR0 | `in_progress` | `docs/plans/node-compatible-runtime-plan.md` | Tightened the plan from "port the fork" to "clean break plus selective re-implementation." The canonical uplift path is now explicitly: start from `agentstation/deno` at `v2.7.14`, re-implement only the validated must-carry Neovex deltas (locker ownership/handoff, warm-reuse lifecycle, and their regression proof), and intentionally drop obsolete standalone-fork scaffolding rather than transplanting the old diff wholesale. | Reviewed and updated the fork-utilization model, version-alignment gate, and deno-core carry-forward guidance in the plan. Documentation-only change; no code verification run. |
| 2026-04-28 | NCR0 | `in_progress` | `docs/plans/node-compatible-runtime-plan.md` | Reviewed the plan for workflow cohesion after naming `~/src/github.com/agentstation/deno` as the intended local monorepo fork checkout. Tightened the canonical-inputs section so `agentstation/deno` is the primary Deno-family worktree, demoted `agentstation/deno_core` to historical-reference-only, updated the current-focus wording, and made the operational workflow start by creating or refreshing the local `agentstation/deno` checkout before any uplift work. | Checked the current worktree state in `neovex`, confirmed `~/src/github.com/agentstation/deno` is not present locally yet with `ls /Users/jack/src/github.com/agentstation`, and updated the plan accordingly. Documentation-only change; no code verification run. |
| 2026-04-28 | NCR0 | `done` | `docs/plans/node-compatible-runtime-plan.md` | Established the local `~/src/github.com/agentstation/deno` checkout from the existing `denoland/deno` worktree, verified that the canonical uplift family is the Deno tag `v2.7.14`, and proved the key reproducibility rule end-to-end: fresh scratch-crate resolves against the same source family still drift into incompatible RustCrypto prereleases, while the exact Deno workspace lock plus the real upstream bootstrap shape successfully boot a Node-enabled runtime and resolve `node:path`. Also recorded the remaining fork-topology caveat: the new local checkout currently has a local-clone `origin`, so the GitHub remote topology still needs deliberate setup later even though the canonical local worktree now exists. | `git clone /Users/jack/src/github.com/denoland/deno /Users/jack/src/github.com/agentstation/deno`; `git remote -v`, `git rev-parse --short HEAD`, `git rev-parse --short v2.7.14^{commit}`, and `git show v2.7.14:Cargo.toml` in `~/src/github.com/agentstation/deno`; created temporary tag-pinned checkout `/private/tmp/agentstation-deno-v2.7.14`; verified Deno lock entries for `ed448`, `ed448-goldilocks`, and `pkcs8`; created temporary exact-lock smoke workspace member `tools/neovex_ncr0_smoke`; patched temporary `.cargo/config.toml` to remove the local `lld` requirement; `cargo run -p neovex_ncr0_smoke` in `/private/tmp/agentstation-deno-v2.7.14` succeeded with output `node smoke ok: macos aarch64`; temporary scratch-crate attempts outside the workspace lock continued to fail in `ed448-goldilocks 0.14.0-pre.12` / `pkcs8 0.11.0` drift, confirming the "exact tested family first" rule. |
| 2026-04-28 | NCR1 | `done` | `crates/neovex-runtime/src/limits.rs`, `crates/neovex-runtime/src/lib.rs`, `crates/neovex-server/src/protocol.rs`, `crates/neovex-server/src/http/metadata.rs`, `docs/plans/node-compatible-runtime-plan.md` | Added `RuntimeCompatibilityTarget` and `RuntimeProfile` as first-class runtime-contract axes distinct from `RuntimeExecutionModel`, set the product default contract to `Application + WebStandardIsolate`, exposed the new axes in runtime diagnostics, added explicit composition helpers for `Application + WebStandardIsolate`, `Application + Node22`, and `Tooling + Node22`, and enforced that the tooling profile currently composes only with the named Node baseline. Added focused tests proving the profile axis is independent from the scheduling axis and moved the control plane forward to NCR2. | `cargo test -p neovex-runtime limits::` → 3 passed, 0 failed; `cargo fmt --all --check` initially reported formatting drift only in `crates/neovex-runtime/src/limits.rs`; `cargo fmt --all`; `cargo fmt --all --check` → clean; `make clippy` → clean. |
| 2026-04-28 | NCR2 | `in_progress` | `docs/plans/node-compatible-runtime-plan.md` | Advanced control to NCR2 and began runtime-surface reconnaissance in `neovex-runtime`. Confirmed that the current public runtime contract is still carried primarily by `RuntimeLimits`, diagnostics, and the existing bootstrap/runtime seams, and that the next implementation work should happen in the runtime bootstrap surface rather than by overloading the scheduling model. | `rg -n "RuntimeExecutionModel|struct Runtime|enum Runtime|Profile|CompatibilityTarget|node_compat|HostBridge" crates/neovex-runtime -g '*.rs'`; `sed -n` reads of `crates/neovex-runtime/src/limits.rs`, `crates/neovex-runtime/src/runtime.rs`, `crates/neovex-runtime/src/runtime/facade.rs`, `crates/neovex-runtime/src/lib.rs`, `crates/neovex-server/src/protocol.rs`, and `crates/neovex-server/src/http/metadata.rs`. |
| 2026-04-28 | NCR2 | `done` | `crates/neovex-runtime/src/runtime/bootstrap/state.rs`, `crates/neovex-runtime/src/runtime/bootstrap/ops.rs`, `crates/neovex-runtime/src/runtime/bootstrap/ops/shared.rs`, `crates/neovex-runtime/src/runtime/bootstrap/source.rs`, `crates/neovex-runtime/src/module_loader.rs`, `crates/neovex-runtime/src/runtime/driver/construction.rs`, `crates/neovex-runtime/src/runtime/tests/basic_invocation.rs`, `docs/architecture/runtime/node-compat-surface-matrix.md`, `docs/plans/node-compatible-runtime-plan.md` | Added the first truthful Node22 runtime surface on top of the existing Neovex bootstrap. Runtime state now carries `RuntimeProfile` and `RuntimeCompatibilityTarget`; post-bootstrap finalization installs minimal Node globals only for `Node22` (`globalThis.global`, `process.version`, `process.versions.node`); `node:` imports now fail explicitly with target-aware error messages instead of generic bundle-loader errors; and the checked-in surface matrix records the current narrow support boundary. Added fixture-backed tests for the new Node22 globals and for explicit `node:` import rejection, while also proving the existing web-standard application fixtures still pass. | `cargo test -p neovex-runtime target_` → 3 passed, 0 failed; `cargo test -p neovex-runtime runtime_loads_bundle_and_invokes_host_bridge` → 1 passed, 0 failed; `cargo test -p neovex-runtime runtime_removes_deno_global_from_bundle_execution` → 1 passed, 0 failed; `cargo fmt --all --check` initially reported formatting drift only in `crates/neovex-runtime/src/runtime/bootstrap/ops/shared.rs`; `cargo fmt --all`; `cargo fmt --all --check` → clean; `make clippy` → clean. |
| 2026-04-28 | NCR3 | `in_progress` | `docs/plans/node-compatible-runtime-plan.md` | Advanced control to NCR3 after closing the first Node22 surface slice. The next work should stay in the runtime bootstrap/module-loader seam and move from target-aware fail-fast behavior into capability-scoped filesystem, env, and module-resolution behavior without collapsing the CLI-owned staging boundary. | Status checkpoint only. NCR3 discovery/implementation not started yet in this session. |
| 2026-04-28 | NCR3 | `done` | `crates/neovex-runtime/src/error.rs`, `crates/neovex-runtime/src/lib.rs`, `crates/neovex-runtime/src/module_loader.rs`, `crates/neovex-runtime/src/runtime/bootstrap/ops.rs`, `crates/neovex-runtime/src/runtime/bootstrap/ops/runtime_local.rs`, `crates/neovex-runtime/src/runtime/bootstrap/ops/shared.rs`, `crates/neovex-runtime/src/runtime/bootstrap/source.rs`, `crates/neovex-runtime/src/runtime/bootstrap/state.rs`, `crates/neovex-runtime/src/runtime/driver/construction.rs`, `crates/neovex-runtime/src/runtime/tests/basic_invocation.rs`, `crates/neovex-runtime/src/runtime_capabilities.rs`, `docs/architecture/runtime/node-compat-surface-matrix.md`, `docs/plans/node-compatible-runtime-plan.md` | Added the first capability-scoped local runtime services on top of Node22 without widening the host boundary. The runtime now installs a bundle-aware root policy and env allowlist into op-state; `process.cwd()` is profile-aware; `process.env` is explicit and allowlist-only; `node:fs/promises` now supports scoped `readFile`/`writeFile`; application profile reads are confined to the generated bundle root and writes are denied; tooling profile reads/writes are confined to `app_root`, `generated_root`, `.neovex/tmp`, and `.neovex/cache`; and local bare-package resolution now reads staged ESM packages from approved `node_modules` trees via `package.json` metadata while still rejecting unsupported Node builtins and deferred CommonJS semantics. Updated the checked-in surface matrix to reflect the new verified contract and recorded that runtime invocation still never acquires packages from the network. | `cargo test -p neovex-runtime basic_invocation::` → 13 passed, 0 failed; `cargo test -p neovex-runtime runtime_capabilities::tests::` → 2 passed, 0 failed; `make clippy` → clean; `cargo fmt --all`; `cargo fmt --all --check` → clean. |
| 2026-04-28 | NCR4 | `in_progress` | `docs/plans/node-compatible-runtime-plan.md` | Advanced control to NCR4 after closing NCR3. The next work should extend the new local package-resolution seam into explicit CommonJS / `package.json` / staged-acquisition behavior, while keeping acquisition ownership in the CLI and keeping `RuntimeProfile::Application` and `RuntimeProfile::Tooling` aligned on language/runtime semantics. | Status checkpoint only. NCR4 discovery/implementation not started yet in this session. |
| 2026-04-28 | NCR4 | `blocked` | `docs/plans/node-compatible-runtime-plan.md` | NCR4 discovery confirmed that the remaining work should be implemented on top of the Deno-family resolver crates, not by expanding the bespoke Neovex loader further. The local `agentstation/deno` monorepo already contains the canonical `node_resolver`, `deno_package_json`, and CJS-analysis layers that own `exports`, conditions, and CommonJS/package-type semantics, while Neovex's own `@neovex/codegen` package already depends on `package.json` `exports` and ESM entrypoint semantics. Stopping here avoids baking a second resolution stack into `neovex-runtime` on the pre-uplift `deno_core 0.395` base. | Blocked pending the chosen next step for resolver-family adoption: either move the workspace to the selected `agentstation/deno` family first, or explicitly vendor/import the matching resolver crates from that family before resuming NCR4 implementation. Discovery evidence: local `agentstation/deno` contains `libs/node_resolver` and `libs/package_json`, plus `libs/resolver/cjs`, and `packages/codegen/package.json` already declares `exports` with `"type": "module"`. |
| 2026-04-28 | NCR4 | `in_progress` | `docs/plans/node-compatible-runtime-plan.md` | Resolved the NCR4 blocker direction. The chosen architectural fix is to move Neovex onto the selected Deno family first, not to keep extending the bespoke pre-uplift loader. The active execution order is now explicit: re-land the validated locker behavior onto `~/src/github.com/agentstation/rusty_v8` at `v147.4.0`, re-implement the validated `deno_core` runtime deltas in `~/src/github.com/agentstation/deno` at `v2.7.14`, then update Neovex workspace patches and resume resolver/package work on the uplifted family. | Control-plane update only. Verified local branch baselines with `git -C ~/src/github.com/agentstation/rusty_v8 status --short --branch`, `git -C ~/src/github.com/agentstation/rusty_v8 log --reverse --oneline v147.0.0..locker-v147`, `git -C ~/src/github.com/agentstation/rusty_v8 show --stat --summary 021b2f4 477bfee 1ea5120 2d87ed6`, and `git -C ~/src/github.com/agentstation/deno status --short --branch`. |
| 2026-04-28 | NCR4 | `in_progress` | `docs/plans/node-compatible-runtime-plan.md` | Completed the first `rusty_v8` uplift checkpoint on top of upstream `v147.4.0`. Re-landed the validated locker commit set on a clean local branch, added the necessary upstream-shape follow-up in `src/scope.rs` so `Locker<'a>` initializes the cached `annex` pointer expected by newer scope structs, cleaned the temporary source-build byproducts, and pushed the result to the canonical `origin/locker-v147.4.0` validation branch. The long-lived `locker-v147` branch should remain the fork's default branch and should only move after `locker-v147.4.0` verifies cleanly; the new `v147.4.0-locker.*` release tag should be cut from that verified commit. | `git -C ~/src/github.com/agentstation/rusty_v8 cherry-pick 021b2f4 477bfee 1ea5120 2d87ed6`; `git -C ~/src/github.com/agentstation/rusty_v8 diff -- src/scope.rs`; `git -C ~/src/github.com/agentstation/rusty_v8 clean -nd gen`; `git -C ~/src/github.com/agentstation/rusty_v8 submodule update --checkout v8`; `git -C ~/src/github.com/agentstation/rusty_v8 clean -fd gen`; `git -C ~/src/github.com/agentstation/rusty_v8 add src/scope.rs`; `git -C ~/src/github.com/agentstation/rusty_v8 commit -m "fix(locker): initialize HandleScope annex in Locker scope"`; `git -C ~/src/github.com/agentstation/rusty_v8 branch locker-v147.4.0 e5e4a8cd76de1dd86266ecf9f584cadc5085d847`; `git -C ~/src/github.com/agentstation/rusty_v8 push -u origin locker-v147.4.0`; `git -C ~/src/github.com/agentstation/rusty_v8 branch -d neovex-locker-v147.4.0`; `git -C ~/src/github.com/agentstation/rusty_v8 push origin :neovex-locker-v147.4.0`; focused local verification remains incomplete because the source-build path triggered heavy `clang` activity and the prebuilt archive path did not yet validate the new `v8__Locker__*` C wrappers. |
| 2026-04-28 | NCR4 | `in_progress` | `docs/plans/node-compatible-runtime-plan.md` | Began the `agentstation/deno` runtime-family uplift on top of `v2.7.14`. Landed the first locker-aware substrate in `libs/core/runtime`: added `managed_isolate.rs`, exposed it from `runtime/mod.rs`, taught `runtime/setup.rs::create_isolate` to return a managed isolate and accept `use_locker`, then rewired `runtime/jsruntime.rs` to store `ManagedIsolate`, expose `RuntimeOptions::use_locker`, and add the first `ensure_v8_lock_held` / lock-guard helpers plus targeted direct-isolate call-site guards. | `git -C ~/src/github.com/agentstation/deno status --short --branch`; read current `libs/core/runtime/{mod.rs,setup.rs,jsruntime.rs}` plus historical `~/src/github.com/agentstation/deno_core` locker equivalents; attempted `cargo check -p deno_core --lib` in `~/src/github.com/agentstation/deno` and it failed before reaching the locker changes because the workspace's macOS target config still injects `-fuse-ld=lld`; attempted a one-shot `CARGO_TARGET_AARCH64_APPLE_DARWIN_RUSTFLAGS=... cargo check -p deno_core --lib`, but Cargo merged that with the existing config and still failed (`clang: error: invalid linker name in argument '-fuse-ld=lld'`). Treat the current blocker as verification-environment setup, not a proven runtime-code regression. |
| 2026-04-28 | NCR4 | `in_progress` | `docs/plans/node-compatible-runtime-plan.md` | Performed a source-only sanity pass on the `agentstation/deno` locker uplift after stopping the stale `rusty_v8` source-build storm. The new managed-isolate files and `jsruntime.rs` edits format and parse cleanly under rustfmt, which gives a truthful intermediate checkpoint even though linker-backed `cargo check` is still blocked by the Deno workspace's macOS `lld` configuration. | `cargo fmt --all --check` in `~/src/github.com/agentstation/deno` initially reported formatting-only diffs for `libs/core/runtime/{mod.rs,setup.rs,jsruntime.rs}`; `cargo fmt --all`; `cargo fmt --all --check` → clean. The earlier `cargo check` attempts remain blocked on `target.aarch64-apple-darwin.rustflags` injecting `-fuse-ld=lld`. |
| 2026-04-28 | NCR4 | `in_progress` | `docs/plans/node-compatible-runtime-plan.md` | Added the first explicit Deno-side behavioral tests for the locker uplift. `libs/core/runtime/tests/locker.rs` now captures the intended `JsRuntime` contract for `use_locker`: basic lock lifecycle, two locker runtimes interleaving on one thread, and a locker runtime coexisting with a standard runtime. This keeps the validation target close to the runtime substrate even before the macOS linker override is solved. | Read historical `~/src/github.com/agentstation/deno_core` `tests/{locker_runtime.rs,locker_spike.rs}` and current `~/src/github.com/agentstation/deno/libs/core/runtime/tests/mod.rs`; added `libs/core/runtime/tests/locker.rs` plus `mod locker;`; `cargo fmt --all --check` in `~/src/github.com/agentstation/deno` → clean after the test addition. Linker-backed test execution is still pending the `-fuse-ld=lld` config workaround. |
| 2026-04-28 | NCR4 | `in_progress` | `docs/plans/node-compatible-runtime-plan.md` | Reconciled the `agentstation/rusty_v8` uplift with the proven `locker-v147` fork release contract before promoting a new release line. Verified via `git range-diff` that `locker-v147.4.0` carries the four original locker commits plus only the intentional `HandleScope` annex follow-up required by upstream `v147.4.0`, restored the simplified 4-target fork CI and the `RUSTY_V8_VERSION` / fork-release download contract, pushed commit `8e8e253`, switched the GitHub default branch to `locker-v147.4.0`, and cut `v147.4.0-locker.1` from that vetted commit so the canonical long-running release build starts from the forked tag instead of a branch-only run. | `git -C ~/src/github.com/agentstation/rusty_v8 log --reverse --oneline v147.0.0..locker-v147`; `git -C ~/src/github.com/agentstation/rusty_v8 log --reverse --oneline v147.4.0..locker-v147.4.0`; `git -C ~/src/github.com/agentstation/rusty_v8 range-diff v147.0.0..locker-v147 v147.4.0..locker-v147.4.0`; `git -C ~/src/github.com/agentstation/rusty_v8 diff locker-v147 -- .github/workflows/ci.yml Cargo.toml build.rs`; `git -C ~/src/github.com/agentstation/rusty_v8 commit -m "build: restore locker fork release contract"`; `git -C ~/src/github.com/agentstation/rusty_v8 push origin locker-v147.4.0`; `gh api repos/agentstation/rusty_v8 -X PATCH -f default_branch=locker-v147.4.0`; `git -C ~/src/github.com/agentstation/rusty_v8 tag -a v147.4.0-locker.1 8e8e253 -m "v147.4.0-locker.1"`; `git -C ~/src/github.com/agentstation/rusty_v8 push origin v147.4.0-locker.1`; `gh run list --repo agentstation/rusty_v8 --limit 8`; `gh run view 25072365549 --repo agentstation/rusty_v8`. The canonical release build now in flight is GitHub Actions run `25072365549` for tag `v147.4.0-locker.1`; the earlier branch-only run `25071782246` was canceled to keep one canonical build in flight. |
| 2026-04-28 | NCR4 | `in_progress` | `AGENTS.md`, `docs/plans/node-compatible-runtime-plan.md`, `~/src/github.com/agentstation/deno/Cargo.toml` | Closed the repo-topology gap the plan had been carrying. `agentstation/rusty_v8` release `v147.4.0-locker.1` is now published and should be treated as the canonical locker line for the Deno-family uplift. Created the real `agentstation/deno` GitHub fork, rewired the local `~/src/github.com/agentstation/deno` checkout so `origin` is the GitHub fork and `upstream` is `denoland/deno`, and repinned the local Deno uplift manifest to the released `v147.4.0-locker.1` tag. Also tightened repo guidance so future agents talk about this correctly: `agentstation/deno` is the canonical fork, while `deno_core` is the crate being edited inside that monorepo and the old standalone `agentstation/deno_core` repo is historical reference only. | `gh release view v147.4.0-locker.1 --repo agentstation/rusty_v8`; `gh repo fork denoland/deno --org agentstation --clone=false`; `git -C ~/src/github.com/agentstation/deno remote -v`; `git -C ~/src/github.com/agentstation/deno remote rename origin local-denoland`; `git -C ~/src/github.com/agentstation/deno remote add upstream https://github.com/denoland/deno.git`; `git -C ~/src/github.com/agentstation/deno remote add origin https://github.com/agentstation/deno.git`; `git -C ~/src/github.com/agentstation/deno remote -v`; `git -C ~/src/github.com/agentstation/deno diff -- Cargo.toml`. A fresh `cargo check -p deno_core --lib` rerun is still pending a clean post-repin compiler pass; the first attempt after repinning was stopped before a compile signal so the remote/topology correction could land first. |
| 2026-04-28 | NCR4 | `in_progress` | `docs/plans/node-compatible-runtime-plan.md` | Aligned the Deno monorepo fork with the established locker branch naming and completed a release-readiness carry-forward audit against the historical `agentstation/deno_core` `locker-v0.395` line. Published `origin/locker-v2.7.14` on `agentstation/deno` so the canonical monorepo fork now follows the same `locker-v*` convention as `agentstation/rusty_v8`. The audit shows the current local `locker-v2.7.14` work covers the first two historical runtime deltas (locker-aware `ManagedIsolate` support plus the public `JsRuntime` lock handoff API) and the `rusty_v8` dependency bump is superseded by the new `v147.4.0-locker.1` tag pin, but the warm-reuse/reset series is still absent from `agentstation/deno`: no `is_warm_reuse_safe()`, no `reset_request_state()`, no reset-boundary regression tests, and no final replacement/removal of the older reset-path assumptions. That means `agentstation/deno` is not release-tag ready yet; the next honest release gate is to re-implement the validated warm-reuse lifecycle contract and its tests on `locker-v2.7.14`, then verify the branch against the released `rusty_v8` line before cutting any `v2.7.14-locker.*` tag. | `git -C ~/src/github.com/agentstation/deno branch -m neovex-node-runtime-v2.7.14 locker-v2.7.14`; `git -C ~/src/github.com/agentstation/deno push -u origin locker-v2.7.14`; `git -C ~/src/github.com/agentstation/deno_core rev-parse upstream/main locker-v0.395`; `git -C ~/src/github.com/agentstation/deno_core log --reverse --oneline upstream/main..locker-v0.395`; `rg -n "reset_request_state|is_warm_reuse_safe|destroy_for_reset|reset_main_realm|EventLoopPendingState|shared_array_buffers" ~/src/github.com/agentstation/deno_core/runtime ~/src/github.com/agentstation/deno_core/tests`; `sed -n '1760,1870p' ~/src/github.com/agentstation/deno_core/runtime/jsruntime.rs`; `sed -n '3230,3395p' ~/src/github.com/agentstation/deno_core/runtime/jsruntime.rs`; `rg -n "reset_request_state|is_warm_reuse_safe|destroy_for_reset|reset_main_realm|use_locker|acquire_v8_lock|release_v8_lock|is_v8_lock_held|ManagedIsolate|shared_array_buffers" ~/src/github.com/agentstation/deno/libs/core/runtime ~/src/github.com/agentstation/deno/libs/core/runtime/tests ~/src/github.com/agentstation/deno/tests`. |
| 2026-04-28 | NCR4 | `in_progress` | `docs/plans/node-compatible-runtime-plan.md`, `~/src/github.com/agentstation/deno/.cargo/config.toml`, `~/src/github.com/agentstation/deno/Cargo.toml`, `~/src/github.com/agentstation/deno/Cargo.lock`, `~/src/github.com/agentstation/deno/libs/core/{modules/map.rs,ops.rs,tasks.rs}`, `~/src/github.com/agentstation/deno/libs/core/runtime/{exception_state.rs,jsruntime.rs,managed_isolate.rs,mod.rs,setup.rs,stats.rs}`, `~/src/github.com/agentstation/deno/libs/core/runtime/tests/{jsrealm.rs,locker.rs,mod.rs}` | Completed the Deno monorepo carry-forward needed to unblock NCR4. The `agentstation/deno` fork now carries the validated locker runtime substrate and the full warm-reuse lifecycle contract from the historical `agentstation/deno_core` fork, including `ManagedIsolate`, `RuntimeOptions::use_locker`, explicit V8 lock handoff helpers, `is_warm_reuse_safe()`, `reset_request_state()`, and the supporting exception/module/task/external-op reset helpers. Also restored the fork-local release wiring expected by this family via `.cargo/config.toml` (`RUSTY_V8_VERSION = "147.4.0-locker.1"` plus `git-fetch-with-cli = true`) and the workspace `[patch.crates-io] v8` tag pin. Verified the branch end-to-end against the released `rusty_v8` line using a single-command-at-a-time isolated Cargo flow, then committed and pushed the result to `origin/locker-v2.7.14` at `84b679af6` (`runtime: port locker lifecycle to deno v2.7.14`). This resolves the Deno-family uplift blocker; the next NCR4 critical-path step is to repoint the Neovex workspace to the pushed `agentstation/deno` family and continue resolver/package work there rather than on the old standalone fork. | `cargo fmt --all`; `cargo fmt --all --check`; isolated compile check in `~/src/github.com/agentstation/deno` with `CARGO_HOME=$(mktemp -d ...)`, `CARGO_NET_OFFLINE=true`, `CARGO_ENCODED_RUSTFLAGS=...`, and `cargo --config 'patch.crates-io.v8.path=\"/Users/jack/src/github.com/agentstation/rusty_v8\"' check -p deno_core --lib` → finished cleanly; same isolated flow `cargo ... test -p deno_core locker_runtime_` → 3 passed, 0 failed; same isolated flow `cargo ... test -p deno_core warm_reuse_` → 8 passed, 0 failed; `git -C ~/src/github.com/agentstation/deno diff --check`; `git -C ~/src/github.com/agentstation/deno commit -m "runtime: port locker lifecycle to deno v2.7.14"`; `git -C ~/src/github.com/agentstation/deno push origin locker-v2.7.14`. |
| 2026-04-28 | NCR4 | `in_progress` | `Cargo.toml`, `Cargo.lock`, `docs/plans/node-compatible-runtime-plan.md` | Repointed the live Neovex workspace to the pushed Deno family. The workspace now declares `deno_core 0.400` and `v8 147.4.0`, patches `deno_core` to `agentstation/deno` commit `84b679af6`, and patches `v8` to `v147.4.0-locker.1`. Using local path overrides for `agentstation/deno` and `agentstation/rusty_v8`, `neovex-runtime` compiles cleanly on the new family and the focused NCR2/NCR3 runtime lanes remain green. This proves the dependency-family uplift is locally integrated enough to move on to actual NCR4 resolver/package work. | `cargo fmt --all --check` → clean; `CARGO_HOME=/tmp/neovex-cargo-home cargo --config 'patch.crates-io.deno_core.path=\"/Users/jack/src/github.com/agentstation/deno/libs/core\"' --config 'patch.crates-io.v8.path=\"/Users/jack/src/github.com/agentstation/rusty_v8\"' check -p neovex-runtime` → finished cleanly; same local-override flow `cargo ... test -p neovex-runtime basic_invocation::` → 13 passed, 0 failed; same local-override flow `cargo ... test -p neovex-runtime runtime_capabilities::tests::` → 2 passed, 0 failed. Focused `cargo clippy -p neovex-runtime --all-targets -- -D warnings` remains a verification follow-up because the workspace's remote `v8` git patch still triggers one-time `rusty_v8` submodule hydration on that lane unless we preseed or redirect that exact source. |
| 2026-04-28 | NCR4 | `in_progress` | `Cargo.toml`, `Cargo.lock`, `docs/plans/node-compatible-runtime-plan.md`, `~/src/github.com/agentstation/deno/Cargo.lock` | Closed the Deno fork release loop and promoted the monorepo fork to canonical status. Verified `agentstation/deno` directly against the released `agentstation/rusty_v8` tag with a single isolated `cargo check -p deno_core --lib`, which confirmed the remaining `Cargo.lock` refresh was real release state rather than local churn. Committed that lock refresh in `agentstation/deno` as `13ca08223` (`build: refresh lock for locker v8 tag`), pushed `origin/locker-v2.7.14`, cut and published the `v2.7.14-locker.1` GitHub release, and switched the repo default branch to `locker-v2.7.14`. Also finalized the historical audit conclusion: the old standalone `agentstation/deno_core` fork's 11-commit delta is fully accounted for by the monorepo fork, with substantive locker/warm-reuse behavior carried forward and intentionally retired reset-path history left behind. The Neovex workspace now tracks the Deno fork by release tag rather than a temporary commit pin, and the focused Neovex runtime lane remains green on that tag. | In `~/src/github.com/agentstation/deno`: isolated `cargo check -p deno_core --lib` with `CARGO_HOME=$(mktemp -d ...)`, `CARGO_NET_OFFLINE=true`, and `CARGO_ENCODED_RUSTFLAGS=...` → finished cleanly against released `v147.4.0-locker.1`; `git add Cargo.lock`; `git commit -m "build: refresh lock for locker v8 tag"` → `13ca08223`; `git push origin locker-v2.7.14`; `git tag -a v2.7.14-locker.1 -m "v2.7.14-locker.1"`; `git push origin refs/tags/v2.7.14-locker.1`; `gh release create v2.7.14-locker.1 --repo agentstation/deno ...`; `gh repo edit agentstation/deno --default-branch locker-v2.7.14`; `gh repo view agentstation/deno --json defaultBranchRef -q .defaultBranchRef.name` → `locker-v2.7.14`. In `neovex`: `cargo fmt --all --check` → clean; `CARGO_HOME=/tmp/neovex-cargo-home CARGO_NET_OFFLINE=true cargo --config 'patch.crates-io.deno_core.path=\"/Users/jack/src/github.com/agentstation/deno/libs/core\"' --config 'patch.crates-io.v8.path=\"/Users/jack/src/github.com/agentstation/rusty_v8\"' test -p neovex-runtime basic_invocation::` → 13 passed, 0 failed; updated the workspace patch from `agentstation/deno` commit `84b679af6` to release tag `v2.7.14-locker.1`. |
| 2026-04-28 | NCR4 | `in_progress` | `Cargo.toml`, `crates/neovex-runtime/Cargo.toml`, `crates/neovex-runtime/src/module_loader.rs`, `crates/neovex-runtime/src/runtime/tests/basic_invocation.rs`, `docs/plans/node-compatible-runtime-plan.md` | Landed the first real post-uplift NCR4 resolver slice on top of the released Deno family. `RestrictedModuleLoader` no longer hand-parses package `main` / `type` / subpath resolution itself. Instead it now delegates bare package resolution to Deno's `node_resolver` with a Neovex-owned scoped `node_modules` folder resolver that preserves the runtime root boundary, then applies a narrow post-resolution guard that still rejects CommonJS entries until the CJS bridge lands. This immediately upgrades the supported runtime contract to include `package.json` `exports` resolution while keeping the prelaunch boundary honest for unresolved CommonJS semantics. Added regression coverage for both directions: `exports`-based package resolution now passes from scoped `node_modules`, and CommonJS package entries fail clearly with a purpose-built NCR4 error. | `cargo fmt --all` → clean; `cargo fmt --all --check` → clean; network-backed one-time dependency fetch for the new resolver crates: `CARGO_HOME=/tmp/neovex-cargo-home cargo --config 'patch.crates-io.deno_core.path=\"/Users/jack/src/github.com/agentstation/deno/libs/core\"' --config 'patch.crates-io.v8.path=\"/Users/jack/src/github.com/agentstation/rusty_v8\"' test -p neovex-runtime basic_invocation::` downloaded `node_resolver 0.85.0`, `deno_package_json 0.49.0`, `deno_semver 0.9.1`, and related crates, then after one local lifetime fix reran successfully; final offline verifier: `CARGO_HOME=/tmp/neovex-cargo-home CARGO_NET_OFFLINE=true cargo --config 'patch.crates-io.deno_core.path=\"/Users/jack/src/github.com/agentstation/deno/libs/core\"' --config 'patch.crates-io.v8.path=\"/Users/jack/src/github.com/agentstation/rusty_v8\"' test -p neovex-runtime basic_invocation::` → 15 passed, 0 failed. Focused `cargo clippy -p neovex-runtime --all-targets -- -D warnings` is still a verification blocker on the released-source path because Cargo begins hydrating the tagged `agentstation/deno` submodule forest (`deno_lsp_benchdata`, `node_test`, `deno_std`, `wpt`) before it reaches actual lint work. |
| 2026-04-28 | NCR4 | `in_progress` | `Cargo.toml`, `crates/neovex-runtime/Cargo.toml`, `crates/neovex-runtime/src/lib.rs`, `crates/neovex-runtime/src/module_loader.rs`, `crates/neovex-runtime/src/node_compat.rs`, `crates/neovex-runtime/src/runtime/bootstrap/ops.rs`, `crates/neovex-runtime/src/runtime/bootstrap/ops/runtime_local.rs`, `crates/neovex-runtime/src/runtime/tests/basic_invocation.rs`, `docs/architecture/runtime/node-compat-surface-matrix.md`, `docs/plans/node-compatible-runtime-plan.md` | Landed the first verified CommonJS bridge on top of the Deno-family resolver stack. A new shared `node_compat` seam now owns Deno-backed package resolution, package-type classification, and CommonJS-to-ESM translation, and both the ESM loader and the runtime-local `require` path use it instead of growing separate resolution logic. `RestrictedModuleLoader` now translates CommonJS files into ESM wrappers when the Node22 target resolves a staged CommonJS entry, and the runtime now exposes a minimal `node:module` builtin backed by explicit sync ops for `createRequire()` / `Module._load()`, local module caching, staged relative `require(...)`, and JSON `require(...)` inside approved runtime roots. This advances NCR4 from "exports-only ESM packages" to a truthful staged local package contract that includes explicit `.cjs` entries plus implicit `.js` CommonJS fallback semantics, while still keeping the unsupported surface narrow (`require()` of ESM targets and most other builtins remain unsupported). Updated the checked-in node-compat surface matrix to reflect the new verified contract. | `rustfmt crates/neovex-runtime/src/lib.rs crates/neovex-runtime/src/module_loader.rs crates/neovex-runtime/src/node_compat.rs crates/neovex-runtime/src/runtime/bootstrap/ops.rs crates/neovex-runtime/src/runtime/bootstrap/ops/runtime_local.rs crates/neovex-runtime/src/runtime/tests/basic_invocation.rs` → clean; `cargo test -p neovex-runtime basic_invocation::` → 16 passed, 0 failed; `cargo fmt --all --check` → clean; `cargo clippy -p neovex-runtime --all-targets -- -D warnings` → clean. |
| 2026-04-28 | NCR4 | `done` | `README.md`, `crates/neovex-bin/src/node.rs`, `crates/neovex-runtime/src/runtime/tests/basic_invocation.rs`, `docs/architecture/runtime/node-compat-surface-matrix.md`, `docs/operating/cli.md`, `docs/plans/neovex-init-plan.md`, `docs/plans/node-compatible-runtime-plan.md` | Closed NCR4 with the missing staged-acquisition and profile-alignment contract. `neovex-bin` now owns a durable dependency-state record at `.neovex/cache/node/dependency-state.json`, fingerprints `package.json` plus `package-lock.json` / `npm-shrinkwrap.json`, forces reinstall when declared manifests are missing or the recorded fingerprint drifts, and records that runtime invocation remains package-acquisition blind. Added the final tooling-profile package fixture so `RuntimeProfile::Tooling` resolves the same supported ESM/CommonJS/package.json semantics as `RuntimeProfile::Application`, only with a broader approved root set. Updated the README, CLI doc, init-plan contract, surface matrix, and this control plan so the ownership boundary is explicit and checkpointable. | `cargo test -p neovex-runtime basic_invocation::` → 17 passed, 0 failed; `cargo test -p neovex-bin node::` → 14 passed, 0 failed; `cargo fmt --all --check` → clean; `make clippy` → clean. |
| 2026-04-28 | NCR5 | `in_progress` | `docs/plans/node-compatible-runtime-plan.md` | Advanced control to NCR5 after closing NCR4. The next work should stay honest about what `@neovex/codegen` actually needs from the Node-compatible tooling profile: Node-API integration through the Deno-family `deno_napi` extension where required, plus the concrete addon or binary-loading path used by the `esbuild` package and its platform-specific install/runtime contract. Start from the checked-in codegen sources and the canonical `agentstation/deno` fork rather than assuming `esbuild` is a generic Node builtin problem. | Reconnaissance completed from `packages/codegen/package.json`, `packages/codegen/src/main.mjs`, and `~/src/github.com/agentstation/deno/ext/napi/lib.rs`; implementation not started yet in this checkpoint. |
| 2026-04-28 | NCR5 | `in_progress` | `Cargo.lock`, `Cargo.toml`, `crates/neovex-runtime/src/runtime/bootstrap/ops/runtime_local.rs`, `crates/neovex-runtime/src/runtime/bootstrap/state.rs`, `crates/neovex-runtime/src/runtime/tests/basic_invocation.rs`, `crates/neovex-runtime/src/runtime_capabilities.rs`, `docs/architecture/runtime/node-compat-surface-matrix.md`, `docs/plans/node-compatible-runtime-plan.md` | Landed the first canonical NCR5 substrate and tightened the control-plane truth. `neovex-runtime` now builds a Deno `PermissionsContainer` from the existing runtime profile/path contract instead of relying on bespoke path/env checks, and the local fs/env host ops normalize Deno permission denials back into Neovex’s explicit capability language. In parallel, the workspace now patches `deno_permissions`, `deno_resolver`, `node_resolver`, and `deno_package_json` from the same `agentstation/deno` `v2.7.14-locker.1` release that already supplies `deno_core`, so the runtime no longer mixes a forked core with crates.io resolver/permission crates from a different source family. Research against the checked-in `esbuild` package also sharpened the next NCR5 step: the blocking gap is broader Node builtin plus subprocess support (`node:path`, `node:fs`, `node:os`, `node:crypto`, `node:tty`, `node:child_process`, optional `node:worker_threads`) for a staged platform binary, not Node-API alone. Added an explicit staged `esbuild`-dependency-profile fixture so that gap is now encoded in tests instead of left as prose. | `cargo test -p neovex-runtime runtime_capabilities::tests::` → 3 passed, 0 failed; `cargo test -p neovex-runtime tooling_node22_rejects_esbuild_dependency_profile_clearly` → 1 passed, 0 failed; `cargo test -p neovex-runtime basic_invocation::` → 18 passed, 0 failed; `cargo fmt --all` → clean; `cargo fmt --all --check` → clean; `make clippy` → clean; supporting source audit from `node_modules/esbuild/lib/main.js`, `package-lock.json`, `~/src/github.com/agentstation/deno/ext/napi/lib.rs`, `~/src/github.com/agentstation/deno/ext/process/lib.rs`, and `~/src/github.com/agentstation/deno/ext/node/lib.rs`. |
| 2026-04-28 | NCR5 | `in_progress` | `crates/neovex-runtime/src/backends/v8/startup.rs`, `crates/neovex-runtime/src/backends/v8/warm_pool.rs`, `crates/neovex-runtime/src/runtime/bootstrap/{extensions.rs,mod.rs,node22_runtime.rs,source.rs,state.rs,transpile.rs}`, `crates/neovex-runtime/src/runtime/bootstrap/js/{01_errors.js,98_global_scope_shared.js,node22_runtime_bootstrap.js}`, `crates/neovex-runtime/src/runtime/tests/basic_invocation.rs`, `docs/architecture/runtime/node-compat-surface-matrix.md`, `docs/plans/node-compatible-runtime-plan.md` | Advanced NCR5 from “first permissions substrate” into a real Deno-family Node22 bootstrap slice. `neovex-runtime` now installs the Node22 bootstrap extension into both snapshot/live extension lists, transpiles TypeScript-backed runtime extension sources for the Node22 target, and keeps Node22 on a live-runtime path while the Deno-style snapshot bootstrap remains incomplete. The Node22 bootstrap now provides the missing Deno-family shared globals (`URL`, fetch/DOM descriptors, console) plus a minimal `Deno` namespace (`errors`, `internal.nodeGlobals`, `build`, `cwd`, `env`, `version`) that `deno_node` polyfills actually rely on. The runtime also now publishes the `PermissionsContainer` at the top level of `OpState`, which unblocks canonical `deno_node` fs ops instead of routing them through bespoke wrappers. Verified outcome: `node:path` imports now work, staged CommonJS package entries and nested `require(...)` continue to work, scoped tooling writes pass inside pre-created approved roots, and the explicit `esbuild` blocker has narrowed from “builtin imports missing” to the subprocess/env boundary (`NODE_V8_COVERAGE` env inheritance via `node:child_process`). The checked-in matrix is updated to reflect that narrower, more honest contract. | `cargo test -p neovex-runtime node22_ --config 'patch.crates-io.deno_core.path=\"/Users/jack/src/github.com/agentstation/deno/libs/core\"' --config 'patch.crates-io.deno_crypto.path=\"/Users/jack/src/github.com/agentstation/deno/ext/crypto\"' --config 'patch.crates-io.deno_fetch.path=\"/Users/jack/src/github.com/agentstation/deno/ext/fetch\"' --config 'patch.crates-io.deno_fs.path=\"/Users/jack/src/github.com/agentstation/deno/ext/fs\"' --config 'patch.crates-io.deno_http.path=\"/Users/jack/src/github.com/agentstation/deno/ext/http\"' --config 'patch.crates-io.deno_io.path=\"/Users/jack/src/github.com/agentstation/deno/ext/io\"' --config 'patch.crates-io.deno_napi.path=\"/Users/jack/src/github.com/agentstation/deno/ext/napi\"' --config 'patch.crates-io.deno_net.path=\"/Users/jack/src/github.com/agentstation/deno/ext/net\"' --config 'patch.crates-io.deno_node.path=\"/Users/jack/src/github.com/agentstation/deno/ext/node\"' --config 'patch.crates-io.deno_node_crypto.path=\"/Users/jack/src/github.com/agentstation/deno/ext/node_crypto\"' --config 'patch.crates-io.deno_os.path=\"/Users/jack/src/github.com/agentstation/deno/ext/os\"' --config 'patch.crates-io.deno_package_json.path=\"/Users/jack/src/github.com/agentstation/deno/libs/package_json\"' --config 'patch.crates-io.deno_permissions.path=\"/Users/jack/src/github.com/agentstation/deno/runtime/permissions\"' --config 'patch.crates-io.deno_process.path=\"/Users/jack/src/github.com/agentstation/deno/ext/process\"' --config 'patch.crates-io.deno_resolver.path=\"/Users/jack/src/github.com/agentstation/deno/libs/resolver\"' --config 'patch.crates-io.deno_telemetry.path=\"/Users/jack/src/github.com/agentstation/deno/ext/telemetry\"' --config 'patch.crates-io.deno_tls.path=\"/Users/jack/src/github.com/agentstation/deno/ext/tls\"' --config 'patch.crates-io.deno_web.path=\"/Users/jack/src/github.com/agentstation/deno/ext/web\"' --config 'patch.crates-io.deno_webidl.path=\"/Users/jack/src/github.com/agentstation/deno/ext/webidl\"' --config 'patch.crates-io.deno_websocket.path=\"/Users/jack/src/github.com/agentstation/deno/ext/websocket\"' --config 'patch.crates-io.node_resolver.path=\"/Users/jack/src/github.com/agentstation/deno/libs/node_resolver\"'` → 11 passed, 0 failed. |
| 2026-04-28 | NCR5 | `done` | `Cargo.lock`, `Cargo.toml`, `crates/neovex-runtime/src/runtime/bootstrap/js/node22_runtime_bootstrap.js`, `crates/neovex-runtime/src/runtime/bootstrap/ops.rs`, `crates/neovex-runtime/src/runtime/bootstrap/ops/runtime_local.rs`, `crates/neovex-runtime/src/runtime/tests/basic_invocation.rs`, `docs/architecture/runtime/node-compat-surface-matrix.md`, `docs/plans/node-compatible-runtime-plan.md`, `~/src/github.com/agentstation/deno/ext/process/40_process.js` | Closed the remaining NCR5 subprocess gap on the canonical Deno-family path. `neovex-runtime` now exposes `Deno.execPath()` through the Node22 bootstrap so `deno_node` child-process internals can identify the host executable correctly, and the `agentstation/deno` fork now publishes `v2.7.14-locker.4`, which forces Node-compatible subprocesses to inherit only the explicit JS-visible env contract instead of re-merging the hidden host process environment. With that release line in place, the staged `esbuild` fixture now passes: the tooling profile can load the staged CommonJS package, use `require("buffer").Buffer`, `node:crypto`, `node:os`, and `node:tty`, and successfully `spawnSync()` an exact pre-existing staged binary under the approved tooling roots. The checked-in surface matrix is updated to reflect that support and to keep Node-API addon loading explicitly unsupported. | `cargo test -p neovex-runtime tooling_node22_executes_esbuild_style_staged_binary` → failed first with `TypeError: Deno.execPath is not a function`; after adding `Deno.execPath()` support, the same test narrowed to an empty `spawnSync()` result shape; `cargo test -p neovex-runtime tooling_node22_executes_esbuild_style_staged_binary --config 'patch.crates-io.deno_core.path=\"/Users/jack/src/github.com/agentstation/deno/libs/core\"' --config 'patch.crates-io.deno_crypto.path=\"/Users/jack/src/github.com/agentstation/deno/ext/crypto\"' --config 'patch.crates-io.deno_fetch.path=\"/Users/jack/src/github.com/agentstation/deno/ext/fetch\"' --config 'patch.crates-io.deno_fs.path=\"/Users/jack/src/github.com/agentstation/deno/ext/fs\"' --config 'patch.crates-io.deno_http.path=\"/Users/jack/src/github.com/agentstation/deno/ext/http\"' --config 'patch.crates-io.deno_io.path=\"/Users/jack/src/github.com/agentstation/deno/ext/io\"' --config 'patch.crates-io.deno_napi.path=\"/Users/jack/src/github.com/agentstation/deno/ext/napi\"' --config 'patch.crates-io.deno_net.path=\"/Users/jack/src/github.com/agentstation/deno/ext/net\"' --config 'patch.crates-io.deno_node.path=\"/Users/jack/src/github.com/agentstation/deno/ext/node\"' --config 'patch.crates-io.deno_node_crypto.path=\"/Users/jack/src/github.com/agentstation/deno/ext/node_crypto\"' --config 'patch.crates-io.deno_os.path=\"/Users/jack/src/github.com/agentstation/deno/ext/os\"' --config 'patch.crates-io.deno_package_json.path=\"/Users/jack/src/github.com/agentstation/deno/libs/package_json\"' --config 'patch.crates-io.deno_permissions.path=\"/Users/jack/src/github.com/agentstation/deno/runtime/permissions\"' --config 'patch.crates-io.deno_process.path=\"/Users/jack/src/github.com/agentstation/deno/ext/process\"' --config 'patch.crates-io.deno_resolver.path=\"/Users/jack/src/github.com/agentstation/deno/libs/resolver\"' --config 'patch.crates-io.deno_telemetry.path=\"/Users/jack/src/github.com/agentstation/deno/ext/telemetry\"' --config 'patch.crates-io.deno_tls.path=\"/Users/jack/src/github.com/agentstation/deno/ext/tls\"' --config 'patch.crates-io.deno_web.path=\"/Users/jack/src/github.com/agentstation/deno/ext/web\"' --config 'patch.crates-io.deno_webidl.path=\"/Users/jack/src/github.com/agentstation/deno/ext/webidl\"' --config 'patch.crates-io.deno_websocket.path=\"/Users/jack/src/github.com/agentstation/deno/ext/websocket\"' --config 'patch.crates-io.node_resolver.path=\"/Users/jack/src/github.com/agentstation/deno/libs/node_resolver\"'` → 1 passed, 0 failed against local Deno commit `a2cc5bfdc` / release `v2.7.14-locker.4`; `git -C ~/src/github.com/agentstation/deno diff --check`; `git -C ~/src/github.com/agentstation/deno commit -m "fix(node): clear hidden host env for subprocesses"` → `a2cc5bfdc`; `git -C ~/src/github.com/agentstation/deno push origin locker-v2.7.14`; `git -C ~/src/github.com/agentstation/deno tag -a v2.7.14-locker.4 -m "v2.7.14-locker.4"`; `git -C ~/src/github.com/agentstation/deno push origin refs/tags/v2.7.14-locker.4`; `gh release create v2.7.14-locker.4 --repo agentstation/deno ...`; workspace patches and `Cargo.lock` now point at `v2.7.14-locker.4` / commit `a2cc5bfdc77713c9028709f386dbd671dd3f1150`. |
| 2026-04-28 | NCR6 | `in_progress` | `docs/plans/node-compatible-runtime-plan.md` | Advanced control to NCR6 after closing NCR5. The next critical-path work is to replace the external `node` subprocess behind a guarded pilot path in `neovex-bin`, starting from the current `crates/neovex-bin/src/codegen.rs` and `packages/codegen/src/main.mjs` contract, and to keep the fallback path explicit until embedded codegen parity is proven with named Convex and Cloud Functions fixtures. | NCR6 implementation not started yet in this checkpoint; control advanced after the NCR5 closeout. |
| 2026-04-28 | NCR6 | `done` | `Cargo.lock`, `Cargo.toml`, `crates/neovex-bin/src/codegen.rs`, `crates/neovex-runtime/src/runtime/bootstrap/js/node22_runtime_bootstrap.js`, `crates/neovex-runtime/src/runtime/bootstrap/ops.rs`, `crates/neovex-runtime/src/runtime/bootstrap/ops/runtime_local.rs`, `packages/codegen/src/app.mjs`, `packages/codegen/src/cloud_functions.mjs`, `packages/codegen/src/parser/http_routes.mjs`, `packages/codegen/src/schema.mjs`, `docs/plans/node-compatible-runtime-plan.md` | Closed the guarded embedded codegen pilot on the canonical runtime path. `neovex-bin` now supports an explicit `NEOVEX_EXPERIMENTAL_EMBEDDED_CODEGEN` runner that stages a small bootstrap bundle into the app root, executes `@neovex/codegen` inside `neovex-runtime`, and keeps the external `node` subprocess as the fallback bridge. To make the pilot honest rather than bespoke, the Node22 bootstrap now initializes Deno build info before `deno_node` bootstrap, exposes the runtime-local fs and target-triple ops the embedded toolchain needs, and keeps the embedded bridge host-call free. The codegen package itself now uses existence-aware optional file reads for `schema.ts`, `http.ts`, and optional JSON inputs so Convex and Cloud Functions fixtures behave the same under the embedded runtime as they do under external Node. | `cargo test -p neovex-bin codegen::tests:: -- --nocapture` → 7 passed, 0 failed (including `embedded_pilot_generates_convex_artifacts_from_staged_workspace_package` and `embedded_pilot_generates_cloud_functions_artifacts_from_staged_workspace_package`); `cargo fmt --all`; `cargo fmt --all --check` → clean; `make clippy` → clean; `npm run test --workspace @neovex/codegen` → passed. |
| 2026-04-28 | NCR7 | `done` | `README.md`, `docs/architecture/runtime/node-compat-surface-matrix.md`, `docs/operating/cli.md`, `docs/plans/neovex-init-plan.md`, `docs/plans/node-compatible-runtime-plan.md` | Closed the product-truth decision in the conservative direction. A full `neovex-bin` crate test pass confirmed that the current onboarding contract still depends on external Node for authoring flows: `neovex init --install` and `neovex dev` still bootstrap dependencies through `npm`, while `neovex codegen` defaults to the external `node` runner and only exposes the embedded path behind `NEOVEX_EXPERIMENTAL_EMBEDDED_CODEGEN`. Upstream version checks were re-run from current official docs and npm metadata: Convex `1.36.1` with `"use node"` support for Node 20 and 22 (default 20), `firebase-functions 7.2.5`, `firebase-admin 13.8.0`, `@google-cloud/functions-framework 5.0.2`, Firebase docs that fully support Node 20 and 22 while marking Node 18 deprecated, and the current Cloud Run functions runtime matrix with Node 22 and 20 stable plus Node 24 preview. Based on that evidence, Neovex's public contract now names `Node22` as the verified authoring baseline, keeps the external Node prerequisite in the happy path, preserves the embedded runner as an experimental pilot only, and narrows `Node20` to upstream-supported-but-unverified instead of silently implying broader compatibility. | `cargo test -p neovex-bin` → 412 passed, 0 failed; `cargo fmt --all --check` → clean; `npm run test --workspace @neovex/codegen` → passed; direct npm registry metadata recheck via Python `urllib` against `https://registry.npmjs.org/{convex,firebase-functions,firebase-admin,@google-cloud/functions-framework}/latest` confirmed versions and `engines`; official docs rechecked via `docs.convex.dev/functions/runtimes`, `firebase.google.com/docs/functions/get-started`, `firebase.google.com/docs/functions/manage-functions`, and `cloud.google.com/run/docs/runtimes/function-runtimes`; attempted `npm run docs:validate-refs:strict` at repo root failed because the script does not exist in this repo, so no stricter docs-ref lane is claimed here. |

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
- `deno_core` crate docs:
  https://docs.rs/crate/deno_core/latest
- `v8` crate docs:
  https://docs.rs/crate/v8/latest
- crates.io `deno_core 0.400.0` metadata:
  https://crates.io/api/v1/crates/deno_core/0.400.0
  https://crates.io/api/v1/crates/deno_core/0.400.0/dependencies
- crates.io `deno_runtime` metadata:
  https://crates.io/api/v1/crates/deno_runtime
  https://crates.io/api/v1/crates/deno_runtime/0.255.0/dependencies
  https://crates.io/api/v1/crates/deno_runtime/0.254.0
  https://crates.io/api/v1/crates/deno_runtime/0.254.0/dependencies
- crates.io `deno_node` metadata:
  https://crates.io/api/v1/crates/deno_node
  https://crates.io/api/v1/crates/deno_node/0.185.0/dependencies
- `deno_node` crate docs:
  https://docs.rs/deno_node/latest/deno_node/
- `deno_fs` crate docs:
  https://docs.rs/deno_fs/latest/deno_fs/
- `node_resolver` crate docs:
  https://docs.rs/node_resolver/latest/node_resolver/
- archived `denoland/deno_core` repository notice:
  https://github.com/denoland/deno_core
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
