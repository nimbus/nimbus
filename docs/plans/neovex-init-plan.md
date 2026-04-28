# Plan: `neovex dev` as Single Entry Point + `neovex init`

Canonical execution plan for zero-friction `neovex dev` onboarding.
The goal: a developer who has never seen Neovex can go from `brew install`
to live reactive data in under 3 minutes with no manual file creation.

## Status

- **Plan status:** `in_progress`
- **Control item:** `—`
- **Status values:** `pending`, `in_progress`, `done`, `blocked`
- **Primary source of truth:** this file plus the current git worktree.
- **Checkpoint rule:** every work session that changes implementation state
  must update the roadmap item status and the execution log before stopping.

## Plan Ownership And Canonical Inputs

This plan owns `neovex init`, `neovex dev` auto-scaffold, and dev-mode
auto-tenant creation. Phase 2+ (React template, `npm create neovex`) are
scoped here but not owned until Phase 1 is `done`.

Hard deps (all landed):
- `neovex dev` watch loop
- `neovex codegen`
- `@neovex/codegen` package (v0.1.22)

Soft deps:
- `neovex/` source root (experimental, not required for Phase 1)

Implementation work must keep these source inputs open:

- Top-level repo references: `README.md`, `ARCHITECTURE.md`, `docs/README.md`,
  `docs/plans/README.md`.
- CLI reference: `docs/operating/cli.md`.
- Convex compatibility: `docs/adapters/convex/compatibility.md`,
  `docs/adapters/convex/ai-guidelines.md`.
- Module structure: `crates/neovex-bin/src/` (dev.rs, start/, codegen.rs,
  main.rs).
- JS packages: `packages/convex/package.json`, `packages/codegen/package.json`.
- Server tenant API: `crates/neovex-server/src/http/tenants.rs`,
  `crates/neovex-server/src/router.rs`.

## Autonomous Execution Contract

This plan is designed for agent-driven execution with minimal human
intervention. Each roadmap item must be completable in a single context window
using only the plan, the git worktree, and the source files.

## Control Plan Rules

1. Read `AGENTS.md`, `README.md`, `ARCHITECTURE.md`, `docs/README.md`,
   `docs/plans/README.md`, and this plan before starting a roadmap item.
2. Run `git status --short` before choosing work. If the worktree is dirty,
   reconcile before editing.
3. If any roadmap item is `in_progress`, resume it. If none, pick the first
   `pending` item in roadmap order whose hard deps are `done`.
4. Mark exactly one item `in_progress` before implementation. Do not advance
   another item until the active item is `done` or `blocked`.
5. A roadmap item is not `done` until its verification is recorded in the
   execution log.

## Verification Contract

Every completed item must leave durable evidence:

- The roadmap item status is updated.
- The execution log records the date, item, files touched, and verification.
- Focused tests cover the changed behavior.
- Run `cargo fmt --all --check` and `make clippy` after each item.
- Run `make test` for items that change Rust behavior.
- Run `npm run typecheck` and `npm run test` for items that change JS
  templates or codegen output.

---

## Problem

The README quick start shows:

```bash
brew install agentstation/tap/neovex
neovex dev
```

This implies a developer can install and run `neovex dev` to see it work.
In reality, between install and `neovex dev` the developer must:

1. Create a project directory
2. Create `convex/schema.ts` with a schema definition
3. Create `convex/messages.ts` with query/mutation functions
4. Create `package.json` with the `convex` dependency
5. Run `npm install`
6. Know that the `_generated/` imports will be created by codegen

Neovex has nothing to bridge this gap.

## Competitive analysis

### Convex CLI subcommands

The Convex CLI has three command groups:

| Group | Commands |
|-------|----------|
| **Configure** | `init`, `logout` |
| **Develop** | `dev`, `dashboard`, `docs`, `run`, `logs`, `import`, `export`, `data`, `insights`, `env` |
| **Deploy** | `deploy`, `codegen` |

