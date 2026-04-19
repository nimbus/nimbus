# Codebase Modularity And Maintainability Control Plan

This is the canonical execution control plane for the next repo-wide
maintainability, modularity, canonical naming, and idiomatic-Rust cleanup
workstream after the archived targeted-domain cleanup pass.

Reviewed against:

- `README.md`
- `ARCHITECTURE.md`
- `docs/README.md`
- `docs/plans/README.md`
- `AGENTS.md`
- `crates/neovex-bin/src/main.rs`
- `crates/neovex-bin/src/machine/mod.rs`
- `crates/neovex-bin/src/machine/manager.rs`
- `crates/neovex-bin/src/service/mod.rs`
- `crates/neovex-engine/src/persistence.rs`
- `crates/neovex-storage/src/sqlite.rs`
- `crates/neovex-storage/src/postgres.rs`
- `crates/neovex-storage/src/libsql.rs`
- `crates/neovex-storage/src/mysql.rs`
- `crates/neovex-sandbox/src/backends/krun/vm.rs`

Baseline verification status for this plan:

- the immediately preceding generic cleanup workstream was completed and
  archived as
  `docs/plans/archive/targeted-domain-modularity-cleanup-plan.md`
- this control plane is being authored as a docs-only review-and-planning pass
  on 2026-04-19 from a clean worktree after reviewing the live CLI, provider,
  and sandbox hotspots
- no new code verification is claimed by this planning pass
- every `CM*` implementation item must record its own focused verification
  before it can be marked `done`

---

## Purpose

The earlier cleanup passes removed the largest runtime, engine, storage, and
test-surface god files. That work paid off: the most urgent maintainability
problems are no longer in the old query, mutation, auth, or runtime roots.

The next cleanup pass should stay disciplined and review-driven. The target is
not lower line counts for their own sake. The target is clearer concept
ownership, thinner composition roots, more canonical file and type naming, and
module boundaries that make future features easier to place, debug, and verify.

Today the remaining pressure is concentrated in three areas:

- CLI ownership in `neovex-bin`
- provider and persistence ownership across `neovex-engine` plus
  `neovex-storage`
- krun backend ownership in `neovex-sandbox`

This is a maintainability and correctness roadmap, not a feature roadmap.

---

## Relationship To Other Plans

- Use `docs/plans/README.md` as the owning plan index.
- This plan is separate from:
  `docs/plans/encryption-at-rest-plan.md`,
  `docs/plans/websocket-protocol-plan.md`,
  `docs/plans/localhost-server-security-plan.md`,
  `docs/plans/system-tenant-api-plan.md`,
  `docs/plans/desktop-ui-plan.md`,
  `docs/plans/install-script-plan.md`,
  `docs/plans/distribution-plan.md`,
  `docs/plans/windows-machine-support-plan.md`,
  `docs/plans/wasmtime-backend-plan.md`,
  `docs/plans/wasi-agent-capabilities-plan.md`,
  `docs/plans/nimbus-rename-plan.md`,
  and `docs/plans/nimbus-rename-satellite-repos-plan.md`.
- Use the archived machine CLI plans only for historical user-surface context
  and proof bundles:
  `docs/plans/archive/machine-cli-follow-on-plan.md`,
  `docs/plans/archive/machine-cli-alignment-plan.md`,
  and `docs/plans/archive/machine-cli-dx-plan.md`.
- Use the archived provider plans only for historical provider semantics and
  proof context:
  `docs/plans/archive/pluggable-storage-backend-plan.md`,
  `docs/plans/archive/postgres-storage-provider-plan.md`,
  `docs/plans/archive/mysql-storage-provider-plan.md`,
  `docs/plans/archive/sqlite-replica-provider-plan.md`,
  and
  `docs/plans/archive/storage-provider-contracts-and-observability-plan.md`.
- If work turns into feature behavior changes, protocol changes, install or
  distribution work, provider-product semantics, or platform-specific machine
  behavior, stop and move to the owning plan instead of stretching this cleanup
  plan across multiple streams.

---

## Scope

This plan covers:

- top-level CLI entrypoint and serve-config ownership inside
  `crates/neovex-bin/src/main.rs`
