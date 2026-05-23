# DU7 embed verification — 2026-05-23

Fresh-build re-verification of the operator console embed at `/ui/*`
(DU1) after the CA (Coverage Acceleration) wave landed on `main`.

## What was verified

1. **Unit tests (embed contract).** `cargo test -p nimbus-server --lib
   tests::local_ui::` → 13/13 pass in 0.99s. Covers CSP header,
   session-cookie gate, launch-ticket consumption, SPA fallback for
   deep routes, and asset-shaped 404 behavior.
2. **`make verify-desktop-ui`.** Builds the binary, then runs the
   10-step deterministic Playwright smoke walk against a live server
   that boots from `target/debug/nimbus`. Walk passed (1 spec, 6.1s).
3. **Live chrome-devtools-mcp walk.** Booted `target/debug/nimbus` in a
   scratch `HOME=/tmp/nimbus-du7-proof-2026-05-23` (server listened on
   `127.0.0.1:8080`), consumed the first-boot launch ticket via the
   `/ui/launch?lt=...` URL, navigated:
   - `/ui/developer` — overview tiles render (see
     `embed-overview-2026-05-23.png`). Console: 0 messages.
   - `/ui/developer/storage` — TABLES sub-drawer renders empty-state
     copy (see `embed-storage-empty-2026-05-23.png`). Console: 1
     DevTools "issue" entry (form field missing id/name a11y nit;
     not an error or warning).
   - `/ui/operator/machines/m_does_not_exist` — deep SPA route renders
     the operator shell + "Not Found" content inside `main` (i.e. the
     embed's SPA fallback returns `index.html`, not a server 404). See
     `embed-spa-fallback-deep-route-2026-05-23.png`. Console: 0 messages.
   - `/ui/operator/machines` — MACHINES sub-drawer + empty state with
     CLI hint (see `embed-operator-machines-2026-05-23.png`). Console:
     same a11y "issue" entry as storage; no errors / warnings.
4. **HTTP wire check.**
   - `GET /ui/developer` without a cookie → `307 → /ui/auth` carrying
     the strict CSP header (`default-src 'self'; script-src 'self'
     'sha256-...'; ...`), confirming the auth gate is in front of the
     SPA shell.
   - Consuming a freshly-minted launch ticket (`nimbus auth url`) at
     `/ui/launch?lt=...` → `303 → /ui/` with
     `Set-Cookie: nimbus_session=...; HttpOnly; SameSite=Strict;
     Path=/; Max-Age=43200`.
   - With that session cookie, `GET /ui/favicon.ico` → `200
     image/x-icon`, confirming the embedded asset path serves
     post-auth.

## Why this matters post-CA

CA1 installed `mold` in the `setup-rust-cached` composite via
`CARGO_TARGET_*_RUSTFLAGS=-C link-arg=-fuse-ld=mold`; CA2 flipped
Coverage to `-j 4`; CA3 sharded Coverage into 3 lanes (`server` /
`engine` / `rest`) with a `cargo llvm-cov report` reducer; CA4
migrated `release.yml`'s 5 inline `dtolnay/rust-toolchain` +
`Swatinem/rust-cache` sites into the composite; CA5 closed the plan
with five hotfixes (profraw paths, libsql gating, show-env
standardization, postgres CRUD CI budget under instrumentation).
None of those changes touch the `rust_embed::Embed` macro in
`crates/nimbus-server/src/http/ui.rs` or the `UI_DIST_INDEX` Make
graph, so a regression in the embed would be a surprise — but the
operator console is the user-facing surface that the CA wave is
supposed to keep healthy, and a fresh-build walk-through is cheap.

Result: embed posture is unchanged from the post-CM proof
(`embed-verify-2026-05-22.md`). The two console-stream "issue"
entries on storage / machines are DevTools accessibility advisories
(form field missing `id` / `name`), not runtime errors or warnings;
they pre-date this proof and are not a CA-induced regression.

## Artifacts

- `embed-overview-2026-05-23.png`
- `embed-storage-empty-2026-05-23.png`
- `embed-spa-fallback-deep-route-2026-05-23.png`
- `embed-operator-machines-2026-05-23.png`

Existing post-CM artifacts remain alongside
(`embed-*-2026-05-22.png`) plus the original DU7-design proof shots
(`overview.png`, `machines-state-chips.png`, `network-methods.png`,
`schema-drop-dialog.png`, `storage-delete-dialog.png`).
