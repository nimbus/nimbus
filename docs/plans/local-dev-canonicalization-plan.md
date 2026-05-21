# Plan: Local-Dev Canonicalization

Make `nimbus-server`'s cross-toolchain dependency on the `nimbus-ui` JS
build explicit and self-healing via Make's dependency graph, so a fresh
clone running `make ci` (or `make check`, `make test`, `make clippy`,
`make verify-desktop-ui`, `make coverage`) builds the prerequisite UI
artifacts automatically, the `build.rs` stub fallback is removed, and
CI workflows no longer carry five copies of explicit `npm run codegen`
/ `npm run build` steps. Aligns the build contract with the dominant
Rust + embedded-SPA pattern (Meilisearch, Tabby, Tauri) — Make as the
dependency-driven orchestrator, `build.rs` as the input-asserter.

---

## Status

- **Status:** `in_progress`
- **Created:** 2026-05-21
- **Primary owner:** this plan
- **Activation gate:** met. The current setup has three load-bearing
  symptoms of one root architectural debt:
  1. **Fresh-clone `cargo check` / `cargo test` / `cargo clippy` fail
     to compile.** `crates/nimbus-server/src/adapters/convex/registry/loading.rs:5-32`
     has seven `include_str!` / `include_bytes!` calls into
     `packages/nimbus-ui/.nimbus/convex/` — a gitignored directory only
     populated by `npm run codegen -w packages/nimbus-ui`. CI papers
     over this with five identical `npm run codegen` steps across
     `.github/workflows/ci.yml` (`rust-fmt`, `rust-clippy`,
     `rust-workspace-tests`, `rust-doctests`, `coverage`); the
     desktop-ui job missed the same step and broke on 2026-05-19,
     fixed only by adding a sixth copy in `Makefile:verify-desktop-ui`.
  2. **`build.rs` lies to keep `cargo build` green.** When `packages/nimbus-ui/dist/index.html`
     is missing, `crates/nimbus-server/build.rs:30-53` writes a stub
     HTML in debug profile (release errors). The stub has no inline
     FOUC `<script>`, so `http::ui::tests::inline_fouc_script_hash_matches_csp`
     (`crates/nimbus-server/src/http/ui.rs:567-606`) panics in any
     environment that compiles cleanly from the stub. CI hits this
     reliably; local dev usually doesn't, because the developer's
     `dist/` is already populated.
  3. **No documented "build from clean clone" contract.** The Makefile's
     `build:` and `release:` targets depend on `build-ui`, but
     `check:`, `test:`, `clippy:`, `ci-required:` do not. A new
     contributor running `make check` on a fresh clone gets a cryptic
     `include_str!` failure with no pointer to the missing `npm`
     step.

  The fix lands one Makefile dependency graph + one honest `build.rs`
  + one CI cleanup and resolves all three symptoms.

## Why

### Architectural root cause: cross-workspace `include_str!`

`nimbus-server` (Rust crate) is compile-time coupled to artifacts
produced by `npm run codegen -w packages/nimbus-ui` (the JS workspace).
This is *not* a Rust-style `build.rs` codegen — it's a separate
toolchain in a separate workspace whose outputs the Rust crate reads
as if they were source. That coupling is real, defensible (the JS
workspace is the source of truth for the system bundle), and not
worth ripping out today — but it must be made *explicit* in the build
contract instead of *implicit* via "remember to run npm first."

The seven coupled paths:

```text
packages/nimbus-ui/.nimbus/convex/auth.config.json
packages/nimbus-ui/.nimbus/convex/bundle.mjs
packages/nimbus-ui/.nimbus/convex/bundle.sha256
packages/nimbus-ui/.nimbus/convex/functions.json
packages/nimbus-ui/.nimbus/convex/http_routes.json
packages/nimbus-ui/.nimbus/convex/node_external_packages.json
packages/nimbus-ui/.nimbus/convex/schema.json
```

Plus the embedded SPA assets under `packages/nimbus-ui/dist/`, served
via `rust-embed` (`crates/nimbus-server/src/http/ui.rs:51-55`) and
pinned by CSP-hash assertion (`crates/nimbus-server/src/http/ui.rs:567-606`).

### Survey: how comparable Rust+embedded-SPA projects solve this

The shape — Rust binary embedding a JS-toolchain-built SPA — is
common enough to have a dominant pattern, and that pattern is *not*
the canonical Rust `build.rs`-orchestrates-everything answer:

