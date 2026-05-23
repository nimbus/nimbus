# DU7 embed verification — 2026-05-22

Fresh-build verification that the operator console embed at `/ui/*`
(DU1) still serves the DU3..DU7 SPA shell and assets after the CM
modernization wave landed on `main`.

## What was verified

1. **Unit tests (embed contract).** `cargo test -p nimbus-server --lib
   tests::local_ui::` → 13/13 pass. Covers CSP header, session-cookie
   gate, launch-ticket consumption, SPA fallback for deep routes, and
   asset-shaped 404 behavior.
2. **`make verify-desktop-ui`.** Builds the binary, then runs the
   10-step deterministic Playwright smoke walk against a live server
   that boots from `target/debug/nimbus`. Walk passed (1 spec, 5.1s),
   zero `console.error`, ≤1 `console.warn`.
3. **Live chrome-devtools-mcp walk.** Booted a fresh server in a
   scratch HOME, consumed the first-boot launch ticket, navigated:
   - `/ui/developer` — overview tiles render, version copy chip
     present, status bar connected (see
     `embed-overview-2026-05-22.png`).
   - `/ui/developer/storage` — TABLES sub-drawer renders empty-state
     copy ("No tenants yet."); breadcrumb scaffolding present (see
     `embed-storage-empty-2026-05-22.png`).
   - `/ui/operator/machines/m_does_not_exist` — deep SPA route renders
     the operator shell + "Not Found" content inside `main` (i.e. the
     embed's SPA fallback returns `index.html`, not a server 404). See
     `embed-spa-fallback-deep-route-2026-05-22.png`.
   - `/ui/operator/machines` — MACHINES sub-drawer + empty state with
     CLI hint (see `embed-operator-machines-2026-05-22.png`).
   - Console message stream: zero errors or warnings across all four
     navigations.
4. **HTTP wire check.** With a valid session cookie the static asset
   path `GET /ui/favicon.ico` → `200 image/x-icon`. Without a cookie,
   navigations return `307 → /ui/auth` carrying the strict CSP header,
   confirming the auth gate is in front of the SPA shell.

## Why this matters post-CM

CM1 introduced the composite `setup-rust-cached` action and migrated
12 build sites; CM2..CM8 SHA-pinned third-party actions, pinned
`ubuntu-24.04`, and added job summaries / CodeQL. None of those changes
touch the `rust_embed::Embed` macro in `crates/nimbus-server/src/http/ui.rs`
or the `UI_DIST_INDEX` make graph, so a regression in the embed would
be a surprise — but the operator console is the user-facing surface
that the CM wave is supposed to keep healthy, and a fresh-build
walk-through is cheap.

## Artifacts

- `embed-overview-2026-05-22.png`
- `embed-storage-empty-2026-05-22.png`
- `embed-spa-fallback-deep-route-2026-05-22.png`
- `embed-operator-machines-2026-05-22.png`

Existing DU7-design proof shots remain alongside (`overview.png`,
`machines-state-chips.png`, `network-methods.png`,
`schema-drop-dialog.png`, `storage-delete-dialog.png`).
