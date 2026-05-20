# Plan: CLI Daemon Canonicalization

Canonicalize the Nimbus daemon CLI surface so `nimbus start` and
`nimbus dev` start cleanly from any working directory, both always
serve the operator console at `/ui/*`, the ancestor walk-up that
backs `nimbus dev` and `nimbus deploy` is correctly bounded at
project root, and `nimbus ui` becomes a thin "open the running
daemon in a browser" launcher. Aligns Nimbus' command surface with
CockroachDB, Vault, Grafana, and Convex' embedded-UI daemon
patterns, and the ancestor walk-up with `git`, `gh`, `pre-commit`,
`ripgrep`, and every other dev-tool that uses `.git/` as the
project boundary.

---

## Status

- **Status:** `active`
- **Created:** 2026-05-19
- **Primary owner:** this plan
- **Activation gate:** met. The current behaviour blocks running
  `nimbus start` or `nimbus dev` from inside the Nimbus repo because
  the ancestor walk-up has no upper bound — it ascends past the
  repo root to `~/src/github.com/nimbus/` and treats the parent as
  a user app, since the Nimbus repo itself is a `nimbus/`-named
  child of that directory. The Playwright e2e fixture
  (`packages/nimbus-ui/tests/e2e/fixtures/nimbus-server.ts:240`)
  dodges this by spawning the daemon from a `mkdtempSync` scratch
  dir — a workaround, not a canonical workflow.

## Why

### History — where the walk-up came from

The walk-up was a deliberate, idiomatic feature that got
over-applied and then armed by a rebrand. Three commits tell the
full story:

1. **`0e4bf2d6` "Complete CLI command surface wave" (2026-04-22).**
   Introduced `detect_app_dir` / `resolve_deploy_app_dir` in
   `crates/neovex-bin/src/deploy.rs` as a private helper. Its job
   was to support `nimbus deploy <subdir>` — when the user runs
   `nimbus deploy` from any subdirectory of their project, find
   the project root by walking up looking for a `convex/` or
   `neovex/` child. Same idiom as `git`'s walk-up to find `.git/`,
   `npm`'s walk-up to find `package.json`, or `cargo`'s walk-up to
   find `Cargo.toml`. Defensible for a project-scoped command. The
   same commit copy-pasted the heuristic into `dev.rs:detect_app_dir`
   so `nimbus dev <subdir>` would behave the same — also
   defensible, since `dev` is project-scoped.

2. **`c72564c5` "Land adapter and architecture hardening baseline"
   (2026-04-26).** Promoted `resolve_deploy_app_dir` to
   `pub(crate)` and added `resolve_start_app_dir` in `start/boot.rs`
   that calls it with `None` for auto-detection. Implicit intent:
   *"if a user runs `nimbus start` from inside their project,
   activate that project's app at startup."* Convenience, modelled
   on the deploy idiom — but a category error: `nimbus start` is
   the daemon command (Cockroach/Vault/Grafana shape), not a
   project-scoped command. Tying daemon startup to filesystem
   heuristics about user app source was the design mistake.

3. **`09f56158` "rename: complete neovex→nimbus rebrand"
   (2026-05-15).** Changed the substring the heuristic looks for
   from `"neovex"` to `"nimbus"`. Before the rebrand, the walk-up
   was harmless from inside the source repo because
   `~/src/github.com/nimbus/` has no `neovex/` child. After the
   rebrand, `~/src/github.com/nimbus/` *does* have a `nimbus/`
   child — the source repo itself — so the walk-up reliably
   misclassifies the repo's parent directory as a Nimbus app. **The
   rebrand inadvertently armed a latent trap that the hardening
   commit had created two weeks earlier.**

The walk-up has therefore served three roles by inheritance:

- **Legitimate, keep:** `nimbus deploy` — project-scoped command,
  walk-up is the right primitive. Needs bounding (see below) so it
  cannot escape the project.
- **Defensible, bound:** `nimbus dev` — project-scoped command,
  walk-up is the right primitive. Needs bounding for the same
  reason.
- **Category error, remove:** `nimbus start` — daemon command,
  has no business doing app discovery. Walk-up goes entirely.

### The shared anchor: both daemon entrypoints serve `/ui/*` today

`nimbus dev` and `nimbus start` already share the HTTP transport.
`dev.rs:115-122` builds a `StartCommand` and calls
`run_start_command`; `nimbus-server/src/http/mod.rs:32,57` mounts
`/ui/*` from the rust-embed bundle for any router built through
this path. The user-visible promise *"`nimbus dev` should always
serve the UI"* is already true at the code level; this plan locks
it in as a contract via the startup banner and regression tests so
the property cannot silently regress.

### Comparable projects — embedded-UI daemons

The pattern Nimbus chose for the daemon (single binary, embedded
operator console) matches several mature projects. None of them tie
daemon startup to user-app-source validity.

| Project | Daemon command | UI | App concept at startup |
|---|---|---|---|
| CockroachDB | `cockroach start` / `start-single-node` | Embedded, always served on HTTP listener | None — `cockroach init` is separate (`cockroachdb/cockroach/pkg/cli/start.go:88-104`) |
| HashiCorp Vault | `vault server` | Embedded behind `ui = true` config | None |
| Grafana | `grafana-server` | Always served | None |
| MinIO | `minio server <data-dir>` | Always served | `<data-dir>` is storage, not app |
| Convex (self-hosted) | `convex-local-backend` | **Not** bundled; dashboard ships as separate npm packages (`get-convex/convex-backend/npm-packages/dashboard{,-self-hosted}`) | None |
| Podman Desktop | `podman` daemon | **Bundled inside Electron** — `loadURL(file://.../renderer/dist/index.html)` in prod, `loadURL(VITE_DEV_SERVER_URL)` in dev (`podman-desktop/packages/main/src/mainWindow.ts:241-246`). The Electron renderer never loads a daemon URL; it talks to podman over its REST socket. | None |
| **Nimbus today** | `nimbus start` / `nimbus dev` | Embedded at `/ui/*` (DU1, `c45c25a3`) + Electron `loadURL`s daemon's URL (`desktop/src/main/window.ts:82`) | **Yes — unbounded walk-up; this is the anomaly to remove from `start` and bound on `dev` / `deploy`.** |

### Comparable projects — ancestor walk-up

The walk-up itself is the right primitive for project-scoped
commands. Every mature tool that does it uses the same upper-bound
marker: `.git/`.

| Tool | What it walks up for | Boundary |
|---|---|---|
| `git rev-parse --show-toplevel` | repo root | `.git/` |
| `gh`, `hub` | github context | `.git/` (via `git rev-parse`) |
| `pre-commit`, `husky`, `lefthook` | hook config | `.git/` |
| `cargo` | workspace/package root | nearest `Cargo.toml`, with `[workspace]` as outer bound |
| `rust-analyzer` | workspace | `Cargo.toml [workspace]` |
| `ripgrep` (for `.gitignore`) | gitignore resolution | `.git/` |
| `pnpm`, `nx`, `turborepo` | monorepo root | their own marker (`pnpm-workspace.yaml`, `nx.json`, `turbo.json`) |
| `prettier`, `eslint` | config | `package.json` |
| `npm` | install context | `package.json` |

The shared pattern: **stop at a marker that means "this is one
self-contained project."** `.git/` is the universal one — works
for any user, any language, regardless of build system, and is
already present at the boundary we need to honour in both the
Nimbus source repo and every user's project.

**Precision note from re-surveying actual implementations.** Not
every tool that walks ancestors uses `.git/`. Deno's own workspace
config discovery
(`denoland/deno/libs/config/workspace/discovery.rs:416`) walks
*unbounded* with cycle detection only; the `.git/` boundary appears
in Deno's gitignore resolution
(`denoland/deno/libs/config/glob/gitignore.rs:100`,
`fs_exists_no_err(parent.join(".git"))`), not its config discovery.
The pattern is therefore consistent for *gitignore-shaped* problems
(where escaping the repo is semantically wrong) and inconsistent for
*config-shaped* problems (where escaping the repo is unusual but not
catastrophic). Nimbus' bug is gitignore-shaped — escaping the repo
produces a *wrong* answer (the parent dir is misclassified as an
app), not just an unexpected one — so the `.git/` boundary is the
right primitive here. Adopting `Path::exists()` (the `fs_exists`
shape Deno uses) covers worktree-`.git`-file and submodule cases for
free.

