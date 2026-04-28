# Plan: Hybrid Directory Layout — `.neovex/` + XDG

Canonical execution plan for restructuring Neovex's directory layout into a
hybrid model: project-local `.neovex/` for project-scoped state (codegen
artifacts, dev database), XDG directories for user-scoped state (license,
auth, shared config), and `.env.local` for deployment identity. This
establishes the deployment-target model that `neovex deploy` will use.

## Status

- **Plan status:** `in_progress`
- **Control item:** `H2`
- **Status values:** `pending`, `in_progress`, `done`, `blocked`
- **Primary source of truth:** this file plus the current git worktree.
- **Checkpoint rule:** every work session that changes implementation state
  must update the roadmap item status and the execution log before stopping.

## Plan Ownership And Canonical Inputs

This plan owns the license extraction from `.neovex/` to XDG, deployment
identity mapping via `.env.local`, and the global config directory at
`~/.config/neovex/`. It does NOT own `neovex deploy` itself — it builds
the directory and identity foundation that `neovex deploy` will use.
`.neovex/`'s internal structure (codegen artifacts, dev database) is
unchanged by this plan.

Hard deps (all landed):
- `neovex dev` watch loop and adapter detection
- `neovex codegen` artifact generation
- `neovex init` scaffold templates (gitignore references `.neovex/`)
- Machine subsystem XDG pattern (`crates/neovex-bin/src/machine/record.rs`)

Implementation work must keep these source inputs open:

- Top-level repo references: `README.md`, `ARCHITECTURE.md`, `docs/README.md`,
  `docs/plans/README.md`.
- CLI reference: `docs/operating/cli.md`.
- Module structure: `crates/neovex-bin/src/` (dev.rs, init.rs, node.rs,
  deploy.rs, codegen.rs, start/, cli_ux.rs, main.rs).
- Server license path: `crates/neovex-server/src/license/mod.rs`,
  `crates/neovex-server/src/license/loading.rs`.
- Machine XDG pattern: `crates/neovex-bin/src/machine/record.rs`.
- Init plan: `docs/plans/neovex-init-plan.md`.

## Autonomous Execution Contract

This plan is designed for agent-driven execution with minimal human
intervention. Each roadmap item must be completable in a single context window
using only the plan, the git worktree, and the source files.

### Quality bar

Every line of code produced under this plan must be enterprise-ready:
idiomatic Rust, correct error handling, exhaustive pattern matching, and
tests that verify behavior — not just compilation. "It compiles" and "tests
pass" are necessary but insufficient. The completion gate for each item
specifies what "done" means; meet every criterion, not a subset.

### NEVER rules

These are hard boundaries. Violating any one means the item is not done.

- **NEVER mark an item `done` without running the verification commands and
  recording the output in the execution log.** "Tests pass" without evidence
  is not verification.
- **NEVER defer work that is inside the completion gate.** If the gate says
  "handle all four `.env.local` states," handle all four. Do not implement
  two and add a TODO for the others.
- **NEVER weaken, skip, or remove a test to make code pass.** Fix the code.
- **NEVER suppress errors, use `unwrap()` in production paths, or paper over
  failures.** Use typed errors and propagate them.
- **NEVER use phrases that signal premature completion:** "good enough for
  now," "can be improved later," "as a first pass," "left as an exercise,"
  "out of scope" (when it is in the completion gate), "for now."
- **NEVER edit a file without reading it first.** Read the file, read its
  tests, read its callers. Then edit.

## Control Plan Rules

1. Read `AGENTS.md`, `README.md`, `ARCHITECTURE.md`, `docs/README.md`,
   `docs/plans/README.md`, and this plan before starting a roadmap item.
2. Run `git status --short` before choosing work. If the worktree is dirty,
   reconcile before editing.
3. If any roadmap item is `in_progress`, resume it. If none, pick the first
   `pending` item in roadmap order whose hard deps are `done`.
4. Mark exactly one item `in_progress` before implementation. Do not advance
   another item until the active item is `done` or `blocked`.
5. A roadmap item is not `done` until every criterion in its completion gate
   is met and its verification is recorded in the execution log.
