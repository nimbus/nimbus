# Plan: Docker/Podman-Compatible Compose Discovery

Canonical execution plan for shared Compose file discovery across `neovex dev`,
`neovex start`, and `neovex compose ...`.

The goal is a zero-flag happy path that feels native to developers coming from
Docker Compose, Podman Compose, and Convex: when no explicit compose path is
provided, Neovex should discover the Compose project from the current working
directory and its parents using Compose-native conventions; when an explicit
compose path is provided, it should win everywhere.

---

## Status

- **Status:** `done`
- **Primary owner:** this plan
- **Related plans:** none active — `docs/plans/archive/cli-command-surface-plan.md`
  records the original `neovex dev` and `neovex compose` rollout
- **Related reference:** `docs/reference/cli.md` (canonical CLI contract),
  `docs/reference/microvm-service-baseline.md` (compose/service baseline)
- **External compatibility inputs:** official Docker Compose docs and Podman
  `podman compose` docs

## Local Reference Audit

Post-implementation audit against local source mirrors under
`~/src/github.com/*` confirmed the following:

- no local `docker/compose` source tree was present to diff against directly;
  local `moby/moby` only vendors the Compose binary, not the provider-side
  discovery implementation
- local `containers/podman/cmd/podman/compose.go` confirms `podman compose` is
  a thin wrapper that forwards to an external provider, so provider semantics
  remain the compatibility target rather than Podman's wrapper itself
- local `containers/podman-compose/podman_compose.py` uses a broader default
  filename list than this slice (`podman-compose.*`, `container-compose.*`, and
  multiple override aliases) and discovers files from the working directory
  without parent traversal
- Neovex intentionally does **not** mirror those broader provider-specific
  defaults; it follows the narrower approved Docker/Podman-compatible contract
  for this slice
- local `podman-compose` does preserve ordered file lists and treats the first
  file as the project directory anchor (`COMPOSE_PROJECT_DIR` / `COMPOSE_FILE`),
  which matches the Neovex `files[0]` identity and relative-path rule

Result: the landed implementation remains correct for the approved contract,
and the local source audit did not require code changes.

## Motivation

Today Compose discovery is split:

- `neovex dev` and `neovex start` require an explicit `--compose-file`
- `neovex compose ...` defaults to `./compose.yaml` relative to the cwd

That split creates avoidable friction:

1. **It does not feel Docker-native.** Docker Compose discovers Compose files
   from the working directory and parent directories, supports the standard
   modern and legacy base filenames, and documents a default override pair for
   the canonical modern file.
2. **It does not feel Podman-native.** `podman compose` is a thin wrapper around
   an external Compose provider, so Podman users still expect Compose-provider
   discovery semantics instead of a Neovex-specific root rule.
3. **It couples the wrong concepts.** `app_dir` is a codegen/runtime concept.
   Compose discovery is an infrastructure/project-layout concept. These often
   align, but they are not the same invariant.
4. **It is brittle for Convex migrations.** Convex projects can customize the
   functions directory via `convex.json`, so treating the resolved app/code root
   as the only valid Compose anchor creates the wrong mental model for teams
   migrating non-trivial repos.

The DX target is simple:

- run `neovex dev` from a project root with Compose files and it just works
- run `neovex compose ps` from a nested directory inside that project and it
  talks to the same Compose project
- pass an explicit compose path and every command uses exactly that path

## Design Decision: Shared Compose Discovery Contract

All compose-aware commands must use one late-bound discovery helper:

- `neovex dev --compose-file ...`
- `neovex start --compose-file ...`
- `neovex compose ... --file ...`

When no explicit compose path is provided, all of them must discover the same
Compose project the same way.

### Anchor discovery to the cwd, not `app_dir`

Compose discovery should be rooted in the current working directory and walk
ancestors upward, matching Docker/Podman operator expectations.

`app_dir` remains important, but for different reasons:

- codegen input selection
- runtime manifest loading
- local dev persistence defaults

It should not silently redefine how Compose project discovery works.

### Support the Docker-compatible base filename family

The shared discovery helper must support the standard base filename family that
Docker documents:

- `compose.yaml`
- `compose.yml`
- `docker-compose.yaml`
- `docker-compose.yml`

Preference rules:

