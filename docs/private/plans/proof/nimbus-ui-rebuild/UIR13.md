# UIR13 Settings

Date: 2026-09-08
Worktree: `~/src/github.com/nimbus/nimbus-worktrees/nimbus-ui-rebuild`, branch `codex/nimbus-ui-rebuild-phase3` (stacked on #332)

Commit `885725545`. Operator Settings was one sub-panel menu of eight
entries, three of which opened an "unavailable" frame, with server
identity folded into General and two danger-zone writes behind a
hand-rolled modal. It is now five built sub-pages on the sub-panel, a
System sub-page that reports the directory the engine opened, and two
writes that go through the shared `ConfirmDialog` with a gate the
operator has to satisfy. Tenant Settings, which has no built sub-page,
contributes no menu and says so once.

## What changed

| Item | Result |
| --- | --- |
| `crates/nimbus-system/src/records/mod.rs`, `tests.rs` | `record_system_status_async` writes `details.dataDir` from `Engine::data_dir()` next to `listenAddress`; the seed test asserts it. |
| `src/components/confirm-dialog.tsx` | New `typedConfirmation: { phrase }` renders a mono field under the description and keeps Confirm inert (`aria-disabled`) until the trimmed input equals the phrase; Enter in the field confirms once it matches. New `confirmDisabled` lets a caller hold the gate closed on its own condition. Story unchanged (the dialog had none). |
| `operator/settings/-sub-panel.ts` | Five items: General, System, Deploys, Integrations, Shutdown. Endpoints, Token, and Environment are not items, per the static-menu rule (DESIGN.md: "a page that is not built yet is not a menu item, disabled or otherwise"). |
| `operator/settings.tsx` | `SECTIONS` is the same five ids; the `UNBUILT` map and its `EmptyState` are gone. General renders Appearance, the tenant header strip, and Configuration; System renders `ServerInfoSection`. |
| `operator/settings/-server-info.tsx` | New `Data directory` row as a `CopyChip` (`settings-server-data-dir`), or "not reported" / "loading…" when the status row lacks it. The tenant header strip no longer shrinks inside the page column (the first proof screenshot showed its values clipped) and repeats the license status only when it differs from the kind. |
| `operator/settings/-danger-zone.tsx` (rewritten, 323 lines) | `RotateTokenDialog` is a `ConfirmDialog` with a password field for the current bearer and `confirmDisabled` until one is typed; a refused rotation stays open with the server's message in `DialogError`. `ShutdownDialog` is a `ConfirmDialog` with `typedConfirmation: { phrase: "shutdown" }`. Outcomes render on the page (`settings-rotate-result`, `settings-shutdown-accepted`). `SHUTDOWN_PHRASE` exported. |
| `operator/settings/-primitives.tsx` | `DialogShell` deleted (58 lines); no other user. |
| `developer/settings.tsx` (rewritten) | No `validateSearch`, no `beforeLoad`, no sub-panel. One `EmptyState` (`settings-empty`, `Settings` icon) names the five planned sub-pages (`TENANT_SETTINGS_PLANNED`) and links to `/operator/settings`. |
| `tests/e2e/sub-panel.spec.ts` | Runs on `/ui/operator/settings`, the settings page that contributes a menu. The storage key is unchanged (`nimbus-ui:panel:settings`, second path segment). |
| `DESIGN.md` | Settings (tenant) says no sub-page is built and the page is one empty state. Settings (server) lists General and System separately (General is what the operator sets; System is what the server reports), marks Endpoints, Token, and Environment planned, states that both danger-zone writes use `ConfirmDialog` with typed proof, and that Deploys moves to its own page in UIR23. The two IA table cells and the Settings And Deploys section agree. |

Plan step 2 (Appearance shows mode only) was already true at UIR13
start: `appearance-section.spec.tsx` holds "offers exactly light, dark
and system" and "renders no palette control and stamps no data-palette",
so the listed fail-before (palette rows removed) had already been paid
in Phase 1. Step 5 (move `-deploys` to the Deploys page) belongs to
UIR23; the Deploys route does not exist yet, so no link was added
(typed routes make a dead link a compile error) and the sub-page stays
under Settings.

## Spec notes

- `confirm-dialog.spec.tsx` (+2, 9 total): typed phrase keeps Confirm
  inert then confirms on click and on Enter; `confirmDisabled` holds the
  gate closed.
- `danger-zone.spec.tsx` (rewritten, 5 tests): the acceptance test for
  the typed confirmation (shutdown inert until "shutdown", then
  `settings-shutdown-accepted` and the dialog gone); rotation inert until
  a bearer is typed, calls `rotateToken` with the trimmed value, shows
  the generation, toasts; a refused rotation keeps the dialog open with
  the server's message; copy checks (no backticks in the placeholder,
  `nimbus start` and the phrase as `code`).