**`convex dev` is the single entry point.** On first run in an uninitialized
project, it handles everything: prompts for login, creates the project,
scaffolds the `convex/` directory with starter functions, writes `.env.local`
with the deployment URL, and starts the watch loop. No separate `convex init`
required. On subsequent runs it just watches and syncs.

`convex init` exists as a separate command for reconfiguration (switching
teams, changing deployment targets) — not for first-run scaffolding.

### Other projects

| Project | Init pattern | Dev handles init? |
|---------|-------------|-------------------|
| **Convex** | `convex dev` does everything on first run | **Yes** — scaffolds, configures, starts |
| **Wrangler** | `wrangler init` required first | No — `wrangler dev` fails without config |
| **Bun** | `bun init` required first | No — `bun run` needs package.json |
| **Hono** | `npm create hono@latest` scaffolds | N/A — no persistent `hono dev` command |
| **tRPC** | `npx create-next-app --example` | N/A — no `trpc dev` command |

**Convex is the only project where `dev` is the complete entry point.** This is
the pattern to follow — Neovex's `dev` command is the direct equivalent.

---

## Target UX

### Primary flow: `neovex dev` does init

```bash
brew install agentstation/tap/neovex
mkdir my-app && cd my-app
neovex dev
```

On first run with no source root, `neovex dev`:

```
$ neovex dev

No convex/ or neovex/ source root found in /Users/dev/my-app.

Creating starter project...
  convex/schema.ts
  convex/messages.ts
  .gitignore
  package.json
  tsconfig.json

Run `npm install` to install dependencies, then `neovex dev` again.
```

After `npm install`:

```
$ neovex dev

Neovex dev ready to start
Local:   http://localhost:3210/
App dir: /Users/dev/my-app
Data:    /Users/dev/my-app/.neovex/dev
Watch:   /Users/dev/my-app/convex

✓ Codegen complete
✓ Tenant "demo" ready
✓ Server listening on http://localhost:3210

Try it:
  curl -X POST localhost:3210/convex/demo/query \
    -H "Content-Type: application/json" \
    -d '{"name":"messages:list","args":{}}'
```

### `neovex init` as standalone

`neovex init` also exists as a standalone command for cases where the developer
wants to scaffold without starting the server, or wants to scaffold into a
specific directory:

```bash
neovex init my-app
cd my-app
npm install
neovex dev
```

### Full-stack flow (Phase 2)

```bash
mkdir my-app && cd my-app
neovex dev --template react
npm install
neovex dev         # terminal 1: backend
npm run dev        # terminal 2: Vite frontend
```

---

## What gets scaffolded

### Backend-only (default)

```
my-app/
├── convex/
│   ├── schema.ts          # messages table with author + body
│   └── messages.ts        # list query + send mutation
├── .gitignore             # .neovex/, node_modules/
├── package.json           # { "type": "module", dependencies: { "convex": "..." } }
└── tsconfig.json          # moduleResolution: bundler, target: esnext
```

#### `convex/schema.ts`

```typescript
import { defineSchema, defineTable } from "convex/server";
import { v } from "convex/values";

export default defineSchema({
  messages: defineTable({
    author: v.string(),
    body: v.string(),
  }),
});
```

#### `convex/messages.ts`

```typescript
import { v } from "convex/values";
import { query, mutation } from "./_generated/server";

export const list = query({
  args: {},
  handler: async (ctx) => await ctx.db.query("messages").take(50),
});

export const send = mutation({
  args: { author: v.string(), body: v.string() },
  handler: async (ctx, { author, body }) =>
    await ctx.db.insert("messages", { author, body }),
});
```

#### `.gitignore`

```
.neovex/
node_modules/
```

#### `tsconfig.json`

```json
{
  "compilerOptions": {
    "target": "esnext",
    "module": "esnext",
    "moduleResolution": "bundler",
    "strict": true,
    "skipLibCheck": true,
    "allowImportingTsExtensions": true,
    "noEmit": true
  },
  "include": ["convex"]
}
```

#### `package.json`

