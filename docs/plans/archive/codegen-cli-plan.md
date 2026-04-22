# Plan: `neovex codegen` CLI Integration

Canonical execution plan for adding a first-party `neovex codegen` subcommand
to the Rust CLI, modernizing the serve-command app-directory contract, and
establishing a developer experience that treats codegen as a built-in
capability of the `neovex` binary rather than a sideband Node.js script.

Successor to the completed source-root plan
(`docs/plans/archive/neovex-source-root-plan.md`), which landed the
`neovex/` vs `convex/` source-root detection and namespace-aware `_generated`
emission but explicitly deferred the CLI integration (`"Follow-on work, not
part of this story: add a first-party neovex codegen command"`).

---

## Status

- **Status:** `completed`
- **Primary owner:** this plan
- **Activation gate:** none — can start immediately; all upstream inputs
  (source-root resolver, namespace-aware codegen, `@neovex/codegen` package)
  are shipped
- **Parent plan:** none
- **Related plans and docs:**
  - `docs/plans/archive/neovex-source-root-plan.md` — landed source-root
    resolver and namespace-aware `_generated` emission
  - `docs/stories/support-neovex-source-directory.md` — product contract that
    explicitly listed this as follow-on work
  - `docs/reference/cli.md` — user-facing CLI reference to update
  - `docs/convex/compatibility.md` — compatibility surface reference

---

## Why This Plan Exists

### The Problem

Today a developer working on a Neovex project must context-switch between two
disconnected tools:

```bash
npx convex codegen --app ./my-app       # JS tool, Convex-branded
neovex serve --convex-app-dir ./my-app  # Rust tool, Neovex-branded
```

This workflow has four concrete problems:

1. **The primary tool has no codegen.** The `neovex` binary is the developer's
   main CLI for serve, machine, and service operations. But it cannot generate
   the artifacts it needs to serve. Users must know to reach for an entirely
   separate tool (`npx convex codegen` or `npx neovex-codegen`) that lives
   outside the Rust binary's command tree.
2. **The command names contradict the product identity.** A developer who
   creates a `neovex/` source root, writes Neovex-native functions, and
   imports from `neovex/server` must then run `npx convex codegen` to build
   their project. The command name contradicts the namespace they chose.
3. **The serve flag is misleading.** `--convex-app-dir` implies the serve
   command only works with Convex-style apps, even though the same flag works
   with both `neovex/` and `convex/` source roots after the source-root plan
   landed.
4. **Two-step startup is fragile.** The Makefile and workspace scripts still
   prove the point: codegen and serve are separate commands, so missing or
   stale manifests remain an avoidable class of startup failure.

### Developer-Experience Goal

The immediate goal is not to replicate the full `npx convex dev` lifecycle.
The goal is to make the first-party Neovex CLI feel coherent and migration-
friendly:

- Neovex users should not have to reach for a Convex-branded command just to
  generate Neovex-native `_generated/` files.
- `neovex serve --app-dir` should be able to perform one startup-time codegen
  pass so the common "generate, then serve" flow becomes one command.
- The docs must be honest that this plan is a one-shot preflight improvement,
  not yet a watched edit loop

### Scope Boundary Against Convex `dev`

Convex's current docs describe `npx convex dev` as a watched development loop
that updates generated code as functions change, and they recommend checking
`convex/_generated/` into version control. This plan intentionally stops short
of that full workflow. It lands:

- a first-party `neovex codegen` entrypoint
- a one-shot preflight codegen step on `neovex serve --app-dir`
- clearer startup errors and better naming

It does **not** land:

- a `neovex dev` watched loop
- background regeneration after server startup
- hot-reload semantics for source edits

Those belong to follow-on work if we want full Convex-style dev ergonomics.

### Design Principle

> The primary Neovex CLI should own the common path from source to running
> server, while being explicit about where the current flow still depends on
> project-owned build and watch scripts.

---

## Current Assessed State

- The `neovex` Rust binary now ships five subcommands: `serve`, `codegen`,
  `machine`, `service`, and `encryption`.
- The `@neovex/codegen` JS package provides the full codegen pipeline and
  exports both a library API (`generateConvexArtifacts()`) and a CLI entrypoint
  (`neovex-codegen`).