| Project | Embed mechanism | Orchestrator | `build.rs` runs npm? |
|---|---|---|---|
| **Meilisearch** | `rust-embed` | Makefile | No — asserts dist |
| **Tabby** (TabbyML) | `RustEmbed` | Makefile | No — asserts dist |
| **Tauri** v2 | `tauri::generate_context!` | `cargo tauri` CLI | No — checks frontend dir |
| **Rerun** | Custom embed | `pixi` tasks | No — asserts WASM |
| Deno | `include_str!` from `OUT_DIR` | `build.rs` | **Yes** — but JS is *checked-in source*, not toolchain output |
| Tonic | `include!` from `OUT_DIR` | `build.rs` invokes `protoc` | **Yes** — single self-contained binary, not a toolchain |
| Helix | `include_bytes!` from `OUT_DIR` | `build.rs` invokes `tree-sitter` | **Yes** — single binary |

The empirical rule: when a Rust crate embeds output from a **simple
single-binary tool** (`protoc`, `tree-sitter`), `build.rs` invokes it.
When it embeds output from a **heavy multi-process toolchain** (`npm`
+ Vite + Rolldown + esbuild), the orchestrator lives *outside*
`build.rs` — Makefile, xtask, or a CLI shell. `build.rs` only
*asserts* inputs exist. Reasons spawning npm from `build.rs` fails in
practice:

- Cargo's stdout filtering swallows npm progress output → debugging is
  awful.
- Every `cargo check`, `cargo clippy`, `cargo doc`, and rust-analyzer
  IDE save risks re-running build.rs (one stale `rerun-if-changed`
  and you eat 30s of npm).
- Cargo's caching is keyed on build.rs's own outputs; the JS toolchain
  has its own cache. Cache-on-cache produces subtle invalidation bugs.
- Cross-toolchain debugging from inside build.rs is one extra layer
  of indirection.

Meilisearch and Tauri both *had* `build.rs`-orchestration variants
early on and walked them back. The Pattern C answer (Make-as-orchestrator,
build.rs-as-asserter) is what shipped.

### The "make bootstrap" ritual trap

The naïve Pattern C is a separate `make bootstrap` step the developer
has to remember. That's a ritual — it puts the burden on the human to
recall the order. The *correct* Pattern C makes Make do its job:

- `$(UI_CODEGEN)` and `$(UI_DIST)` are file targets with real
  prerequisites (their JS source files).
- Every cargo-invoking Makefile target lists `$(UI_DIST)` as a Make
  prerequisite.
- A fresh-clone `make test` walks the dependency graph: target needs
  `$(UI_DIST)` → recipe runs `npm run build` → which needs `$(UI_CODEGEN)`
  → recipe runs `npm run codegen` → done; then cargo runs.

The user runs `make test`. Make handles bootstrap. No ritual, no docs
required to recall.

### Can Nimbus build Nimbus?

Nimbus's runtime hosts JS via deno_core + Node-compat, so it is
reasonable to ask whether `nimbus run` could replace `npm run codegen`
and `npm run build`. Three sub-questions, honest answers:

1. **Could `nimbus run` execute `@nimbus/codegen` (our own tool)
   today?** Likely close. `@nimbus/codegen` is a Node script in
   `packages/codegen/` that emits the system bundle. If its API
   surface stays inside what `nimbus-runtime`'s Node-compat covers
   today (`fs`, `path`, `JSON.parse`, glob), it should work — but
   that's a verification effort, not a known property. Testable
   independently. **Defer to a follow-on plan, not this one.**
2. **Could `nimbus run` execute Vite today?** No. Vite drags in
   esbuild (spawned native binary), Rolldown (Rust binary via N-API),
   chokidar (native fsevents), ESM-loader hooks, worker_threads
   parallelism, and a plugin ecosystem with arbitrary Node API
   surface. Even Bun (a far more mature Node-compat runtime) only
   recently got Vite running cleanly. Multi-quarter scope, small
   payoff since Vite under Node works fine today.
3. **Could we skip Vite and use Rolldown directly from Rust?**
   Rolldown is published as a Rust crate. A Rust-native bundler from
   `build.rs` or `xtask` would eliminate Node from the SPA build —
   but we'd reimplement Vite's plugin system, dev server, HMR, and
   ecosystem support. Bad cost/benefit until Vite-the-orchestrator
   becomes a problem.