```json
{
  "name": "my-app",
  "private": true,
  "type": "module",
  "dependencies": {
    "convex": "^{{CONVEX_VERSION}}"
  },
  "devDependencies": {
    "@neovex/codegen": "^{{CODEGEN_VERSION}}"
  }
}
```

`convex` provides the server function types (`convex/server`, `convex/values`).
`@neovex/codegen` is required for `neovex dev` to run codegen outside the
monorepo — the Rust binary invokes `node` which imports `@neovex/codegen`
from `node_modules`. The template uses a `.tmpl` extension so the scaffold
logic can fill in current package versions at embed time. A `build.rs` in
`neovex-bin` reads the version from `packages/convex/package.json` and
`packages/codegen/package.json` at compile time and emits
`env!("NEOVEX_CONVEX_VERSION")` / `env!("NEOVEX_CODEGEN_VERSION")` for
substitution (see Decision 5).

### React template (Phase 2)

Adds to the above:

```
my-app/
├── convex/              # same as above
├── src/
│   ├── main.tsx         # ReactDOM.createRoot + ConvexProvider
│   └── App.tsx          # useQuery + useMutation example
├── index.html           # Vite entry point
├── package.json         # adds react, react-dom, vite, @vitejs/plugin-react
├── tsconfig.json
└── vite.config.ts
```

---

## `neovex dev` init behavior

When `neovex dev` runs and no `convex/` or `neovex/` source root is found:

1. **Check if the directory is empty or has no source root.** Walk up ancestors
   as today. If no source root found anywhere, enter the init flow.

2. **Scaffold the starter project.** Write the template files into the current
   directory (or the directory specified by `--app-dir`). Print each file
   created.

3. **Check for `node_modules/`.** If the `convex` package is not resolvable,
   print "Run `npm install` to install dependencies, then `neovex dev` again."
   and exit. Do not attempt to run `npm install` — the Rust binary should not
   depend on Node.js being installed for the init path, and the developer may
   prefer yarn/pnpm/bun.

4. **If dependencies are installed, continue normally.** Run codegen, start
   server, start watch loop.

### Existing project (has some files, no `convex/`)

If the directory has no `convex/` or `neovex/` source root but already
contains some project files, `neovex dev` still scaffolds the `convex/`
directory and its contents. For root-level config files (`package.json`,
`tsconfig.json`, `.gitignore`), the scaffold logic checks each output path
individually and skips any file that already exists. Skipped files are
reported so the developer knows what was preserved:

```
No convex/ source root found. Creating starter functions...
  convex/schema.ts
  convex/messages.ts
  skipped: package.json (already exists)
  skipped: tsconfig.json (already exists)
  skipped: .gitignore (already exists)

Add the convex dependency to your existing project:
  npm install convex @neovex/codegen
Then run `neovex dev` again.
```

If the scaffold wrote a new `package.json` (no existing one was found),
the message says `npm install` instead, since the template already
includes both dependencies.

This handles the common case of adding Neovex to an existing React or
Node.js project without clobbering the developer's existing config files.

### Safety: refuse to scaffold into suspicious directories

Before scaffolding, check that the current directory looks intentional:

- If the directory is `$HOME`, error: "Refusing to scaffold into your home
  directory. Create a project directory first: `mkdir my-app && cd my-app`"
- If the directory is `/`, `/tmp`, or a system path, error similarly.

This prevents accidental scaffolding when a developer runs `neovex dev` from
the wrong directory.

### Skip scaffolding

Scaffolding only applies to `neovex dev`. Protocol adapters (MongoDB,
Firebase, Cloud Functions, Native) use `neovex start`, which is a separate
command that never scaffolds, never watches, and never auto-creates tenants.
`neovex start` is the operator path; `neovex dev` is the developer inner
loop for the Convex-style function authoring workflow.

If the developer passes `--skip-codegen` to `neovex dev`, the init flow is
also suppressed — scaffolded files need codegen to produce `_generated/`,
so scaffolding without codegen would leave the project in a broken state.
If the developer wants to scaffold without starting the server, use
`neovex init` instead.