- The `convex` JS package wraps `@neovex/codegen` behind `convex codegen`.
- Source-root detection (`neovex/` vs `convex/`) is fully implemented in
  `packages/codegen/src/app.mjs:resolveSourceRoot()`.
- Namespace-aware `_generated/*` emission is fully implemented in
  `packages/codegen/src/main.mjs` and
  `packages/codegen/src/emit/generated_files.mjs`.
- The serve command now accepts `--app-dir` and loads manifests from
  `{appDir}/.neovex/convex/`.
- `neovex serve --app-dir` now runs a one-shot preflight codegen pass before
  registry load unless `--skip-codegen` is provided. It does not watch for
  later edits after startup.
- The runtime artifact contract is more nuanced than "all files required":
  `functions.json` is the only required manifest, `http_routes.json`,
  `schema.json`, and `auth.config.json` are optional, and `bundle.mjs` is
  optional. If a runtime bundle is present, its `bundle.sha256` sidecar remains
  mandatory.
- Named Convex queries, mutations, and actions intentionally fall back to the
  compiled-plan path when no runtime bundle is loaded. This plan must preserve
  that contract unless a later architecture change makes the runtime bundle
  mandatory everywhere.
- Workspace and hoisted installs are already a real local-development case in
  this repo. For example, the demo apps can resolve `@neovex/codegen` through
  workspace Node resolution even though
  `{appDir}/node_modules/@neovex/codegen/` does not exist beneath each app.
- The root workspace scripts, demo app scripts, and Makefile still run explicit
  codegen before frontend dev/build/test flows, so the current DX depends on
  project-owned scripts and checked/generated outputs rather than a server-owned
  watch loop.
- The live CLI/reference and compatibility docs now describe `neovex codegen`,
  `--app-dir`, `--skip-codegen`, checked-in generated files, and the honest
  one-shot-preflight scope.
- When the required manifest is missing or unreadable, the serve boot boundary
  now raises an actionable error that points at
  `.neovex/convex/functions.json`, includes the exact `neovex codegen --app`
  recovery command, and mentions `--skip-codegen` when that flag blocked the
  preflight path.

---

## Control Plan Rules

1. **Codegen logic stays in JavaScript.** The Rust `neovex codegen` command
   spawns the `@neovex/codegen` pipeline as a Node.js subprocess. It does not
   reimplement the codegen pipeline in Rust.
2. **`convex codegen` stays.** It remains the compatibility entry point for
   existing Convex-migrating users. Both CLIs call the same underlying
   pipeline.
3. **No `--convex` / `--neovex` flag.** The chosen source-root directory
   (`neovex/` or `convex/`) remains the signal.
4. **`.neovex/convex/` internal artifacts stay unchanged.** This plan does not
   rename the internal runtime artifact namespace.
5. **`--convex-app-dir` becomes `--app-dir` as a clean break.** The old flag is
   deleted, not aliased. Pre-launch breaking changes are preferred here.
6. **Node resolution must honor workspaces and hoisting.** Do not assume an
   app-local `node_modules/@neovex/codegen/...` filesystem path exists.
7. **Serve-side auto-codegen is a one-shot preflight.** This plan does not add
   file watching, background regeneration, or a `neovex dev` loop.
8. **Missing required manifests are blocking; missing runtime bundle alone is
   not.** `functions.json` must exist and load cleanly. A missing `bundle.mjs`
   must continue to allow compiled-plan fallback.
9. **If a runtime bundle exists, integrity errors remain blocking.** A present
   `bundle.mjs` without a valid `bundle.sha256` sidecar should still fail
   loudly rather than silently downgrading.
10. **Codegen failure must be a clear, blocking error.** When `neovex serve`
    runs codegen and it fails, the serve process must not start. Child output
    must be surfaced directly.

---

## Target UX

### Standalone codegen

```bash
# Codegen for the current directory (detects neovex/ or convex/)
neovex codegen

# Codegen for a specific app directory
neovex codegen --app ./demos/convex/html

# Equivalent compatibility entrypoints
npx convex codegen --app ./my-app
npx neovex-codegen --app ./my-app
```