- `server-info.spec.tsx` (+4): data directory shown and copyable, "not
  reported" when absent, the strip declines to shrink, license status
  repeated only when it adds to the kind.
- `operator/settings.spec.tsx`: General asserts Appearance, strip, and
  Configuration present and server info absent; System renders server
  info; the three planned ids are rejected by the parser; every menu
  entry renders a built pane and never an "unavailable" frame.
- `developer/settings.spec.tsx` (new, 5 tests): contributes no sub-panel,
  carries no search, one empty state naming every planned sub-page, cta
  to the operator settings, subtitle through `PageHeader`.
- Fail-before: the typed-confirmation test cannot pass on the old
  danger zone (no `settings-shutdown-dialog-typed` element existed) and
  the developer settings spec did not exist (`verify.sh` reported the
  missing spec as its one failure through UIR10–UIR12).
- `tests/e2e/settings.spec.ts` (new, chromium, 2 tests): System shows a
  non-empty data directory and the menu holds exactly the five built
  labels; General shows the strip and no server info; the rotation
  dialog is inert until a value is typed, a bogus bearer is refused
  inside the dialog and no result renders, Cancel closes it; the
  shutdown dialog is inert on a partial phrase and armed on the full one,
  then cancelled (the API specs `rotate-token` and `shutdown` prove the
  writes; the UI walk never signs out or stops the fixture). Tenant
  Settings shows the empty state, no `sub-panel`, and its cta lands on
  `/ui/operator/settings?section=general`.
- Net: 111 files, 884 tests (110 files, 871 at UIR12).

## Verification

| Check | Result |
| --- | --- |
| `npm run lint` (286 files) | clean, 1 pre-existing warning |
| `npm run typecheck` (nimbus-ui) and root `npm run typecheck` | clean |
| `npm run test` | 111 files, 884 passed |
| `npm run build` | ok |
| `npm run storybook:build` | ok |
| `cargo fmt --all --check` | clean |
| `cargo test -p nimbus-system prepare_system_tenant_seeds_network_and_adapter_posture_documents` | ok |
| `make build` (worktree root) | ok, 1m 04s |
| `npm run test:e2e` | 18 passed, 1 skipped (chromium and mobile; settings spec skipped on mobile) |
| `verify.sh` | 27 ok, 0 failing |

Screenshots at 1280×720 from the e2e walk against the rebuilt binary:
`UIR13-system.png` (the System sub-page with the data directory and the
five-item menu), `UIR13-general.png` (Appearance, the strip, and
Configuration after the shrink fix), `UIR13-shutdown-dialog.png` (the
shutdown `ConfirmDialog` with the phrase typed and Confirm armed,
captured with animations settled), `UIR13-tenant.png` (Tenant Settings
as one empty state with no sub-panel).

## Open items

- `details.storageBackend` is read by the System and General surfaces
  but no server path writes it, so the row shows "—". The server owns
  recording the backend on the status row; the UI row is ready for it.
- Deploys stays under Settings until UIR23 builds the Deploys page.
- The Token sub-page is planned; rotation lives under Shutdown until
  then, and DESIGN.md says so.