6. Before declaring an item `done`, re-read the completion gate line by line
   and verify each criterion is satisfied. If any criterion is not met, the
   item is `in_progress`, not `done`.
7. When a test fails, fix the root cause. Do not delete the test, weaken the
   assertion, or change the expected value to match wrong output.
8. When a clippy warning appears, fix the code. Do not allow or suppress the
   warning.

## Verification Contract

Every completed item must leave durable evidence:

- The roadmap item status is updated to `done`.
- The execution log records the date, item, all files touched, and the exact
  verification commands run with their output summary (e.g., "366 passed,
  0 failed" — not just "tests pass").
- Focused tests cover the changed behavior. Each test must verify a specific
  outcome, not just that code runs without crashing. Tests must include:
  - Happy path (expected inputs produce expected outputs).
  - Edge cases specified in the completion gate.
  - Error cases (invalid input produces the correct error, not a panic).
- Run `cargo fmt --all --check` and `make clippy` after each item. Both must
  be clean with zero warnings.
- Run `cargo test -p neovex-bin` for items that change `neovex-bin` code
  (H1, H2, H3).
- Run `cargo test -p neovex-server` for items that change `neovex-server`
  code (H3).
- Run `npm run typecheck` if JS packages are touched (none expected).
- If any verification step fails, the item is not `done`. Fix the issue and
  re-verify.

---

## Problem

### Current: flat `.neovex/` conflates three concerns

`neovex dev` creates a `.neovex/` directory inside the project:

```
my-app/
├── convex/                      # source (developer-authored)
├── .neovex/                     # ← generated/ephemeral, gitignored
│   ├── dev/                     # SQLite databases, control plane
│   │   ├── demo.db
│   │   └── neovex-control.db
│   ├── convex/                  # codegen artifacts
│   │   ├── functions.json
│   │   ├── bundle.mjs
│   │   └── bundle.sha256
│   ├── firebase/                # Cloud Functions artifacts
│   │   ├── artifact.json
│   │   ├── targets.json
│   │   ├── bundle.mjs
│   │   └── bundle.sha256
│   └── license.json             # optional license
├── package.json
└── .gitignore                   # ignores .neovex/
```

This conflates three concerns in one directory:
1. **Project-scoped dev state** (databases, codegen artifacts) — belongs
   with the project, like `.convex/` or `.wrangler/state/`
2. **User-scoped configuration** (license) — belongs at user level, like
   `~/.convex/config.json`
3. **Deployment identity** (which deployment this project targets) — should
   be explicit in `.env.local`, like `CONVEX_DEPLOYMENT`

The fix is not moving everything out of `.neovex/` — the industry consensus
is that project-scoped state belongs in a project-local dotdir. The fix is
separating the three concerns properly.

### What Convex does — the verified reference architecture

Convex's layout, verified from source (`get-convex/convex-backend`,
`filePaths.ts`, `deployment.ts`, `utils.ts`):

```
my-app/
├── convex/                            # developer-authored source
│   └── _generated/                    # generated types (committed to git)
├── .convex/                           # project-scoped dev state (gitignored)
│   ├── .gitignore                     # self-ignoring: "/*"
│   └── local/default/
│       ├── config.json                # ports, admin key, backend version
│       ├── convex_local_backend.sqlite3
│       └── convex_local_storage/
├── convex.json                        # project config (committed)
├── .env.local                         # CONVEX_DEPLOYMENT=dev:slug (gitignored)
└── .gitignore

~/.convex/config.json                  # auth token (user-scoped)
~/.cache/convex/binaries/{version}/    # server binary (shared cache)
~/.cache/convex/dashboard/             # dashboard build (shared cache)
```

Key facts:
- **Convex started with user-level state** at `~/.convex/convex-backend-state/`
  and **moved to project-local** `.convex/` in v1.32.0. The stated reason in
  source: "This allows worktrees/clones to have isolated storage without
  conflicts."
- Auth tokens stay user-level (`~/.convex/config.json`).
- Shared binary cache stays user-level (`~/.cache/convex/binaries/`).
- Deployment identity is in `.env.local` — not inside `.convex/`.
- `.convex/` is self-gitignoring (creates its own `.gitignore` internally).