- machine command, record, rendering, and root-layout ownership inside
  `crates/neovex-bin/src/machine/mod.rs`
- machine lifecycle orchestration, image materialization, helper resolution,
  networking, and SSH ownership inside
  `crates/neovex-bin/src/machine/manager.rs`
- service command execution-surface, backend-resolution, rendering, log, and
  process inspection ownership inside
  `crates/neovex-bin/src/service/mod.rs`
- provider registry, tenant persistence facade, snapshot facade, and
  cross-provider delegation ownership inside
  `crates/neovex-engine/src/persistence.rs`
- concept-owned decomposition of the provider backends in
  `crates/neovex-storage/src/sqlite.rs`,
  `crates/neovex-storage/src/postgres.rs`,
  `crates/neovex-storage/src/libsql.rs`,
  and `crates/neovex-storage/src/mysql.rs`
- krun backend launch, manifest, readiness, restart, and stop ownership inside
  `crates/neovex-sandbox/src/backends/krun/vm.rs`
- follow-on doc, verification, and archive cleanup needed to keep this work
  resumable through handoff and compaction

This plan does not cover:

- new product features
- intentional CLI, route, wire, or persistence behavior changes unless an item
  explicitly records them
- storage-format changes
- install/distribution channel work
- rename work
- compatibility code for pre-launch behavior
- speculative performance rewrites that are not justified by ownership,
  readability, or maintainability

---

## Cleanup Invariants

These rules are mandatory for every item in this plan.

1. Preserve externally observable behavior by default.
   Machine CLI behavior, service CLI behavior, provider semantics, scheduler
   semantics, journal semantics, and sandbox lifecycle behavior stay unchanged
   unless a specific item explicitly records otherwise.

2. Keep the core architecture invariants intact.
   `neovex-core` stays zero I/O.
   `neovex-runtime` stays zero workspace dependencies.
   All mutations still flow through `Service::apply_mutation` or its queued
   async journal path.
   Storage atomicity stays unchanged.

3. Prefer concept-owned modules over helper piles.
   A successful split makes ownership easier to name, test, and debug locally.

4. Keep composition roots thin once ownership moves out.
   Do not rename a god file into a facade without actually moving ownership.

5. Preserve CLI UX contracts.
   Help text, default resolution, output formats, status rendering, and remote
   versus local command semantics must remain stable unless a work item records
   a deliberate contract change.

6. Keep provider capability seams explicit.
   Shared provider abstractions should reflect durable capability boundaries,
   not erase backend-specific semantics behind generic indirection.

7. Keep sandbox lifecycle semantics explicit and testable.
   krun launch planning, readiness, restart policy, process cleanup, and
   published-endpoint behavior must remain easy to reason about.

8. Treat the current git worktree as baseline reality.
   Resume from the code and this plan's ledger, not from chat memory.

---

## Current Assessed State

- Before this planning pass, the repo did not have an active generic cleanup or
  refactor control plane, so future broad maintainability work needed a new
  active owner rather than another revival of archived plans.
- The earlier cleanup passes materially improved the runtime, tenant, auth,
  scheduler, subscription, and test-root ownership map. Those are no longer the
  highest-return generic refactor targets.
- The strongest remaining production hotspots are now concentrated in CLI,
  provider, and sandbox surfaces:
  - `crates/neovex-bin/src/machine/mod.rs` is 6517 lines
  - `crates/neovex-bin/src/machine/manager.rs` is 5407 lines
  - `crates/neovex-bin/src/service/mod.rs` is 3959 lines
  - `crates/neovex-bin/src/main.rs` is 1538 lines
  - `crates/neovex-engine/src/persistence.rs` is 1778 lines
  - `crates/neovex-storage/src/sqlite.rs` is 2711 lines
  - `crates/neovex-storage/src/postgres.rs` is 3946 lines
  - `crates/neovex-storage/src/libsql.rs` is 3879 lines
  - `crates/neovex-storage/src/mysql.rs` is 3760 lines
  - `crates/neovex-sandbox/src/backends/krun/vm.rs` is 2654 lines
