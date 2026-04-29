# Plan: `neovex init` + `neovex dev` Onboarding

Canonical execution plan for zero-friction Neovex onboarding. The goal: a
developer who has never seen Neovex can go from `brew install` to live
reactive data in under 3 minutes.

## Status

- **Plan status:** `done` (Phase 1 and Phase 1.5 complete)
- **Phase 1:** I1–I8 all `done` — Convex adapter scaffold, auto-tenant,
  docs.
- **Phase 1.5:** I9–I13 all `done` — multi-adapter refactor, Cloud Functions
  adapter, shared npm install, explicit adapter detection.
- **Status values:** `pending`, `in_progress`, `done`, `blocked`
- **Primary source of truth:** this file plus the current git worktree.

## Plan Ownership And Canonical Inputs

This plan owns `neovex init`, `neovex dev` adapter detection, dev-mode
auto-tenant creation, and adapter-specific npm install orchestration.

Hard deps (all landed):
- `neovex dev` watch loop
- `neovex codegen` (handles both Convex and Cloud Functions in a single pass)
- `@neovex/codegen` package (v0.1.22)

Soft deps:
- `neovex/` source root (experimental, not required)

Implementation work must keep these source inputs open:

- Top-level repo references: `README.md`, `ARCHITECTURE.md`, `docs/README.md`,
  `docs/plans/README.md`.
- CLI reference: `docs/operating/cli.md`.
- Convex compatibility: `docs/adapters/convex/compatibility.md`,
  `docs/adapters/convex/ai-guidelines.md`.
- Cloud Functions compatibility: `docs/adapters/cloud-functions/compatibility.md`,
  `docs/adapters/cloud-functions/README.md`.
- Module structure: `crates/neovex-bin/src/` (dev.rs, init.rs, node.rs,
  start/, codegen.rs, main.rs).
- JS packages: `packages/convex/package.json`, `packages/codegen/package.json`.
- Server tenant API: `crates/neovex-server/src/http/tenants.rs`,
  `crates/neovex-server/src/router.rs`.

---

## Current Implementation

### `neovex init <ADAPTER> [DIRECTORY]`

Scaffolds a new Neovex project for the selected adapter. The adapter argument
is a required positional argument — there is no silent default.

```bash
neovex init <ADAPTER> [DIRECTORY] [--source-root convex] [--install]
```

| Argument / Flag | Default | Meaning |
|------|---------|---------|
| `ADAPTER` | *(required)* | Adapter to scaffold: `convex`, `cloud-functions` |
| `DIRECTORY` | `.` (current directory) | Target directory (created if absent) |
| `--source-root` | `convex` | Source root directory name (convex adapter only); `neovex` exits with advisory |
| `--install` | `false` | Bootstrap adapter dependencies after scaffolding |

#### Behavior

1. If adapter is `convex`, validate `--source-root` (reject `neovex` as
   experimental).
2. Create target directory if it does not exist.
3. Check for adapter-specific "already exists" markers:
   - **Convex:** `convex/` or `neovex/` directory → error
   - **Cloud Functions:** `firebase.json` file → error
4. Select adapter-specific template set.
5. Write template files with per-file skip logic (never overwrites existing
   files).
6. When `--install` is set, run `npm install` if the adapter needs Node.js
   dependencies and either:
   - `package.json` declares packages that are missing from the local
     `node_modules/` tree, or
   - the recorded dependency fingerprint no longer matches `package.json` plus
     the npm lockfile (`package-lock.json` or `npm-shrinkwrap.json` when
     present)
   The install flow records its current dependency fingerprint in
   `.neovex/cache/node/dependency-state.json`. For Cloud Functions, npm install
   runs in the `functions/` subdirectory.
7. Print next steps (`cd` + `neovex dev`).

#### Convex adapter templates

```
my-app/
├── convex/
│   ├── schema.ts          # messages table with author + body
│   └── messages.ts        # list query + send mutation
├── .gitignore             # .neovex/, .env.local, node_modules/
├── package.json           # convex + @neovex/codegen
└── tsconfig.json          # ESNext/bundler
```

`package.json` uses `{{CONVEX_VERSION}}` and `{{CODEGEN_VERSION}}`
placeholders substituted at compile time via `build.rs`.