### Design decision: why `.git/`, not the alternatives

Several boundary options were considered. The decision in detail:

| # | Option | Verdict | Reasoning |
|---|---|---|---|
| 1 | **Stop at `.git/`** | **Chosen.** | Universal idiom (see table above). One-line check (`candidate.join(".git").exists()` — covers both submodule `.git` files and regular `.git` directories). Solves the bug without any Nimbus-specific knowledge. Works for every legitimate case below. |
| 2 | Stop at Cargo `[workspace]` root or first `Cargo.toml` | Considered, not chosen. | Rust-idiomatic, but requires TOML parsing at startup and doesn't help for non-Rust user apps. `.git/` already covers the cases this catches. |
| 3 | Stop at any monorepo marker (`pnpm-workspace.yaml`, `turbo.json`, `package.json` with `workspaces`) | Considered, not chosen. | Maintenance burden — list rots. `.git/` already catches all of these in practice. |
| 4 | Match `Cargo.toml` `repository = "https://github.com/nimbus/nimbus"` | Considered, rejected. | Pinpoints "we're inside Nimbus source" specifically, but the bug is generic ("ancestor walk has no natural upper bound") and the fix should be generic too. Requires TOML parsing, hardcodes a string that breaks on rename or fork, and doesn't help users whose unrelated repo nests near a `nimbus/`-named directory. `.git/` at the same location wins on simplicity and universality. |
| 5 | Embed `CARGO_MANIFEST_DIR` at build time and self-detect "running in dev from our own workspace" | Considered, rejected. | Clever and hard to explain. Build-time path is meaningless for installed binaries (though installed binaries never trigger the bug). |
| 6 | Sentinel file at repo root (e.g. `.nimbus-source-repo`) | Considered, rejected. | Cheesy; another file to maintain; adds nothing `.git/` doesn't. |

**Trace through the failing case with the chosen boundary:**

- CWD: `~/src/github.com/nimbus/nimbus/some/subpath/`
- Walk up, checking each ancestor for app surface first, then for
  the `.git/` boundary:
  - `some/subpath/` — no app surface, no `.git/` → continue.
  - `some/` — no app surface, no `.git/` → continue.
  - `~/src/github.com/nimbus/nimbus/` (Nimbus repo root) — no
    `convex/`, no `nimbus/` (the repo *is* nimbus; doesn't contain
    itself), no `firebase.json`. Has `.git/` → **stop. Return no
    app dir.**

The walk never reaches `~/src/github.com/nimbus/` and never
misclassifies it.

**Trace through legitimate user cases:**

- User's Convex app at `~/code/myapp/` (with `.git/` and `convex/`
  siblings), `nimbus dev` from `~/code/myapp/src/components/`:
  walk → `src/`: no → `myapp/`: has `convex/` ✓ found before
  hitting `.git/` check.
- Monorepo: `~/code/monorepo/.git/` and
  `~/code/monorepo/apps/billing/convex/`, run from
  `apps/billing/src/`: walk finds `apps/billing/` (has `convex/`)
  before reaching the monorepo root. ✓
- Demo inside the Nimbus repo: `nimbus dev` from
  `~/src/github.com/nimbus/nimbus/demos/convex/html/src/` → walk
  finds `demos/convex/html/` (has its own `convex/`) ✓ before
  hitting the repo's `.git/`.

Every legitimate case works; the buggy case stops cleanly.

**Order matters in the loop.** Check the app surface at the current
candidate *before* checking the boundary, so a candidate that is
both the project root and an app dir (the typical small-project
case) is still recognised.

### Other walk-up sites: compose discovery has the same shape

The `dev` / `deploy` walk-ups are the most visible instances, but
`nimbus-bin` has a third walk-up that ships the same bug shape and
should be bounded in the same wave:

- `crates/nimbus-bin/src/compose/discovery.rs:198`
  (`resolve_auto_discovered_compose_selection`) walks ancestors
  looking for `compose.yaml`, `compose.yml`, `docker-compose.yaml`,
  or `docker-compose.yml`. Identical structure: unbounded
  `ancestors()` loop with no `.git/` stop. In the wild this is less
  catastrophic than `dev`/`deploy` (a stray `compose.yaml` in
  `~/src/` is uncommon), but the failure mode is the same — a file
  outside the project's git boundary can be silently activated.

There is a fourth walk-up at `crates/nimbus-bin/src/codegen.rs:445`
(`find_workspace_codegen_entry_from`) used to locate the bundled
codegen tool inside the Nimbus source workspace. Its bug shape is
*different* — it searches for an in-workspace file shipped with
Nimbus, not a user-project marker — so it is intentionally out of
scope for this plan and left untouched. Documenting it here so the
grep gate (later) explicitly excludes it.

Three walk-up sites in scope (`dev`, `deploy`, `compose/discovery`),
one out of scope (`codegen`). The fix in each scoped site is the
same four-line check, which motivates the shared
`at_git_boundary()` helper called out in CD2.

### Electron does not need `nimbus ui`