### Serve with one-shot preflight codegen

```bash
# Run one preflight codegen pass, then start serving
neovex serve --app-dir ./my-app

# Opt out when another build step already generated manifests
neovex serve --app-dir ./my-app --skip-codegen
```

### Serve without an app directory

```bash
# Pure database server, no Convex/Neovex functions
neovex serve
```

### Migration Taste for Convex Users

After this plan, the Neovex-native story should feel like:

```bash
neovex codegen --app ./my-app
neovex serve --app-dir ./my-app
```

or, for the common backend-startup path:

```bash
neovex serve --app-dir ./my-app
```

But the docs must also say the quiet part clearly:

- `_generated/` should still be checked into version control for stable
  typechecking and frontend workflows, matching current Convex guidance
- project-owned `npm run dev` / `npm run build` flows may still run explicit
  codegen until a future watch-mode plan lands

---

## Roadmap

### C1: `neovex codegen` subcommand

Add a `codegen` subcommand to the Rust binary that invokes the existing
`@neovex/codegen` pipeline through Node's module-resolution semantics.

**Implementation:**

- Add `crates/neovex-bin/src/codegen.rs` with a `CodegenCommand` struct:
  ```rust
  #[derive(Debug, clap::Args)]
  #[command(
      about = "Generate _generated files and runtime bundle from neovex/ or convex/ source",
      help_template = cli_ux::COMMAND_HELP_TEMPLATE,
  )]
  pub(crate) struct CodegenCommand {
      /// App directory containing the neovex/ or convex/ source root.
      /// Defaults to the current directory.
      #[arg(long, default_value = ".")]
      pub(crate) app: PathBuf,
  }
  ```
- Implement `run_codegen_command()` that:
  1. resolves `app_dir`
  2. spawns `node` with its working directory set to `app_dir`
  3. invokes `@neovex/codegen` through Node module resolution
     (`@neovex/codegen/cli` or a tiny bootstrap that calls
     `runCliFromArgs()`), not through a hardcoded
     `{app_dir}/node_modules/@neovex/codegen/...` path
  4. inherits stdio so the user sees codegen output directly
  5. returns the exit status
- Add `Codegen(CodegenCommand)` to the `Command` enum in `main.rs`
- Add `neovex codegen` to `ROOT_HELP_EXAMPLES`

**Resolution policy:**

- **Initial implementation:** Node module resolution from `app_dir` is the
  required path. This supports workspace and hoisted installs cleanly.
- **Deferred fallback:** a bundled helper or `npx` fallback may be added later
  if distribution work shows we need a non-workspace escape hatch.

**Verification:**

- `cargo run -p neovex-bin -- codegen --app ./demos/convex/html` succeeds from
  the repo root and produces `.neovex/convex/` artifacts
- `cargo run -p neovex-bin -- codegen --app ./demos/convex/node` succeeds from
  the repo root
- `neovex codegen` in a directory with `neovex/` or `convex/` source root works
- `neovex codegen` in a directory with no source root fails with the existing
  JS resolver error
- `neovex --help` shows `codegen` in the command list
- existing `npx convex codegen` still works unchanged

---

### C2: Rename `--convex-app-dir` to `--app-dir`

Modernize the serve command's flag name to match the broader scope.

**Implementation:**

- In `crates/neovex-bin/src/serve/mod.rs`, rename the field:
  ```rust
  /// App directory containing generated .neovex/convex/ runtime artifacts.
  /// Defaults to no app directory (pure database server mode).
  #[arg(long)]
  pub(crate) app_dir: Option<PathBuf>,
  ```
- Delete all references to the old `convex_app_dir` name — no alias, no
  compatibility shim
- Update `boot.rs` to reference the renamed field
- Update workspace scripts, Makefile targets, docs, and test fixtures that
  reference `--convex-app-dir`
- Update `docs/reference/cli.md` and `SERVE_HELP_EXAMPLES`

**Verification:**

- `neovex serve --app-dir ./my-app` works
- `neovex serve --help` shows `--app-dir`
- workspace scripts and Makefile targets use `--app-dir`
- all serve tests still pass

---

### C3: One-shot preflight codegen on serve