**Verdict for this plan:** Node stays as a documented dev build
dependency. The *interesting* follow-on — `nimbus run packages/codegen/...`
replacing `npm run codegen` — is named in the deferred section but
not bundled into this wave. It is a 1-item plan that can land once
`@nimbus/codegen`'s API surface is verified under our runtime.

### The user-visible promise

After this plan:

- `git clean -fdx && make ci` on a freshly cloned tree produces a
  green run with no manual `npm` invocation. Same for `make check`,
  `make test`, `make clippy`, `make verify-desktop-ui`, `make build`,
  `make release`.
- `cargo build -p nimbus-server` on a tree that already has
  `packages/nimbus-ui/dist/index.html` succeeds (current behavior
  preserved).
- `cargo build -p nimbus-server` on a tree without `dist/` fails with
  a single-line actionable error pointing at `make` (no stub, no
  silent inline-script-missing CSP test panic downstream).
- `.github/workflows/ci.yml` has zero `npm run codegen` and zero
  `npm run build` steps in the Rust jobs; each Rust job invokes the
  appropriate `make` target and trusts the dependency graph.
- `docs/operating/local-dev.md` exists and documents the build
  contract: `make` is the entry point; Node is a dev build
  dependency; release tarballs ship prebuilt assets and skip the
  npm step.
- The /goal control plane has a single shell exit-code condition
  that verifies all of the above.

## Scope

In scope:

- **Makefile dependency graph.** Add file-target variables for
  `$(UI_CODEGEN)` and `$(UI_DIST)`, give them proper recipes with
  source-file prerequisites, and add `$(UI_DIST)` as a prereq on
  every cargo-invoking target (`check`, `test`, `clippy`,
  `ci-required`, `test-rust-runtime`, `test-rust-workspace`,
  `test-rust-docs`, `verify-desktop-ui`, plus existing `build` /
  `release`). Remove the recipe-step `npm run codegen` line in
  `verify-desktop-ui` — it becomes a Make-prerequisite instead.
- **`build.rs` honesty.** Delete the stub-emitting branch in
  `crates/nimbus-server/build.rs:30-53`. Replace with a single
  assertion that errors in **all profiles** (not just release) when
  `dist/index.html` is missing, with an error message that names
  `make` as the fix. Keep the existing `cargo:rerun-if-changed` on
  the dist directory.
- **CI cleanup.** Remove the five `Generate nimbus-ui convex codegen
  artifacts` steps and the one `Build nimbus-ui SPA` step from
  `.github/workflows/ci.yml`. Each Rust-running job that previously
  needed these inlined steps invokes a `make` target whose Make-level
  prerequisites cover them. The `desktop-ui.yml` workflow is already
  using `make verify-desktop-ui`; no change needed once `verify-desktop-ui`
  picks up the new graph.
- **Docs.** Add `docs/operating/local-dev.md` documenting the build
  contract; update `CLAUDE.md`'s "Verification Commands" section to
  cross-reference it; add a routing entry under "Routing By Work
  Type" for local-dev / build-contract work.
- **Verification script.** Add `scripts/verify-local-dev-canonicalization.sh`
  that exits 0 iff all completion-gate conditions hold. Used as the
  /goal stop-hook.

Out of scope:

- Replacing `npm run codegen` with `nimbus run packages/codegen/...`
  (deferred — see "Successor work" below).
- Replacing Vite/Rolldown with a Rust-native bundler (deferred — see
  "Successor work").
- Moving the system bundle into a checked-in pre-built JS Rust crate
  (`crates/nimbus-system-bundle/`) à la Deno's `ext/` pattern. Real
  refactor; only worth doing if 1-3 prove insufficient (they will
  not).
- Any change to the `nimbus dev --ui` orchestration story (live HMR
  loop, Vite-proxies-to-daemon). Separate concern, separate plan if
  pursued.
- Any change to the embedded UI assets, routing, auth surface, or
  `rust-embed` configuration.
- Any change to `tonic-build` / `protoc` orchestration in
  `build.rs:6-23`. Stays as-is.

## Target build contract

After this plan, the canonical contract is:

