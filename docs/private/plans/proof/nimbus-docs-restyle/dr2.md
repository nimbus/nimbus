# DR2 Desktop mark

Repository: `~/src/github.com/nimbus/desktop`, branch `nimbus-mark` from 50bf6ca.

Fail-before: `grep -rn "322 201" buildResources` found the wisp SVG in
`buildResources/setup/cli-not-found.html` (lines 208-215) before the change; after it,
no match in the repository outside node_modules.

Source of truth: `docs/brand/mascot/{mascot-tile,mascot-template}.svg` in the nimbus
repository, rendered by `DESKTOP_DIR=~/src/github.com/nimbus/desktop bash docs/brand/mascot/render.sh`.

Outputs:
- `icon.png` 512x512; `icon.icns` (iconutil, 16..512 plus @2x); `icon.ico` 16 24 32 48 64 128 256.
- `trayTemplate.png` 24x16, `trayTemplate@2x.png` 48x32: black silhouette, face cut out.
- `cli-not-found.html`: console palette in both schemes, inline solid mascot with the
  heavy face (24px lockup), lowercase wordmark, accent hover and link tokens.

Checks (desktop): `npm run typecheck` exit 0; `npm run lint` 40 files, no fixes; `npx vitest run`
17 files, 186 tests passed.

Screenshots: `dr2/setup-card-light.png`, `dr2/setup-card-dark.png` (Playwright over installed Chrome,
720x420 at 2x). Tray preview checked at 16x and 8x enlargement.