- Some large files are intentionally not first-wave targets for this pass.
  Several benchmark and regression files are large, but they are lower-value
  cleanup seams until the production ownership boundaries above are clearer.

---

## Current Review Findings

1. `crates/neovex-bin/src/machine/mod.rs` is now the clearest CLI god file.
   It mixes clap command modeling, machine record and root-layout ownership,
   image-source policy, status or list or inspect rendering, SSH/SCP target
   parsing, JSON file helpers, record locking, and subcommand dispatch in one
   root.

2. `crates/neovex-bin/src/machine/manager.rs` is too dense as a lifecycle
   orchestration surface.
   OCI artifact selection, attestation verification, helper-binary discovery,
   bootstrap identity, launch planning, readiness waits, SSH operations,
   signal handling, process cleanup, and port allocation all live together.

3. `crates/neovex-bin/src/service/mod.rs` still combines several distinct CLI
   concepts.
   Command definitions, backend selection, remote-versus-local execution
   policy, machine API forwarding, service lifecycle operations, renderer
   logic, log tailing, and process snapshot parsing all share one module.

4. `crates/neovex-bin/src/main.rs` and
   `crates/neovex-engine/src/persistence.rs` are both overly dense glue layers.
   `main.rs` mixes CLI parsing, file-plus-env-plus-CLI config resolution,
   provider selection, runtime limits, scheduler startup, and server
   orchestration. `persistence.rs` mixes provider enums, write traits, store
   mapping, snapshot mapping, query-read delegation, and cross-provider match
   walls in one file.

5. The provider backend files now carry too many capabilities each.
   `sqlite.rs`, `postgres.rs`, `libsql.rs`, and `mysql.rs` each combine config,
   tenant registration or opening, schema cache ownership, read snapshots,
   write transactions, scheduler or journal behavior, schema or index helpers,
   and backend-local utility functions in single files. Cross-provider parity is
   harder to maintain because capability seams are buried inside file-local
   helpers.

6. `crates/neovex-sandbox/src/backends/krun/vm.rs` is the clearest remaining
   sandbox god file.
   It mixes launch planning, image/build materialization, guest-user helper
   handling, manifest persistence, readiness probing, restart policy, inspect
   semantics, and stop or cleanup behavior in one module.

7. Several benchmark and regression surfaces are still large, but they are not
   the best first-wave targets.
   The provider benchmarks, krun smoke tests, and larger server or engine
   regression files should be revisited only after the production ownership
   seams above settle.

---

## Success Criteria

This plan is successful only when all of the following are true:

- `main.rs`, `machine/mod.rs`, `machine/manager.rs`, and `service/mod.rs` read
  as thin CLI composition roots instead of multi-concept implementation piles
- `persistence.rs` becomes a smaller capability-oriented facade rather than a
  long sequence of cross-provider match walls
- the provider backends use more canonical module layouts that group config,
  tenant open, read, write, scheduler, journal, and backend utility ownership
  by concept
- `krun/vm.rs` becomes a backend composition root with clearer launch,
  readiness, manifest, restart, and stop ownership
- file naming, type naming, helper placement, and visibility are more
  canonical and easier to maintain
- no unintentionally observable behavior changes are introduced
- the plan can be archived cleanly once the workstream completes

---

## Assessed But Not Selected

- `crates/neovex-engine/benches/{embedded-provider-benchmarks.rs,postgres-provider-benchmarks.rs,mysql-provider-benchmarks.rs,libsql-replica-provider-benchmarks.rs}`
  are large, but benchmark cleanup is a lower priority than the production
  provider seams they exercise.
- `crates/neovex-sandbox/tests/krun_linux_smoke.rs` is large, but it is a
  scenario-rich test surface that should follow the production krun ownership
  split rather than lead it.
- `crates/neovex-engine/src/tests/*` and `crates/neovex-server/src/tests/*`
  still contain some large files, but the highest maintainability return is now
  in production ownership rather than another broad test-root cleanup pass.
- `packages/neovex/src/*` was reviewed at a high level and does not currently
  look like the highest-value generic cleanup seam compared with the Rust
  surfaces above.

---

## Feature Preservation Matrix