### Industry consensus — verified patterns

Research across 10+ tools confirms a consistent hybrid pattern:

**Project-local state (industry consensus: keep project-local):**
- `.convex/` — dev database + local deployment config (Convex)
- `.wrangler/state/` — local D1/KV/R2/DO state (Cloudflare Wrangler)
- `.next/` — build output and cache (Next.js)
- `.svelte-kit/` — generated types and build output (SvelteKit)
- `target/` — compiled artifacts (Cargo/Rust)
- `.firebase/` — deployment cache (Firebase CLI)
- Supabase — Docker volumes for local Postgres (not even on disk)

**User-level XDG (industry consensus: keep at user level):**
- `~/.convex/config.json` — auth token (Convex)
- `~/.cache/convex/binaries/` — server binary (Convex)
- `~/.config/planetscale/` — auth tokens (PlanetScale)
- `~/.config/turso/` — auth tokens, settings (Turso)
- `~/.cache/go-build/` — compiled packages (Go, content-addressed)
- `~/.local/share/containers/` — image layers (Podman, content-addressed)

**Why project-local for dev state specifically:**
1. **Filesystem does the namespacing for free.** No slugs, no hashes.
2. **Discoverability.** `rm -rf .neovex` is the idiomatic reset. Every
   developer understands this without documentation.
3. **State travels with the project.** Copy/move a directory and state comes
   with it. User-level keyed by path would orphan state on move.
4. **Worktree/clone isolation.** Each checkout gets independent state
   automatically. Convex learned this the hard way — it's why they migrated
   from `~/.convex/` to `.convex/`.
5. **CI friendliness.** Project directory is the unit of work in CI.

**Why user-level for auth/config/shared caches:**
1. Auth tokens belong to the user, not to any project.
2. Shared binaries/caches are content-addressed and duplicating per-project
   wastes disk space.
3. Config (license, registry settings) applies to the installation.

### What Neovex's machine subsystem already does

The `neovex machine` commands follow XDG correctly for user-scoped resources:
- `~/.config/neovex/machine/` — config
- `~/.local/share/neovex/machine/` — data (disk images, shared)
- `~/.local/state/neovex/machine/` — state (status, locks)
- `~/.cache/neovex/machine/` — cache (OCI images, shared)

This is the right pattern for machine resources because they are user-scoped
(images and machines are shared across projects). The dev server data is
project-scoped — different concern, different pattern.

### Target: hybrid layout

After this plan:

```
my-app/
├── convex/                      # source (developer-authored)
├── convex/_generated/           # generated types (unchanged)
├── .neovex/                     # project-scoped dev state (gitignored)
│   ├── dev/                     # SQLite databases, control plane
│   │   ├── demo.db
│   │   └── neovex-control.db
│   ├── convex/                  # codegen artifacts (Convex adapter)
│   │   ├── functions.json
│   │   ├── bundle.mjs
│   │   └── bundle.sha256
│   └── firebase/                # codegen artifacts (Cloud Functions adapter)
│       ├── artifact.json
│       ├── targets.json
│       ├── bundle.mjs
│       └── bundle.sha256
├── .env.local                   # NEOVEX_DEPLOYMENT=local:<slug> (gitignored)
├── package.json
└── .gitignore                   # .neovex/, .env.local

~/.config/neovex/
├── config.json                  # global CLI config (future: auth tokens)
└── license.json                 # license file (optional)
```

What changed:
1. **License moved out** of `.neovex/` to `~/.config/neovex/` (user-scoped)
2. **Deployment identity** added via `.env.local` (Convex pattern)
3. **`.neovex/` kept** for project-scoped state (codegen artifacts + dev DB)
4. **Global config dir** established for user-scoped CLI state

What stayed the same:
- `.neovex/` is still project-local and gitignored
- Codegen artifacts still write to `.neovex/convex/` and `.neovex/firebase/`
- Dev database still lives at `.neovex/dev/`
- `--data-dir` still overrides the data location

---

## Design Decisions

### D1: Keep `.neovex/` project-local for project-scoped state

**Decision:** `.neovex/` stays in the project directory for codegen artifacts
and dev database state.

