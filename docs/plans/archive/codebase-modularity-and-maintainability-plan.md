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
| CM1 | `done` | split `crates/neovex-bin/src/main.rs` into a thinner CLI entrypoint plus concept-owned serve-config and runtime-limit modules | CM0 | completed on 2026-04-19 with the new `serve/` module tree preserving CLI-over-env-over-file precedence and the pre-existing `service/mod.rs` dirt for the later `CM4` slice |
| CM2 | `done` | split `crates/neovex-bin/src/machine/mod.rs` into command definitions, root or record ownership, renderers, and command handlers while keeping the root ergonomic | CM1 recommended first | completed on 2026-04-19 with the new `command`, `record`, `files`, `render`, and `handlers` module tree while preserving machine UX, record layout, and the existing inline regression suite |
| CM3 | `done` | split `crates/neovex-bin/src/machine/manager.rs` into launch, image, helper, networking, SSH, stop, and port-allocation ownership seams | CM2 recommended first | preserve start or stop or readiness semantics exactly while extracting lifecycle, helper, readiness, stop, image, and port-allocation ownership into a module tree |
| CM4 | `done` | split `crates/neovex-bin/src/service/mod.rs` into execution-surface selection, backend loading, lifecycle actions, renderers, and log or ps helpers | CM1 recommended first | completed on 2026-04-19 with the new `service/` module tree preserving service CLI output, machine auto-start rules, forwarded backend validation, and the existing `service ps` regression adjustments already in the worktree |
| CM5 | `done` | split `crates/neovex-engine/src/persistence.rs` into capability-owned provider, store, snapshot, and executor facades with clearer cross-provider delegation seams | CM0 | completed on 2026-04-19 with a new `persistence/` module tree that keeps the provider contract stable while separating provider registry, control-plane, tenant facade, executor facade, snapshot facade, query delegation, and write-op ownership |
| CM6 | `done` | turn `crates/neovex-storage/src/sqlite.rs` into the reference provider module tree for embedded read, write, scheduler, journal, and schema or index ownership | CM5 recommended first | completed on 2026-04-19 with `sqlite.rs` reduced to a thin composition root over `sqlite/config.rs`, `read.rs`, `write.rs`, `scheduler.rs`, `journal.rs`, `schema.rs`, and `backend.rs`, preserving the existing SQLite behavior and focused verification contract |
| CM7 | `done` | decomposed `crates/neovex-storage/src/postgres.rs`, `libsql.rs`, and `mysql.rs` into parallel capability-owned module trees that preserve backend-specific semantics | CM5 and CM6 recommended first | completed on 2026-04-19 with Postgres, MySQL, and libsql now aligned around provider, storage, read, and write seams while keeping backend-specific coordination explicit |
| CM8 | `done` | split `crates/neovex-sandbox/src/backends/krun/vm.rs` into launch planning, materialization, manifest or inspect, readiness or restart, and stop or cleanup ownership | CM0 | completed on 2026-04-19 with `vm.rs` reduced to the krun backend composition root over `vm/launch.rs`, `vm/lifecycle.rs`, and `vm/readiness.rs` while preserving published-endpoint, restart, and cleanup semantics |
| CM9 | `done` | updated docs, reran the full verification sweep, and archived the completed plan cleanly | CM1 through CM8 | completed on 2026-04-19 after the full verification bundle, archive-state doc updates, and control-plane archive move |

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
| CM1 | done | `main.rs` is now a thin command root while `serve/` owns serve boot, runtime-limit defaults, config-file and env loading, provider selection, and the moved characterization tests without changing config precedence or CLI ergonomics |
| CM2 | done | `machine/mod.rs` is now the shared machine CLI composition root for the new `command`, `record`, `files`, `render`, and `handlers` seams; machine CLI behavior and record formats remain unchanged while the existing inline regression suite continues to cover the moved surfaces |
| CM3 | done | `machine/manager.rs` is now a thin lifecycle composition surface that delegates launch planning to `manager/launch.rs`, image materialization and attestation helpers to `manager/image.rs`, helper discovery to `helpers.rs`, managed-port leasing to `ports.rs`, localhost SSH and guest-shell helpers to `ssh.rs`, bootstrap and readiness waits to `readiness.rs`, guest bootstrap and binary-sync behavior to `guest.rs`, and stop or cleanup recovery to `stop.rs`. The focused machine lane, rustfmt, `neovex-bin` clippy lane, and workspace check are green for the full split. |
| CM4 | done | `service/mod.rs` is now the service CLI composition root for the extracted `execution`, `lifecycle`, `render`, `logs`, and `process` seams. Service CLI output, machine auto-start rules, forwarded backend validation, and the pre-existing `service ps` regression adjustments all stayed intact while the focused service lane, rustfmt check, workspace check, and `neovex-bin` clippy lane passed for the completed split. |
| CM5 | done | `persistence.rs` is now a thin provider-facade composition root over `persistence/provider.rs`, `control.rs`, `tenant.rs`, `executor.rs`, `snapshot.rs`, `query.rs`, and `write_ops.rs`. The engine and storage verification lanes, rustfmt check, workspace check, and combined engine or storage clippy lane are green for the completed split. |
| CM6 | done | `sqlite.rs` is now a thin provider composition root over `sqlite/config.rs`, `read.rs`, `write.rs`, `scheduler.rs`, `journal.rs`, `schema.rs`, and `backend.rs`. The public store surface now hangs off concept-owned impl blocks, the focused SQLite lane passed, and the workspace check plus storage clippy contract stayed green for the completed split. |
| CM7 | done | Postgres, MySQL, and libsql now follow the same capability-owned provider layout. `postgres.rs` delegates through `postgres/config.rs`, `notifications.rs`, `provider.rs`, `storage.rs`, `read.rs`, and `write.rs`; `mysql.rs` delegates through `mysql/provider.rs`, `storage.rs`, `read.rs`, and `write.rs`; and `libsql.rs` now routes provider connect/open through `libsql/provider.rs`, async storage bridging through `storage.rs`, public store reads through `read.rs`, public store writes plus transaction lifecycle through `write.rs`, and remote namespace/transport helpers through `remote.rs` and `transport.rs` while the root keeps the replica freshness state machine and shared low-level helpers. The focused storage and engine verification lanes, workspace check, rustfmt check, and combined engine/storage clippy lane are green for the completed split. |
| CM8 | done | `crates/neovex-sandbox/src/backends/krun/vm.rs` is now the krun backend composition root over `vm/launch.rs`, `vm/lifecycle.rs`, and `vm/readiness.rs`. Launch planning, image/build materialization, guest-user and VM-config helpers, manifest-backed inspect/start/stop/restart/cleanup behavior, and readiness or published-endpoint rendering now live in concept-owned modules while the existing krun regression coverage and sandbox verification contract stayed green. |
| CM9 | done | Reran the repo-wide verification bundle (`make check`, `make test`, `make clippy`, npm workspace test/build, and `make ci`), updated `AGENTS.md`, `docs/plans/README.md`, and `ARCHITECTURE.md` for the landed archive state, then moved this completed control plane into `docs/plans/archive/`. The unrestricted `make ci` retry succeeded after the sandbox-only advisory-db lock failure on `cargo deny`. |

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
| 2026-04-19 | CM1 | `in_progress` | Reconciled the live worktree before starting implementation. Confirmed there is no existing `in_progress` cleanup item in the plan and the only pre-existing dirt is a small test-only change in `crates/neovex-bin/src/service/mod.rs`, which belongs to the later `CM4` surface and does not overlap the `main.rs` split. Began `CM1` from the live `main.rs` serve/config/provider seam. | plan review plus `git status --short`; `git diff -- crates/neovex-bin/src/service/mod.rs` | map `main.rs` into CLI entrypoint, serve boot, runtime-limit defaults, and config-loading modules before moving code |
| 2026-04-19 | CM1 | `done` | Split `crates/neovex-bin/src/main.rs` into a thin command root and a new `serve/` module tree. `serve/config.rs` now owns config-file loading, env loading, provider selection, and the precedence merge into `ServicePersistenceConfig`; `serve/runtime_limits.rs` owns runtime default helpers; `serve/boot.rs` owns server boot plus optional Convex and sandbox-service startup; and the moved tests in `serve/tests.rs` keep the config precedence and provider characterization coverage with the pre-existing `service/mod.rs` dirt left untouched for `CM4`. Updated `ARCHITECTURE.md` to record the landed `serve/` ownership seam. | `cargo fmt --all --check`; `cargo check --workspace`; `cargo test -p neovex-bin`; `cargo clippy -p neovex-bin --all-targets -- -D warnings` | start `CM2` by mapping `crates/neovex-bin/src/machine/mod.rs` into command, record/root, renderer, and handler seams before touching `machine/manager.rs` |
| 2026-04-19 | CM2 | `in_progress` | Mapped the live `crates/neovex-bin/src/machine/mod.rs` surface and confirmed it currently mixes clap command models, root/path and record ownership, JSON/lock helpers, renderers, command handlers, and a large inline test suite. Chose a concept-owned split of `command`, `record`, `files`, `render`, `handlers`, and `tests`, keeping the machine root as the composition surface and preserving the existing `manager.rs`, `api.rs`, and bootstrap call contracts. | `wc -l crates/neovex-bin/src/machine/mod.rs`; targeted `sed`/`rg` reads over the command, handler, record, renderer, helper, and cross-module usage ranges | extract the new module tree, re-export the shared machine types/helpers needed by sibling modules, then rerun the focused `neovex-bin` verification lane |
| 2026-04-19 | CM2 | `done` | Split `crates/neovex-bin/src/machine/mod.rs` so the production surface is now organized under `machine/command.rs`, `record.rs`, `files.rs`, `render.rs`, and `handlers.rs`. Kept `mod.rs` as the shared composition root and helper surface for sibling machine modules while preserving the existing inline regression suite, updated `ARCHITECTURE.md` to record the landed machine ownership map, and kept machine CLI flags, record formats, and render contracts unchanged. | `cargo test -p neovex-bin machine`; `cargo fmt --all --check`; `cargo check --workspace`; `cargo clippy -p neovex-bin --all-targets -- -D warnings` | start `CM3` by decomposing `crates/neovex-bin/src/machine/manager.rs` into launch, image, helper, networking, readiness, and stop/cleanup seams without changing lifecycle semantics |
| 2026-04-19 | CM3 | `in_progress` | Mapped the live `crates/neovex-bin/src/machine/manager.rs` lifecycle surface and confirmed the strongest concept seams are `launch`, `guest`, `readiness`, `stop`, `helpers`, `image`, and `ports`, with the existing manager root staying as the lifecycle composition surface. The start path currently mixes launch planning, image materialization, SSH/bootstrap prep, readiness gates, stop recovery, helper lookup, and managed-port allocation in one file. | `wc -l crates/neovex-bin/src/machine/manager.rs`; targeted `sed`/`rg` reads over the launch, guest sync, readiness, stop/cleanup, helper resolution, image materialization, and port-allocation ranges | extract the first manager submodules and re-export the shared lifecycle types/constants needed by the remaining manager root and tests before rerunning the focused `neovex-bin` lane |
| 2026-04-19 | CM3 | `in_progress` | Landed the first manager submodules without changing lifecycle semantics: `manager/helpers.rs` now owns helper-binary discovery and the helper-env test guard, `manager/ports.rs` owns managed SSH port reservation, `manager/ssh.rs` owns localhost SSH/SCP command construction plus guest-shell execution helpers, and `manager/readiness.rs` owns bootstrap serving, gvproxy/vm startup gates, forwarded machine-API readiness, and SSH readiness waits. Kept `manager.rs` as the lifecycle composition root by delegating into those seams and retargeted the existing regression tests at the new module ownership instead of the former god-file locals. | `cargo check -p neovex-bin` (after helper/ports split); `cargo check -p neovex-bin` (after SSH/readiness split) | continue `CM3` by extracting the remaining stop/cleanup, guest-sync, launch, and image-materialization seams and then rerun the full focused `neovex-bin` verification contract |
| 2026-04-19 | CM3 | `in_progress` | Added `manager/stop.rs` so graceful stop, stale-state refresh, start-failure recovery, runtime-artifact cleanup, pid/signal helpers, and krunkit control requests now live under a dedicated stop/recovery seam. Kept the manager root as the external machine lifecycle surface with thin wrappers for `stop_machine` and `refresh_machine_state`, pointed the existing stop/readiness tests at the new module ownership, and backed out an unfinished guest-module attempt so the live tree stayed coherent and compiling. | `cargo check -p neovex-bin` | continue `CM3` by extracting the remaining guest-sync, launch, and image-materialization seams, then run the full focused `neovex-bin` verification contract |
| 2026-04-19 | CM3 | `in_progress` | Re-ran the focused machine verification after landing the helper, ports, SSH, readiness, and stop/recovery seams. The refactored manager surface passes the full `machine` test lane, `cargo fmt --all --check`, `cargo clippy -p neovex-bin --all-targets -- -D warnings`, and `cargo check --workspace`, so the partial CM3 split is stable before taking the next ownership slice. | `cargo test -p neovex-bin machine`; `cargo fmt --all`; `cargo fmt --all --check`; `cargo clippy -p neovex-bin --all-targets -- -D warnings`; `cargo check --workspace` | continue `CM3` by extracting the remaining guest-sync, launch, and image-materialization seams and then rerun the focused `neovex-bin` contract before deciding whether CM3 is complete |
| 2026-04-19 | CM3 | `in_progress` | Moved the guest bootstrap/sync contract into `manager/guest.rs`. That module now owns host-managed SSH identity generation, machine-image contract reconciliation, guest neovex release-asset resolution and extraction, guest binary staging over SSH, and forwarded machine-API readiness checks. Kept `manager.rs` as the lifecycle composition root with thin delegating wrappers for the guest-facing helpers that other machine surfaces and the inline regression suite already reference. | `cargo check -p neovex-bin` | continue `CM3` by extracting the remaining launch and image-materialization seams, then rerun the focused `neovex-bin` verification contract |
| 2026-04-19 | CM3 | `done` | Completed the `machine/manager.rs` split by moving OCI or HTTP image materialization, digest verification, and attestation helpers into `manager/image.rs`, and launch planning plus helper command-line assembly into `manager/launch.rs`. `manager.rs` now holds the lifecycle composition flow and thin shared wrappers while the regression suite imports the concept-owned seams directly, and `ARCHITECTURE.md` records the landed manager ownership map. | `cargo check -p neovex-bin`; `cargo test -p neovex-bin machine`; `cargo fmt --all`; `cargo fmt --all --check`; `cargo clippy -p neovex-bin --all-targets -- -D warnings`; `cargo check --workspace` | start `CM4` by mapping the current `crates/neovex-bin/src/service/mod.rs` execution surface and extracting the first concept-owned service CLI seams |
| 2026-04-19 | CM4 | `in_progress` | Reconciled the live `service/mod.rs` worktree before editing. The active CM4 seam map is `execution` for backend or host-platform resolution plus forwarded machine API validation, `lifecycle` for up or down outcome flows, `render` for list or inspect or ps output shaping, `logs` for persisted and forwarded log reads, and `process` for PID snapshot inspection. The existing worktree dirt is limited to `service ps` test assertions and will be preserved while the module split proceeds. | `wc -l crates/neovex-bin/src/service/mod.rs`; targeted `sed` or `rg` reads over the run, render, execution-surface, lifecycle, log, process, and test sections; `git diff --stat -- crates/neovex-bin/src/service/mod.rs`; `git diff -- crates/neovex-bin/src/service/mod.rs` | extract the first CM4 submodules without changing service CLI output, machine auto-start rules, or forwarded backend validation semantics |
| 2026-04-19 | CM4 | `in_progress` | Landed the first CM4 service modules without changing behavior: `service/render.rs` now owns the low-level list or inspect or process-snapshot renderers and lifecycle action summaries, while `service/execution.rs` owns host-platform backend defaults, forwarded machine API validation, service-target lookup, and execution-surface resolution. `service/mod.rs` stays the public CLI surface and high-level orchestration layer, and it now compiles cleanly against the live `service ps` test adjustments already in the worktree. | `cargo fmt --all`; `cargo check -p neovex-bin` | continue `CM4` by extracting the remaining lifecycle, logs, and process-inspection seams, then rerun the focused `neovex-bin` service verification contract |
| 2026-04-19 | CM4 | `in_progress` | Added `service/logs.rs` so persisted krun log-path resolution, local log chunk reads, forwarded machine API log streaming, and follow-loop flushing now live under a dedicated log seam. `service/mod.rs` continues to compile cleanly as the CLI composition root while the remaining lifecycle and process-snapshot seams stay in place for the next extraction pass. | `cargo fmt --all`; `cargo check -p neovex-bin` | continue `CM4` by extracting the remaining lifecycle and process-inspection seams, then rerun the focused `neovex-bin` service verification contract |
| 2026-04-19 | CM4 | `done` | Completed the `service/mod.rs` split by moving service lifecycle flows into `service/lifecycle.rs` and PID or process inspection into `service/process.rs`, while keeping `mod.rs` as the service CLI composition root over the existing `execution`, `render`, and `logs` seams. Updated `ARCHITECTURE.md` to record the landed service ownership map and preserved the pre-existing `service ps` regression adjustments and service CLI behavior throughout the split. | `cargo test -p neovex-bin service`; `cargo fmt --all`; `cargo fmt --all --check`; `cargo check -p neovex-bin`; `cargo check --workspace`; `cargo clippy -p neovex-bin --all-targets -- -D warnings` | start `CM5` by splitting `crates/neovex-engine/src/persistence.rs` into provider, tenant, executor, snapshot, query, and write-op ownership seams without widening the provider contract |
| 2026-04-19 | CM5 | `done` | Split `crates/neovex-engine/src/persistence.rs` into a thin facade over `persistence/provider.rs`, `control.rs`, `tenant.rs`, `executor.rs`, `snapshot.rs`, `query.rs`, and `write_ops.rs`. The provider registry, opened-tenant mapping, control-plane usage facade, tenant store facade, async executor facade, snapshot facade, and query-read delegation now live in concept-owned modules while the engine/provider contract and scheduler or journal behavior stayed unchanged. Updated `ARCHITECTURE.md` to record the landed persistence ownership map. | `cargo fmt --all`; `cargo check -p neovex-engine`; `cargo test -p neovex-engine`; `cargo test -p neovex-storage`; `cargo fmt --all --check`; `cargo check --workspace`; `cargo clippy -p neovex-engine -p neovex-storage --all-targets -- -D warnings` | start `CM6` by mapping `crates/neovex-storage/src/sqlite.rs` into the reference provider seams for config/open, read, write, scheduler, journal, schema/index, and backend utility ownership |
| 2026-04-19 | CM6 | `in_progress` | Began the SQLite reference-provider split by extracting the helper wall at the bottom of `crates/neovex-storage/src/sqlite.rs` into `sqlite/schema.rs`, `sqlite/scheduler.rs`, `sqlite/journal.rs`, and `sqlite/backend.rs`. `sqlite.rs` now delegates schema/index builders, scheduler helpers, durable-journal helpers, and low-level SQLite utility functions into named seams while preserving the existing public store types and helper entrypoints. | `cargo fmt --all`; `cargo check -p neovex-storage` | continue `CM6` by moving the store open/config logic plus the read-snapshot and write-transaction impl blocks into the new module tree, then rerun the focused storage verification contract |
| 2026-04-19 | CM6 | `done` | Completed the SQLite reference-provider split by turning `crates/neovex-storage/src/sqlite.rs` into a thin composition root and moving the public store surface onto concept-owned impl blocks across `sqlite/config.rs`, `read.rs`, `write.rs`, `scheduler.rs`, `journal.rs`, `schema.rs`, and `backend.rs`. Updated `ARCHITECTURE.md` to record the landed SQLite ownership map while preserving the existing SQLite behavior, durable-journal semantics, scheduler semantics, and index helpers. | `cargo fmt --all`; `cargo test -p neovex-storage sqlite_`; `cargo fmt --all --check`; `cargo check --workspace`; `cargo clippy -p neovex-storage --all-targets -- -D warnings` | start `CM7` by mapping the current `postgres.rs`, `libsql.rs`, and `mysql.rs` provider ownership seams against the SQLite reference layout before moving code |
| 2026-04-19 | CM7 | `in_progress` | Mapped the live external-provider seams from the current worktree. `postgres.rs` (3946 lines) and `mysql.rs` (3760 lines) both concentrate provider connect/open, tenant-store facade methods, async storage bridges, write transactions, read snapshots, schema-cache helpers, and backend SQL helpers in one file, while `libsql.rs` (3879 lines) adds replica freshness metrics, namespace lifecycle, admin API flows, and local SQLite cache coordination around a similarly broad tenant-store surface. Chose `postgres.rs` as the first CM7 extraction target because it provides the clearest external-provider reference seam for config/connect, tenant registration/open, read, write, scheduler/journal helpers, and notification coordination. | `wc -l crates/neovex-storage/src/postgres.rs crates/neovex-storage/src/libsql.rs crates/neovex-storage/src/mysql.rs`; targeted `rg`/`sed` reads over provider, store, snapshot, transaction, helper, and backend-coordination seams | extract the initial `postgres/` module tree and keep the public provider contract stable before mirroring the pattern into MySQL and libsql |
| 2026-04-19 | CM7 | `in_progress` | Landed the first external-provider module seams by extracting Postgres provider config, pool or identifier helpers, and tenant bootstrap SQL into `crates/neovex-storage/src/postgres/config.rs`, moving LISTEN/NOTIFY payload plus listener coordination into `postgres/notifications.rs`, then peeling provider connect/open flows into `postgres/provider.rs` and the async storage bridge plus blocking-write executor into `postgres/storage.rs`. `postgres.rs` still owns the tenant-store, snapshot, write-transaction, and backend-session helper surfaces, but the file now delegates the clearest backend-specific coordination and lifecycle entrypoints through named modules. Updated `ARCHITECTURE.md` to reflect the landed partial Postgres ownership map. | `cargo fmt --all`; `cargo check -p neovex-storage` | continue `CM7` by moving the remaining Postgres read-snapshot and write-transaction impl blocks into named modules before mirroring the resulting external-provider layout into MySQL and libsql |
| 2026-04-19 | CM7 | `in_progress` | Completed the Postgres reference split by moving the public `PostgresTenantStore` read facade, durable-journal reads, scheduler reads, and index-scan helpers into `crates/neovex-storage/src/postgres/read.rs`, and the store write facade plus transaction bootstrap into `postgres/write.rs`. `postgres.rs` now reads as a shared composition root over the new `config`, `notifications`, `provider`, `storage`, `read`, and `write` seams, and `ARCHITECTURE.md` now records that landed ownership map. | `cargo fmt --all`; `cargo check -p neovex-storage` | mirror the same concept-owned layout into `crates/neovex-storage/src/mysql.rs`, then tackle the libsql replica-specific split |
| 2026-04-19 | CM7 | `in_progress` | Mirrored the external-provider reference layout into MySQL by extracting provider connect/open flows into `crates/neovex-storage/src/mysql/provider.rs`, the async storage bridge plus blocking-write executor into `mysql/storage.rs`, the public read facade plus snapshot ownership into `mysql/read.rs`, and the public write facade plus transaction ownership into `mysql/write.rs`. `mysql.rs` now reads as a much smaller composition root over those modules while keeping provider config, shared store types, runtime bridge helpers, and backend SQL utilities in one place. Updated `ARCHITECTURE.md` to record the landed MySQL ownership map. | `cargo fmt --all`; `cargo check -p neovex-storage` | tackle the replica-specific `crates/neovex-storage/src/libsql.rs` split next without flattening its namespace, remote-primary, or local-cache coordination semantics |
| 2026-04-19 | CM7 | `in_progress` | Started the libsql replica-provider split by extracting provider connect/open, namespace lifecycle, tenant snapshot materialization, metadata-namespace ownership, and opened-tenant accessors into `crates/neovex-storage/src/libsql/provider.rs`, and moving the async storage bridge plus blocking-write executor into `libsql/storage.rs`. `libsql.rs` is smaller and now concentrates the replica-cache freshness state machine, transport wiring, read/write store surfaces, write transaction, and remote helper utilities that still need dedicated ownership. Updated `ARCHITECTURE.md` to record the landed partial libsql ownership map. | `cargo fmt --all`; `cargo check -p neovex-storage` | move the public `LibsqlReplicaTenantStore` read/write facades and write-transaction ownership into named modules, then decide whether the remaining replica coordination should split further before closing `CM7` |
| 2026-04-19 | CM7 | `done` | Finished the provider realignment by moving the libsql public read facade into `crates/neovex-storage/src/libsql/read.rs`, the public write facade plus transaction lifecycle into `libsql/write.rs`, and the remaining namespace/snapshot/transport helpers into `libsql/remote.rs` and `transport.rs`. `libsql.rs` now acts as the replica freshness composition root, `ARCHITECTURE.md` records the final ownership map, and the external providers now share a canonical provider/storage/read/write layout without erasing backend-specific coordination. | `cargo fmt --all`; `cargo fmt --all --check`; `cargo check -p neovex-storage`; `cargo check --workspace`; `cargo test -p neovex-storage`; `cargo test -p neovex-engine`; `cargo clippy -p neovex-engine -p neovex-storage --all-targets -- -D warnings` | start `CM8` by extracting the first `crates/neovex-sandbox/src/backends/krun/vm/` ownership seams |
| 2026-04-19 | CM8 | `in_progress` | Reconciled the krun backend target for the next cleanup wave. `crates/neovex-sandbox/src/backends/krun/vm.rs` is still a 2654-line module that mixes backend config/defaults, sync start/stop/inspect entrypoints, launch planning, image/build materialization, manifest persistence, readiness probes, restart policy, and stop/cleanup helpers. | `wc -l crates/neovex-sandbox/src/backends/krun/vm.rs`; targeted `rg`/`sed` reads over config, entrypoint, planning, materialization, manifest, readiness, restart, and cleanup seams | extract the first `krun/vm/` module tree with launch/materialize, manifest/inspect, readiness/restart, and stop ownership while preserving published-endpoint and cleanup behavior |
| 2026-04-19 | CM8 | `done` | Completed the krun backend split by keeping `crates/neovex-sandbox/src/backends/krun/vm.rs` as the backend composition root and moving launch planning, image/build materialization, guest-user and VM-config helpers into `vm/launch.rs`, manifest-backed inspect/start/stop/restart/cleanup behavior into `vm/lifecycle.rs`, and readiness/published-endpoint logic into `vm/readiness.rs`. Removed the duplicated readiness helpers left in the live worktree, updated `ARCHITECTURE.md` to record the landed ownership map, and kept the existing krun behavior and inline regression coverage intact. | `cargo fmt --all`; `cargo fmt --all --check`; `cargo test -p neovex-sandbox`; `cargo check --workspace`; `cargo clippy -p neovex-sandbox --all-targets -- -D warnings` | start `CM9` by rerunning the repo-wide verification sweep, updating the plan index plus repo entrypoint docs for archive state, and archiving the completed control plane |
| 2026-04-19 | CM9 | `in_progress` | Began workstream closeout after CM8 completed. The remaining scope is the repo-wide verification bundle, archive-state doc updates (`AGENTS.md` and `docs/plans/README.md`), and moving this plan into `docs/plans/archive/` once the closeout evidence is recorded. | CM8 verification bundle above; no CM9-wide closeout commands yet | run `make check`, `make test`, `make clippy`, the npm workspace test/build lanes, and `make ci` if practical before archiving the plan |
| 2026-04-19 | CM9 | `done` | Finished the workstream closeout by updating `AGENTS.md`, `docs/plans/README.md`, and `ARCHITECTURE.md` for the completed maintainability wave, rerunning the full repo verification bundle, and archiving this control plane into `docs/plans/archive/codebase-modularity-and-maintainability-plan.md`. The first sandboxed `make ci` attempt stopped at `cargo deny` because `~/.cargo/advisory-dbs` was read-only, so the final `make ci` proof came from one unrestricted retry; that retry passed, with only pre-existing `cargo deny` duplicate-crate warnings reported. | `make check`; `make test`; `make clippy`; `npm run test --workspaces --if-present`; `npm run build --workspaces --if-present`; `make ci` | none; the maintainability control plane is complete and archived |