- Serve-command config precedence must remain unchanged:
  CLI flags over env, env over config file, config file over defaults.
- Machine CLI root, record, lock, SSH/SCP, status, image, and guest-binary
  semantics must remain unchanged.
- Service CLI config, backend selection, forwarded machine API, lifecycle,
  logs, and process inspection semantics must remain unchanged.
- Provider semantics must remain unchanged:
  tenant registration or open behavior, scheduler persistence, durable journal
  behavior, schema persistence, index behavior, and read/write cancellation
  expectations.
- krun sandbox launch, inspect, readiness, restart, published-endpoint, and
  stop semantics must remain unchanged.
- Existing focused Rust verification for the touched crates must remain intact
  even when tests or helpers move into new files.

---

## Control Plan Rules

1. This document is the durable control plane for this cleanup workstream.
2. Update this plan before or during every meaningful implementation burst.
3. Keep exactly one `CM*` item `in_progress` at a time.
4. Do not skip forward while an earlier eligible item is still `todo`.
5. If an item spans multiple sessions, leave it `in_progress` and update its
   checkpoint instead of starting the next item.
6. Record verification in `Execution Log` before marking an item `done`.
7. If a blocker appears, record it in the ledger and execution log before
   stopping.
8. Treat the roadmap plus the git worktree as the source of execution state.

---

## Verification Contract

Every Rust implementation item in this plan must:

1. run its focused verification before it is marked `done`
2. run `cargo fmt --all --check`
3. run `cargo check --workspace`
4. run the appropriate focused crate tests and clippy checks for the changed
   surface
5. record any environment limitation explicitly in `Execution Log`

Suggested focused lanes by work area:

- CLI items:
  `cargo test -p neovex-bin`
- persistence/provider items:
  `cargo test -p neovex-engine`,
  `cargo test -p neovex-storage`
- sandbox item:
  `cargo test -p neovex-sandbox`

Before archiving this plan, also run:

- `make check`
- `make test`
- `make clippy`
- `npm run test --workspaces --if-present`
- `npm run build --workspaces --if-present`
- `make ci` if practical

If `make ci` cannot complete because of environment-only advisory-db locking or
another non-code limitation, record that limitation explicitly rather than
silently skipping it.

---

## Roadmap Status Ledger

| Item | Status | Summary | Hard Dependencies | Gate Note |
| --- | --- | --- | --- | --- |
| CM0 | `done` | reviewed the live post-targeted-cleanup codebase and promoted a new active maintainability control plane around the current CLI, provider, and sandbox hotspots | none | docs-only review and planning pass on 2026-04-19 |
| CM1 | `todo` | split `crates/neovex-bin/src/main.rs` into a thinner CLI entrypoint plus concept-owned serve-config and runtime-limit modules | CM0 | start with config precedence and provider selection because they shape the rest of `neovex-bin` |
| CM2 | `todo` | split `crates/neovex-bin/src/machine/mod.rs` into command definitions, root or record ownership, renderers, and command handlers while keeping the root ergonomic | CM1 recommended first | do not change machine UX or record layout while moving ownership |
| CM3 | `todo` | split `crates/neovex-bin/src/machine/manager.rs` into launch, image, helper, networking, SSH, stop, and port-allocation ownership seams | CM2 recommended first | preserve start or stop or readiness semantics exactly while extracting modules |
| CM4 | `todo` | split `crates/neovex-bin/src/service/mod.rs` into execution-surface selection, backend loading, lifecycle actions, renderers, and log or ps helpers | CM1 recommended first | keep local versus forwarded machine-service behavior stable |
| CM5 | `todo` | split `crates/neovex-engine/src/persistence.rs` into capability-owned provider, store, snapshot, and executor facades with clearer cross-provider delegation seams | CM0 | avoid widening the abstraction seam beyond the already-landed provider contract |
| CM6 | `todo` | turn `crates/neovex-storage/src/sqlite.rs` into the reference provider module tree for embedded read, write, scheduler, journal, and schema or index ownership | CM5 recommended first | keep the current SQLite semantics and tests as the baseline for later provider alignment |
| CM7 | `todo` | decompose `crates/neovex-storage/src/postgres.rs`, `libsql.rs`, and `mysql.rs` into parallel capability-owned module trees that preserve backend-specific semantics | CM5 and CM6 recommended first | align naming and layout where it helps, but do not erase backend-specific behavior |
| CM8 | `todo` | split `crates/neovex-sandbox/src/backends/krun/vm.rs` into launch planning, materialization, manifest or inspect, readiness or restart, and stop or cleanup ownership | CM0 | keep backend behavior stable and prefer the same conceptual names used in the OCI helpers |
| CM9 | `todo` | update docs, rerun the full verification sweep, and archive the completed plan cleanly | CM1 through CM8 | close out only after every earlier cleanup slice is done and verified |