#### Cloud Functions adapter templates

```
my-app/
├── firebase.json          # points to functions/ source
├── functions/
│   ├── package.json       # firebase-functions, firebase-admin, @neovex/codegen
│   ├── tsconfig.json      # Node.js TypeScript config
│   └── src/
│       └── index.ts       # starter HTTP + Firestore trigger handlers
└── .gitignore             # .neovex/, .env.local, node_modules/, lib/
```

`functions/package.json` uses `{{PROJECT_NAME}}` and `{{CODEGEN_VERSION}}`
placeholders. Firebase dependency versions (`firebase-functions ^6.3.0`,
`firebase-admin ^13.0.0`) are hardcoded in the template since they are
third-party packages, not build-system-tracked.

### `neovex dev` adapter detection

`neovex dev` does **not** auto-scaffold. When no compatible adapter is
detected and `--skip-codegen` is not set, it exits with guidance:

```
No compatible adapter detected.

To get started:
  neovex init convex          # Convex adapter
  neovex init cloud-functions # Cloud Functions adapter
  neovex dev
```

#### `DevAdapter` enum

Adapter detection is explicit via a typed enum:

```rust
enum DevAdapter {
    Convex { source_root: PathBuf },
    CloudFunctions { source_root: PathBuf },
}
```

Detection priority (first match wins):
1. `neovex/` directory → `DevAdapter::Convex`
2. `convex/` directory → `DevAdapter::Convex`
3. `firebase.json` file → `DevAdapter::CloudFunctions` (source dir from
   `functions.source` field, defaults to `functions/`)
4. `@google-cloud/functions-framework` in `package.json` dependencies →
   `DevAdapter::CloudFunctions` (source root is the app directory itself)

Each adapter variant provides:
- `name()` — `"convex"` or `"cloud-functions"`
- `source_root()` — path to watch for codegen changes
- `needs_node_dependencies()` — delegates to `node::Adapter::needs_node_dependencies()`
- `npm_install_dir()` — project root for Convex, `functions/` for Cloud Functions

#### App directory detection

`detect_app_dir` walks up ancestor directories looking for:
- `neovex/` or `convex/` directory
- `.neovex/convex/functions.json` file
- `firebase.json` file

Falls back to the current working directory.

### Shared npm install (`node.rs`)

`crates/neovex-bin/src/node.rs` is the single source of truth for adapter
Node.js dependency management:

- `Adapter::needs_node_dependencies()` — returns `true` for `Convex` and
  `CloudFunctions`.
- `auto_install_node_dependencies(app_dir: &Path)` — runs `npm install` if
  `package.json` declares packages that are missing from the local
  `node_modules/` tree.

`neovex dev` always uses this shared module for authoring flows. `neovex init`
only calls it when `--install` is passed. The adapter determines which
directory to pass (project root vs `functions/`).

### Auto-tenant creation

`neovex dev` auto-creates a `demo` tenant on startup via server-internal
boot path. `auto_tenant: Option<String>` on `StartCommand` with
`#[arg(skip)]` — not exposed on `neovex start`. The tenant is created after
`Service::new_with_persistence_config` and before the HTTP listener binds.
If the tenant already exists, the error is silently ignored.

### Template system

Templates are embedded via `include_str!()` from
`crates/neovex-bin/templates/`. Two content types:

- `TemplateContent::Static(&'static str)` — written verbatim
- `TemplateContent::Template(&'static str)` — placeholder substitution via
  `render_template()` which replaces `{{PROJECT_NAME}}`,
  `{{CONVEX_VERSION}}`, and `{{CODEGEN_VERSION}}`

`scaffold_project(target_dir, templates)` takes a template slice parameter.
`adapter_templates(adapter)` selects `CONVEX_TEMPLATE` or
`CLOUD_FUNCTIONS_TEMPLATE` based on the adapter string.

Safety checks refuse to scaffold into `$HOME`, `/`, `/tmp`, or
`/private/tmp`.

---

## Decisions

1. **Default source root: `convex/`.** The `neovex/` source root is
   experimental. Scaffold into `convex/` until `neovex/` is promoted.

