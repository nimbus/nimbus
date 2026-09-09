# UIR1 Stack

Date: 2026-09-08
Worktree: `~/src/github.com/nimbus/nimbus-worktrees/nimbus-ui-rebuild`, branch `codex/nimbus-ui-rebuild`

## What changed (`packages/nimbus-ui`)

| Item | Result |
| --- | --- |
| `@base-ui/react` | 1.4.1 → 1.8.0 |
| `@tanstack/react-router` / `router-plugin` | 1.169.2 → 1.170.33 / 1.168.35 → 1.168.36 |
| Added | `@tanstack/react-table` 9.2.4, `@tanstack/react-virtual` 3.14.11, `@tanstack/charts` 0.16.0, `react-resizable-panels` 4.12.4, `class-variance-authority` 0.7.1, `cn` 0.2.6 |
| Upgraded | `lucide-react` 1.42.0, `sonner` 2.0.8, `tailwindcss` + `@tailwindcss/vite` 4.3.3, `shadcn` 4.21.0 (dev) |
| Removed | `clsx`, `tailwind-merge` (the `cn` package replaces both) |
| `components.json` | Copy of Starport's: style `base-nova`, base colour neutral, `tailwind.css` → `src/styles/globals.css`, aliases `@/components`, `@/lib/utils`, `@/components/ui`, `@/lib`, `@/hooks` |
| `@/` alias | `tsconfig.json` `paths`, `vite.config.ts`, `vitest.config.ts` |
| `src/lib/utils.ts` | `export { cn } from "cn"`; `src/lib/cn.ts` deleted; 50 importers rewritten to `@/lib/utils`; `cn.spec.ts` → `utils.spec.ts` |
| `src/components/ui/button.tsx` | Written by `npx shadcn@4.21.0 add button --yes --overwrite`. Not edited by hand. |

## Deviations from the task steps

- React stays at 19.2.6. The examples pin 19.2.6 and `@nimbus/nimbus/react`
  is a peer; a second React copy is the failure to avoid.
- Base UI is 1.8.0 and shadcn is 4.21.0 (the plan text said 1.7.x and
  4.19.x). Both are the current releases at the date above.
- Step 5 ("keep `cn` in `src/lib/cn.ts`, re-export from `utils.ts`") did not
  survive contact with the registry. The `base-nova` registry payload for
  `button` (`https://ui.shadcn.com/r/styles/base-nova/button.json`) now
  ships `import { cn } from "cn"` and declares the `cn` npm package
  (`shadcn-ui/cn`, "drop-in replacement for clsx + tailwind-merge") as its
  dependency. Starport's copy still says `@/lib/utils` because it was added
  with shadcn 4.19.1. Nimbus adopts the registry's current shape: the `cn`
  package is the implementation, `src/lib/utils.ts` re-exports it for
  Nimbus-owned code, and `clsx`/`tailwind-merge` are gone. `utils.spec.ts`
  (4 tests) proves the replacement merges the same way.
- TypeScript 6 rejects `baseUrl` (TS5101), so the alias is `paths` only.

## Seams the registry forced open

1. **Colour vocabulary.** `button.tsx` names `bg-primary`, `border-border`,
   `ring-ring`, `bg-destructive`, … `token-utilities.spec.ts` flagged
   `border-border` because `--nimbus-border` exists and no `--color-border`
   bridged it. `globals.css` `@theme inline` now bridges the shadcn names
   (`background`, `foreground`, `card`, `popover`, `primary`, `secondary`,
   `muted-foreground`, `accent-foreground`, `destructive`, `border`,
   `input`, `ring`) to the existing `--nimbus-*` values. `muted` keeps its
   Nimbus meaning (muted text) until UIR2 replaces the whole block.
2. **Focus ring colour.** The destructive variant carries
   `focus-visible:ring-destructive/20`, which `contrast.spec.ts` rejects
   (DESIGN.md: a focus ring names `--focus`). Editing registry output is not
   the fix; it comes back on the next `shadcn add --overwrite`. `globals.css`
   gains one unlayered rule, `:focus-visible { --tw-ring-color:
   var(--nimbus-focus) }`. Tailwind utilities live in `@layer utilities`, so
   the unlayered declaration wins over every `focus-visible:ring-*` colour,
   registry or not. The spec skips `components/ui/` in its call-site scan and
   adds a test that parses nesting depth to prove the rule is present and
   unlayered (moving it into `@layer base` fails the test).

## Router 1.170 fallout (untouched pages, type-only)

- `ErrorComponentProps.error` is `unknown` now. The four service loader error
  components accept `unknown` and read the message through
  `error instanceof Error`.
- `RouteMatch.globalNotFound` is gone; the router sets `_notFound` on the
  match that renders the global not-found component (the same predicate
  router-core uses internally). `__root.tsx` and its spec follow.

## Checks (`packages/nimbus-ui`)

| Command | Result |
| --- | --- |
| `npm run lint` (`biome check src`) | 238 files, 0 errors (the pre-existing `src/test/msw.spec.ts` format error is fixed by `biome check --write`) |
| `npm run typecheck` | exit 0 |
| `npm run test` | 98 files, 852 tests passed (851 baseline + the unlayered focus rule test) |
| `npm run build` | vite build OK, `index-*.js` 408.76 kB (132.97 kB gzip) |
| `bash docs/private/plans/proof/nimbus-ui-rebuild/verify.sh` | `verify: 15 failing` (was 20); the UIR1 rows (`components.json`, `@tanstack/react-table`, `react-virtual`, `charts`, `react-resizable-panels`) are `ok` |
| `make build` (worktree, fresh `target/`) | exit 0, 4m 11s; the binary embeds `packages/nimbus-ui/dist` |
| `npm run test:e2e:smoke` | 1 passed (3.6s) against `target/debug/nimbus` from this worktree |

Note on the smoke run: the first attempt timed out in the `nimbusServer`
fixture. A leftover Nimbus server from the design review held port 9000, and
the disposable server's S3 listener fails on that port (`Address already in
use`). The fixture's own error path never surfaced it because the Playwright
test timeout (30 s) is shorter than the fixture readiness timeout (60 s).
Stopping the stray server fixed the run. Nothing in the fixture changed.
