# UIR23 Deploys

Date: 2026-09-08
Worktree: `~/src/github.com/nimbus/nimbus-worktrees/nimbus-ui-rebuild`, branch `codex/nimbus-ui-rebuild-phase4` (stacked on #333)

Commit `d0aa2ba9c`. The server recorded only the bundle and
function inventory of the latest activation, so the console could show
what is deployed now but not what came before it, and nothing could bring
an earlier bundle back. A deploy staged its files in a temporary directory
that vanished with the generation. The server now appends one `deploys`
row per activation, keeps a content-addressed copy of every deployed
bundle, and re-activates a retained bundle through a rollback route that
checks the copy before the generation changes. The console has a Deploys
page under Build with the history, the active pill, a per-row function
delta, a compare strip against the active bundle, and a rollback behind a
confirmation. The Settings sub-page that listed the inventory is gone.

## Seam finding

The plan named the Deploys page and a rollback. The missing piece on the
server was ownership of the deployed files after a deploy returns: the
client uploads the bundle, the server stages it in a private temporary
directory, and the directory lives only as long as the generation. A
rollback needs the bytes without the client, so `nimbus-compute` now owns
`DeployArtifactStore` in `crates/nimbus-compute/src/deploy_artifacts.rs`:
`retain(sha256, app_dir)` copies `.nimbus/convex` into
`<data_dir>/deploy-artifacts/<sha256>/` beside a `manifest.json` that names
every file with its SHA-256, written beside the final path and renamed
into place; `stage(sha256)` copies it back into a fresh private directory
and refuses when any file no longer hashes to its manifest entry or the
runtime bundle no longer hashes to its provenance record. That is the
same rule the runtime applies before every invocation (CLAUDE.md, runtime
bundles), applied once more before the generation swap.

The history itself is a system-tenant table. `nimbus-system` gained a
`deploys` table (`SystemKey::Deploys`, `records/deployment.rs`), and
`record_deployment_state_async` appends one row per activation with the
kind (`deploy`, `rollback`, `startup`), the actor, the source ref, the
silo and the function inventory, while still replacing the `bundles` and
`functions` inventory as before. Startup records the bundle the server
loaded from disk as generation 0 so the history is never empty on a
server that serves functions; that row has no silo and no retained copy,
so it cannot be rolled back to and the console says why. Rollback stays
in `deploy.rs` beside deploy and shares `activate_next_generation`, so
both paths swap the generation the same way. Cloud Functions keep their
active registry through a rollback; only the Convex bundle moves.

## What changed

| Item | Result |
| --- | --- |
| `crates/nimbus-compute/src/deploy_artifacts.rs` (new), `lib.rs` | `DeployArtifactStore { is_retained, retain, stage }`; 4 unit tests (round trip, tampered runtime bundle, provenance disagreement, never retained). |
| `crates/nimbus-compute/src/deploy.rs` | Every successful deploy retains its artifacts and records `kind: deploy`. `rollback_deploy(compute, sha256, actor)` answers `{ activated, generation, previousGeneration, sha256 }`; it is 404 for an unrecorded hash, 400 for the active bundle, for an activation without a silo, and for a copy that fails the integrity check, and the active generation is unchanged on every refusal. `deploy_history(compute)` answers `{ active, activations }` newest first with `retained` read from the store. `ACTIVATION_KIND_*`, `DEPLOY_ACTOR`, `STARTUP_ACTOR`. |
| `crates/nimbus-server/src/http/deploys.rs` (new), `http/mod.rs`, `router.rs`, `crates/nimbus-system/src/inventory.rs` | `GET /api/admin/deploys` and `POST /api/admin/deploys/{sha256}/rollback`, authorized like the other local admin routes; the rollback names `operator:<method>` as the actor and writes an audit row. Startup records generation 0 when a Convex registry loaded. |
| `crates/nimbus-system/src/schema.rs`, `keys.rs`, `records/deployment.rs`, `records/mod.rs`, `lib.rs` | `deploys` table (`by_sha256`, `by_activatedAt`), `SystemDeploymentActivation`, `deployment_history_async`; the record input carries kind, actor, generation and silo. |
| `packages/nimbus-ui/convex/schema.ts` | Mirrors the `deploys` table. |
| `src/lib/types/deploy.ts` (new), `src/lib/api-mutations.ts` | `DeployHistory`, `DeployActivation`, `RollbackResponse`, `diffFunctionPaths(from, to)` and `canRollBack(activation, active)`; `deploys.list()` and `deploys.rollback(sha256)`. |
| `src/routes/developer/deploys.tsx` (new), `deploys/-deploy-sub-panel.tsx` (new) | The page: loading, offline, error and empty states (`nimbus deploy` snippet); the table with When, Bundle (copy chip plus the `active` pill), Kind, Gen, Actor, Functions (count with `+n` and `−n` against the previous row), Retained and a row menu; **Compare with active** opens a strip that lists the paths that come back, stop resolving and stay; **Roll back to this bundle** opens a confirmation that names the same delta and posts the rollback, keeps the dialog open with the server's message on a refusal, and toasts the new generation on success. The action is disabled with the reason as its hint on the active row and on a row without retained files. The sub-panel shows the active bundle, its generation and function count, and the activation count with how many are retained. |
| `src/components/storage/row-context-menu.tsx` | `RowMenuItem.disabled`; keyboard navigation skips a disabled item. |
| `src/shell/nav-entries.ts`, `src/routes/index.tsx` | Deploys under Build after Compute; restorable section. |
| `src/routes/operator/settings.tsx`, `settings/-sub-panel.ts`, `settings/-types.ts` | The Deploys sub-page is gone; Settings has General, System, Integrations and Shutdown. `?section=deploys` falls back to General. |
| `DESIGN.md` | Deploys row in the Developer IA; the Settings and Deploys section describes the page, the routes, the diff strip, the confirmation and the refusals. |

## Spec notes

- `crates/nimbus-server/src/tests/deploy.rs`
  `deploy_history_orders_activations_newest_first_and_rollback_reactivates_a_bundle`:
  two deploys (generations 2 and 3, kind `deploy`, actor `deploy-admin`,
  silo `demo`, both retained), the history lists 3 before 2 with 3
  active; a rollback to the active bundle is 400 and an unknown hash is
  404; the rollback to generation 2's bundle answers `activated true`,
  `generation 4`, `previousGeneration 3`, its function serves again and
  the newer one does not, the history has a `rollback` row by `operator`
  on top, and the `bundles` inventory holds one bundle.
  `deploy_rollback_rejects_a_tampered_retained_bundle`: a byte changed in
  the retained `bundle.mjs` makes the rollback 400 with "integrity" in
  the message and the active generation unchanged.
- `crates/nimbus-compute` `deploy_artifacts` (4) and `deploy` (10 in the
  module) pass; `nimbus-system` `record_deployment_state_projects_neutral_bundle_and_functions`
  reads the appended `deploys` row.
- `deploy.spec.ts` (4): `diffFunctionPaths` sorts added, removed and kept;
  `canRollBack` refuses the active and the unretained row.
- `api-mutations.spec.ts` (+2): the list GET and the rollback POST with an
  encoded hash and no body; a 400 reads back as `{ ok: false, error }`.
- `deploys.spec.tsx` (7): newest first with the hash chip, the active pill
  and the `3 +2 −1` delta; the empty state names `nimbus deploy`; a 403
  shows its message; Compare lists the paths that come back and stop
  resolving, and the active row lists its inventory; the rollback posts
  only after Confirm, refetches once, toasts "generation 4" and closes;
  a refused rollback keeps the dialog open with "integrity check failed";
  the action is disabled with "already active" on the active row and
  "files not retained" on the startup row.
- `settings.spec.tsx`: `deploys` is a rejected section value.
- `tests/e2e/deploys.spec.ts` (1): the real server answers
  `{ active: null, activations: [] }`; the sidebar link lands on
  `/ui/developer/deploys` with the empty state and the sub-panel at `0 (0
  retained)`; `?section=deploys` on Settings falls back to `general`.
  `settings.spec.ts` no longer expects a Deploys menu item.
- Fail-before: `$S/uir23-fail-before.txt` records both server tests
  failing against the missing routes (`left: 404, right: 200`). The first
  full unit run failed the command-palette arrow-key walk because the
  palette follows nav order and Deploys now sits after Compute; the
  expectation names Deploys. `make clippy` refused a collapsible `if` in
  `deploy.rs`; it is a let-chain. The first e2e run expected the panel's
  loading text on an empty history; the panel renders the empty counts
  and the walk asserts them.
- Net: 130 files, 1125 tests (1118 at UIR22).

## Verification

| Check | Result |
| --- | --- |
| `cargo test -p nimbus-server --lib -- tests::deploy::` | 12 passed |
| `cargo test -p nimbus-compute --lib -- deploy` | 10 passed |
| `cargo test -p nimbus-system --lib -- record_deployment_state` | 1 passed |
| `cargo fmt --all --check`, `make check`, `make clippy` | clean |
| `npm run lint` | clean, 1 pre-existing warning, 3 infos |
| `npm run typecheck` (nimbus-ui) and root `npm run typecheck` | clean |
| `npm run test` | 130 files, 1125 passed |
| `npm run build`, `npm run storybook:build` | ok |
| `make build` | ok |
| `npm run test:e2e` | 27 passed, 1 skipped, 1 failed (the panel assertion above); the deploys walk 1 passed after the fix |
| `verify.sh` | 0 failing |
| `make ci` | UNVERIFIED. Two runs: the first ended with `No space left on device` while linking the workspace test profile (the data volume was 100% full); after 11.6 GiB of regenerable workspace artifacts were removed from this worktree's `target`, the second run was stopped by the host for low memory while compiling `nimbus-cli`. Format, the Node 22 anchor snapshot check and the nimbus-runtime lane (9 test binaries) passed in both runs. Hosted CI owns the remaining gate. |

Screenshot at 1280×720 from the e2e walk: `UIR23-deploys-empty.png` (the
Deploys page and sub-panel on a server with no recorded activation).

## Open items

- A startup row (generation 0) has no retained copy, so the server cannot
  roll back to the bundle it booted from until a deploy of the same
  bundle retains it.
- Rollback moves the Convex bundle only; Cloud Functions keep their active
  registry.
- The console has no deploy trigger; a deploy arrives through the CLI.
- The history with rows, the compare strip and the rollback are proven
  under msw and the server tests; the e2e walk covers the empty history
  only, because the fixture server has no deployable bundle.
- The artifact store keeps every deployed bundle; nothing prunes it.
- `make ci` is UNVERIFIED on this host (disk and memory); every other gate is green.