1. Prefer the canonical modern name `compose.yaml` when present.
2. Prefer modern `compose.*` names over legacy `docker-compose.*` names.
3. If multiple remaining candidates in one directory are still ambiguous after
   applying the preference rules, fail with an actionable error instead of
   silently picking one.

This keeps the modern happy path simple while still supporting existing
Docker/Podman projects.

### Support a selection model, not a single optional path

The current plan shape is too narrow. The runtime needs to know:

- whether the compose selection was explicit or auto-discovered
- which directory owns discovery/project identity
- which compose files were loaded, in order

Use a shared resolved type such as:

```rust
enum ComposeSelectionOrigin {
    ExplicitFlag,
    AutoDiscovered,
}

struct ResolvedComposeSelection {
    origin: ComposeSelectionOrigin,
    project_root: PathBuf,
    files: Vec<PathBuf>,
}
```

Rules:

- `files[0]` is the primary Compose file
- `files[0]` owns relative-path resolution and project identity
- explicit flags produce `files.len() == 1` in this slice
- auto-discovery may produce more than one file

This model keeps the UX honest and avoids stuffing resolved state back into raw
CLI parse structs.

### Support the documented default override pairing

When auto-discovery selects the canonical modern base file `compose.yaml`,
Neovex should also look for the documented default override companion
`compose.override.yaml` in the same directory and include it after the base
file when present.

For this slice:

- explicit `--compose-file` / `--file` means “use exactly this file”
- auto-discovery may resolve to `[compose.yaml, compose.override.yaml]`

The list-based selection model is required so this works without another CLI
shape change.

If later compatibility work adds more default override aliases, it must reuse
the same ordered selection model and preserve `files[0]` as the identity and
relative-path anchor.

## Discovery Algorithm

```
if an explicit compose path flag is present:
    path = resolve relative to cwd
    selection = ExplicitFlag(project_root = parent(path), files = [path])
else:
    for directory in cwd and each parent directory:
        base = discover_supported_base_file(directory)
        if base exists:
            files = [base]
            if base.file_name() == "compose.yaml":
                override = directory / "compose.override.yaml"
                if override exists as a file:
                    files.push(override)
            selection = AutoDiscovered(project_root = directory, files = files)
            break
    if no directory matches:
        selection = None
```

Notes:

- discovery is **cwd-first**, not `app_dir`-first
- explicit compose flags always win
- the same helper must be used by `dev`, `start`, and `compose`
- auto-discovery must happen **after** CLI parsing, not through clap
  `default_value`, so the code can still distinguish explicit vs inferred
  selection

## Expected Layouts

Canonical project:

```text
my-app/
├── neovex/
├── compose.yaml
├── compose.override.yaml
├── package.json
└── .neovex/
```

Nested invocation inside the same project:

```text
my-app/
├── compose.yaml
└── packages/
    └── web/
        └── src/
```

Running `neovex compose ps` from `packages/web/src/` should still discover
`my-app/compose.yaml`.

Explicit override of discovery:

```bash
neovex start --app-dir ./apps/chat --compose-file ./infra/dev-compose.yaml
```

This must use `./infra/dev-compose.yaml` exactly, regardless of cwd discovery.

---

## Phase Ledger

### CD1 — Add a shared compose discovery module and selection model

**Scope:** Introduce one late-bound resolver for compose-aware commands plus a
resolved selection type that preserves origin and ordered file list.

**Files:**

- `crates/neovex-bin/src/compose/` — add a concept-owned discovery module such
  as `discovery.rs`
- shared callers in `crates/neovex-bin/src/dev.rs`,
  `crates/neovex-bin/src/start/`, and `crates/neovex-bin/src/compose/`

**Behavior contract:**

- supports the Docker-compatible base filename family
- searches cwd first, then parent directories
- preserves `ExplicitFlag` vs `AutoDiscovered`
- preserves ordered file list for later loading and UX output

**Status:** `done`

### CD2 — Make compose loading selection-aware

**Scope:** The compose loader and control-plane wiring must accept a resolved
selection instead of a single file path.

**Files:**

- `crates/neovex-bin/src/compose/file/` — add selection-aware loading and
  ordered merge support
- `crates/neovex-bin/src/compose/project.rs` — derive project identity from
  `files[0]`
- `crates/neovex-bin/src/compose/mod.rs` and execution helpers — thread the
  resolved selection through lifecycle/config/log/inspect/top flows

