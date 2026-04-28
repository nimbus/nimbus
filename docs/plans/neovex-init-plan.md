# Plan: `neovex dev` as Single Entry Point + `neovex init`

Canonical execution plan for zero-friction `neovex dev` onboarding.
The goal: a developer who has never seen Neovex can go from `brew install`
to live reactive data in under 3 minutes with no manual file creation.

---

## Status

- **Status:** `not_started`
- **Primary owner:** this plan
- **Hard deps:** `neovex dev` watch loop (landed), `neovex codegen` (landed),
  `@neovex/codegen` package (landed)
- **Soft deps:** `neovex/` source root (experimental, not required for MVP)

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
  curl localhost:3210/convex/demo/api/query \
    -d '{"path":"messages:list","args":{}}'
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
    "convex": "^0.1.0"
  }
}
```

The `convex` dependency refers to the Neovex-published `convex` compatibility
package (see open question 4 below). The template uses a `.tmpl` extension
so the scaffold logic can fill in the current package version at embed time.

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

### Existing project (has `package.json`, no `convex/`)

If the directory has a `package.json` but no `convex/` or `neovex/` source
root, `neovex dev` should still scaffold the `convex/` directory but skip
writing `package.json`, `tsconfig.json`, and `.gitignore`. Instead, print
a message telling the developer to add the `convex` dependency manually:

```
No convex/ source root found. Creating starter functions...
  convex/schema.ts
  convex/messages.ts

Add the convex dependency to your existing project:
  npm install convex