When `--app-dir` is provided, the serve command runs one codegen pass before
loading manifests unless `--skip-codegen` is passed.

**Implementation:**

- Add a `--skip-codegen` flag to `ServeCommand`:
  ```rust
  /// Skip automatic codegen before serving. Use when manifests are
  /// pre-built by a separate build step.
  #[arg(long, default_value_t = false)]
  pub(crate) skip_codegen: bool,
  ```
- In `boot.rs`, before `load_convex_registry()`, call `run_codegen_command()`
  when `app_dir` is `Some` and `skip_codegen` is `false`
- If codegen fails, abort serve with the child-process error output
- Print brief status lines before and after the preflight step
- Document explicitly that this is a startup-time convenience, not a watched
  edit loop

**Non-goals:**

- no background regeneration after startup
- no file watching
- no `neovex dev` command

**Verification:**

- `neovex serve --app-dir ./my-app` runs codegen once, then serves
- `neovex serve --app-dir ./my-app --skip-codegen` skips codegen
- `neovex serve` without an app dir does not attempt codegen
- if codegen fails, serve does not start and the child error is visible
- docs and help text describe this as one-shot preflight behavior rather than a
  Convex-style watched loop

---

### C4: Clear error on missing required manifests

When `--app-dir` is provided with `--skip-codegen` and the required manifest is
missing or unreadable, the serve command must produce a clear, actionable
error instead of only surfacing a low-level file read failure.

**Implementation:**

- In `load_convex_registry()` or the registry-loading boundary, detect the
  specific case where `.neovex/convex/functions.json` is missing or unreadable
  and produce a structured error such as:
  ```text
  Error: No generated function manifest found at <app_dir>/.neovex/convex/functions.json.

  Run "neovex codegen --app <app_dir>" to generate it, or remove
  --skip-codegen to generate manifests automatically on serve.
  ```
- Preserve the current optional-file semantics:
  - missing `http_routes.json` still means no HTTP routes
  - missing `schema.json` still means no loaded schema manifest
  - missing `auth.config.json` still means default auth config
  - missing `bundle.mjs` still allows compiled-plan-only startup
- Preserve the current runtime-only semantics:
  - functions with `plan: null` still require the runtime bundle at execution
    time even if startup is allowed without `bundle.mjs`
- Preserve the current runtime-bundle integrity rule:
  - if `bundle.mjs` exists but `bundle.sha256` is missing or invalid, startup
    still fails with an integrity-focused error

**Verification:**

- `neovex serve --app-dir ./empty-dir --skip-codegen` gives the actionable
  error
- the error message includes the exact `neovex codegen` command to run
- a manifest-only app directory without `bundle.mjs` still starts, and
  plan-backed functions continue to work through the compiled-plan path
- a present `bundle.mjs` without `bundle.sha256` still fails and mentions the
  hash sidecar

---

### C5: Makefile, scripts, and docs alignment

Update all developer-facing surfaces to reflect the new CLI shape and the
honest scope of this plan.

**Implementation:**

- **Workspace scripts and Makefile:** Update root scripts, differential-test
  helpers, and `convex-demo` targets to use `--app-dir`
- **`docs/reference/cli.md`:** Add `neovex codegen`, update `neovex serve`
  flags, document `--skip-codegen`, and describe one-shot preflight behavior
- **`docs/convex/compatibility.md`:** Document that `convex codegen` and
  `neovex codegen` are equivalent entry points to the same pipeline, and state
  clearly that generated code should still be checked in
- **`SERVE_HELP_EXAMPLES` in `cli_ux.rs`:** Add app-dir and skip-codegen examples
- **`ROOT_HELP_EXAMPLES` in `cli_ux.rs`:** Add `neovex codegen`

**Verification:**

- doc examples are accurate and runnable
- `neovex --help` and `neovex serve --help` reflect the new surface
- developer-facing scripts no longer reference `--convex-app-dir`

---

## Verification Contract

Each roadmap item must satisfy before closing:

- `cargo fmt --all --check` — green
- `make clippy` — green
- `make test` — green
- `npm run test --workspaces --if-present` — green
- manual verification described per item

---

## File Map

### Files to create

