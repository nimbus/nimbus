# Plan: `neovex codegen` CLI Integration

Canonical execution plan for adding a first-party `neovex codegen` subcommand
to the Rust CLI, modernizing the serve-command app-directory contract, and
establishing a developer experience that treats codegen as a built-in capability
of the `neovex` binary rather than a sideband Node.js script.

Successor to the completed source-root plan
(`docs/plans/archive/neovex-source-root-plan.md`), which landed the
`neovex/` vs `convex/` source-root detection and namespace-aware `_generated`
emission but explicitly deferred the CLI integration (`"Follow-on work, not
part of this story: add a first-party neovex codegen command"`).

---

## Status

- **Status:** `pending`
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
npx convex codegen --app ./my-app      # JS tool, Convex-branded
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

4. **Two-step startup is fragile.** The Makefile already proves this: every
   `make convex-demo` target chains `npx convex codegen` then `cargo run --
   serve` because missing artifacts cause silent failures. A single CLI that
   can codegen-then-serve (or at least fail clearly) eliminates a class of
   "forgot to codegen" bugs.

### Prior Art: How Other Tools Handle This

The design draws from tools that developers already use daily. The common
thread: codegen is a built-in subcommand of the primary CLI, not a separate
binary, and auto-codegen on serve is expected.

**Convex (`npx convex dev`)**

Convex bundles codegen into its `dev` command — running `npx convex dev`
generates `_generated/` files, pushes to the cloud, and starts watching for
changes. There is no separate codegen-then-serve sequence. The developer runs
one command and the project is ready. Our `convex codegen` is already a
simplified version of this pattern; what is missing is the equivalent
`neovex codegen` entrypoint and the serve-side auto-codegen.

**Next.js (`next dev` / `next build`)**

Next.js generates route manifests, type declarations, and the `.next/` build
cache as part of `next dev` and `next build`. Developers never run a separate
`next codegen` step. The build artifacts are treated as an internal concern
of the dev and build commands. When artifacts are stale, the dev server
regenerates them automatically.

**Prisma (`prisma generate` / `prisma db push`)**

Prisma exposes `prisma generate` as a first-class subcommand of its primary
CLI. The generated client goes into `node_modules/.prisma/client/`, and
`prisma db push` can auto-run `prisma generate` afterwards. Developers stay
inside one CLI tool for the full workflow.

**Rails (`rails generate` / `rails server`)**

Rails bundles generators and server into the same CLI. `rails generate model`
produces migrations and models; `rails server` starts the app. The two are
subcommands of the same binary. When migrations are pending, the server tells
you instead of silently serving stale state.

**Docker / Podman (`docker build` / `docker run`)**

Build and run are subcommands of the same binary. A developer never reaches
for a separate `docker-build` tool. The CLI owns the full lifecycle from image
creation through container execution.

### Design Principle

> The developer's primary CLI should own the full lifecycle from source to
> running server. Codegen is not a separate concern — it is the first step of
> `serve`.

---

## Current Assessed State

- The `neovex` Rust binary ships four subcommands: `serve`, `machine`,
  `service`, `encryption`. There is no `codegen` subcommand.
- The `@neovex/codegen` JS package provides the full codegen pipeline and
  exports both a library API (`generateConvexArtifacts()`) and a CLI entrypoint
  (`neovex-codegen`).
- The `convex` JS package wraps `@neovex/codegen` behind `convex codegen`.
- Source-root detection (`neovex/` vs `convex/`) is fully implemented in
  `packages/codegen/src/app.mjs:resolveSourceRoot()`.
- Namespace-aware `_generated/*` emission is fully implemented in
  `packages/codegen/src/emit/generated_files.mjs`.
- The serve command accepts `--convex-app-dir` and loads pre-generated
  artifacts from `{appDir}/.neovex/convex/`.
- The Makefile's `convex-demo` target chains `npx convex codegen` then
  `cargo run -- serve` as a two-step sequence, demonstrating the expected
  combined workflow.
- `neovex serve` gives no useful error when `.neovex/convex/` artifacts are
  missing — it either panics or silently skips Convex function loading.

---

## Control Plan Rules

1. **Codegen logic stays in JavaScript.** The Rust `neovex codegen` command
   spawns the `@neovex/codegen` pipeline as a Node.js subprocess. It does not
   reimplement the codegen pipeline in Rust.
2. **`convex codegen` stays.** It remains the compatibility entry point for
   existing Convex-migrating users. Both CLIs call the same underlying
   pipeline.
