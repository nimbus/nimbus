# Plan: Explicit Compose File Lists And `COMPOSE_FILE`

Canonical execution plan for adding ordered explicit Compose file lists and
`COMPOSE_FILE` support across `nimbus compose ...`, `nimbus dev`, and
`nimbus start`.

This plan extends the landed shared-discovery contract in
`docs/plans/archive/compose-discovery-plan.md`. Auto-discovery stays unchanged; this
wave adds the missing explicit multi-file workflow used by modern Docker
Compose users for environment overlays, CI stacks, and alternate local
profiles.

---

## Status

- **Status:** `done`
- **Primary owner:** this plan
- **Related plans:** `docs/plans/archive/compose-discovery-plan.md` (landed shared
  discovery baseline), `docs/plans/archive/cli-command-surface-plan.md`
  (historical CLI command-surface rollout)
- **Related reference:** `docs/reference/cli.md`,
  `docs/reference/microvm-service-baseline.md`
- **External compatibility inputs:** Docker Compose CLI and environment-variable
  docs; Podman `podman compose` wrapper docs

## Goal

Keep the landed zero-flag discovery contract, and add the modern explicit
workflow that Docker users expect:

- repeated `--file` / `--compose-file` flags preserve order
- `COMPOSE_FILE` works when explicit flags are absent
- `files[0]` remains the project-identity and relative-path anchor
- `dev`, `start`, and `compose` still share one resolver

This wave does **not** broaden Nimbus into provider-specific default filenames
such as `podman-compose.yaml` or `container-compose.yaml`.

## Behavior Contract

1. Explicit CLI compose file lists win over everything else.
2. `COMPOSE_FILE` is used only when no explicit compose-path flags are present.
3. Auto-discovery remains the final fallback.
4. Ordered explicit file lists are loaded exactly as provided; do not
   auto-append `compose.override.yaml` to explicit selections.
5. Relative paths for explicit CLI paths and `COMPOSE_FILE` entries resolve
   from the current working directory.
6. `files[0]` remains the project identity, project root, and relative-path
   anchor for merged Compose semantics.
7. Provenance must remain visible in UX output:
   `explicit flag`, `COMPOSE_FILE`, or `auto-discovered`.

## Phase Ledger

### CEM1 — Extend the resolver for ordered explicit selections

**Scope:** Teach the shared resolver about explicit ordered file lists and
environment-driven selections without breaking the landed auto-discovery path.

**Files:**

- `crates/nimbus-bin/src/compose/discovery.rs`

**Behavior contract:**

- selection origin distinguishes CLI flags, `COMPOSE_FILE`, and auto-discovery
- explicit ordered file lists preserve input order
- `COMPOSE_FILE` supports platform-aware separators and
  `COMPOSE_PATH_SEPARATOR`
- `files[0]` remains the project root anchor

**Implementation note:** the shared resolver now accepts ordered explicit file
lists, supports `COMPOSE_FILE` plus `COMPOSE_PATH_SEPARATOR`, and keeps display
paths alongside resolved file paths so startup/dev output can distinguish CLI,
environment, and auto-discovered selections cleanly.

**Status:** `done`

### CEM2 — Thread ordered explicit files through `compose`, `dev`, and `start`

**Scope:** Replace singular compose-path parsing with ordered lists and keep the
three command families aligned on the shared resolver.

**Files:**

- `crates/nimbus-bin/src/compose/commands.rs`
- `crates/nimbus-bin/src/compose/mod.rs`
- `crates/nimbus-bin/src/dev.rs`
- `crates/nimbus-bin/src/start/`

**Behavior contract:**

- `nimbus compose ... --file a --file b` loads `[a, b]` in order
- `nimbus dev --compose-file a --compose-file b` loads `[a, b]` in order
- `nimbus start --compose-file a --compose-file b` loads `[a, b]` in order
- all three families honor `COMPOSE_FILE` when explicit flags are absent

**Implementation note:** compose command structs plus `dev` / `start` now store
ordered `Vec<PathBuf>` compose selections, and all command-family call sites
thread those ordered lists into the shared resolver instead of collapsing back
to a singular path.

**Status:** `done`

### CEM3 — Update provenance-aware UX and docs

**Scope:** Keep startup/dev/help/reference output honest about where the
selection came from and how ordered explicit files behave.

**Files:**

- `crates/nimbus-bin/src/dev.rs`
- `crates/nimbus-bin/src/start/boot.rs`
- `docs/reference/cli.md`

**Behavior contract:**

- explicit CLI lists render as explicit selections
- `COMPOSE_FILE` renders as environment-driven selection
- docs explain precedence: flags, then `COMPOSE_FILE`, then auto-discovery

**Implementation note:** startup summaries, dev banner output, clap parsing, and
the CLI reference now describe ordered repeated compose flags and
`COMPOSE_FILE` provenance explicitly instead of implying a singular compose
path. Follow-up help-surface polish now keeps `COMPOSE_FILE` visible directly in
`dev`, `start`, and `compose` `--help` output instead of requiring the reference
docs for discovery.

**Status:** `done`

### CEM4 — Tests and verification

**Scope:** Add focused regression coverage for explicit ordered lists,
environment-driven selections, and cross-command consistency.

**Files:**

- `crates/nimbus-bin/src/compose/` tests
- `crates/nimbus-bin/src/dev.rs` tests
- `crates/nimbus-bin/src/start/tests.rs`

**Coverage contract:**

- repeated explicit flags preserve order
- `COMPOSE_FILE` resolves ordered file lists
- CLI flags override `COMPOSE_FILE`
- auto-discovery still works when neither explicit source is provided
- project identity stays anchored on `files[0]`
- startup/dev UX reflects CLI vs env vs auto provenance

**Implementation note:** focused regression coverage now exercises repeated
explicit flags, `COMPOSE_FILE`, precedence over environment defaults,
cross-command parser behavior, `files[0]` project anchoring, and provenance-aware
dev/start output. Closeout verification passed with `cargo fmt --all --check`,
`cargo test -p nimbus-bin`, `cargo clippy -p nimbus-bin --all-targets -- -D warnings`,
`cargo check --workspace`, `make clippy`, `make check`, and `make test`.
Follow-up test hardening now routes all cwd-mutating tests through the shared
crate-level helper so `dev`, `start`, and `compose` cwd scenarios serialize on
one process-global lock instead of three independent mutexes.

**Status:** `done`

## Verification Contract

Focused verification during implementation:

```bash
cargo fmt --all --check
cargo test -p nimbus-bin
cargo clippy -p nimbus-bin --all-targets -- -D warnings
cargo check --workspace
```

Closeout verification:

```bash
cargo fmt --all --check
make clippy
make check
make test
```

## Control Plan Rules

1. This plan extends the landed compose-discovery baseline; it does not replace
   the shared auto-discovery contract.
2. Do not add provider-specific default filename families in this wave.
3. Keep `files[0]` as the control-plane identity anchor.
4. Do not special-case one command family; `compose`, `dev`, and `start` must
   keep sharing one resolver.
5. If `COMPOSE_FILE` support requires provenance changes, extend the selection
   model instead of reintroducing ad hoc booleans.