---

## Dependency Graph

- `CM1` is the recommended first slice because it is isolated and establishes a
  cleaner `neovex-bin` entrypoint plus config pattern for the rest of the CLI.
- `CM2` should usually follow `CM1` because `machine/mod.rs` is the clearest
  CLI god file and defines the machine-side composition root that `CM3` will
  depend on.
- `CM3` should follow `CM2` because the machine manager split is easier once
  command, record, and renderer ownership are clearer.
- `CM4` can run after `CM1`; it is mostly independent from the machine items
  but should reuse the same CLI composition patterns.
- `CM5` can start once `CM0` is done, but it is easier to tackle after some CLI
  cleanup momentum because it is the next major cross-cutting facade seam.
- `CM6` should usually follow `CM5` and establish the reference provider module
  layout before the external providers are realigned.
- `CM7` should follow `CM6` so the external providers can converge on the
  clearer capability layout rather than each inventing a different split.
- `CM8` is largely independent, but it should wait until the CLI and provider
  patterns are stable enough that the sandbox split can follow the same naming
  discipline.
- `CM9` closes the workstream after all production cleanup slices land.

---

## Recommended Delivery Order

1. `CM1` — CLI entrypoint and serve-config split
2. `CM2` — machine command-root split
3. `CM3` — machine manager split
4. `CM4` — service command-root split
5. `CM5` — engine persistence facade split
6. `CM6` — SQLite provider module tree split
7. `CM7` — Postgres/libsql/MySQL provider module tree split
8. `CM8` — krun backend split
9. `CM9` — docs, verification, and archive

---

## Implementation Checkpoints

| Item | Checkpoint | Next Step |
| --- | --- | --- |
| CM0 | done | start `CM1` by mapping the serve-config merge path, runtime-limit defaults, and provider-selection logic currently living in `main.rs` |
| CM1 | todo | extract a `serve/` or `config/` module tree from `main.rs` without changing config precedence or top-level CLI ergonomics |
| CM2 | todo | map `machine/mod.rs` into command model, record/root, render, and handler slices before moving any machine-manager internals |
| CM3 | todo | map `machine/manager.rs` into image/materialization, launch/networking, SSH/helpers, and stop/cleanup slices while preserving the current bootstrap contract |
| CM4 | todo | group `service/mod.rs` into execution-surface selection, lifecycle operations, renderers, and log/process helpers |
| CM5 | todo | split `persistence.rs` into provider registry, tenant persistence facade, tenant executor facade, snapshot facade, and query-read delegation |
| CM6 | todo | establish the reference SQLite module layout for provider config/open, read, write, scheduler, journal, schema/index, and backend utility ownership |
| CM7 | todo | realign the Postgres, libsql, and MySQL providers to the clearer capability layout while preserving backend-specific code where needed |
| CM8 | todo | map `krun/vm.rs` into plan/materialize, manifest/inspect, readiness/restart, and stop/cleanup ownership before moving code |
| CM9 | todo | rerun the repo-wide verification sweep, update the docs and plan index, and archive the completed plan cleanly |

---

## Work Items

### CM0. Baseline review and hotspot map

#### Outcome

- Completed during this planning pass.

### CM1. Split `main.rs` into a thinner CLI entrypoint plus serve-config modules

#### Implementation plan

1. Keep `crates/neovex-bin/src/main.rs` as the top-level CLI entrypoint and
   high-level dispatch surface.