| File | What |
|------|------|
| `crates/neovex-bin/src/codegen.rs` | `CodegenCommand` struct and `run_codegen_command()` |

### Files to modify

| File | Change |
|------|--------|
| `crates/neovex-bin/src/main.rs` | Add `Codegen` variant, `mod codegen`, dispatch |
| `crates/neovex-bin/src/serve/mod.rs` | Rename `convex_app_dir` → `app_dir` with no alias, add `skip_codegen` |
| `crates/neovex-bin/src/serve/boot.rs` | Preflight codegen before registry load, updated field name, clearer load errors |
| `crates/neovex-bin/src/serve/tests.rs` | Update parse and startup tests for `--app-dir`, `--skip-codegen`, and required-manifest error wording |
| `crates/neovex-bin/src/cli_ux.rs` | Update help examples |
| `crates/neovex-server/src/adapters/convex/registry/loading.rs` | Optional only if registry-layer error wording is the cleanest place to improve required-manifest diagnostics |
| `package.json` | Update root workflow scripts to `--app-dir` |
| `packages/convex/src/differential.mjs` | Update local Neovex launch command to `--app-dir` |
| `Makefile` | Update demo targets |
| `docs/reference/cli.md` | Add `neovex codegen`, update serve flags and preflight semantics |
| `docs/convex/compatibility.md` | Document CLI equivalence and checked-in generated-code guidance |

### Files not modified

| File | Reason |
|------|--------|
| `packages/codegen/**` | The JS codegen pipeline behavior is already correct; Rust invokes it as-is |
| `packages/neovex/**` | SDK surface unchanged |
| `packages/convex/src/cli.mjs` | `convex codegen` remains the standalone compatibility entrypoint |

---

## DX Lifecycle After This Plan

```
                           neovex codegen --app .
                           ┌──────────────────────┐
                           │  Spawns Node.js using │
                           │  @neovex/codegen via  │
                           │  module resolution    │
                           │                       │
                           │  Detects neovex/ or   │
                           │  convex/ source root  │
                           │                       │
                           │  Writes:              │
                           │  ├─ {root}/_generated/│
                           │  └─ .neovex/convex/   │
                           └──────────┬───────────┘
                                      │
                                      ▼
                       neovex serve --app-dir . [--skip-codegen]
                           ┌──────────────────────┐
                           │  Runs one preflight  │
                           │  codegen pass unless │
                           │  --skip-codegen      │
                           │                       │
                           │  Loads required:      │
                           │  ├─ functions.json    │
                           │  Loads optional:      │
                           │  ├─ http_routes.json  │
                           │  ├─ schema.json       │
                           │  ├─ auth.config.json  │
                           │  └─ bundle.mjs + hash │
                           │                       │
                           │  Starts HTTP/WS       │
                           │  server               │
                           └──────────────────────┘
```

If `bundle.mjs` is absent, named query, mutation, and action execution still
falls back to the compiled-plan path.

### Command Equivalence Diagram

```
┌──────────────────────────────────────────────────────────────────┐
│                     @neovex/codegen pipeline                     │
│                   (single JS implementation)                     │
│                                                                  │
│  resolveSourceRoot() → parse → emit _generated/ + .neovex/convex │
└──────┬───────────────────────┬────────────────────────┬──────────┘
       │                       │                        │
       ▼                       ▼                        ▼
  neovex codegen         convex codegen          npx neovex-codegen
  (Rust CLI,             (JS CLI,                (JS CLI,
   spawns Node)           compatibility)          direct)
       │                       │                        │
       │    All three invoke the same pipeline.         │
       │    All three detect neovex/ vs convex/.        │
       │    All three produce identical output.         │
       └───────────────────────┴────────────────────────┘
```

---

## Execution Log