**Evidence:** Convex moved FROM user-level TO project-local in v1.32.0 after
worktree conflicts. Wrangler, Next.js, SvelteKit, Firebase CLI, and Cargo
all use project-local directories for project-scoped state. No successful
dev tool with a local server component has moved project-scoped state to
XDG user-level directories.

**Tradeoffs accepted:**
- Developers see a dotdir in their project (mitigated: universally understood
  pattern, gitignored)
- Cannot share dev state across projects (correct: dev state should not be
  shared)

**Tradeoffs rejected:**
- "Cleaner project directories" — the industry has spoken: a gitignored dotdir
  is the expected pattern. Removing it creates discoverability and portability
  problems that aren't worth the aesthetic benefit.

### D2: Deployment identity via `.env.local`

Each project gets a deployment identity, written to `.env.local`:

```
NEOVEX_DEPLOYMENT=local:<slug>
```

The slug is derived from the project directory:
```
<dir-name>-<sha256(canonical_path)[:8]>
```

Example: `/Users/jack/src/my-app` → `NEOVEX_DEPLOYMENT=local:my-app-a1b2c3d4`

This follows the Convex pattern exactly:
- Convex writes `CONVEX_DEPLOYMENT=dev:tall-forest-1234` to `.env.local`
- Neovex writes `NEOVEX_DEPLOYMENT=local:my-app-a1b2c3d4` to `.env.local`

**Future `neovex deploy` migration:** When remote deployment ships, the value
changes to `NEOVEX_DEPLOYMENT=<server-url>:<server-assigned-slug>`. The
dev flow is "deploy to local", the deploy flow is "deploy to remote". The
`.env.local` pattern scales to both with no model change.

`.env.local` is added to the gitignore template (matches Convex behavior).

### D3: User-scoped state to `~/.config/neovex/`

User-scoped configuration that applies to the Neovex installation (not to
any specific project) lives at `~/.config/neovex/`:

| File | Content |
|------|---------|
| `config.json` | Global CLI config (future: auth tokens, default settings) |
| `license.json` | License file (optional) |

This is consistent with:
- `~/.convex/config.json` (Convex auth tokens)
- `~/.config/planetscale/` (PlanetScale auth)
- `~/.config/turso/` (Turso auth and settings)
- `~/.config/neovex/machine/` (existing machine subsystem config)

The machine subsystem already resolves `~/.config/neovex/machine/` via
`resolve_config_root()` in `crates/neovex-bin/src/machine/record.rs`. The
global config lives one level up at `~/.config/neovex/`.

### D4: `neovex dev` writes `.env.local` on startup

`neovex dev` writes `NEOVEX_DEPLOYMENT=local:<slug>` to `.env.local` in the
app directory on startup. Behavior:
- If `.env.local` does not exist, create it with the deployment variable.
- If `.env.local` exists but has no `NEOVEX_DEPLOYMENT` line, append it.
- If `.env.local` exists with the correct `NEOVEX_DEPLOYMENT` value, no-op.
- If `.env.local` exists with a different `NEOVEX_DEPLOYMENT` value,
  overwrite that line (the developer switched deployment targets).
- Never delete other content in `.env.local`.

This matches Convex's `changesToEnvFile()` in `deployment.ts`.

### D5: License file location

Move from `.neovex/license.json` (project-local) to
`~/.config/neovex/license.json` (user-level). The license applies to the
Neovex installation, not to a specific project.

Override chain (first wins):
1. `--license-file` CLI flag
2. `NEOVEX_LICENSE_FILE` env var
3. `~/.config/neovex/license.json` (default)

### D6: `neovex start` behavior unchanged

`neovex start` is the operator/production path. It does NOT resolve
`.env.local` or derive deployment slugs. Operators specify explicit paths
via `--data-dir` and `--app-dir`. The `--app-dir` path resolves codegen
artifacts from `.neovex/convex/` and `.neovex/firebase/` relative to the
app directory, same as today.

### D7: Shared `dirs` module for XDG resolution

Create a shared `crate::dirs` module that provides XDG resolution functions,
consistent with the machine subsystem pattern. This handles:
- `global_config_dir()` → `~/.config/neovex/`
- `deployment_slug(app_dir)` → `<dir-name>-<sha256(canonical_path)[:8]>`

