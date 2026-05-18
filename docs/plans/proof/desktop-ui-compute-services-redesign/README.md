# Proof — Compute & Services Redesign

Verification artifacts for `docs/plans/desktop-ui-compute-services-redesign-plan.md`.

## Before (CS0)

Baselines copied from the predecessor plan's CS9 verification matrix
(`docs/plans/proof/desktop-ui-shell-overhaul/`) — the same surfaces in
their pre-redesign state.

- `before/cs0-dev-compute-before.png` — `/ui/app/compute` showing four
  inner tabs (Services / Functions / Scheduled / Cron) and the flat
  function list in the sub-drawer.
- `before/cs0-ops-services-before.png` — `/ui/admin/services`
  rendering `PlaceholderPage`.

## After (CS9)

Captured on 2026-05-18 against a fresh nimbus server on
`http://127.0.0.1:8089` (data dir `/tmp/nimbus-cs9/data`, started with
`--skip-codegen`). Viewport 1440×900. Empty cluster — no tenants, no
deployed services — so list pages render the "nothing here yet" path
that the redesign was meant to dignify. Console was clean (zero
`error`/`warn` messages) on every screenshot. Vitest: 171/171 pass
(`cd packages/nimbus-ui && npx vitest run`).

- `after/cs9-01-dev-compute.png` — `/ui/app/compute` Functions-only
  landing. Sub-drawer shows the hierarchical function tree (CS3) and the
  body has been stripped to the Convex-style empty state.
- `after/cs9-02-dev-services.png` — new `/ui/app/services` developer
  view. Sub-drawer scoped to the active tenant; body is the same
  `ServicesTable` used by the operator view (`showTenantColumn={false}`).
- `after/cs9-03-dev-schedules-scheduled.png` — `/ui/app/schedules`
  with the **Scheduled** tab active (migrated from `/app/compute` tabs).
- `after/cs9-04-dev-schedules-cron.png` — same page, **Cron** tab
  active; `?section=cron` URL state.
- `after/cs9-05-ops-services.png` — `/ui/admin/services` rendering the
  real cross-tenant `ServicesTable` with the operator summary chip
  (`{N} services · {M} tenants`) pinned to the right of the header. The
  chip uses `whitespace-nowrap` to keep its single-line shape at narrow
  layouts (this was the only post-CS9 fix).

## Mapping

| Phase | Asset | Notes |
|-------|-------|-------|
| CS2/CS3/CS5 | after/cs9-01-dev-compute.png | Functions-only landing + hierarchical tree |
| CS6 | after/cs9-02-dev-services.png | Developer Services list |
| CS8 | after/cs9-03-dev-schedules-scheduled.png | Schedules — Scheduled tab |
| CS8 | after/cs9-04-dev-schedules-cron.png | Schedules — Cron tab |
| CS7 | after/cs9-05-ops-services.png | Operator Services list (real content) |