2. Move serve-config merge logic, config file loading, env loading, provider
   selection, and runtime-limit defaults into concept-owned modules under a
   small `serve/` or `config/` tree.
3. Keep the public CLI shape and config precedence contract unchanged.

#### Focused verification

- `cargo test -p neovex-bin`
- `cargo fmt --all --check`
- `cargo check --workspace`
- `cargo clippy -p neovex-bin --all-targets -- -D warnings`

#### Acceptance criteria

- `main.rs` reads as a CLI composition root instead of a full configuration and
  server boot implementation pile
- config precedence and provider-selection semantics are unchanged
- serve startup remains easy to trace

### CM2. Split `machine/mod.rs` into concept-owned CLI surfaces

#### Implementation plan

1. Keep `crates/neovex-bin/src/machine/mod.rs` as the public machine-command
   entrypoint and shared re-export surface.
2. Move command models, root or record types, renderers, file helpers, and
   subcommand handlers into concept-owned modules under `machine/`.
3. Keep machine record formats, CLI flags, help text, and render contracts
   unchanged.

#### Focused verification

- `cargo test -p neovex-bin machine`
- `cargo fmt --all --check`
- `cargo check --workspace`
- `cargo clippy -p neovex-bin --all-targets -- -D warnings`

#### Acceptance criteria

- `machine/mod.rs` becomes a thin machine CLI composition root
- machine records, roots, renderers, and handlers have clearer canonical homes
- machine command behavior and output are unchanged

### CM3. Split `machine/manager.rs` into lifecycle-owned modules

#### Implementation plan

1. Keep `crates/neovex-bin/src/machine/manager.rs` or a successor
   `machine/manager/mod.rs` as the machine lifecycle composition root.
2. Separate launch planning, image materialization, helper resolution, SSH and
   guest sync, readiness waits, stop and cleanup, and port allocation into
   concept-owned modules.
3. Preserve the bootstrap identity, guest binary sync, readiness timeout, and
   stop semantics exactly.

#### Focused verification

- `cargo test -p neovex-bin machine`
- `cargo fmt --all --check`
- `cargo check --workspace`
- `cargo clippy -p neovex-bin --all-targets -- -D warnings`

#### Acceptance criteria

- machine lifecycle logic is easier to follow by concept
- start, stop, sync, and readiness behavior are unchanged
- helper and image logic are no longer buried inside one giant file

### CM4. Split `service/mod.rs` into execution-surface and renderer modules

#### Implementation plan

1. Keep `crates/neovex-bin/src/service/mod.rs` as the service-command
   entrypoint and public CLI surface.
2. Move execution-surface resolution, backend loading, lifecycle operations,
   renderers, logs, process inspection, and forwarded machine API helpers into
   concept-owned submodules.
3. Preserve service CLI output, machine auto-start rules, and forwarded backend
   validation semantics.

#### Focused verification

- `cargo test -p neovex-bin service`
- `cargo fmt --all --check`
- `cargo check --workspace`
- `cargo clippy -p neovex-bin --all-targets -- -D warnings`

#### Acceptance criteria

- `service/mod.rs` becomes a thin service CLI composition root
- execution-surface selection and lifecycle behavior remain unchanged
- logs and process inspection helpers live in clearer modules

### CM5. Split `persistence.rs` into capability-owned provider facades

#### Implementation plan

1. Keep `crates/neovex-engine/src/persistence.rs` or a successor directory
   module as the provider-facade composition root.
2. Separate provider registry enums, tenant persistence facade, executor
   facade, snapshot facade, and query-read delegation into clearer modules.
3. Reduce duplicated match walls where a durable capability seam is already
   stable, but do not widen the provider abstraction surface.

#### Focused verification

- `cargo test -p neovex-engine`
- `cargo test -p neovex-storage`
- `cargo fmt --all --check`
- `cargo check --workspace`
- `cargo clippy -p neovex-engine -p neovex-storage --all-targets -- -D warnings`

#### Acceptance criteria

- provider, store, executor, and snapshot ownership are easier to name
- the existing provider contract stays stable
- cross-provider delegation is clearer and less repetitive

### CM6. Turn `sqlite.rs` into the reference provider module tree

#### Implementation plan