3. **No `--convex` / `--neovex` flag.** The directory you created (`neovex/` or
   `convex/`) is the signal. The resolver already handles detection, priority,
   and feedback. Adding a mode flag would contradict the convention-over-
   configuration principle that the source-root plan established.
4. **`.neovex/convex/` internal artifacts stay unchanged.** This plan does not
   rename the internal runtime artifact namespace. That is independent work
   with its own migration considerations.
5. **`--convex-app-dir` becomes `--app-dir`.** The old name is deleted, not
   aliased. Pre-launch: breaking changes preferred, no compatibility shims.
6. **Codegen failure is a clear, blocking error.** When `neovex serve` runs
   codegen and it fails, the serve process must not start. The error output
   from the JS codegen must be surfaced verbatim to the user.

---

## Target UX

### Standalone codegen

```bash
# Codegen for the current directory (detects neovex/ or convex/)
neovex codegen

# Codegen for a specific app directory
neovex codegen --app ./demos/convex/html

# Equivalent Convex-compat entrypoints (unchanged, still work)
npx convex codegen --app ./my-app
npx neovex-codegen --app ./my-app
```

### Serve with auto-codegen

```bash
# Codegen is run automatically before serving
neovex serve --app-dir ./my-app

# Opt out of auto-codegen when you manage the build yourself
neovex serve --app-dir ./my-app --skip-codegen
```

### Serve without an app directory (unchanged)

```bash
# Pure database server, no Convex/Neovex functions
neovex serve
```

### DX comparison

Before this plan:

```bash
npx convex codegen --app ./my-app       # step 1: separate JS tool
neovex serve --convex-app-dir ./my-app   # step 2: hope artifacts are fresh
```

After this plan:

```bash
neovex serve --app-dir ./my-app          # one step: codegen + serve
```

Or, when the developer wants explicit control:

```bash
neovex codegen --app ./my-app            # explicit codegen
neovex serve --app-dir ./my-app --skip-codegen  # serve pre-built artifacts
```

---

## Roadmap

### C1: `neovex codegen` subcommand

Add a `codegen` subcommand to the Rust binary that spawns `@neovex/codegen` as
a Node.js subprocess.

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
  1. Resolves the codegen script path (see C1 Node resolution below)
  2. Spawns `node <script> --app <app_dir>` as a child process
  3. Inherits stdio so the user sees codegen output directly
  4. Returns the exit code
- Add `Codegen(CodegenCommand)` to the `Command` enum in `main.rs`
- Add `neovex codegen` to `ROOT_HELP_EXAMPLES`

**Node resolution strategy:**

The `neovex codegen` command must find the `@neovex/codegen` entry point. Three
resolution strategies, in priority order:

1. **`{app_dir}/node_modules/@neovex/codegen/src/cli.mjs`** — the app's own
   dependency, guarantees version match
2. **Bundled fallback** — if no local install exists, the Rust binary ships a
   known-good codegen script path relative to its own install prefix (details
   in C1 implementation)
3. **`npx @neovex/codegen`** — last resort, slower but always works if npm is
   installed

For the initial implementation, strategy 1 is sufficient. The app must have
`@neovex/codegen` as a dependency (it already does, transitively via `convex`
or directly). Strategy 2 and 3 can be deferred to the distribution plan.

**Verification:**
- `neovex codegen --app ./demos/convex/html` succeeds and produces
  `.neovex/convex/` artifacts
- `neovex codegen` in a directory with `neovex/` or `convex/` source root works
- `neovex codegen` in a directory with no source root fails with a clear error
  (from the JS resolver)
- `neovex --help` shows `codegen` in the command list
- Existing `npx convex codegen` still works unchanged

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
  backwards compatibility shim (pre-launch, breaking changes preferred)
- Update `boot.rs` to reference the renamed field
- Update all Makefile targets, docs, and test fixtures that reference
  `--convex-app-dir` to use `--app-dir`
- Update `docs/reference/cli.md` and `SERVE_HELP_EXAMPLES`

**Verification:**
- `neovex serve --app-dir ./my-app` works
- `neovex serve --help` shows `--app-dir`
- All Makefile targets use `--app-dir`
- All serve tests still pass

---

### C3: Auto-codegen on serve

When `--app-dir` is provided, the serve command automatically runs codegen
before loading artifacts unless `--skip-codegen` is passed.

**Implementation:**

- Add a `--skip-codegen` flag to `ServeCommand`:
  ```rust
  /// Skip automatic codegen before serving. Use when artifacts are
  /// pre-built by a separate build step.
  #[arg(long, default_value_t = false)]
  pub(crate) skip_codegen: bool,
  ```