2. **`init` scaffolds; `dev` bootstraps.** `neovex init` stops after
   scaffolding by default so the command is deterministic and leaves a clean
   project behind even when the Node toolchain is unavailable. `neovex dev`
   owns automatic dependency bootstrap, and `neovex init --install` is an
   opt-in convenience for teams that want a one-command setup.

3. **Node.js 22 is the verified authoring baseline, not a runtime-only requirement.**
   `neovex dev`, `neovex codegen`, and `neovex init --install` call external
   Node.js tooling today, so Convex and Cloud Functions authoring still
   requires Node.js 22 with `npm`. Runtime-only `neovex start` workflows such
   as MongoDB, Firebase client, or native HTTP/WebSocket access do not require
   the Node toolchain. Upstream Convex and Firebase / Cloud Functions stacks
   still support Node 20, but Neovex does not yet claim a separate verified
   Node 20 compatibility target.

4. **Adapter argument is required and positional.** `neovex init convex`
   not `neovex init --template convex`. No silent defaults — the developer
   makes an explicit choice. This scales cleanly as adapters are added.

5. **Template versions baked in at compile time.** `build.rs` reads versions
   from `packages/convex/package.json` and `packages/codegen/package.json`.
   Third-party versions (firebase-functions, firebase-admin) are hardcoded
   in templates since they are not build-system-tracked.

6. **`--source-root neovex` is deferred.** Accepted but exits with advisory.

7. **Scaffold skips files that already exist.** Per-file check, not
   all-or-nothing. Skipped files are reported to the developer.

8. **`neovex dev` does not auto-scaffold.** It detects adapters and runs
   codegen/watch/server. If no adapter is found, it exits with guidance to
   use `neovex init`. This keeps `dev` predictable — it never creates files
   the developer didn't ask for.

9. **Cloud Functions npm install targets `functions/` subdirectory.** Firebase
   projects have dependencies in `functions/package.json`, not the project
   root. The `adapter_npm_install_dir()` function routes to the correct
   directory per adapter.

10. **Adapter detection is explicit via typed enum.** `DevAdapter::Convex`
    and `DevAdapter::CloudFunctions` — not string matching or implicit
    heuristics. Each variant carries its source root path. Convex takes
    priority when both adapters are present (codegen handles both anyway).

11. **Firebase.json parsing for source directory.** `read_firebase_functions_source()`
    handles the three firebase.json shapes: `functions` as object with
    `source` key, as array of descriptors, or absent (defaults to
    `"functions"`). Uses `serde_json` (already a neovex-bin dependency).

---

## Phase Status Ledger

| Phase | Status | Items | Done when |
|-------|--------|-------|-----------|
| P1: Build infrastructure | `done` | I1 | `build.rs` emits package versions as compile-time env vars |
| P2: Scaffold module | `done` | I2 | Shared scaffold module with embedded templates, per-file skip logic, safety checks |
| P3: `neovex dev` auto-init | `done` | I3, I4 | `neovex dev` scaffolded when no source root (later replaced by adapter guidance in I9) |
| P4: `neovex init` command | `done` | I5 | Standalone `neovex init` command using shared scaffold module |
| P5: Auto-tenant | `done` | I6 | `neovex dev` auto-creates `demo` tenant via server-internal boot path |
| P6: Documentation | `done` | I7 | README, getting-started, and Convex adapter docs updated |
| P7: CLI reference | `done` | I8 | `docs/operating/cli.md` updated with `neovex init` and dev behavior |
| P8: Dev refactor | `done` | I9 | Remove auto-scaffold from dev, add auto npm install, explicit adapter detection |
| P9: Multi-adapter init | `done` | I10, I11 | `neovex init` requires adapter arg, Cloud Functions templates and scaffold |
| P10: Cloud Functions detection | `done` | I12 | `DevAdapter::CloudFunctions` with firebase.json parsing and framework detection |
| P11: Shared npm install | `done` | I13 | `node.rs` module, adapter-specific npm install directories |

## Roadmap Items

### Phase 1 Work Queue (all `done`)

