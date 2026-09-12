# DR1 Console mark

Tree: `nimbus-docs-restyle`, dirty console files listed in the work commit.

Changes:
- `packages/nimbus-ui/src/styles/tokens.css`: `--mark` / `--mark-ink` (dark
  `#f6f7f8` / `#18181b`, light `#f0b23e` / `#1a1204`) plus the `--color-mark*` bridge.
- `packages/nimbus-ui/src/components/mascot.tsx`: body is one solid `fill="var(--mark)"`
  group; face inked with `var(--mark-ink)`; `variant` prop removed; face thickens below 40px.
- Call sites, specs, and stories updated; `DESIGN.md` §Mascot and the mark token table.

Fail-before: the new spec "fills the body with the mark colour and inks the face,
never the accent" failed against the outline body (stroke present, `var(--accent)` in markup).

Verification (in `packages/nimbus-ui`):
- `npx vitest run`: 130 files, 1124 tests passed.
- `npm run typecheck`: exit 0.
- `npm run lint`: exit 0 (after `biome format --write` on the six edited files).