```text
Fresh clone → ready to build:
    make ci                         # builds UI artifacts on demand, runs full CI

Run a single Rust lane on a clean tree:
    make check                      # cargo check, UI prereqs auto-built
    make clippy                     # cargo clippy, UI prereqs auto-built
    make test                       # cargo test, UI prereqs auto-built
    make verify-desktop-ui          # E2E smoke, UI prereqs auto-built

Run cargo directly (advanced):
    cargo build -p nimbus-server    # succeeds iff dist/ already exists
                                    # else fails with: "run `make build-ui`"

Bypass the UI build (for non-server cargo work):
    cargo check -p nimbus-runtime   # nimbus-runtime has no UI dependency

Release:
    make release                    # already correct, UI prereqs auto-built
```

The contract is: **`make` is the supported entry point. `cargo` works
once UI artifacts exist.**

## Ledger

| ID  | Phase                                                                                              | Status      |
|-----|----------------------------------------------------------------------------------------------------|-------------|
| LD0 | This plan written and committed under `docs/plans/local-dev-canonicalization-plan.md`; indexed in `docs/plans/README.md`. | done        |
| LD1 | Makefile dependency graph. Define `UI_CODEGEN_OUTPUTS`, `UI_DIST_INDEX`, `UI_CODEGEN_SOURCES`, `UI_SPA_SOURCES`. Add file-target recipes for `$(UI_CODEGEN_OUTPUTS)` (depends on `UI_CODEGEN_SOURCES`, runs `npm run codegen -w packages/nimbus-ui`) and `$(UI_DIST_INDEX)` (depends on `$(UI_CODEGEN_OUTPUTS)` and `UI_SPA_SOURCES`, runs `npm run build -w packages/nimbus-ui`). Add `$(UI_DIST_INDEX)` as a prereq on `check`, `clippy`, `test`, `test-rust-runtime`, `test-rust-workspace`, `test-rust-docs`, `verify-desktop-ui`, `ci-required`. Remove the recipe-step `npm run codegen -w packages/nimbus-ui` from `verify-desktop-ui:` (line 128) — Make handles it via the prereq. Keep `build-ui:`, `build:`, `release:` as-is (already correct, now redundant prereqs are harmless). | not started |
| LD2 | `build.rs` honesty. In `crates/nimbus-server/build.rs:30-53`, delete the stub-emitting branch (lines 47-49). Replace with a single error path that fires when `dist/index.html` is missing in **any** profile, with the message: `"packages/nimbus-ui/dist/index.html is missing — run `make build-ui` (or any `make` target; Make builds it on demand). Cargo-direct builds of nimbus-server require dist to exist."`. Keep the existing `println!("cargo:rerun-if-changed={}", dist_dir.display());` (line 38). Add a second `cargo:rerun-if-changed` on the `.nimbus/convex/` directory so codegen output changes also re-trigger the build. | not started |
| LD3 | CI cleanup. In `.github/workflows/ci.yml`, delete the six inlined npm steps (line numbers approximate, will drift during edit): the `Generate nimbus-ui convex codegen artifacts` step at ~91, ~223, ~427, ~485, ~645, and the `Build nimbus-ui SPA` step at ~230 and ~651. Replace any direct `cargo …` invocation in those jobs with the appropriate `make` target (`make check`, `make clippy`, `make test-rust-workspace`, `make test-rust-docs`, `make ci-required`, etc.) so Make's dependency graph handles the prerequisites. `.github/workflows/desktop-ui.yml` already invokes `make verify-desktop-ui`; verify it still works under the new graph. The npm-workspaces step at `ci.yml:511` (`npm run build --workspaces --if-present`) is part of the JS-tests job, not Rust — leave it alone. | not started |
| LD4 | Documentation. Add `docs/operating/local-dev.md` with sections: "Build contract" (the `make`-is-the-entry-point promise), "Prerequisites" (Node 22+, cargo, make), "Common commands" (`make ci`, `make check`, `make test`, `make verify-desktop-ui`, `make release`), "Why Make orchestrates" (one-paragraph reference to this plan), "Cargo-direct builds" (works iff `dist/` exists, errors actionably otherwise). Update `CLAUDE.md`'s "Verification Commands" section to add: `- See docs/operating/local-dev.md for the build contract; Node is a dev build dependency for any Rust target that touches nimbus-server.`. Add a routing entry under "Routing By Work Type" for local-dev / build-contract work pointing at this plan and `docs/operating/local-dev.md`. Update `docs/plans/README.md` to list this plan under "Active execution plans". | not started |
| LD5 | Fresh-clone verification. Run `git clean -fdx && make ci-required` on the working tree, capture stdout/stderr to `docs/plans/proof/local-dev-canonicalization/clean-tree-make-ci-required.log`, and confirm the run is green. Run `git clean -fdx && cargo check -p nimbus-server` separately and confirm it produces the new actionable error (no stub, no panic). Capture that stderr to `docs/plans/proof/local-dev-canonicalization/cargo-direct-fresh-clone.log`. **Important:** these clean runs must be done in a worktree or under a careful state-restore pattern; do not lose uncommitted plan-related work. | not started |
| LD6 | /goal control plane. Add `scripts/verify-local-dev-canonicalization.sh` that exits 0 iff all of: (a) `docs/plans/local-dev-canonicalization-plan.md` exists; (b) `grep -nE 'npm run (codegen\|build)' .github/workflows/ci.yml` returns nothing; (c) `grep -n 'stub\|"<!doctype html>"' crates/nimbus-server/build.rs` returns nothing; (d) the Makefile defines `UI_DIST_INDEX` and lists it as a prereq on `check`, `clippy`, `test`, `ci-required`; (e) `docs/operating/local-dev.md` exists; (f) every ledger row in this plan is marked `done`. Document the `/goal` invocation in the plan (see "Control plane" section below). | not started |
| LD7 | Plan closeout. Mark every ledger row `done`. Append an Execution Log section with the actual commit SHAs that landed each LD. Move this file to `docs/plans/archive/local-dev-canonicalization-plan.md`. Update `docs/plans/README.md`: remove from "Active execution plans", add a one-paragraph entry under "Current Reference Baselines" with closeout date. Update `CLAUDE.md`'s routing entry to point at the archived path. Final commit pushed to main; `gh run list --branch main --limit 1` reports `success`. | not started |