| Item | Status | Description |
|------|--------|-------------|
| I1 | `done` | `build.rs` version embedding for `NEOVEX_CONVEX_VERSION` and `NEOVEX_CODEGEN_VERSION` |
| I2 | `done` | Shared scaffold module with embedded templates, per-file skip, safety checks |
| I3 | `done` | `neovex dev` scaffold integration (later superseded by I9) |
| I4 | `done` | `neovex dev --app-dir` edge cases |
| I5 | `done` | `neovex init` standalone command |
| I6 | `done` | Dev-mode auto-tenant creation (`demo` tenant) |
| I7 | `done` | Onboarding docs update (README, getting-started, Convex adapter) |
| I8 | `done` | CLI reference update (`docs/operating/cli.md`) |

### Phase 1.5 Work Queue (all `done`)

| Item | Status | Description |
|------|--------|-------------|
| I9 | `done` | Remove auto-scaffold from `neovex dev`; add auto npm install via shared `node.rs`; add `DevAdapter` enum replacing implicit source root detection; drop `npm create neovex` from Phase 2 |
| I10 | `done` | Make `neovex init` require explicit adapter positional arg (`convex`, `cloud-functions`); replace `--template` with adapter arg; generalize `TemplateContent::PackageJson` to `TemplateContent::Template(&'static str)` |
| I11 | `done` | Create Cloud Functions template files (`firebase.json`, `functions/package.json.tmpl`, `functions/tsconfig.json`, `functions/src/index.ts`, `gitignore`); add `CLOUD_FUNCTIONS_TEMPLATE` and adapter-specific scaffold logic |
| I12 | `done` | Add `DevAdapter::CloudFunctions` detection: parse `firebase.json` for source dir, detect `@google-cloud/functions-framework` in `package.json`; update `detect_app_dir` for `firebase.json`; adapter-specific `npm_install_dir()` |
| I13 | `done` | Extend `node::Adapter::needs_node_dependencies()` to cover Cloud Functions; update CLI docs and Cloud Functions adapter README with `neovex init cloud-functions` path |

---

## Phase 2+ (Out of Scope)

### Phase 2 — React template

1. Add `--template react` with Vite + React + ConvexProvider scaffold
2. The template includes a working `App.tsx` with `useQuery` and `useMutation`
3. `npm run dev` starts Vite alongside `neovex dev`
4. `--source-root neovex` template support

### Future adapters

Adding a new adapter to `neovex init` requires:

1. Template files under `crates/neovex-bin/templates/<adapter>/`
2. Template constant array in `init.rs` (like `CLOUD_FUNCTIONS_TEMPLATE`)
3. Add adapter string to `InitCommand` value_parser and `adapter_templates()`
4. Add `check_adapter_already_exists()` case
5. Add `adapter_npm_install_dir()` case (if adapter uses npm)
6. Add `DevAdapter` variant in `dev.rs` with detection logic
7. Extend `node::Adapter::needs_node_dependencies()` if the adapter uses npm
8. Update `docs/operating/cli.md`

---

## Execution Log

| Date | Item | Status | Description |
|------|------|--------|-------------|
| 2026-04-27 | I1 | `done` | `build.rs` version embedding |
| 2026-04-27 | I2 | `done` | Shared scaffold module with per-file skip logic and safety checks |
| 2026-04-27 | I3 | `done` | `neovex dev` scaffold integration |
| 2026-04-27 | I4 | `done` | `neovex dev --app-dir` edge cases |
| 2026-04-27 | I5 | `done` | `neovex init` standalone command |
| 2026-04-27 | I6 | `done` | Dev-mode auto-tenant creation |
| 2026-04-27 | I7 | `done` | Onboarding docs update |
| 2026-04-27 | I8 | `done` | CLI reference update |
| 2026-04-27 | I9 | `done` | Remove auto-scaffold from dev, add auto npm install, `DevAdapter` enum, drop `npm create neovex` |
| 2026-04-27 | I10 | `done` | Multi-adapter init: required adapter arg, generalized template system |
| 2026-04-27 | I11 | `done` | Cloud Functions templates and scaffold support |
| 2026-04-27 | I12 | `done` | `DevAdapter::CloudFunctions` detection with firebase.json parsing |
| 2026-04-27 | I13 | `done` | Shared npm install for cloud-functions, docs updates |
