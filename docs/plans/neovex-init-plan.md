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
  curl localhost:3210/api/tenants/demo/query \
    -H "Content-Type: application/json" \
    -d '{"table":"messages","filters":[]}'
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

### Skip scaffolding

When `neovex dev` is used with protocol adapters (MongoDB, Firebase, Native)
that don't need a source root, the developer runs `neovex start` instead.
The `--skip-codegen` flag on `neovex dev` also suppresses the init flow.

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
        ├── package.json.tmpl
        └── tsconfig.json
```

---

## Auto-tenant creation in dev mode

Today, the Convex adapter requires tenants to exist before clients connect.
In dev mode this is unnecessary friction.

**Change:** `neovex dev` auto-creates a `demo` tenant on startup. The dev
command should POST to create the tenant after the server is listening, or
the server should accept an internal flag that creates the tenant at startup
before accepting connections.

This eliminates the gap between `neovex dev` starting and a Convex client
being able to connect to `http://localhost:3210/convex/demo`.

---

## Phases

### Phase 1 — `neovex dev` auto-init + `neovex init` + auto-tenant

1. Add shared scaffold module with embedded backend template
2. `neovex dev` scaffolds when no source root found, checks for node_modules
3. Add `neovex init` as standalone command using the same scaffold module
4. Auto-create `demo` tenant in dev mode on startup
5. Update README quick start to show `neovex dev` as the only command needed
6. Update `docs/adapters/convex/README.md` quick start

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

**Target UX:**

```bash
neovex init --template react
npm install
neovex dev         # terminal 1
npm run dev        # terminal 2
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

---

## Validation

- [ ] `neovex dev` in an empty directory scaffolds all expected files
- [ ] `neovex dev` after scaffold but before `npm install` prints install prompt and exits
- [ ] `neovex dev` after `npm install` runs codegen, starts server, starts watch
- [ ] `neovex dev` auto-creates the `demo` tenant
- [ ] `neovex dev` in a directory with existing `convex/` skips scaffold, runs normally
- [ ] `neovex init` in an empty directory creates all expected files
- [ ] `neovex init` in a directory with existing `convex/` errors cleanly
- [ ] `neovex init my-app` creates the directory if it doesn't exist
- [ ] Codegen creates `_generated/` successfully from the template files
- [ ] A Convex client can connect to `localhost:3210/convex/demo` immediately
- [ ] The `list` query returns an empty array; `send` mutation inserts a document;
  `list` reactively returns the new document