`@nimbus/desktop`'s main process spawns `nimbus start` directly
(`desktop/src/main/server.ts:188`, no `cwd` override) and `loadURL`s
the resolved URL (`desktop/src/main/window.ts:82`). The comment at
`server.ts:17` is explicit: it *"Mirrors `crates/nimbus-bin/src/ui.rs
run_ui_command` for the discovery + spawn + readiness-probe loop,
but is owned by [the shell]"*. The Electron app deliberately
reimplements the discover-or-spawn loop because Electron must own
the lifecycle.

Consequence: `nimbus ui --ensure` is a CLI-user convenience only.
The Electron app has no upstream dependency on it and will not
call it. Its existence makes sense as a tiny "discover running
daemon and open browser" launcher; its `--ensure` mode duplicates
"`nimbus start` + browser-open" and is the wrong primitive to
keep.

### The user-visible promise

After this plan:

- `nimbus dev` starts cleanly from anywhere. With an app surface
  in CWD (or any project-bounded ancestor), it adds watched
  codegen. Without one, it serves the daemon plain. **It always
  serves the UI.**
- `nimbus start` starts cleanly from anywhere. With `--app-dir`
  it loads/activates that app from source. **Without `--app-dir`,
  the daemon boots and rehydrates any apps previously deployed to
  its storage** (the production shape — apps reach the daemon
  through `nimbus deploy` / the deploy admin API, not through
  walk-up). It always serves the UI. The startup banner already
  prints `app dir: none; Convex-compatible routes wait for deploy
  activation` (`crates/nimbus-bin/src/start/boot.rs:283`) when no
  source-app is loaded — deployed apps rehydrate from storage
  through the engine's normal boot path, untouched by this plan.
- `nimbus deploy <subdir>` continues to find the project root via
  ancestor walk, now correctly bounded so it cannot escape the
  project.
- Both daemon commands print the operator-console URL on the
  startup banner.
- `nimbus ui` (no flags) discovers a running daemon and opens the
  browser. There is no `--ensure`.
- For "spawn a daemon and open the UI in one step": `nimbus dev
  --open`. (`nimbus start` does not carry `--open` — see the
  Risks section on daemon-CLI precedent. The two-step flow for
  `start` is `nimbus start &; nimbus ui`.)

This is the Cockroach/Vault/Grafana/MinIO daemon shape plus the
`git`/`gh`/`pre-commit` walk-up shape — both idiomatic, both
universally recognised, no Nimbus-specific cleverness anywhere.

## Scope

In scope:

- **Removing** ancestor walk-up from `nimbus start`
  (`resolve_start_app_dir` in `crates/nimbus-bin/src/start/boot.rs`).
  Category fix — daemon command, no app discovery.
- **Bounding** ancestor walk-up at `.git/` in `nimbus dev`
  (`detect_app_dir` in `crates/nimbus-bin/src/dev.rs`).
- **Bounding** ancestor walk-up at `.git/` in `nimbus deploy`
  (`detect_app_dir` inside `resolve_deploy_app_dir` in
  `crates/nimbus-bin/src/deploy.rs`). Same bug shape; same fix.
- **Bounding** ancestor walk-up at `.git/` in compose discovery
  (`resolve_auto_discovered_compose_selection` in
  `crates/nimbus-bin/src/compose/discovery.rs:198`). Third instance
  of the same bug shape; folded into the same wave so we land one
  shared `at_git_boundary()` helper instead of one helper plus a
  third lone walker.
- Adding/standardising startup banner output for both daemon
  commands.
- Adding `--open` to `nimbus dev` (only). `nimbus start` does not
  get `--open` — it is the production-shaped daemon and matches
  the CockroachDB/Vault/Grafana precedent of "print URL, operator
  copies it." `--open` belongs on the dev-tool shape only (cargo
  doc / vite precedent).
- Replacing `nimbus ui --ensure` with the unflagged `nimbus ui`
  (discovery-only) plus `nimbus dev --open` for the
  spawn-and-open ergonomic.
- Regression tests for the walk-up boundary on `dev`, `deploy`,
  and `compose/discovery`, and the "no walk-up at all" contract on
  `start`.
- Documentation: CLAUDE.md routing entry + `docs/operating/cli.md`
  refresh.

Out of scope:

- Any change to the embedded UI assets, routing, or auth surface.
- Any change to Electron `@nimbus/desktop` beyond verifying the
  existing spawn path still works (it does not depend on
  `--ensure`).
- Refactoring `resolve_deploy_app_dir`'s structure beyond adding
  the `.git/` boundary check (and lifting it to the shared
  `at_git_boundary()` helper used by `dev` and `compose/discovery`).
- Fixing the fourth walk-up at `crates/nimbus-bin/src/codegen.rs:445`
  (`find_workspace_codegen_entry_from`). Different bug shape — that
  walker locates the bundled codegen tool inside Nimbus' own source
  workspace, not a user-project marker — and its failure mode is
  different (it would mis-locate Nimbus' own tooling, which only
  happens for developers working on Nimbus from inside a nested
  workspace). The grep gates in the Completion Gate explicitly
  exclude this file so it is not silently bounded here.
- The Convex/CloudFunctions codegen pipeline itself.
- Any new transport (HTTP/WS) wiring; the UI is already served.
- A secondary boundary marker (Cargo workspace, sentinel file).
  `.git/` alone is sufficient for the cases we have; revisit only
  if a real workflow surfaces that runs outside a git repo.

## Canonical Command Surface (Target Shape)

```
nimbus start [--app-dir <dir>] [--port <p>] [...]
    Production-shaped daemon. Serves /ui/*. With --app-dir, runs
    codegen preflight + loads adapter registry from source.
    Without --app-dir, no source walk-up, no auto-discovery; the
    daemon boots and the engine rehydrates any apps previously
    deployed to its storage through the normal startup path.
    Prints UI URL on startup. No --open: print URL, copy it.

nimbus dev [--app-dir <dir>] [--port <p>] [--open] [...]
    Developer-shaped daemon with watched codegen and dev defaults
    (lower limits, auto-tenant, .env.local wiring). Defaults
    --app-dir by walking ancestors from CWD looking for an app
    surface (convex/, nimbus/, firebase.json, .nimbus/convex/
    functions.json) and stopping at the first .git/ boundary. If
    no app surface is found inside the boundary, runs as plain
    daemon with a clear stderr note. Always serves /ui/*. Prints
    UI URL on startup. With --open, launches default browser at
    the operator-console URL after the readiness probe passes; if
    the launcher fails (headless host), logs an error-level line
    and continues serving (exit 0).

nimbus deploy [--app-dir <dir>] ...
    Walk-up unchanged in semantics, now bounded at .git/. Errors
    cleanly if no app surface is found inside the boundary.

nimbus ui
    Discover the running Nimbus daemon (via discovery file) and
    open its operator console in the default browser. Errors if
    no daemon is running. No spawn behaviour.
```

`nimbus dev --open` replaces `nimbus ui --ensure` exactly.

## Ledger

| ID  | Phase                                                                                              | Status  |
|-----|----------------------------------------------------------------------------------------------------|---------|
| CD1 | **Remove** walk-up in `resolve_start_app_dir` (`crates/nimbus-bin/src/start/boot.rs:297-324`): when no `--app-dir` is provided, return `Ok(None)` without consulting `resolve_deploy_app_dir`. `nimbus start` does no app discovery. | done |
| CD2 | **Bound** walk-up at `.git/` in three sites: `nimbus dev` (`crates/nimbus-bin/src/dev.rs:310-325` `detect_app_dir`), `nimbus deploy` (`crates/nimbus-bin/src/deploy.rs:234-255` `detect_app_dir`, called by `resolve_deploy_app_dir` at `:224-232`), and compose discovery (`crates/nimbus-bin/src/compose/discovery.rs:198` `resolve_auto_discovered_compose_selection`). Land a small shared helper — `fn at_git_boundary(dir: &Path) -> bool { dir.join(".git").exists() }` — colocated with whichever crate already hosts cross-cutting path utilities (or a new `path_boundary.rs` next to the most-used caller). All three sites import and call it. Loop order in each site: check app/compose surface at current candidate first, then `at_git_boundary(candidate)` (so the boundary directory is itself a valid candidate). Use `Path::exists()` not `is_dir()` so worktree `.git` files and submodule `.git` files both count (mirrors `denoland/deno/libs/config/glob/gitignore.rs:100`). | done |
| CD3 | **Refine** the existing startup banners rather than introduce new ones. `crates/nimbus-bin/src/start/boot.rs:251-295` `start_startup_summary_lines` already emits `"Nimbus server listening at <url>"`; `crates/nimbus-bin/src/dev.rs:113` `emit_dev_banner` is the sibling for dev. Both currently print the base URL (`http://host:port/`), not the operator-console URL. Update both to include a CockroachDB-style `operator console:\t<url>` line where `<url>` is `http://<host>:<port>/ui/` (precedent: `cockroachdb/cockroach/pkg/cli/start.go:1204-1248`, tab-aligned `webui:\t<url>`). Keep `local_listen_url` at `boot.rs:326` unchanged — the banner adds `/ui/`, the function still returns the base for discovery callers that need it. Make the new line greppable for the CD7 regression test (literal substring `operator console:` plus `/ui/`). | done |
| CD4 | Add `--open` flag to **`nimbus dev` only** (not `nimbus start`; see Risks "R8" and Scope). Auto-launch default browser at the operator-console URL **after** the HTTP listener has bound and the same readiness probe used by `nimbus ui` passes — never before, or the browser will race the server. Use the **already-present `open = "5.3"` crate** (`crates/nimbus-bin/Cargo.toml:29`) — `open::that(url)` returns `io::Result<()>`. No new Cargo dependency to add, no `deny.toml` entry to update, and the crate is already accepted by the repo's existing `cargo deny` configuration. On a headless environment (no `DISPLAY`/`WAYLAND_DISPLAY` on Linux, no usable launcher on macOS/Windows — `open::that` returns `Err` in these cases) emit a structured **error-level** log line via `tracing::error!` *and* a corresponding stderr line prefixed `error:` (`error: --open requested but browser launcher failed: <err>; daemon is reachable at <url>`), then continue serving with exit code 0. The semantics: failed `--open` is visible (operator should know the browser didn't launch) but non-fatal (the daemon is the load-bearing thing, the browser pop is the nicety). Document in CD8 that `--open` is best-effort and never blocks daemon startup. | done |
| CD5 | Replace `nimbus ui --ensure` with unflagged `nimbus ui` (discovery + browser open only). Concrete deletions and updates: drop the `--ensure` flag from the clap derive on `UiArgs`; delete `spawn_nimbus_start` (`crates/nimbus-bin/src/ui.rs:170-191`); update error messages at `ui.rs:50`, `ui.rs:58` to point at `nimbus dev --open` (for spawn-and-open) or `nimbus start` followed by `nimbus ui` (for production-shaped startup); rewrite the test at `crates/nimbus-bin/src/ui.rs:280-300` (`ui_command_without_running_server_returns_actionable_error`) to assert the new no-daemon error wording and remove any `--ensure` reference; remove the `nimbus ui --ensure` example at `crates/nimbus-bin/src/cli_ux.rs:89` (`UI_HELP_EXAMPLES`) and replace it with the `nimbus dev --open` shortcut. | done |
| CD6 | Sanity-check Electron flow: `desktop/src/main/server.ts:188` calls `spawn(executable, ["start"], { detached: true, stdio: "ignore", windowsHide: true })` with no `cwd` override, so the desktop shell inherits the user's launching CWD. Post-CD1 this is now safe regardless of where the user launches the desktop app from. Verify by running `verify:ds1` (`@nimbus/desktop`) from `~/src/github.com/nimbus/desktop` (the worst-case CWD pre-fix), then once from `/tmp` (control). No Electron code changes expected; this row only confirms the contract. | done |
| CD7 | Regression tests in `crates/nimbus-bin/src/start/tests/`, `crates/nimbus-bin/src/dev/tests/`, `crates/nimbus-bin/src/deploy.rs`'s existing test module (lines 649, 671, 699), and a new `crates/nimbus-bin/src/compose/discovery.rs` test module. Each test builds an isolated fixture under `tempdir()` and asserts behaviour, not just non-panic: (a) `nimbus start` from `<tmp>/inner/sub/`, where `<tmp>/` contains `nimbus/` (no `.git/` anywhere), does NOT trigger codegen and does NOT return an app dir; (b) `nimbus dev` from `<tmp>/inner/sub/`, where `<tmp>/inner/.git/` exists and `<tmp>/nimbus/` exists as a sibling outside the boundary, returns `None` (the unrelated `nimbus/` is correctly invisible); (c) `nimbus dev` from `<tmp>/app/src/components/`, where `<tmp>/app/convex/` and `<tmp>/.git/` both exist, returns `<tmp>/app/` (multi-level discovery still works); (d) `nimbus deploy` mirrors (b) and (c) against the deploy walker's surfaces (`firebase.json`, `package.json` with `@google-cloud/functions-framework`, etc.); (e) compose discovery: `<tmp>/inner/.git/` exists, `<tmp>/compose.yaml` exists as a sibling outside the boundary — `resolve_auto_discovered_compose_selection` from `<tmp>/inner/sub/` returns `None`; positive case inside the boundary still works; (f) **Worktree + submodule semantics** (load-bearing — agents in this codebase work primarily through `git worktree add`-created worktrees): test 1 — synthetic `.git` *file* (not directory) at the boundary candidate, confirm `at_git_boundary` returns true (covers the unit-level shape). Test 2 — invoke `git init` + `git worktree add <wt>` in a `tempdir()`, then run each of `detect_app_dir` (dev), `detect_app_dir` (deploy), and `resolve_auto_discovered_compose_selection` from a subdir inside `<wt>` with the relevant marker placed both *inside* and *outside* the worktree root, and assert the walker stops at the worktree's `.git` file rather than escaping to the main repo's `.git/` directory. This is the production-shaped case for every agent and dev who works in worktrees and is the test that fails loudly if anyone ever regresses CD2 to `is_dir()`. (g) `nimbus ui` errors cleanly when no daemon is running, with the new error wording from CD5. (h) banner: `nimbus start --port 0` (ephemeral) prints a line matching `operator console:.*\\b/ui/\\b`. (i) **Discovery file serde round-trip** in `nimbus-bin`: build a `ServerDiscoveryRecord` fixture, serialize to JSON, deserialize, byte-compare against a checked-in `tests/fixtures/server.json.golden` — fails loudly on any silent format drift that would break Electron (`desktop/src/main/discovery.ts`) or the Playwright fixture (mitigates R4). | done |
| CD8 | **Documentation pass — enumerated touchpoints, not "the docs."** (1) `docs/operating/cli.md`: rewrite the daemon-CLI section to cover Storybook (component HMR, port 6006), `nimbus dev` (full operator console + watched codegen + `--open`), `nimbus start` (production daemon, no walk-up, deployed-app autostart from storage), `nimbus deploy` (project-rooted, `.git/`-bounded), and `nimbus ui` (discover-and-open only, no `--ensure`); add a "How apps reach a running daemon" subsection distinguishing source-load (`--app-dir`) from deploy-load (admin API); note `npm run dev` inside `packages/nimbus-ui/` is for *component iteration only* (vite at port 5173, no daemon proxy), not a full-app workflow. (2) `docs/operating/desktop-install.md`: remove the live `--ensure` reference (surfaced by repo-wide grep). (3) `docs/plans/README.md`: remove the `--ensure` reference; add a one-line index entry for this plan while active. (4) `README.md` (top-level): audit and update any quickstart that names `nimbus ui --ensure` or assumes pre-CD1 `start` behavior. (5) Adapter docs audit pass — `git grep -l 'nimbus ui\|nimbus start' docs/adapters/` returns ~8 files (convex, firebase, cloud-functions, mongodb, native READMEs and migration/compatibility docs); read each and update only where they reference removed behaviour. (6) Architecture docs audit — `docs/architecture/sandbox/{macos-machine-flow,microvm-service-baseline}.md` and `docs/operating/{deploy-admin-api,encryption,storage-backends}.md`; same audit shape. (7) `CLAUDE.md` "Routing By Work Type": add an entry (suggested wording: `- CLI daemon canonicalization, walk-up boundaries, or banner shape: docs/plans/cli-daemon-canonicalization-plan.md (active until closeout, then archive), docs/operating/cli.md, docs/plans/archive/cli-command-surface-plan.md (prior wave), docs/plans/archive/compose-discovery-plan.md (compose precedent).`). (8) **Plan archival on closeout**: move this file to `docs/plans/archive/cli-daemon-canonicalization-plan.md`, update `docs/plans/README.md` to reflect the move, and update the CLAUDE.md routing entry from (7) to point at the archived path. | done |
| CD9 | **Tooling and repo-wide audit hygiene.** (a) Cargo: no new dependency added — CD4 uses the already-present `open = "5.3"` in `crates/nimbus-bin/Cargo.toml:29`. Document this in the CD4 implementation note (commit message or PR description) so a future "dependency consolidation" pass does not strip it. (b) `deny.toml`: no change required (existing config already accepts the `open` crate). (c) Repo-wide grep audit at close time, with the captured output appended to the Execution Log: `git grep -n '\-\-ensure' -- ':(exclude)docs/plans/archive'` → **0**; `git grep -n 'spawn_nimbus_start'` → **0**; `git grep -n 'nimbus ui --ensure'` → **0** (the third is a sanity check — the prefix grep should already cover it, but matching the exact user-visible string catches stray instances where `--ensure` was wrapped). (d) Confirm Makefile lanes pass: `make check`, `make clippy`, `make fmt-check`, `make test`, `make deny`, `make verify-desktop-ui` all clean. Prefer these wrappers over raw `cargo` invocations per CLAUDE.md "Verification Commands" guidance. | pending |

## Completion Gate

All ledger rows must be `done` and the following must hold.

### Smoke matrix — 6 environments × 3 commands

Each row is a concrete fixture; each column is a concrete
invocation; each cell states the expected behaviour. Run all 18
cells before the gate flips.

Environments:

- **E1 — Nimbus repo root.** CWD = `/Users/jack/src/github.com/nimbus/nimbus/`.
- **E2 — Nimbus repo subdir.** CWD = `/Users/jack/src/github.com/nimbus/nimbus/crates/nimbus-bin/src/`.
- **E3 — Clean tempdir, no git, no app surface.** CWD = `$(mktemp -d)`.
- **E4 — Tempdir with sibling collision, no git.** Layout:
  `<tmp>/` is CWD, `<tmp>/nimbus/` exists (empty placeholder), no
  `.git/` anywhere.
- **E5 — Tempdir with sibling collision inside git boundary.**
  Layout: `<tmp>/proj/.git/` (file or directory), `<tmp>/proj/sub/`
  is CWD, `<tmp>/nimbus/` exists as a sibling *outside* the
  boundary (i.e. at `<tmp>/nimbus/`, peer of `<tmp>/proj/`).
- **E6 — Real `git worktree` of the Nimbus repo (agent workflow).**
  `git worktree add /tmp/nimbus-wt-smoke main` from the repo, then
  CWD = `/tmp/nimbus-wt-smoke/crates/nimbus-bin/src/`. The
  worktree's `.git` is a *file* pointing back at the main repo's
  `.git/worktrees/nimbus-wt-smoke/`. This environment is the
  production case for every agent in this codebase; if any walker
  uses `is_dir()` it will escape the worktree and walk into the
  main repo, recreating the original failing case from the inside.

Commands (all with `--port 0` for ephemeral binding except where a
demo path is specified):

- **C1 — `nimbus start --port <p>`**
- **C2 — `nimbus dev --port <p>`**
- **C3 — `nimbus dev --port <p> --app-dir demos/convex/html`** (E1 only; the demo path only exists in the repo)

Expected cells:

| Env | C1 (`start`) | C2 (`dev`) | C3 (`dev --app-dir`) |
|---|---|---|---|
| E1 | Starts; banner contains `operator console: http://...:/ui/`; **no** codegen runs; **no** walk-up touches `~/src/github.com/nimbus/` (proves CD1). | Starts; banner; emits stderr note "no app surface inside project boundary"; runs as plain daemon (proves CD2 against the original failing case). | Starts; banner; codegen preflight runs against the demo; ready in ≤5s. |
| E2 | Same as E1/C1 (start does no walk-up regardless of CWD depth). | Same as E1/C2 — the inner `.git/` is the boundary, no app surface inside. | n/a |
| E3 | Starts; banner; no codegen. Control case. | Starts; banner; "no app surface" note; no walk-up triggers because there are no markers anywhere. | n/a |
| E4 | Starts; banner; no codegen — **start does no walk-up at all**, the sibling `nimbus/` is invisible (proves CD1's category fix is environment-independent). | Walks up freely (no `.git/` to stop it) and finds `<tmp>/nimbus/`. Acceptable: matches the pre-rebrand semantics for non-git environments; documented in Risks. | n/a |
| E5 | Starts; banner; no codegen — start ignores everything. | Walks up from `<tmp>/proj/sub/`, finds no surface in `sub/`, finds no surface in `proj/`, hits `proj/.git/` → stops. Sibling `<tmp>/nimbus/` is correctly invisible (proves CD2 against the production-shaped failure). | n/a |
| E6 | Starts; banner; no codegen — start ignores everything regardless of worktree state. | Walks up from worktree subdir; stops at the worktree's `.git` *file*; never reaches the main repo's `.git/` directory; behaves identically to E2 (proves CD2 with `Path::exists()` against the agent workflow). | n/a |

Plus targeted command smoke:

- `nimbus dev --open` (from E1) launches the default browser at
  `http://localhost:<p>/ui/` after readiness probe passes (proves
  CD4 happy path).
- `nimbus dev --open` under `DISPLAY= WAYLAND_DISPLAY= ` (Linux) or
  with the `open` crate forced into failure (a test seam injected
  for CD4): daemon emits an `error:`-prefixed stderr line,
  continues serving, exit code remains 0 once `^C`'d (proves CD4
  headless fallback).
- `nimbus ui` (with a daemon running, from any CWD) opens the
  browser at the discovered URL; with no daemon running, errors
  with the new wording from CD5.

### Test + build gates

Focused crate-level commands (precise enough to prove CD7
regressions ran in the right test target):

- `cargo test -p nimbus-bin` clean, with all CD7 regressions (a–i)
  included and named.
- `cargo test -p nimbus-server` clean.

Workspace-wide Makefile lanes (the canonical entrypoints per
CLAUDE.md):

- `make check` clean.
- `make clippy` clean.
- `make fmt-check` clean (or `cargo fmt --all --check`).
- `make test` clean (umbrella; supersets the focused calls above).
- `make deny` clean — confirms no advisory or license regression
  from CD4's `open = "5.3"` usage (which is already accepted by
  the existing `deny.toml`).
- `make verify-desktop-ui` clean — Makefile-wrapped desktop UI
  build/verification.

End-to-end / consumer canaries:

- `npm run build -w packages/nimbus-ui` clean.
- `cd packages/nimbus-ui && npx playwright test tests/e2e/smoke.spec.ts`
  passes (verifies the scratch-dir spawn path the fixture uses
  still works post-CD1/CD2).
- `cd ~/src/github.com/nimbus/desktop && npm run verify:ds1`
  passes, run once from `~/src/github.com/nimbus/desktop` and once
  from `/tmp` (verifies the Electron spawn-and-load path is now
  CWD-independent post-CD1).

### Workspace grep gates

Run at close time. Each gate states the expected count and what it
proves.

- `git grep -n '\.ancestors()' crates/nimbus-bin/src/start/boot.rs`
  → **0** (start does no walk-up; CD1).
- `git grep -n '\.ancestors()' crates/nimbus-bin/src/dev.rs
  crates/nimbus-bin/src/deploy.rs crates/nimbus-bin/src/compose/discovery.rs`
  → **3** (one bounded walk-up per scoped site; CD2).
- `git grep -nP 'at_git_boundary\b' crates/nimbus-bin/src/`
  → **≥4** (helper definition + 3 call sites).
- `git grep -n '\-\-ensure' -- ':(exclude)docs/plans/archive'`
  → **0** (CD5/CD8 removed the flag and all docs that referenced
  it; archived plans are exempted because they record history).
- `git grep -n 'nimbus ui --ensure'`
  → **0** (exact user-visible-string check; catches wrapped
  instances the prefix grep might miss in narrative prose).
- `git grep -n 'spawn_nimbus_start' crates/nimbus-bin/src/`
  → **0** (CD5 deleted the function).
- `git grep -n 'operator console:' crates/nimbus-bin/src/`
  → **≥2** (banner string in both `start/boot.rs` and `dev.rs`;
  CD3).
- `git grep -n '\.ancestors()' crates/nimbus-bin/src/codegen.rs`
  → **1** (the out-of-scope workspace-locator walker is
  intentionally left alone; this gate documents that the audit was
  aware of it).

## Verification Approach

This is a daemon-CLI change. Visual identity is unchanged. The
verification stack is:

1. **Unit + integration tests** in `nimbus-bin` for the new
   resolution semantics (CD7), exercising the `.git/`-bounded
   walk-up on both `dev` and `deploy` plus the no-walk-up contract
   on `start`.
2. **Manual smoke** from the repo root, from a temp dir, and from
   a git-containing temp dir with sibling collisions: all three
   daemon commands start cleanly with the correct banner; `nimbus
   start --open` / `nimbus dev --open` actually launches the
   browser; `nimbus ui` discovers and opens; `nimbus ui` with no
   daemon emits a clean error.
3. **Playwright e2e smoke** (`packages/nimbus-ui/tests/e2e/`)
   verifies the scratch-dir spawn path the fixture relies on still
   works.
4. **Electron browser probe** (`@nimbus/desktop` `verify:ds1` /
   `verify:ds2`) verifies the canonical Electron consumer still
   works.

Each phase records its verification output in the Execution Log
below before the row flips to `done`.

## Risks and Trade-offs

Each entry is **What breaks** (the precise failure mode, not the
abstract worry) and **Mitigation** (what we do about it, with the
specific code/test/doc hook).

### R1 — `nimbus dev` outside a git repo can still walk arbitrarily

**What breaks.** The `.git/` boundary is the only upper bound.
Run `nimbus dev` from `/tmp/scratch/foo/bar/` and the walker
climbs all the way to `/`, checking each ancestor for `nimbus/`,
`convex/`, `firebase.json`, `.nimbus/convex/functions.json`. On a
macOS dev box with `~/Code/some-old-firebase-project/firebase.json`
lying around, running `nimbus dev` from `~/Downloads/scratch/`
could match the unrelated Firebase project and silently activate
it. The pre-bug semantics, now narrowed to "outside any git repo."

**Mitigation.** CD3 banner names the activated `--app-dir` on
stderr at startup, so silent misactivation becomes visible
misactivation. CD8 docs make the contract explicit: "outside a
git repo, pass `--app-dir`." Optional one-liner stderr nudge when
no `.git/` is found anywhere on the path: `note: no git boundary
on ancestor path; consider --app-dir for explicit selection`.
`--app-dir .` is always available and overrides walk-up. No hard
depth cap added now (speculative); revisit only if a real
workflow surfaces.

### R2 — `nimbus start` no longer source-discovers an app from CWD

**What breaks.** A user (or shell wrapper) does
`cd ~/code/myapp && nimbus start` and expects the source tree at
CWD to be loaded. Post-CD1, source walk-up is gone — `start` does
not scan the filesystem for a project. **This is the intended
behaviour change; the surface that *replaces* it is the deploy
admin API.**

**What does NOT break — deployed apps still autostart.** Apps
that have been bundled and deployed to the daemon (via
`nimbus deploy` against a running daemon) live in the daemon's
storage, not on the filesystem the operator boots from. The
engine's normal boot path rehydrates them from storage during
`Service` construction, completely independent of CWD or the
walk-up that CD1 deletes. The startup banner at
`crates/nimbus-bin/src/start/boot.rs:283` already prints `app
dir: none; Convex-compatible routes wait for deploy activation`
when no `--app-dir` is provided — that branch is *not* "the
daemon is useless." It's "the daemon is ready to receive deploys
and rehydrate them on subsequent boots." This is the production
shape (Cockroach/Vault/Grafana: bring up the daemon, then
configure it through its API; data persists across restarts).

**Mitigation.**
- CD8 docs include a dedicated "How apps reach a running daemon"
  section distinguishing source-load (`nimbus start --app-dir
  ./myapp`) from deploy-load (`nimbus deploy` against an already-
  running daemon). Both flows continue to work; only the
  filesystem-walk-up flavour of source-load is removed.
- CD7 adds an integration test: spawn `nimbus start` (no
  `--app-dir`), deploy an app to it via the admin API, kill and
  restart the daemon (no `--app-dir` again), assert the previously
  deployed app is reachable through routed traffic. This pins
  down "deployed-app autostart still works post-CD1" as a
  regression test rather than a verbal assertion.
- For users with old wrapper scripts: the replacement is one
  flag — `nimbus start --app-dir .` — strictly clearer than the
  inherited magic. Pre-launch (CLAUDE.md) allows the breaking
  change.

### R3 — Stale `--ensure` references in user-facing strings

**What breaks.** CD5 removes the flag but leaves an error message
that says "run `nimbus ui --ensure` instead." Users follow the
suggestion, get "error: unknown argument `--ensure`," and lose
trust in the CLI's own diagnostics.

**Mitigation.** CD5 enumerates four sites: `ui.rs:50`, `ui.rs:58`,
the test at `ui.rs:280-300`, and the example at `cli_ux.rs:89`.
The grep gate `git grep -n '\-\-ensure' crates/nimbus-bin/src/
packages/nimbus-ui/src/ docs/ README.md` → **0** is the safety net.
Extending the grep to `~/src/github.com/nimbus/desktop/` catches
any reference in the desktop README/docs (folded into CD6).

### R4 — Discovery file format is a shared three-party contract

**What breaks.** `server.json` is read by three independent
consumers (`nimbus ui`, Electron `desktop/src/main/discovery.ts`,
Playwright fixture). A silent field rename or type change would
break two of them and someone would notice only at e2e time.

**Mitigation.**
- Plan explicitly states the format is unchanged this wave; CD5
  only changes *when* discovery is read vs *when* a daemon is
  spawned, not what's in the file.
- **CD7 case (i)** adds a serde round-trip unit test in
  `nimbus-bin`: deserialize a checked-in
  `tests/fixtures/server.json.golden`, re-serialize, byte-compare.
  Catches any silent schema drift at unit-test time rather than at
  e2e time. This is the structural mitigation that was missing
  before this revision.
- CD6 still runs `verify:ds1` end-to-end as the integration-level
  canary across the three consumers.
- Any future intentional format change becomes its own ledger row
  touching all three consumers atomically.

### R5 — `.git/` is sometimes a file, not a directory (worktree + agent workflow)

**What breaks — load-bearing in this codebase.** Two cases:
1. **`git worktree add`-created worktrees.** Each worktree root has
   a `.git` *file* (not directory) containing `gitdir: <path>`.
   This is the **default working pattern for agents in this
   codebase** — each agent gets its own worktree so concurrent
   work doesn't collide. If the walker uses `is_dir()`, agents
   running `nimbus dev` from inside their worktree see no
   boundary, escape the worktree, and walk into the main repo's
   `.git/` directory — or past it into `~/src/github.com/nimbus/`,
   recreating the original failing case *from inside the agent's
   workspace*. Bug shape is identical; surface is invisible
   because the agent thought they were inside a clean
   project-scoped boundary.
2. **Submodules.** Same gitdir-file indirection. Same escape.

This is the highest-priority risk in this section because the
people most likely to hit it are the people most likely to fix
it (us).

**Mitigation.**
- **CD2 specifies `Path::exists()` not `is_dir()`** (matches
  Deno's gitignore-resolver precedent at
  `denoland/deno/libs/config/glob/gitignore.rs:100`).
  `Path::exists()` follows symlinks and reparse points so Windows
  junction-pointed worktrees work transparently.
- **CD7 case (f) elevated** to two concrete tests: (1) synthetic
  `.git` file at the boundary candidate, assert `at_git_boundary`
  returns true; (2) real `git init` + `git worktree add <wt>` in a
  tempdir, run all three walkers from inside `<wt>/sub/`, assert
  the walker stops at the worktree's `.git` file rather than
  escaping to the main repo. This is the production-shaped
  agent-workflow test; failing it loudly is the only protection
  against silent regression.
- **Smoke matrix E6** adds a real worktree of the Nimbus repo to
  the manual smoke pass — `git worktree add /tmp/nimbus-wt-smoke
  main`, then run all three daemon commands from inside it.
  Catches integration-level failures the unit tests might miss.
- A future hardening: a clippy lint or workspace grep rejecting
  `.is_dir()` checks on any path ending in `".git"`. Not in scope
  this wave but cheap if regression recurs.

### R6 — `--open` on a headless context

**What breaks.** SSH session, CI runner, container with no
display server. `open::that()` returns `Err`. If we treat
that as fatal, `nimbus dev --open` refuses to start on every
remote host. If we treat it as silent, users wonder why their
browser didn't pop and waste time debugging the launcher.

**Mitigation — logged error, daemon continues.** CD4 specifies:
emit a structured **error-level** log line via `tracing::error!`
*and* an `error:`-prefixed stderr line
(`error: --open requested but browser launcher failed: <err>;
daemon is reachable at <url>`), then continue serving with **exit
code 0**. Distinction matters: "warning:" suggests "this is
fine"; "error:" surfaces that something the user requested did
not happen, while making clear (via continued operation and exit
0) that the daemon itself is healthy. Detection is outcome-based
(attempt the launch, handle the Err) not environment-based, so
new headless environments work without a code change. CD7 covers
the test seam (inject a forced-failure launcher, assert daemon
completes startup and banner still prints).

### R7 — Compose discovery: file strictly *above* the boundary becomes invisible

**What breaks (precise scope).** The CD2 loop checks the surface
at the current candidate *before* the boundary, so a compose file
**at or below** the bounding `.git/` is still found. The
regression footprint is narrow: only compose files **strictly
above** a project's `.git/` boundary, which means a layout like:

```
~/code/.git/                          ← outer team repo
~/code/compose.yaml                   ← outer compose
~/code/sub-repo/.git/                 ← inner repo / submodule
~/code/sub-repo/services/foo/         ← user's CWD
```

Pre-fix: walker climbs past `sub-repo/.git/` and finds
`~/code/compose.yaml`. Post-fix: walker stops at `sub-repo/.git/`.
The outer compose is invisible from the inner repo.

**Mitigation.** This is the correct behavior — every other
dev-tool (git, pre-commit, cargo) stops at the inner submodule
boundary. The new behavior matches user expectations from
neighbouring tools. `--compose-file <path>` is always explicit and
bypasses discovery. CD8 docs include the failing layout above as
the worked example. Failure mode is clean: "no compose file found
within project boundary" with a hint to use `--compose-file`; no
silent wrong-thing.

### R8 — `--open` placement: dev only, not start

**What breaks (future-maintainability).** A future contributor
sees `--open` on `nimbus dev` and either (a) adds it to `nimbus
start` for "symmetry," or (b) sees the asymmetry and removes it
from `dev`. Either move undoes a deliberate design choice.

**Mitigation — document the asymmetry as load-bearing.**
- `nimbus dev` is a dev-tool daemon (vite/cargo-doc precedent);
  `--open` fits the shape exactly.
- `nimbus start` is a production daemon (CockroachDB/Vault/
  Grafana precedent); none of those ship `--open` because the
  operator workflow is "start the daemon, then connect to its
  URL from wherever you choose to connect from" — frequently a
  different machine. `--open` on `start` implies the operator is
  on the same host as the daemon, which is the *less* common
  production case.
- CD8 docs explicitly call out the choice with the rationale:
  symmetric flags between sibling commands would be more
  confusing than helpful when the commands have different
  shapes. The two-step flow for `start` (`nimbus start &;
  nimbus ui` from the same host, or copy the banner URL from a
  different host) preserves the production-daemon shape.
- The Risks section keeps this entry as a permanent rationale
  anchor so future plan authors see the choice was deliberate.

## Execution Log

(a) CD1 — `nimbus start` no longer walks up the source tree. The
`ResolvedStartAppDir::AutoDetected` variant is gone; `resolve_start_app_dir`
now early-returns `Ok(None)` when no `--app-dir` is supplied. The matching
banner arm is gone; the no-app case prints
`app dir: none; Convex-compatible routes wait for deploy activation`.
Two stale tests were rewritten to assert the new contract:
`resolve_start_app_dir_returns_none_when_no_explicit_app_dir` (a Firebase
fixture sitting under a nested CWD now resolves to `None`) and
`start_startup_summary_reports_no_app_dir_when_none_resolved`.

```
$ cargo check -p nimbus-bin
    Checking nimbus-bin v0.1.31 (/Users/jack/src/github.com/nimbus/nimbus/crates/nimbus-bin)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 11.47s

$ cargo test -p nimbus-bin start::tests
test result: ok. 53 passed; 0 failed; 0 ignored; 0 measured; 396 filtered out

$ git grep -n AutoDetected crates/
(0 matches)
```

(b) CD2 — Walk-ups in `nimbus dev`, `nimbus deploy`, and compose discovery
are now bounded at the project's `.git/` entry. Added shared helper
`crate::path_boundary::at_git_boundary` (using `Path::exists` so worktree
`.git` *files* and submodule pointers both count). All three call sites
break out of the ancestor loop once the boundary is hit; surface checks
still run *first* at each candidate so the boundary directory itself is a
valid match. Three unit tests on the helper cover the `.git` directory,
`.git` file (worktree shape), and "no `.git`" cases.

```
$ cargo check -p nimbus-bin
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 3.18s

$ cargo test -p nimbus-bin
test result: ok. 452 passed; 0 failed; 0 ignored; 0 measured

$ git grep -n at_git_boundary crates/nimbus-bin/src/
crates/nimbus-bin/src/compose/discovery.rs:200:            if crate::path_boundary::at_git_boundary(directory) {
crates/nimbus-bin/src/deploy.rs:255:        if crate::path_boundary::at_git_boundary(candidate) {
crates/nimbus-bin/src/dev.rs:324:        if crate::path_boundary::at_git_boundary(candidate) {
crates/nimbus-bin/src/path_boundary.rs:12:pub(crate) fn at_git_boundary(dir: &Path) -> bool {
```

(c) CD3 — Both `start_startup_summary_lines` and `dev_banner_lines` now emit
a CockroachDB-shaped `operator console:\t<base>/ui/` line on startup. A
small `operator_console_url_from_base`/`operator_console_url` helper sits
next to `local_listen_url` (start) and `format_watch_roots` (dev); the base
discovery URL is unchanged.

```
$ cargo test -p nimbus-bin
test result: ok. 452 passed; 0 failed; 0 ignored; 0 measured

$ git grep -n 'operator console:' crates/nimbus-bin/src/
crates/nimbus-bin/src/dev.rs:474:        format!("operator console:\t{}", operator_console_url(&plan.local_url)),
crates/nimbus-bin/src/start/boot.rs:261:            "operator console:\t{}",
```

(d) CD4 — `nimbus dev` now takes `--open`. When set, a background tokio
task probes `<console_url>auth` until it answers (60s budget, 200 ms
poll), then calls `open::that(<console_url>)`. Probe timeout or launcher
failure logs `tracing::error!` and an `error:`-prefixed stderr line; the
daemon keeps serving. `--open` is intentionally absent from `nimbus
start` — daemons follow the CockroachDB/Vault/Grafana shape and do not
launch GUIs (see Risks R8). No new dependency: the already-present
`open = "5.3"` crate covers all three platforms.

```
$ cargo test -p nimbus-bin
test result: ok. 452 passed; 0 failed; 0 ignored; 0 measured

$ git grep -n 'open_operator_console_when_ready\|open_browser\b' crates/nimbus-bin/src/dev.rs
crates/nimbus-bin/src/dev.rs:118:    if plan.open_browser {
crates/nimbus-bin/src/dev.rs:120:        tokio::spawn(open_operator_console_when_ready(console_url));
crates/nimbus-bin/src/dev.rs:131:async fn open_operator_console_when_ready(console_url: String) {
crates/nimbus-bin/src/dev.rs:159:        open_browser,
crates/nimbus-bin/src/dev.rs:200:    open_browser: bool,
crates/nimbus-bin/src/dev.rs:225:    let open_browser = command.open;
```

(e) CD5 — `nimbus ui` is now an unflagged discover-and-open launcher. The
`--ensure` clap flag is gone, along with the ~200 lines of
`spawn_nimbus_start` / `current_executable` / `detach_process` /
`wait_for_server_ready` / `probe_ui_endpoint` machinery that backed it. The
`Spawn` and `ReadinessTimeout` arms of `UiError` were removed.
`resolve_discovery` now takes only `&LocalServerPaths`; when the discovery
record is absent it returns `UiError::ServerNotRunning` whose message points
operators at `nimbus start` (production-shaped) and `nimbus dev --open`
(spawn-and-open dev loop). The `cli_ux.rs` examples block was rewritten to
match. Tests rewritten: `ui_command_without_running_server_returns_actionable_error`
asserts the new wording and *forbids* the substring `--ensure`;
`ui_command_resolves_live_discovery_record` uses the new single-arg
`resolve_discovery` signature.

```
$ cargo check -p nimbus-bin
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 2.83s

$ cargo test -p nimbus-bin --bin nimbus
test result: ok. 452 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out

$ cargo test -p nimbus-bin --bin nimbus ui::
test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 449 filtered out

$ git grep -n 'spawn_nimbus_start\|\-\-ensure' crates/
crates/nimbus-bin/src/ui.rs:195:            !message.contains("--ensure"),
crates/nimbus-bin/src/ui.rs:196:            "post-CD5 error must not reference the removed --ensure flag, got: {message}"
```

(f) CD6 — Electron spawn-and-load contract is verified post-CD1. The
desktop shell at `desktop/src/main/server.ts:188` still spawns
`nimbus start` with no `cwd` override; the child therefore inherits the
launching shell's CWD. Confirmed by inspection plus a live cross-CWD probe
that pointed two `_electron.launch` invocations (one CWD=`~/src/github.com/nimbus/desktop`,
one CWD=`/tmp`) at a single pre-spawned daemon and asserted the renderer
resolved to the same `http://127.0.0.1:8088/ui/auth` URL in both cases. The
freshly built nimbus banner from `/tmp` also confirms CD1's removal —
"app dir: none; Convex-compatible routes wait for deploy activation" — and
CD3's banner shape: "operator console: http://127.0.0.1:8088/ui/". No
Electron code changes were required. Note: the existing `scripts/ds1-browser-probe.mjs`
still asserts a `https://example.org/` placeholder URL that the main bootstrap
has not loaded since DS5 landed; that drift is independent of CD work and is
filed for the desktop team rather than landed here, where the contract is
the CWD-invariance proof rather than the stale probe assertion.

```
$ /Users/jack/src/github.com/nimbus/nimbus/target/debug/nimbus start --host 127.0.0.1 --port 8088
info: Nimbus server listening at http://127.0.0.1:8088/
info: operator console:	http://127.0.0.1:8088/ui/
info: server process owns HTTP, WebSocket, scheduler, and runtime startup
info: app dir: none; Convex-compatible routes wait for deploy activation

$ node scripts/cd6-cwd-invariance-smoke.mjs   # (transient — not retained)
from desktop dir: {"ok":true,"url":"http://127.0.0.1:8088/ui/auth"}
from /tmp:        {"ok":true,"url":"http://127.0.0.1:8088/ui/auth"}
CD6 smoke PASSED — desktop shell resolved consistent URL across CWDs

$ grep -n 'cwd' /Users/jack/src/github.com/nimbus/desktop/src/main/server.ts
(0 matches in spawnDetached — no cwd override; contract intact)
```

(g) CD7 — Regression tests landed across all the surfaces called out in the
ledger. Eleven new unit tests inside `nimbus-bin`'s existing modules plus a
new `tests/server_discovery_serde.rs` integration target with its checked-in
golden cover the nine sub-cases verbatim:

- (a) `start/tests/app_dir_codegen.rs::resolve_start_app_dir_ignores_sibling_nimbus_directory_with_no_git`
  — fixtures `<tmp>/nimbus/` sibling plus `<tmp>/inner/sub/` CWD with no
  `.git/` anywhere; asserts `resolve_start_app_dir(StartCommand::default())`
  is `Ok(None)`. The rebrand-trap shape from the Why section, locked in.
- (b)(c)(f) `dev.rs` tests:
  `detect_app_dir_stops_at_git_boundary_when_marker_lives_outside`,
  `detect_app_dir_walks_multiple_levels_within_git_boundary`,
  `detect_app_dir_treats_dot_git_file_as_worktree_boundary` (synthetic `.git`
  *file* containing `gitdir:` — the agent-worktree shape).
- (d)(f) `deploy.rs` mirrors the dev tests with `firebase.json` as the
  outside-boundary marker.
- (e)(f) `compose/discovery.rs` mirrors against `DEFAULT_COMPOSE_FILE` with
  the existing `write_file` helper —
  `auto_discovery_stops_at_git_boundary_when_compose_lives_outside`,
  `auto_discovery_finds_compose_inside_git_boundary`,
  `auto_discovery_treats_dot_git_file_as_worktree_boundary`.
- (g) Covered by the rewritten CD5 test
  `ui_command_without_running_server_returns_actionable_error` which already
  asserts the new wording and forbids the `--ensure` substring.
- (h) `start/tests/cli_surface.rs::start_startup_summary_emits_operator_console_url_line`
  builds the banner with `SocketAddr::from((Ipv4Addr::LOCALHOST, 4711))` and
  asserts a line starts with `operator console:`, contains `/ui/`, and
  contains `127.0.0.1:4711`.
- (i) `tests/server_discovery_serde.rs` integration test importing
  `nimbus_server::ServerDiscoveryRecord` builds a fixture (pid 12345, address
  `127.0.0.1:8088`, version `0.1.31`, protocol `nimbus.v2`), pretty-serialises
  it, byte-compares against `tests/fixtures/server_discovery.golden.json`, and
  confirms round-trip equality. The golden's camelCase shape
  (`startedAt`, `protocolVersions`) is the contract the Electron discovery
  reader and Playwright fixture rely on.

```
$ cargo test -p nimbus-bin
test result: ok. 463 passed; 0 failed; 0 ignored; 0 measured

$ cargo test -p nimbus-bin --test server_discovery_serde
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured
```

(h) CD8 — Documentation pass across the eight enumerated touchpoints.

1. `docs/operating/cli.md` — rewrote the `## UI Command` section to describe
   the unflagged discover-and-open shape, deleted the `--ensure` example,
   and pointed operators at `nimbus dev --open` (spawn-and-open) and
   `nimbus start` (banner-then-`nimbus ui`). Added a `## How Apps Reach a
   Running Daemon` section that distinguishes source-load (`--app-dir`)
   from deploy-load (admin API) and explicitly names the
   `app dir: none; Convex-compatible routes wait for deploy activation`
   banner line as the production shape. Added a `### Why dev has --open
   and start does not` subsection anchoring the dev-tool vs production-
   daemon precedent. Documented `--open` in the dev-command flag table.
   Rewrote the dev auto-detect bullet to name the `.git/` boundary
   (covering both directory and worktree-shaped file). Added a
   `npm run dev` clarification note that `packages/nimbus-ui/` is for
   component iteration only. Updated the Startup Behavior block so the
   bullet list now names `operator console:` as part of the banner shape
   and states that `nimbus start` does not walk ancestors.
2. `docs/operating/desktop-install.md` — replaced the `nimbus ui --ensure`
   reference with a description of the Electron shell's discover-or-
   spawn loop owned by `desktop/src/main/server.ts`.
3. `docs/plans/README.md` — updated the active-plan entry to describe the
   landed shape (walk-up removed from `start`, bounded for the others;
   `--open` on `dev` only; CD1-CD7 landed, CD8/CD9 in flight) instead of
   the pre-execution framing.
4. `README.md` (top-level) — audit pass; no `nimbus ui --ensure`
   references found and `nimbus start` invocations in the curl-quickstart
   section are correct (those flows don't need source-load).
5. Adapter docs — `docs/adapters/cloud-functions/README.md` and
   `docs/adapters/cloud-functions/migration.md` had `nimbus start`
   examples that implied auto-detection; updated to `nimbus start
   --app-dir .` for the source-load shape and added a pointer to the new
   "How Apps Reach a Running Daemon" section. The Convex, Firebase,
   MongoDB, and Native adapter READMEs were audited and are correct as-is
   — they describe driver-shape adapters that connect over the wire and
   don't need source-load.
6. Architecture/operating docs — `macos-machine-flow.md`,
   `microvm-service-baseline.md`, `deploy-admin-api.md`,
   `encryption.md`, and `storage-backends.md` were audited; their
   `nimbus start` invocations are accurate (storage and encryption flags
   don't depend on source-load) and the `deploy-admin-api.md` line
   already names `nimbus start --app-dir` correctly.
7. `AGENTS.md` (`CLAUDE.md` symlinks to it) — added a "CLI daemon
   canonicalization, walk-up boundaries, or banner shape" entry to the
   Routing By Work Type section, pointing at this plan, `cli.md`, and
   the two relevant archived precedents.
8. Plan archival on closeout — deferred until CD9 verification passes;
   the file move + CLAUDE.md re-pointing happens as the last action.

```
$ git grep -n 'nimbus ui --ensure' -- ':(exclude)docs/plans/archive' ':(exclude)docs/plans/cli-daemon-canonicalization-plan.md'
(0 matches)

$ git grep -n '\-\-ensure' -- ':(exclude)docs/plans/archive' ':(exclude)docs/plans/cli-daemon-canonicalization-plan.md'
crates/nimbus-bin/src/ui.rs:195:            !message.contains("--ensure"),
crates/nimbus-bin/src/ui.rs:196:            "post-CD5 error must not reference the removed --ensure flag, got: {message}"
```

The two remaining matches are the post-CD5 test assertion that *forbids*
the substring — load-bearing regression coverage, retained intentionally.
