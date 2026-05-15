# Nimbus UI

This package owns the embedded Nimbus operator console served by
`nimbus-server` at `/ui/*`.

It contains two related pieces:

- React 19 + Vite 8 SPA source under `src/`.
- `_nimbus` system-tenant Convex query source under `convex/`, generated for
  the UI through `npm run codegen`.

Common commands:

- `npm run build --workspace nimbus-ui` — regenerate Convex/router code,
  typecheck, and build `dist/` for `rust-embed`.
- `npm run test --workspace nimbus-ui` — run Vitest component/unit specs.
- `npm run lint --workspace nimbus-ui` — run Biome over the UI source.
- `npm run storybook:build --workspace nimbus-ui` — build the visual/a11y
  story matrix.
- `npm run test:e2e --workspace nimbus-ui` — run Playwright E2E. Until the
  DU11 disposable-server fixture lands, prefer setting `NIMBUS_E2E_TOKEN_PATH`
  and `NIMBUS_E2E_BASE_URL` explicitly so tests do not depend on the
  operator's real local profile.

The active implementation plan is
`docs/plans/desktop-ui-plan.md`; the native Electron shell follow-up lives in
`docs/plans/desktop-shell-plan.md`.