**Behavior contract:**

- `files[0]` remains the path used for project identity and relative-path base
- auto-discovered `[compose.yaml, compose.override.yaml]` loads as one logical
  Compose project
- explicit single-file selection remains supported unchanged

**Status:** `done`

### CD3 — Unify `neovex compose ...` on the shared resolver

**Scope:** Remove clap-time `./compose.yaml` defaults from compose subcommands
and resolve the Compose selection after parsing using the shared discovery
helper.

**Files:**

- `crates/neovex-bin/src/compose/commands.rs`
- `crates/neovex-bin/src/compose/mod.rs`

**Behavior contract:**

- `neovex compose up` with no `--file` discovers from cwd/parents
- `neovex compose ps` from a nested directory resolves to the same project as
  `neovex dev` in that project
- `--file` stays the explicit compose-path flag for the `compose` namespace

**Status:** `done`

### CD4 — Unify `neovex dev` and `neovex start` on the shared resolver

**Scope:** `dev` and `start` must use the same late-bound compose discovery
contract as `compose`.

**Files:**

- `crates/neovex-bin/src/dev.rs`
- `crates/neovex-bin/src/start/`

**Behavior contract:**

- no `--compose-file` means cwd/parent discovery
- `--app-dir` does not silently redefine compose discovery
- explicit `--compose-file` still wins

**Status:** `done`

### CD5 — Make startup and dev UX provenance-aware

**Scope:** Dev and start output should accurately explain what happened without
guesswork or ad hoc string logic.

**Files:**

- `crates/neovex-bin/src/dev.rs` — banner output
- `crates/neovex-bin/src/start/boot.rs` — startup summary output

**Behavior contract:**

- explicit path: show the explicit compose file
- auto-discovered single file: show that the file was auto-discovered
- auto-discovered multi-file selection: show the primary file and note that the
  default override companion was also loaded
- no compose selection: show nothing about compose (unchanged)

**Status:** `done`

### CD6 — Update CLI reference docs

**Scope:** Document the shared compose discovery contract and the fact that
compose-aware commands now align.

**Files:**

- `docs/reference/cli.md`

**Behavior contract:**

- `dev`, `start`, and `compose` describe one discovery rule
- docs clearly separate `app_dir` semantics from compose discovery semantics
- docs explain explicit compose-path flags for each command family

**Verification note:** repo root does not currently define
`npm run docs:validate-refs:strict`, so this phase used targeted doc review
instead of that missing script.

**Status:** `done`

### CD7 — Tests

**Scope:** Add unit and integration coverage for shared discovery, ordered file
selection, command-family consistency, and provenance-aware output.

**Files:**

- `crates/neovex-bin/src/compose/` tests
- `crates/neovex-bin/src/dev.rs` tests
- `crates/neovex-bin/src/start/tests.rs`

**Coverage contract:**

- explicit path wins for all three command families
- cwd discovery finds supported base filenames
- parent traversal works
- canonical `compose.yaml` plus `compose.override.yaml` yields an ordered
  two-file selection
- `dev`, `start`, and `compose` resolve the same project from the same cwd
- ambiguous same-directory candidates fail with an actionable error
- startup/banner output reflects explicit vs auto-discovered selection

**Status:** `done`

---

## Verification Contract

All phases verified by:

```bash
cargo fmt --all --check
make clippy
make check
make test
```

CD7 specifically adds regression coverage for:

- explicit compose path selection
- cwd and parent-directory discovery
- base filename compatibility
- canonical override pairing
- cross-command-family consistency

---

## Control Plan Rules

1. This plan owns compose discovery behavior for all compose-aware commands:
   `dev`, `start`, and `compose`.
2. Compose discovery is cwd-anchored and shared across command families unless
   an explicit compose path flag is provided.
3. `app_dir` remains a codegen/runtime concern and must not silently redefine
   compose discovery semantics.
4. Do not reintroduce clap-time default compose paths for `compose` subcommands;
   discovery must remain late-bound so provenance and ordered file selection are
   preserved.
5. Keep project identity anchored on `files[0]` so control-plane state remains
   stable and relative-path behavior stays predictable.
6. If future compatibility work broadens the default override filename set, it
   must extend the selection model and tests, not invent a second discovery
   path.
7. Archive this plan after all phases reach `done` and verification passes.