The machine subsystem's existing resolvers in `machine/record.rs` stay
separate for now. A follow-on can unify them under the shared module.

### D8: Breaking change — no migration

Per CLAUDE.md: "This project has NOT launched yet. Breaking changes are
preferred." There are no users to migrate. Move the license file directly
rather than adding compatibility shims.

---

## Phase Status Ledger

| Phase | Status | Items | Done when |
|-------|--------|-------|-----------|
| P1: Shared dirs module | `done` | H1 | `dirs.rs` module with `global_config_dir()` and `deployment_slug()` |
| P2: Deployment identity | `done` | H2 | `neovex dev` writes `NEOVEX_DEPLOYMENT=local:<slug>` to `.env.local` |
| P3: License migration | `pending` | H3 | License default path is `~/.config/neovex/license.json` |
| P4: Gitignore + docs | `pending` | H4 | `.env.local` in gitignore templates, docs updated |

## Roadmap Items

### P1 Work Queue: Shared Dirs Module

| Item | Status | Hard deps | Completion gate |
|------|--------|-----------|-----------------| 
| H1 | `done` | none | `crates/neovex-bin/src/dirs.rs` created with `global_config_dir()` (XDG_CONFIG_HOME with fallback to `~/.config/neovex/`) and `deployment_slug(app_dir)` (SHA-256 of canonical path, truncated to 8 hex chars, prefixed with sanitized dir name — strip non-alphanumeric chars except hyphens, lowercase). `mod dirs` added to `main.rs`. Tests pass for slug derivation determinism, XDG override, default paths, and dir names with spaces/special chars/non-ASCII. Follows the pattern from `machine/record.rs` for XDG env var resolution. `sha2` is already a workspace dependency. |

### P2 Work Queue: Deployment Identity

| Item | Status | Hard deps | Completion gate |
|------|--------|-----------|-----------------| 
| H2 | `done` | H1 | `crates/neovex-bin/src/dev.rs` updated: after resolving the app directory and before starting the server, writes `NEOVEX_DEPLOYMENT=local:<slug>` to `<app_dir>/.env.local`. Uses `dirs::deployment_slug()` for the slug. Handles the four `.env.local` states (absent, exists without var, exists with correct var, exists with different var). Dev banner shows `Deployment: local:<slug>` line. Tests cover all four `.env.local` states. |

### P3 Work Queue: License Migration

| Item | Status | Hard deps | Completion gate |
|------|--------|-----------|-----------------| 
| H3 | `pending` | H1 | License default resolution moved to `neovex-bin`: in `start/boot.rs:47`, resolves the default license path via `dirs::global_config_dir().join("license.json")` when `command.license_file` is `None` and `NEOVEX_LICENSE_FILE` is not set. Passes the resolved path to `LicenseState::load()`. `DEFAULT_LICENSE_PATH` removed from `neovex-server/src/license/mod.rs:15`. Default-path fallback removed from `neovex-server/src/license/loading.rs:43-46` so the server crate has no XDG knowledge — it receives an explicit path or returns `community()`. `start/mod.rs:143` help text updated to say `~/.config/neovex/license.json`. `ARCHITECTURE.md:693` updated. Existing license tests pass. |

### P4 Work Queue: Gitignore And Docs

| Item | Status | Hard deps | Completion gate |
|------|--------|-----------|-----------------| 
| H4 | `pending` | H2, H3 | `.env.local` added to gitignore templates (`templates/convex/gitignore`, `templates/cloud-functions/gitignore`). `.env.local` and `**/.env.local` added to root `.gitignore`. `docs/operating/cli.md` updated with deployment identity behavior and license path change. `docs/plans/neovex-init-plan.md` updated with `.env.local` in template listings (lines 87, 105). `README.md` updated if it references license path. |

---

## Affected Files

### Production code