1. Convert `crates/neovex-storage/src/sqlite.rs` into a directory module or a
   similarly thin composition root.
2. Group config/open, read snapshot, write transaction, scheduler, journal, and
   schema/index helpers by concept ownership.
3. Use the resulting layout as the reference pattern for the external providers
   without forcing backend-specific code into unnatural modules.

#### Focused verification

- `cargo test -p neovex-storage sqlite_`
- `cargo fmt --all --check`
- `cargo check --workspace`
- `cargo clippy -p neovex-storage --all-targets -- -D warnings`

#### Acceptance criteria

- `sqlite.rs` is no longer a single-file provider stack
- SQLite behavior remains unchanged
- the new layout provides a clearer template for later provider alignment

### CM7. Realign Postgres, libsql, and MySQL providers to the clearer module layout

#### Implementation plan

1. Convert `postgres.rs`, `libsql.rs`, and `mysql.rs` into capability-owned
   module trees.
2. Keep backend-specific semantics explicit: namespaces, schemas, pools,
   listeners, replica cache ownership, admin API flows, and SQL-dialect helpers
   should remain easy to find.
3. Align naming and layout only where it materially improves maintainability.

#### Focused verification

- `cargo test -p neovex-storage postgres_`
- `cargo test -p neovex-storage libsql_`
- `cargo test -p neovex-storage mysql_`
- `cargo fmt --all --check`
- `cargo check --workspace`
- `cargo clippy -p neovex-storage --all-targets -- -D warnings`

#### Acceptance criteria

- external provider ownership is easier to compare and maintain
- backend-specific semantics remain explicit
- provider parity is easier to reason about without forcing fake uniformity

### CM8. Split `krun/vm.rs` into backend-owned lifecycle modules

#### Implementation plan

1. Keep `crates/neovex-sandbox/src/backends/krun/vm.rs` or a successor
   directory module as the krun backend composition root.
2. Move launch planning, image/build materialization, manifest or inspect
   ownership, readiness or restart behavior, and stop or cleanup helpers into
   concept-owned modules.
3. Preserve the current published-endpoint, restart-policy, and process cleanup
   behavior exactly.

#### Focused verification

- `cargo test -p neovex-sandbox`
- `cargo fmt --all --check`
- `cargo check --workspace`
- `cargo clippy -p neovex-sandbox --all-targets -- -D warnings`

#### Acceptance criteria

- the krun backend is easier to navigate by lifecycle concern
- launch, inspect, restart, readiness, and stop behavior remain unchanged
- backend helper names align more cleanly with the OCI helper surfaces

### CM9. Docs, verification, and archive closeout

#### Implementation plan

1. Update `AGENTS.md`, `docs/plans/README.md`, and any touched reference docs
   so the landed ownership map is discoverable.
2. Run the full verification sweep required by this plan.
3. Archive this control plane once all items are complete and future generic
   cleanup work needs a newly promoted plan.

#### Focused verification

- `make check`
- `make test`
- `make clippy`
- `npm run test --workspaces --if-present`
- `npm run build --workspaces --if-present`
- `make ci` if practical

#### Acceptance criteria

- the docs reflect the landed ownership map
- the full verification sweep is recorded
- this plan can move to `docs/plans/archive/` cleanly

---

## Execution Log

| Date | Item | Status | Notes | Verification | Next Step |
| --- | --- | --- | --- | --- | --- |
| 2026-04-19 | CM0 | `done` | Reviewed the live repo after the earlier targeted-domain cleanup pass and identified a new first-wave maintainability map centered on the CLI (`main.rs`, `machine/mod.rs`, `machine/manager.rs`, `service/mod.rs`), the engine/provider seam (`persistence.rs`, `sqlite.rs`, `postgres.rs`, `libsql.rs`, `mysql.rs`), and the krun backend (`krun/vm.rs`). Promoted this document as the new active generic cleanup control plane, updated the plan index and repo entrypoint docs to route future work through it, and indexed the predecessor targeted-domain pass as archived history. | docs-only review; no new code verification claimed | start `CM1` by mapping the serve-config merge path and provider-selection seam out of `crates/neovex-bin/src/main.rs` |
