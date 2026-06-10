# Desktop UI — Design Review Fixes (proof bundle)

Proof artifacts for `docs/plans/archive/desktop-ui-design-review-fixes-plan.md`
(originally promoted 2026-05-18 from
`docs/design-review/2026-05-18-operator-console-review.md`, 16 findings,
14 in scope).

Captures are at 1440×900 against the Vite dev server
(`http://localhost:5173/ui/`) running the production source tree as of
plan closure. `before/` images are the design-review baselines copied
forward from `.review-shots/`; `after/` images are post-DR1–DR7 captures
with a `dr8-` prefix.

## Verifications recorded at DR8

- vitest: `cd packages/nimbus-ui && npx vitest run` → 27 files, 190 tests passing.
- typecheck: `npm run typecheck` clean (no errors).
- build: `cd packages/nimbus-ui && npm run build` clean (only the
  informational `chunkSizeWarningLimit` note from rolldown).
- Plan-code grep: `grep -rn 'DU-shell\|Phase 1 · Embedded\|PlaceholderPage'
  packages/nimbus-ui/src` returns zero matches (exit 1).
- Console hygiene during the walk: zero React or runtime `error`/`warn`
  on any /app/* or /admin/* surface. The single console entry observed
  (`Failed to load resource: 404`) is the dev-server fetch returning 404
  for unmounted API endpoints in this auth-less capture context — it is
  not a UI defect and does not surface in the rendered output.
- ⌘\ lens gating: verified live. ⌘\ on `/ui/app` opens the
  `System tenant lens` region (`dr8-x-02-lens.png`); the same shortcut
  on `/ui/admin/machines` is a no-op — the page snapshot after the
  keypress contains no `System tenant lens` region
  (`dr8-x-03-lens-blocked-on-admin.png`).

## Finding → after-screenshot mapping

| Finding | Summary | After-image |
|---------|---------|-------------|
| **F1** | Strip internal plan codes (`DU-shell OX`, `Phase 1 · Embedded SPA`); unify "not yet" surfaces behind canonical `EmptyState` | `after/dr8-dev-06-files.png`, `after/dr8-dev-08-settings.png`, `after/dr8-ops-01-system.png`, `after/dr8-ops-05-observability.png` |
| **F2** | ⌘\ system tenant lens fires on Developer only, never on Operator | Positive: `after/dr8-x-02-lens.png` (lens opens on `/app`). Negative: `after/dr8-x-03-lens-blocked-on-admin.png` (no lens region on `/admin/machines` after ⌘\) |
| **F3** | Observability page surfaces one section-nav: sub-drawer is the truth, body tab strip mirrors it; disabled tabs `aria-disabled` | `after/dr8-dev-07-observability.png`, `after/dr8-ops-05-observability.png` |
| **F4** | Schedules page collapses dynamic sub-drawer to two static sections (Scheduled, Cron); body tab strip removed | `after/dr8-dev-04-schedules.png` |
| **F5** | Developer pages auto-default to first tenant; no `tenant: all` ScopeChip fallback in /app | `after/dr8-dev-01-overview.png` (header chip `TENANT beta`), `after/dr8-dev-03-services.png` (`TENANT: BETA`) |
| **F6** | Operator service detail (`/admin/services/$service`) shows only the Placement tab; Restarts/Density/Drift removed until follow-up plans land | covered by `routes/admin/services_.$service.spec.ts` (`TABS.length === 1`, `isTab("placement")` only); listing surface in `after/dr8-ops-01-system.png` confirms operator shell shape |
| **F7** | `/app/storage` breadcrumb starts at the tenant id; redundant leading `storage` segment dropped | `after/dr8-dev-05-storage.png` |
| **F9** | Operator service detail tab buttons drop `uppercase tracking-wide` to match every other operator tab strip | covered in DR7 (`services_.$service.tsx` className change); operator tab strip reference in `after/dr8-ops-05-observability.png` |
| **F10** | `/admin` index renders an 8-field overview grid (no placeholder shell) | `after/dr8-ops-01-system.png` |
| **F11** | `/admin/observability` renders the real logs/runs surfaces with tenant-scope chip (no placeholder shell) | `after/dr8-ops-05-observability.png` |
| **F12** | Tenant chip on /app surfaces an actual default tenant (e.g. `beta`) rather than `tenant: all` | `after/dr8-dev-01-overview.png`, `after/dr8-dev-02-compute.png`, `after/dr8-dev-03-services.png`, `after/dr8-dev-04-schedules.png`, `after/dr8-dev-05-storage.png`, `after/dr8-dev-07-observability.png` |
| **F14** | Operator service detail sub-drawer renders tenant-grouped (mirrors operator services list); state chips per service | covered by DR7 (`routes/admin/services_.$service.tsx` `AdminDetailSubDrawer` rewrite using exported `groupByTenant`); list surface in DR5 baseline still applies |

Out of scope (recorded for traceability, not bundled as proof here):

- **F8** — already-fixed during the design review pass; no DR phase
  required.
- **F13** — folded into F1 via the canonical `EmptyState`.
- **F15** — theme matrix smoke (Light/Dark/System × Blue/Mono/Warm).
  Belongs in a separate verification-tooling plan.
- **F16** — positive-note assertion. Verified by the console-hygiene
  step above.

## Screenshot inventory

`before/` (16 PNGs, no prefix) — pre-DR baselines from the 2026-05-18 review.

`after/` (17 PNGs, `dr8-` prefix):

- Developer (`/app/*`): `dr8-dev-01-overview` through
  `dr8-dev-08-settings` — Overview, Compute, Services, Schedules,
  Storage, Files, Observability, Settings.
- Operator (`/admin/*`): `dr8-ops-01-system` through
  `dr8-ops-06-settings` — System, Tenants, Machines, Network,
  Observability, Settings.
- Cross-cut: `dr8-x-01-cmdk` (command palette open), `dr8-x-02-lens`
  (system tenant lens open on `/app`), and
  `dr8-x-03-lens-blocked-on-admin` (lens not present on `/admin/machines`
  after ⌘\ — DR2 evidence).