| File | Change |
|------|--------|
| `crates/neovex-bin/src/dirs.rs` | **New** — shared XDG + deployment slug module |
| `crates/neovex-bin/src/main.rs` | Add `mod dirs` |
| `crates/neovex-bin/src/dev.rs` | Write `.env.local` on startup, add `Deployment:` to banner |
| `crates/neovex-bin/src/start/boot.rs` | Resolve default license path via `dirs::global_config_dir()` before calling `LicenseState::load()` |
| `crates/neovex-bin/src/start/mod.rs` | Update `--license-file` help text (line 143) |
| `crates/neovex-server/src/license/mod.rs` | Remove `DEFAULT_LICENSE_PATH` constant |
| `crates/neovex-server/src/license/loading.rs` | Remove default-path fallback from `LicenseState::load()` (lines 43-46) |

### Templates

| File | Change |
|------|--------|
| `crates/neovex-bin/templates/convex/gitignore` | Add `.env.local` line |
| `crates/neovex-bin/templates/cloud-functions/gitignore` | Add `.env.local` line |

### Docs

| File | Change |
|------|--------|
| `docs/operating/cli.md` | Document `.env.local` deployment identity, license path change |
| `docs/plans/neovex-init-plan.md` | Add `.env.local` to template listings |
| `ARCHITECTURE.md` | Update license path reference (line 693) |
| Root `.gitignore` | Add `.env.local` and `**/.env.local` |

### Not changed (project-scoped `.neovex/` paths stay as-is)

| File | Reference | Why unchanged |
|------|-----------|---------------|
| `crates/neovex-bin/src/codegen.rs` | Passes `--app .` | Codegen writes to `.neovex/convex/` relative to app dir |
| `crates/neovex-bin/src/deploy.rs:428,432` | `generated_convex_dir()`, `generated_cloud_functions_dir()` | Resolve from `app_dir.join(".neovex")` |
| `crates/neovex-bin/src/start/boot.rs:359,388` | Artifact manifest paths | `.neovex/convex/` and `.neovex/firebase/` resolution unchanged (license resolution in same file changes in H3) |
| `crates/neovex-bin/src/dev.rs:169` | Data dir default | `.neovex/dev` stays as project-local default |
| `crates/neovex-bin/src/dev.rs:209` | `detect_app_dir` | `.neovex/convex/functions.json` check stays (artifact detection) |
| `crates/neovex-bin/src/dev.rs:607` | Watch skip list | `.neovex` stays in skip list |
| `crates/neovex-bin/src/cli_ux.rs:51` | Dev help example | `--data-dir ./.neovex/dev` stays (correct example) |
| `packages/codegen/src/main.mjs:23` | `internalDir` | `path.join(appDir, ".neovex", "convex")` stays |
| `packages/codegen/src/cloud_functions/project.mjs:4` | `CLOUD_FUNCTIONS_INTERNAL_DIR` | `[".neovex", "firebase"]` stays |

---

## Resolved Questions

1. **Why not move everything to XDG?** Convex tried user-level state and
   reversed course in v1.32.0 due to worktree conflicts. The industry
   consensus is that project-scoped dev state belongs in a project-local
   dotdir. Moving codegen artifacts and dev databases to XDG would sacrifice
   discoverability (`rm -rf .neovex` as reset), portability (state travels
   with project), and worktree isolation — all for an aesthetic improvement
   that no successful dev tool has found worthwhile.

2. **Why keep `.neovex/` instead of renaming to `.convex/`-style?**
   `.neovex/` is the correct namespace — it is Neovex-specific state, not
   adapter-specific. It contains both Convex and Cloud Functions artifacts.
   The name is already established in the codebase and gitignore templates.

3. **`.env.local` written on `neovex dev` from day one.** Convex writes
   `CONVEX_DEPLOYMENT=...` to `.env.local` — Neovex follows the same
   pattern. `neovex dev` writes `NEOVEX_DEPLOYMENT=local:<slug>` to
   `.env.local` in the app directory. This makes the deployment identity
   inspectable, matches Convex's developer experience, and when
   `neovex deploy` ships the value changes to a remote URL with no model
   change.

4. **License is user-scoped, not project-scoped.** A license applies to the
   Neovex installation, not to a specific project. Storing it in `.neovex/`
   would mean copying the license file into every project directory. The
   correct location is `~/.config/neovex/license.json`, consistent with how
   Convex stores auth at `~/.convex/config.json` and the machine subsystem
   stores config at `~/.config/neovex/machine/`.