- In `boot.rs`, before `load_convex_registry()`, call `run_codegen_command()`
  when `app_dir` is `Some` and `skip_codegen` is `false`
- If codegen fails, abort serve with the codegen error (do not fall through to
  loading stale or missing artifacts)
- Print a brief status line before codegen: `Generating runtime artifacts...`
- Print a brief status line after codegen: `Codegen complete, starting server.`

**Verification:**
- `neovex serve --app-dir ./my-app` runs codegen then serves (one command)
- `neovex serve --app-dir ./my-app --skip-codegen` skips codegen
- `neovex serve` (no app dir) does not attempt codegen
- If codegen fails, serve does not start and the error is visible
- `make convex-demo` still works (can switch to single-command flow or keep
  explicit `npx convex codegen` + `--skip-codegen`)

---

### C4: Clear error on missing artifacts

When `--app-dir` is provided with `--skip-codegen` and the `.neovex/convex/`
directory is missing or incomplete, the serve command must produce a clear,
actionable error instead of panicking.

**Implementation:**

- In `load_convex_registry()`, when `from_app_dir()` fails because files are
  missing, produce a structured error:
  ```text
  Error: No runtime artifacts found at <app_dir>/.neovex/convex/.

  Run "neovex codegen --app <app_dir>" to generate them, or remove
  --skip-codegen to generate automatically on serve.
  ```
- Check for the specific case of a directory that exists but has stale or
  incomplete files (e.g., `functions.json` present but `bundle.mjs` missing)

**Verification:**
- `neovex serve --app-dir ./empty-dir --skip-codegen` gives the actionable
  error
- The error message includes the exact `neovex codegen` command to run

---

### C5: Makefile and docs alignment

Update all developer-facing surfaces to reflect the new CLI shape.

**Implementation:**

- **Makefile:** Update `convex-demo` and related targets to use
  `neovex serve --app-dir` (can keep the explicit two-step or switch to
  auto-codegen)
- **`docs/reference/cli.md`:** Add `neovex codegen` section, update
  `neovex serve` flags, document `--skip-codegen`
- **`docs/convex/compatibility.md`:** Document that `convex codegen` and
  `neovex codegen` are equivalent entry points to the same pipeline
- **`SERVE_HELP_EXAMPLES` in `cli_ux.rs`:** Add codegen and app-dir examples
- **`ROOT_HELP_EXAMPLES` in `cli_ux.rs`:** Add `neovex codegen` line

**Verification:**
- All doc examples are accurate and runnable
- `neovex --help` and `neovex serve --help` reflect the new surface

---

## Verification Contract

Each roadmap item must satisfy before closing:

- `cargo fmt --all --check` — green
- `make clippy` — green
- `make test` — green
- `npm run test --workspaces --if-present` — green
- Manual verification described per item

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
| `crates/neovex-bin/src/serve/mod.rs` | Rename `convex_app_dir` → `app_dir` with alias, add `skip_codegen` |
| `crates/neovex-bin/src/serve/boot.rs` | Auto-codegen before `load_convex_registry()`, updated field name |
| `crates/neovex-bin/src/cli_ux.rs` | Updated help examples |
| `Makefile` | Updated demo targets |
| `docs/reference/cli.md` | Add `neovex codegen`, update serve flags |
| `docs/convex/compatibility.md` | Document CLI equivalence |

### Files not modified

| File | Reason |
|------|--------|
| `packages/codegen/**` | JS codegen pipeline is unchanged; Rust spawns it as-is |
| `packages/convex/src/cli.mjs` | `convex codegen` stays as a standalone compat path |
| `packages/neovex/**` | SDK unchanged |
| `crates/neovex-server/**` | Registry loading contract unchanged (still `.neovex/convex/`) |

---

## DX Lifecycle After This Plan

```
                           neovex codegen --app .
                           ┌──────────────────────┐
                           │  Spawns Node.js with  │
                           │  @neovex/codegen      │
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
                           neovex serve --app-dir .
                           ┌──────────────────────┐
                           │  (auto-codegen first  │
                           │   unless --skip)      │
                           │                       │
                           │  Loads .neovex/convex/ │
                           │  ├─ functions.json    │
                           │  ├─ schema.json       │
                           │  ├─ bundle.mjs        │
                           │  └─ bundle.sha256     │
                           │                       │
                           │  Starts HTTP/WS       │
                           │  server               │
                           └──────────────────────┘
```

### Command equivalence diagram

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
   spawns Node)           Convex compat)          direct)
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
| C1 | `pending` | | |
| C2 | `pending` | | |
| C3 | `pending` | | |
| C4 | `pending` | | |
| C5 | `pending` | | |
