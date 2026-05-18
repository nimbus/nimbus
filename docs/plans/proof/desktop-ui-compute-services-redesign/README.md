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

Populated during CS9 verification once CS1-CS8 have landed.

## Mapping

| Phase | Asset | Notes |
|-------|-------|-------|
| CS9 | after/cs9-dev-compute.png | Functions-only landing, hierarchical tree |
| CS9 | after/cs9-dev-compute-detail-statistics.png | Function detail Statistics tab |
| CS9 | after/cs9-dev-compute-detail-source.png | Function detail Source tab |
| CS9 | after/cs9-dev-compute-runner-docked.png | Detail page with runner expanded |
| CS9 | after/cs9-dev-services.png | Developer Services list |
| CS9 | after/cs9-dev-services-detail.png | Developer Service detail (Overview tab) |
| CS9 | after/cs9-ops-services.png | Operator Services list (real content) |
| CS9 | after/cs9-ops-services-detail.png | Operator Service detail (Placement tab) |
| CS9 | after/cs9-dev-schedules.png | Schedules with migrated Scheduled+Cron tabs |