5. **Dev banner shows deployment identity.** The dev banner adds a
   `Deployment:` line showing `local:<slug>` so developers can see their
   deployment identity without opening `.env.local`.

6. **`neovex start` is unaffected.** The operator path does not resolve
   `.env.local` or derive slugs. This keeps the explicit operator contract
   clean — operators specify paths, not identity.

## Open Questions

1. **Should slug use canonical path or a stable project identifier?**
   Canonical path is simple and deterministic but changes on project move.
   An alternative is writing a stable UUID to `.neovex/project-id` on first
   run. This would survive moves but adds a file that needs to be either
   committed (shared identity) or gitignored (per-clone identity). **Current
   decision:** canonical path. Revisit if project-move identity loss becomes
   a real user complaint post-launch.

2. **Should `.neovex/` be self-gitignoring?** Convex creates `.convex/.gitignore`
   containing `/*` so the directory is always gitignored regardless of the
   project's root `.gitignore`. This is a nice UX touch — the developer never
   needs to add `.neovex/` to their gitignore manually. Consider adopting
   this pattern when `.neovex/` is first created (in `neovex dev` or
   `neovex init`). **Decision deferred** to implementation.

---

## Phase 2+ (Out of Scope)

### `neovex deploy`

When remote deployment ships:
- Deployment identity becomes server-assigned
- `.env.local` stores `NEOVEX_DEPLOYMENT=<server-url>:<slug>`
- `neovex dev` is "deploy to local", `neovex deploy` is "deploy to remote"
- Same codegen artifacts, different upload target
- `.neovex/` still holds project-local artifacts; the deploy command reads
  from `.neovex/` and uploads to the remote server

### Machine subsystem unification

The machine subsystem has its own XDG resolvers in `machine/record.rs`. A
follow-on could unify both into the shared `dirs` module. Not in scope here
to avoid coupling the migration with machine subsystem changes.

### `neovex auth`

Future authentication flow. Auth tokens would live at
`~/.config/neovex/auth.json` (user-scoped), following the pattern established
by this plan's `global_config_dir()`. Not in scope here.

---

## Execution Log

| Date | Item | Status | Description | Verification |
|------|------|--------|-------------|--------------|
| — | — | — | Plan created | — |
| 2026-04-28 | H1 | `done` | Created `crates/neovex-bin/src/dirs.rs` with `global_config_dir()` (XDG_CONFIG_HOME + HOME fallback), `deployment_slug()` (SHA-256 canonical path, 8 hex chars, sanitized dir name), `sanitize_dir_name()` (strip non-alphanumeric except hyphens, lowercase, empty→"app"). Added `mod dirs` to `main.rs`. Files: `dirs.rs` (new), `main.rs` (mod added). | `cargo fmt --all --check`: clean. `cargo clippy -p neovex-bin --all-targets -- -D warnings`: clean. `cargo test -p neovex-bin -- dirs::`: 14 passed, 0 failed (5 consecutive parallel runs, no races). `cargo test -p neovex-bin`: 380 passed, 0 failed. |
| 2026-04-28 | H2 | `done` | Updated `dev.rs`: added `write_env_local_deployment()` handling 4 `.env.local` states (absent, no var, correct value, different value); added `deployment_slug` field to `DevPlan`; `resolve_dev_plan` computes slug via `dirs::deployment_slug()`; dev banner shows `Deployment: local:<slug>` line with aligned column widths; removed `#[allow(dead_code)]` from `main.rs` `mod dirs`. Files: `dev.rs` (modified), `main.rs` (allow removed), `dirs.rs` (allow on `global_config_dir`). | `cargo fmt --all --check`: clean. `cargo clippy -p neovex-bin --all-targets -- -D warnings`: clean. `cargo test -p neovex-bin -- dev::tests::env_local`: 6 passed, 0 failed. `cargo test -p neovex-bin -- dev::tests::dev_banner_includes_deployment_line`: 1 passed. `cargo test -p neovex-bin`: 387 passed, 0 failed. |