## Completion Gate

All ledger rows must be `done`. The aggregate stop condition for the
/goal control plane is **`bash scripts/verify-local-dev-canonicalization.sh`
exits 0**. That script checks every condition below; the conditions
exist independently for human verification:

### Conditions

1. **Plan checked in.** `test -f docs/plans/local-dev-canonicalization-plan.md`
   *or* `test -f docs/plans/archive/local-dev-canonicalization-plan.md`
   (LD7 archives it on closeout — the script accepts either).
2. **CI workflows have no inlined npm orchestration.** `grep -nE 'npm run (codegen|build)' .github/workflows/ci.yml` returns nothing; the `npm run build --workspaces` step at `ci.yml:511` is allowed (JS-tests job) and the script's grep is scoped to exclude it.
3. **No stub in `build.rs`.** `grep -n -E 'stub|<!doctype html>' crates/nimbus-server/build.rs` returns nothing.
4. **Makefile encodes the dependency graph.** `grep -nE '^UI_DIST_INDEX' Makefile` returns at least one line; `grep -E '^(check|test|clippy|ci-required|verify-desktop-ui|test-rust-runtime|test-rust-workspace|test-rust-docs):' Makefile` shows each target lists `$(UI_DIST_INDEX)` in its prereqs.
5. **Build contract documented.** `test -f docs/operating/local-dev.md`.
6. **Routing entry exists.** `grep -n 'local-dev-canonicalization-plan' CLAUDE.md` returns at least one line.
7. **Fresh-clone proof captured.** `test -f docs/plans/proof/local-dev-canonicalization/clean-tree-make-ci-required.log`.
8. **Ledger rows all done.** Every row in the ledger table of this plan ends with `| done |` (or, if archived under LD7, the archived copy has every row `done`). The verify script grep-counts `| not started |` and `| in_progress |` in the plan file and fails if either is non-zero.
9. **Branch state.** `git log --oneline origin/main..HEAD` is empty (all work pushed to main).
10. **CI green on main.** `gh run list --branch main --limit 1 --json conclusion -q '.[0].conclusion'` returns `success`.

The /goal stop hook is one shell exit code over those ten conditions —
satisfiable, machine-checkable, no prose interpretation needed.

## Risks

- **R1 — Make's recipe order on parallel `-j` runs.** If multiple
  Make jobs depend on `$(UI_DIST_INDEX)` and Make schedules them
  with `-j`, Make's own dependency model ensures the recipe runs
  once and dependents wait. Tested in practice by existing Make
  targets (`build:` already depends on `build-ui:`). Not a real
  risk for the file-target shape, but worth verifying by running
  `make -j4 ci-required` on a clean tree as part of LD5.