A dedicated `--no-init` flag may be added later if there's demand, but
`--skip-codegen` covers the known use case (running `neovex dev` as a
lightweight server without the function authoring flow).

If the developer explicitly passes `--app-dir` pointing to a non-empty
directory that has no source root, `neovex dev` should error rather than
scaffold into an unexpected location. If the `--app-dir` target is empty
(or doesn't exist yet), scaffold normally — the developer clearly intended
to create a project there.

---

## `neovex init` command design

```bash
neovex init [directory] [--template <name>] [--source-root convex|neovex]
```

| Flag | Default | Meaning |
|------|---------|---------|
| `directory` | `.` (current directory) | Where to create the project |
| `--template` | `backend` | Which template to scaffold (`backend`, `react` in Phase 2) |
| `--source-root` | `convex` | Whether to use `convex/` or `neovex/` source root (Phase 1: only `convex` is implemented; `neovex` exits with an advisory message) |

### Behavior

1. If the target directory doesn't exist, create it.
2. If `convex/` or `neovex/` already exists, error: "Source root already exists.
   Run `neovex dev` to start the development server."
3. Write the template files.
4. Print next steps.

### Implementation

The scaffold logic lives in a shared module used by both `neovex init` and
`neovex dev`. Template files are embedded in the binary via `include_str!()`
from a `templates/` directory in the crate.

```
crates/neovex-bin/
├── src/
│   ├── init.rs              # InitCommand + shared scaffold logic
│   ├── dev.rs               # calls scaffold logic when no source root
│   ├── main.rs              # add Command::Init
│   └── ...
└── templates/
    └── backend/
        ├── convex/
        │   ├── schema.ts
        │   └── messages.ts
        ├── gitignore          # installed as .gitignore (no dot in source tree)
        ├── package.json.tmpl  # version filled at embed time via build.rs
        └── tsconfig.json
```

---

## Auto-tenant creation in dev mode

Today, the Convex adapter requires tenants to exist before clients connect.
In dev mode this is unnecessary friction.

**Change:** `neovex dev` auto-creates a `demo` tenant on startup.

**Recommended approach:** Add an `auto_tenant: Option<String>` field to
`StartCommand` with `#[arg(skip)]` so it is not exposed as a CLI flag on
`neovex start` (same pattern as the existing `deploy_admin_token` field).
The dev command sets it to `Some("demo".to_string())` when constructing
the `StartCommand`; `neovex start` never populates it.

In `start/boot.rs`, after `Service::new_with_persistence_config` succeeds
and before the HTTP listener binds, check `command.auto_tenant`. If set,
call `service.create_tenant_async(tenant_id).await` directly. If the
Convex registry is loaded, also call
`registry.apply_schema_to_tenant_async()` on the new tenant — the same
two calls that `http/tenants.rs::create_tenant` makes. If the tenant
already exists, ignore the error silently.

This is server-internal: no HTTP round-trip, no admin token, no
server-readiness wait. The tenant is guaranteed to exist before the first
client request arrives.

**Why not POST from the watch loop?** The `/api/tenants` route lives behind
`build_local_admin_router` and requires the `x-neovex-admin-token` header
(a local-server credential, separate from the deploy token the dev command
generates). The watch loop also races with `run_start_command` via
`tokio::select!` and has no server-readiness signal — there is no existing
mechanism for the watch loop to know the HTTP listener is bound. Both
problems disappear when tenant creation happens inside the server boot path.

This eliminates the gap between `neovex dev` starting and a Convex client
being able to connect to `http://localhost:3210/convex/demo`.

---

## Documentation updates

When auto-init and auto-tenant land, three docs need to change to reflect the
new developer journey. The goal: every doc a new developer touches should
show the zero-file-creation path as the primary flow.

### `README.md` — Quick start

The quick start currently shows the developer manually writing
`convex/messages.ts` and then running `neovex dev`. After auto-init,
`neovex dev` creates those files. The quick start should show the install →
scaffold → run flow with no manual file creation.

**Before (current):**

```
brew install agentstation/tap/neovex
[manual code block: convex/messages.ts]
neovex dev
[manual code block: useQuery in React]
```

**After:**

```bash
brew install agentstation/tap/neovex
mkdir my-app && cd my-app
neovex dev
```

```
No convex/ source root found. Creating starter project...
  convex/schema.ts
  convex/messages.ts
  .gitignore
  package.json
  tsconfig.json

Run `npm install` to install dependencies, then `neovex dev` again.
```

```bash
npm install
neovex dev
```

```
✓ Codegen complete
✓ Tenant "demo" ready
✓ Server listening on http://localhost:3210
```

Then show what you can do with it — call the scaffolded Convex function
via curl and a React `useQuery` one-liner:

```bash
curl -X POST localhost:3210/convex/demo/query \
  -H "Content-Type: application/json" \
  -d '{"name":"messages:list","args":{}}'
```

```tsx
// In your React app — data updates in real time
const messages = useQuery(api.messages.list);
```

The scaffolded code (`convex/schema.ts`, `convex/messages.ts`) should appear
below the quick start as a "What's inside" or "What gets created" section so
the developer sees the code they can now edit. This is the hook — they see
real TypeScript they can modify and immediately re-run `neovex dev` to see
changes.

The "Or try it with curl" section uses `neovex start` (not `neovex dev`),
so it does NOT get auto-tenant. Keep the manual `POST /api/tenants` call
in that section — it's the correct flow for `neovex start`. No changes
needed to the curl section.

### `docs/getting-started.md` — Server-side functions path

Currently says "Write TypeScript queries and mutations." After auto-init, the
primary path is:

```markdown
## Server-side functions

Run `neovex dev` in an empty directory — it scaffolds a starter project with
a schema and server functions, runs codegen, and serves everything on
`localhost:3210`.

```bash
mkdir my-app && cd my-app
neovex dev          # scaffolds on first run
npm install
neovex dev          # codegen + server + watch
```

This is the recommended path for new projects. Your frontend connects with
`useQuery` and `useMutation` — data updates in real time without REST
endpoints, GraphQL, or polling.

**[Full tutorial →](adapters/convex/)**
```

### `docs/adapters/convex/README.md` — Quick start

The current quick start has 5 steps that require the developer to manually
create `convex/schema.ts`, `convex/messages.ts`, and the project layout.
After auto-init, the quick start becomes:

**1. Create a project:**

```bash
mkdir my-app && cd my-app
neovex dev
```

This scaffolds `convex/schema.ts`, `convex/messages.ts`, `package.json`, and
`tsconfig.json`.

**2. Install dependencies:**

```bash
npm install
```

**3. Start the dev server:**

```bash
neovex dev
```

Codegen runs, the `demo` tenant is created, and the server starts on port
3210. Changes to `convex/` rebuild automatically.

**4. Connect your frontend** (same React example as today).

The existing code-first lead (query + mutation code block at the top of the
page) stays, but the description changes from "Write TypeScript functions"
to something like "These are the server functions `neovex dev` creates for
you — edit them and changes rebuild instantly."

The manual schema and messages code blocks move from "steps to do" to "what
got scaffolded" — the developer sees them as reference, not as instructions
to copy-paste.

#### Configuration section

The current text says:

> Tenants must exist before the Convex client connects. Create via
> `POST /api/tenants`.

After auto-tenant, this becomes:

> In dev mode, `neovex dev` auto-creates a `demo` tenant. Your Convex
> client connects to `http://localhost:3210/convex/demo` immediately.
> In production, pre-provision tenants via the admin API or CLI.

### Other docs — no changes needed

- **`docs/adapters/mongodb/README.md`** — Uses `neovex start`, unaffected.
- **`docs/adapters/firebase/README.md`** — Uses `neovex start`, unaffected.
- **`docs/adapters/cloud-functions/README.md`** — Uses `neovex start`, unaffected.
- **`docs/adapters/native/README.md`** — Uses `neovex start`, unaffected. The
  native quick start shows manual tenant creation which is correct for the
  `neovex start` path.

---

## Neovex CLI subcommand comparison with Convex

Current Neovex CLI surface mapped against the Convex CLI:

| Convex | Neovex | Status |
|--------|--------|--------|
| `convex dev` | `neovex dev` | Landed (codegen + server + watch). Port 3210. Missing: auto-init scaffold, auto-tenant (this plan). |
| `convex init` | `neovex init` | **Not implemented.** This plan. |
| `convex deploy` | `neovex deploy` | Landed. |
| `convex codegen` | `neovex codegen` | Landed. |
| `convex run` | — | Not implemented. Run a function from the CLI. |
| `convex import` | — | Not implemented. Import data from JSON/CSV. |
| `convex export` | — | Not implemented. Export data. |
| `convex data` | — | Not implemented. Inspect table data. |
| `convex logs` | — | Not implemented. Tail function logs. |
| `convex env` | — | Not implemented. Manage environment variables. |
| `convex dashboard` | — | Not planned (desktop UI plan covers this). |
| `convex docs` | — | Not needed (static docs). |
| — | `neovex start` | Neovex-specific. Operator server-start (one-shot codegen preflight if `--app-dir` set, no watch, no scaffold, no auto-tenant). Port 8080 by default. |
| — | `neovex compose ...` | Neovex-specific. Service lifecycle management. |
| — | `neovex machine ...` | Neovex-specific. macOS VM management. |
| — | `neovex token rotate` | Neovex-specific. Admin token lifecycle. |

### Priority for developer onboarding

The commands that matter most for the `neovex dev` experience, in order:

1. **`neovex init` (this plan)** — unblocks the README quick start
2. **auto-tenant in dev mode** — unblocks Convex client connection
3. **`neovex run`** — execute a function from the CLI without a frontend
4. **`neovex logs`** — see function execution output in the terminal
5. **`neovex data`** — inspect table contents from the CLI
6. **`neovex import`** — seed data for demos and testing

Items 3-6 are not in scope for this plan but are the natural follow-on
commands that complete the developer inner loop.

---

## Decisions

1. **Default source root: `convex/`.** The `neovex/` source root is
   experimental. Scaffold into `convex/` until `neovex/` is promoted.

2. **`npm install` is a separate step.** The Rust binary does not depend on
   any package manager. Scaffold the files, tell the developer to install,
   exit. On the second `neovex dev` run, dependencies are present and
   everything works. Same pattern as `convex dev`.

3. **Node.js is required.** `neovex dev` calls `node` to run
   `@neovex/codegen`. If Node.js is missing, fail with a clear message:
   "Node.js is required for codegen. Install it from https://nodejs.org/"

4. **Scaffold uses `convex` + `@neovex/codegen` as dependency names.** These
   match the current `packages/convex/` and `packages/codegen/` package
   names. `@neovex/codegen` is required because outside the monorepo the
   Rust binary falls back to `import("@neovex/codegen")` from
   `node_modules` (no workspace `packages/codegen/src/main.mjs` to find).
   Package publishing and npm registry naming are distribution concerns
   resolved separately from this plan.

5. **Template versions are baked in at compile time.** The
   `package.json.tmpl` template uses `{{CONVEX_VERSION}}` and
   `{{CODEGEN_VERSION}}` placeholders. A `build.rs` in `neovex-bin` reads
   the `"version"` field from `packages/convex/package.json` and
   `packages/codegen/package.json` at compile time and emits them as
   `env!("NEOVEX_CONVEX_VERSION")` / `env!("NEOVEX_CODEGEN_VERSION")`. The
   scaffold module substitutes these into the template when writing
   `package.json`. This works for both workspace builds and release builds
   (the `packages/` directories are present in the source tree at build
   time). The `.tmpl` extension prevents the file from being treated as a
   real `package.json` by tooling.

6. **`--source-root neovex` is deferred to Phase 2.** The `neovex/` source
   root is experimental and the template imports (`convex/server`,
   `convex/values`) only match the `convex/` root. The `--source-root` flag
   on `neovex init` is accepted in Phase 1 but only `convex` is
   implemented; passing `neovex` prints "The neovex/ source root is
   experimental and not yet supported by the scaffold templates." and exits.

7. **Scaffold skips files that already exist.** Rather than checking only
   for `package.json` to decide which files to skip, the scaffold logic
   checks each output path individually. If a file already exists at the
   target path, that file is skipped and the developer is told. This covers
   all edge cases (existing `.gitignore`, existing `tsconfig.json` without
   `package.json`, etc.) without special-case heuristics.

---

## Phase Status Ledger

| Phase | Status | Items | Done when |
|-------|--------|-------|-----------|
| P1: Build infrastructure | `done` | I1 | `build.rs` emits package versions as compile-time env vars |
| P2: Scaffold module | `done` | I2 | Shared scaffold module with embedded templates, per-file skip logic, safety checks |
| P3: `neovex dev` auto-init | `done` | I3, I4 | `neovex dev` scaffolds when no source root, checks for node_modules, handles existing projects and edge cases |
| P4: `neovex init` command | `pending` | I5 | Standalone `neovex init` command using shared scaffold module |
| P5: Auto-tenant | `pending` | I6 | `neovex dev` auto-creates `demo` tenant via server-internal boot path |
| P6: Documentation | `pending` | I7 | README, getting-started, and Convex adapter docs updated |
| P7: CLI reference | `pending` | I8 | `docs/operating/cli.md` updated with `neovex init` and auto-init behavior |

## Roadmap Items

### P1 Work Queue: Build Infrastructure

| Item | Status | Hard deps | Completion gate |
|------|--------|-----------|-----------------| 
| I1: `build.rs` version embedding | `done` | none | `build.rs` in `neovex-bin` reads `"version"` from `packages/convex/package.json` and `packages/codegen/package.json`, emits `NEOVEX_CONVEX_VERSION` and `NEOVEX_CODEGEN_VERSION` as compile-time env vars. `cargo build -p neovex-bin` succeeds. Template placeholder substitution confirmed via unit test. |

### P2 Work Queue: Scaffold Module

| Item | Status | Hard deps | Completion gate |
|------|--------|-----------|-----------------| 
| I2: Shared scaffold module with embedded templates | `done` | I1 | `init.rs` module in `neovex-bin` with: embedded backend templates via `include_str!()`, `scaffold_project()` function that writes files with per-file skip logic (Decision 7), safety check for `$HOME`/`/`/`/tmp`, `package.json.tmpl` version substitution using `env!()` vars, `--source-root neovex` advisory exit (Decision 6). Unit tests for: all files written to empty dir, per-file skip when file exists, safety refusal for `$HOME`, version substitution in `package.json`. |

### P3 Work Queue: `neovex dev` Auto-Init

| Item | Status | Hard deps | Completion gate |
|------|--------|-----------|-----------------| 
| I3: `neovex dev` scaffold integration | `done` | I2 | When `neovex dev` finds no `convex/` or `neovex/` source root and `--skip-codegen` is not set: calls `scaffold_project()`, checks for `node_modules/convex`, prints install prompt and exits if missing, otherwise continues to codegen + server + watch. Existing behavior unchanged when source root exists. |
| I4: `neovex dev --app-dir` edge cases | `done` | I3 | `--app-dir` pointing to empty or nonexistent dir scaffolds normally. `--app-dir` pointing to non-empty dir without source root errors with clear message. Existing `--app-dir` behavior unchanged when source root exists. |

### P4 Work Queue: `neovex init` Command

| Item | Status | Hard deps | Completion gate |
|------|--------|-----------|-----------------| 
| I5: `neovex init` standalone command | `pending` | I2 | `Command::Init` added to `main.rs`. `InitCommand` struct with `directory`, `--template`, `--source-root` args. Creates target directory if absent. Errors if `convex/` or `neovex/` already exists. Calls shared `scaffold_project()`. Prints next steps. `neovex init my-app` creates `my-app/` with all template files. `neovex init` in current dir works. |

### P5 Work Queue: Auto-Tenant

| Item | Status | Hard deps | Completion gate |
|------|--------|-----------|-----------------| 
| I6: Dev-mode auto-tenant creation | `pending` | none | `auto_tenant: Option<String>` added to `StartCommand` with `#[arg(skip)]`. Dev command sets `Some("demo")`. In `start/boot.rs`, after `Service::new_with_persistence_config` and before listener bind: creates tenant, applies Convex schema if registry loaded, silently ignores already-exists. A Convex client can connect to `localhost:3210/convex/demo` immediately after `neovex dev` starts. Test: `neovex dev` with app dir creates tenant and serves query. |

### P6 Work Queue: Documentation

| Item | Status | Hard deps | Completion gate |
|------|--------|-----------|-----------------| 
| I7: Update onboarding docs | `pending` | I3, I6 | README quick start shows zero-file-creation path. `docs/getting-started.md` server-side functions shows scaffold flow. `docs/adapters/convex/README.md` quick start uses auto-init. Convex adapter configuration section reflects auto-tenant. README "Or try it with curl" section unchanged (uses `neovex start`). |

### P7 Work Queue: CLI Reference

| Item | Status | Hard deps | Completion gate |
|------|--------|-----------|-----------------| 
| I8: Update `docs/operating/cli.md` | `pending` | I5, I6 | `neovex init` command documented with flags, defaults, and examples. `neovex dev` section updated to describe auto-init scaffold behavior and auto-tenant. Dev command taxonomy entry updated. |

---

## Phase 2+ (Out of Scope for Phase 1)

### Phase 2 — React template

1. Add `--template react` with Vite + React + ConvexProvider scaffold
2. The template includes a working `App.tsx` with `useQuery` and `useMutation`
3. `npm run dev` starts Vite alongside `neovex dev`
4. `--source-root neovex` template support

### Phase 3 — `npm create neovex` (stretch)

1. Publish `create-neovex` to npm
2. `npm create neovex@latest my-app` scaffolds + runs `npm install`
3. Interactive prompt for template selection (like `create-hono`)

---

## Execution Log

| Date | Item | Status | Description | Verification |
|------|------|--------|-------------|--------------|
| 2026-04-27 | — | — | Plan created and audited against codebase | — |
| 2026-04-27 | I4 | `done` | `--app-dir` nonexistent: pre-created before plan resolve. `--app-dir` non-empty without source root: errors. `--app-dir` empty: scaffolds normally. Added `is_dir_empty()` helper. 4 new edge-case tests. | `cargo test -p neovex-bin -- init::tests dev::tests` 31/31 pass, clippy clean |
| 2026-04-27 | I3 | `done` | `run_dev_command()` calls `scaffold_project()` when no source root and `--skip-codegen` not set. Prints file creation/skip output. Checks `node_modules/convex`, prints install prompt and exits if missing. Re-detects source root after scaffold. 3 new dev tests. | `cargo test -p neovex-bin -- init::tests dev::tests` 27/27 pass, `cargo fmt --all --check` clean, `cargo clippy -p neovex-bin --all-targets` clean |
| 2026-04-27 | I2 | `done` | `scaffold_project()` with per-file skip logic, safety checks ($HOME, /, /tmp), version substitution, `check_source_root_flag()`. 9 unit tests: empty dir, skip existing, refuse $HOME/root/tmp, source-root advisory, version substitution. | `cargo test -p neovex-bin -- init::tests` 9/9 pass, `cargo fmt --all --check` clean, `cargo clippy -p neovex-bin --all-targets` clean |
| 2026-04-27 | I1 | `done` | `build.rs` reads versions from `packages/convex/package.json` and `packages/codegen/package.json`, emits `NEOVEX_CONVEX_VERSION` (0.1.22) and `NEOVEX_CODEGEN_VERSION` (0.1.22). Created `init.rs` with constants, template, and `render_package_json()`. Created `templates/backend/` with all 5 template files. | `cargo build -p neovex-bin` succeeds, `cargo test -p neovex-bin -- init::tests` 2/2 pass, `cargo fmt --all --check` clean, `cargo clippy -p neovex-bin --all-targets` clean |