| Item | Status | Date | Notes |
|------|--------|------|-------|
| Status promotion | `active` | 2026-04-22 | Promoted this plan from pending draft state to the active control plane for first-party CLI codegen integration, app-dir naming, one-shot serve preflight codegen, and manifest-loading UX. |
| Plan rewrite | — | 2026-04-22 | Rewrote the control rules and roadmap after review so the plan now assumes workspace-safe Node resolution, keeps `--app-dir` as a clean break with no alias, describes serve-side codegen as one-shot preflight rather than watch mode, and preserves the current compiled-plan fallback when `bundle.mjs` is absent. |
| C1 | `done` | 2026-04-22 | Added a first-party `neovex codegen` subcommand in `crates/neovex-bin`, invoking `@neovex/codegen` through Node module resolution from the app directory instead of probing app-local package paths. Updated root/codegen help examples and CLI parse coverage. Verification: `cargo test -p neovex-bin`; `cargo run -p neovex-bin -- codegen --app ./demos/convex/html`; `cargo run -p neovex-bin -- codegen --app ./demos/convex/node`. Next: rename `--convex-app-dir` to `--app-dir` across serve, tests, scripts, and docs. |
| C2 | `done` | 2026-04-22 | Renamed the serve app-directory flag to `--app-dir` with no alias, updated serve help/examples, and moved the owned package scripts, differential helper, and Makefile target to the new flag. Verification: `cargo test -p neovex-bin`; targeted grep over `crates/neovex-bin/src/serve`, `crates/neovex-bin/src/cli_ux.rs`, `package.json`, `packages/convex/src/differential.mjs`, and `Makefile` confirmed the old flag only remains in explicit rejection tests. Next: add one-shot preflight codegen and `--skip-codegen` to `neovex serve --app-dir`. |
| C3 | `done` | 2026-04-22 | Added one-shot serve preflight codegen plus `--skip-codegen`, reusing the first-party Rust wrapper before any listener or scheduler startup. Added CLI parse/help coverage and preflight tests that generate real artifacts in a repo-local temp app. Verification: `cargo test -p neovex-bin`; `cargo run -p neovex-bin -- serve --app-dir ./demos/convex/node --data-dir /tmp/neovex-c3-data --control-data-dir /tmp/neovex-c3-control --port 18091` then `curl -i -sS http://127.0.0.1:18091/health` (required escalated execution because sandboxed serve died after preflight with `Os { code: 1, kind: PermissionDenied, message: "Operation not permitted" }`); `cargo run -p neovex-bin -- serve --app-dir ./demos/convex/node --skip-codegen --data-dir /tmp/neovex-c3-skip-data --control-data-dir /tmp/neovex-c3-skip-control --port 18093` then `curl -i -sS http://127.0.0.1:18093/health`; `cargo run -p neovex-bin -- serve --data-dir /tmp/neovex-c3-noapp-data --control-data-dir /tmp/neovex-c3-noapp-control --port 18094` then `curl -i -sS http://127.0.0.1:18094/health`; `cargo run -p neovex-bin -- serve --app-dir ./target/neovex-c3-empty.zn65zE --port 18092` showed preflight blocking startup and surfaced the JS source-root error. Next: tighten startup diagnostics for missing required manifests while preserving optional bundle semantics. |
| C4 | `done` | 2026-04-22 | Added a serve-boundary required-manifest check for `.neovex/convex/functions.json`, returning an actionable `neovex codegen --app …` recovery message and a `--skip-codegen` hint when preflight was bypassed. Reordered serve startup so registry/service-manager validation happens before listener bind, preserving fail-fast behavior. Verification: `cargo test -p neovex-bin`; `cargo test -p neovex-server`. New focused tests cover the actionable missing-manifest error, manifest-only startup without `bundle.mjs`, and the existing `neovex-server` suite continues to prove that a present `bundle.mjs` still requires `bundle.sha256`. Next: align remaining docs and developer-facing surfaces with the shipped command shape and scoped Convex migration story. |
| C5 | `done` | 2026-04-22 | Updated the remaining live docs and developer-facing surfaces: CLI reference, compatibility guide, HTTP/API reference, architecture note, package scripts, Makefile, and the differential helper now all describe `neovex codegen`, `--app-dir`, `--skip-codegen`, checked-in generated files, and one-shot serve preflight honestly. Verification: `cargo fmt --all --check`; `make clippy`; `make test`; `npm run test --workspaces --if-present`; `npm run build --workspaces --if-present`; targeted grep over live docs/scripts confirmed the old flag is absent from shipped surfaces. Workstream complete. |