- **R2 — `cargo:rerun-if-changed` on `dist/` doesn't list every
  asset.** `cargo:rerun-if-changed=<dir>` only watches the directory
  inode, not contents. Currently this is fine because the assertion
  in `build.rs` only checks `index.html` existence; if the asset set
  changes, the rust-embed macro re-walks at compile time. No
  behavior change from current.
- **R3 — Distro packagers running `cargo install nimbus-bin`.** Not
  a current shipping path (we ship prebuilt binaries via the
  install script, Homebrew, apt/rpm — all already documented in
  `docs/plans/distribution-plan.md`). Documented in `docs/operating/local-dev.md`
  as a caveat: building from source via `cargo install` requires
  the UI artifacts to exist beforehand or `make` to be the entry
  point.
- **R4 — Make's missing-file semantics differ from cargo's.** If a
  developer manually deletes `packages/nimbus-ui/dist/index.html`
  between a Make build and a subsequent `cargo build` invocation,
  Make's view (file is missing → rebuild) is right; cargo's view
  (build.rs ran successfully last time, no rerun-if-changed
  trigger, file is still missing → confusing error) could be
  surprising. Mitigation: `build.rs` runs the existence check on
  every invocation regardless of cache, so the error is
  deterministic.
- **R5 — Submodule / worktree gotchas with `git clean -fdx`.** LD5
  runs `git clean -fdx` to verify the fresh-clone contract. This
  is destructive of any untracked work in the worktree. Mitigation:
  do LD5 inside a fresh `git worktree add` rather than the user's
  primary working tree.
- **R6 — The `npm run codegen --` invocation has its own
  cross-toolchain quirk** (it spawns the convex CLI which spawns
  the bundler). If npm caches are clean, first-time codegen can
  take 30-60 seconds. Acceptable cost for fresh-clone; warm
  iteration is sub-second. Documented in `docs/operating/local-dev.md`.

## Control plane (/goal invocation)

Once `scripts/verify-local-dev-canonicalization.sh` is checked in
(LD6), the canonical /goal invocation that drives this plan to
completion autonomously is:

```
/goal bash scripts/verify-local-dev-canonicalization.sh exits 0
```

The stop hook re-evaluates that command on every prompt; once the
script exits 0, the goal is satisfied and the loop terminates. No
prose interpretation, no rule parsing — one exit code.

During execution the autonomous loop should:

1. Read this plan and pick the next `not started` ledger row.
2. Make the smallest change that moves that row to `done`.
3. Commit with a focused message that names the LD id.
4. Push to main (no PR — per the standing autonomous-mode
   authorization for desktop / local-dev tooling work).
5. Update this plan: flip the row from `not started` → `done`, then
   commit the plan update.
6. Repeat until LD7 closes the plan.

The order LD0 → LD1 → LD2 → LD3 → LD4 → LD5 → LD6 → LD7 is the
recommended sequence; LD2 and LD3 are independent and can swap. LD5
cannot run before LD1+LD2+LD3 because the fresh-clone proof depends
on the new contract being in place. LD6 can run any time after LD1
defines `UI_DIST_INDEX`, but is cleanest immediately before LD7.

## Successor work (deferred, separate plans)

- **Self-hosted codegen.** Replace `npm run codegen -w packages/nimbus-ui`
  with `nimbus run packages/codegen/dist/cli.js …` once `@nimbus/codegen`'s
  API surface is verified under `nimbus-runtime`'s Node-compat. One
  ledger row, scoped to the codegen step only; Vite stays under
  Node. Activation gate: someone has run the verification, and at
  least one of the existing Node-compat plans has closed out the
  required builtin coverage.
- **Rust-native bundler.** Evaluate dropping Vite for direct Rolldown
  invocation from `build.rs` or `xtask`. Multi-quarter scope.
  Activation gate: Vite-the-orchestrator becomes a real
  pain-point (it is not today).
- **System bundle as a Rust crate.** Move the
  `packages/nimbus-ui/.nimbus/convex/*` outputs into
  `crates/nimbus-system-bundle/src/*` as checked-in pre-built JS
  literals (Deno's `ext/` pattern). Makes the Rust workspace
  self-contained at the source level. Real refactor; only justified
  if cross-workspace include_str! produces ongoing pain after this
  plan lands.

## Execution Log

(populated as LD rows close)