Then run `neovex dev` again.
```

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

When `neovex dev` is used with protocol adapters (MongoDB, Firebase, Native)
that don't need a source root, the developer runs `neovex start` instead.

If the developer passes `--skip-codegen`, the init flow is also suppressed —
scaffolded files need codegen to produce `_generated/`, so scaffolding
without codegen would leave the project in a broken state. If the developer
wants to scaffold without starting the server, use `neovex init` instead.

A dedicated `--no-init` flag may be added later if there's demand, but
`--skip-codegen` covers the known use case (running `neovex dev` as a
lightweight server without the function authoring flow).

If the developer explicitly passes `--app-dir` pointing to a directory
without a source root, `neovex dev` should error rather than scaffold into
an unexpected location.

---

## `neovex init` command design

```bash
neovex init [directory] [--template <name>] [--source-root convex|neovex]
```

| Flag | Default | Meaning |
|------|---------|---------|
| `directory` | `.` (current directory) | Where to create the project |
| `--template` | `backend` | Which template to scaffold (`backend`, `react` in Phase 2) |
| `--source-root` | `convex` | Whether to use `convex/` or `neovex/` source root |

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

**Recommended approach:** The dev watch loop (which already waits for the
server to be listening before running codegen) should POST to
`/api/tenants` to create `"demo"` after the server is ready. This keeps
tenant creation in the existing HTTP API path — no new internal flags, no
special server-side bootstrapping code. If the tenant already exists (409),
ignore the error silently.

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
curl localhost:3210/convex/demo/api/query \
  -d '{"path":"messages:list","args":{}}'
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

## Phases

### Phase 1 — `neovex dev` auto-init + `neovex init` + auto-tenant

1. Add shared scaffold module with embedded backend template
2. `neovex dev` scaffolds when no source root found, checks for node_modules
3. Add `neovex init` as standalone command using the same scaffold module
4. Auto-create `demo` tenant in dev mode on startup
5. Update docs (see "Documentation updates" section below)

**Target UX after Phase 1:**

```bash
brew install agentstation/tap/neovex
mkdir my-app && cd my-app
neovex dev          # scaffolds on first run
npm install         # install dependencies
neovex dev          # codegen + server + watch
```

### Phase 2 — React template

1. Add `--template react` with Vite + React + ConvexProvider scaffold
2. The template includes a working `App.tsx` with `useQuery` and `useMutation`
3. `npm run dev` starts Vite alongside `neovex dev`

**Target UX** (either `neovex dev` or `neovex init` works):

```bash
mkdir my-app && cd my-app
neovex dev --template react   # scaffolds React + Convex project
npm install
neovex dev         # terminal 1: backend on :3210
npm run dev        # terminal 2: Vite frontend on :5173
```

### Phase 3 — `npm create neovex` (stretch)

1. Publish `create-neovex` to npm
2. `npm create neovex@latest my-app` scaffolds + runs `npm install`
3. Interactive prompt for template selection (like `create-hono`)

---

## Neovex CLI subcommand comparison with Convex

Current Neovex CLI surface mapped against the Convex CLI:

| Convex | Neovex | Status |
|--------|--------|--------|
| `convex dev` | `neovex dev` | Landed (watch + codegen + server). Missing: auto-init, auto-tenant. |
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
| — | `neovex start` | Neovex-specific. Operator server-start without codegen. |
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

## Open questions

1. **`convex/` vs `neovex/` as default source root:** The `neovex/` source root
   is experimental. Should `neovex dev` default to `convex/` (stable,
   compatible with Convex migration) or `neovex/` (forward-looking, native
   branding)? Recommendation: default to `convex/` until the `neovex/` root
   is promoted from experimental.

2. **`npm install` as a separate step:** `convex dev` does not run
   `npm install` either. The Rust binary should not depend on a specific
   package manager. Scaffold the files, tell the developer to install, exit.
   On the second `neovex dev` run, dependencies are present and everything
   works.

3. **What if Node.js is not installed?** `neovex dev` needs Node.js for
   codegen (the `@neovex/codegen` package runs via `node`). If Node.js is
   missing, `neovex dev` should fail with a clear message: "Node.js is
   required for codegen. Install it from https://nodejs.org/" — same as
   `convex dev` requires Node.js.

4. **What npm package name for the `convex` dependency?** The in-repo
   `packages/convex/` is `convex@0.1.22` — a Neovex-published compatibility
   package, not the Convex Inc `convex` package on npm. Before launch, this
   package name needs to be resolved:
   - If published as `convex` to npm, it conflicts with the official Convex
     package. Developers who have both would collide.
   - If published as `@neovex/convex`, the import paths change (`import
     from "@neovex/convex/server"` instead of `import from "convex/server"`).
     The `_generated/` files would need to import from the correct package.
   - If the template should use the real Convex npm package (`convex` from
     Convex Inc) pointed at a Neovex server, codegen needs to be compatible
     with that package's type definitions.

   **This must be resolved before implementing Phase 1.** The scaffolded
   `package.json` dependency and the codegen import paths must agree.

---

## Validation

### Implementation

- [ ] `neovex dev` in an empty directory scaffolds all expected files (including `.gitignore`)
- [ ] `neovex dev` after scaffold but before `npm install` prints install prompt and exits
- [ ] `neovex dev` after `npm install` runs codegen, starts server, starts watch
- [ ] `neovex dev` auto-creates the `demo` tenant
- [ ] `neovex dev` in a directory with existing `convex/` skips scaffold, runs normally
- [ ] `neovex dev` in a directory with `package.json` but no `convex/` scaffolds `convex/` only
- [ ] `neovex dev` in `$HOME` or `/` refuses to scaffold with clear error
- [ ] `neovex dev --skip-codegen` suppresses scaffold
- [ ] `neovex init` in an empty directory creates all expected files
- [ ] `neovex init` in a directory with existing `convex/` errors cleanly
- [ ] `neovex init my-app` creates the directory if it doesn't exist
- [ ] Codegen creates `_generated/` successfully from the template files
- [ ] A Convex client can connect to `localhost:3210/convex/demo` immediately
- [ ] The `list` query returns an empty array; `send` mutation inserts a document;
  `list` reactively returns the new document

### Documentation

- [ ] README quick start shows zero-file-creation path (install → mkdir → dev → npm install → dev)
- [ ] README "Or try it with curl" section unchanged (uses `neovex start`, still needs manual tenant)
- [ ] `docs/getting-started.md` server-side functions path shows scaffold flow
- [ ] `docs/adapters/convex/README.md` quick start uses auto-init instead of manual file creation
- [ ] `docs/adapters/convex/README.md` configuration section reflects auto-tenant in dev mode
