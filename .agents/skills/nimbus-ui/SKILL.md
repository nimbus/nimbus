---
name: nimbus-ui
description: Build and change the Nimbus operator console (packages/nimbus-ui) — stack, token bridge, shadcn registry ownership, the styling contract and its lint gate, tests, and the verification commands.
---

# Nimbus operator console (packages/nimbus-ui)

The console is the embedded SPA that `nimbus-server` serves at `/ui/*`. It is
a Vite + React 19 app on TanStack Router, Tailwind v4, and shadcn
`base-nova` components over Base UI. `DESIGN.md` at the repo root is the
design system. This skill is the working procedure; read `DESIGN.md` for the
why and the visual rules, and `.agents/skills/shadcn/SKILL.md` for the
upstream shadcn rules that this project adopts.

## Read first

1. `DESIGN.md` → "Implementation Rules" and "Styling Contract".
2. `.agents/skills/shadcn/SKILL.md` and `rules/*.md` (composition, styling,
   forms, icons, base-vs-radix). This is a project-local, pinned copy
   (`skills-lock.json`); it is not installed in a home directory.
3. `packages/nimbus-ui/CATALOG.md` for the component inventory and
   `packages/nimbus-ui/README.md` for scripts.

## Where things live

| Concern | Path |
| --- | --- |
| Registry components (shadcn-owned) | `src/components/ui/*` |
| Nimbus components (wrappers, variants, domain widgets) | `src/components/*` |
| Shell: sidebar, top bar, panels, lenses | `src/shell/*` |
| Routes (file-based, `-` prefix = private module) | `src/routes/**` |
| Tokens and the `@theme inline` bridge | `src/styles/tokens.css` |
| Global CSS order | `src/styles/globals.css` |
| shadcn config (style, aliases, icon library) | `components.json` |
| Design lint | `eslint.config.mjs` |
| General lint and format | `biome.json` (workspace root) |
| Stories | `src/stories/*.stories.tsx` |
| Unit tests | `*.spec.ts(x)` beside the source |

`cn` is the `cn` npm package. `src/lib/utils.ts` re-exports it for the
`@/lib/utils` alias; registry files import it from `"cn"` directly.

## Token bridge

Nimbus role tokens (`--color-bg-canvas`, `--color-text-1`, `--color-accent`,
`--success`, `--warning`, ...) are the source of truth. `tokens.css` maps
them under `@theme inline` to both the nimbus names and the shadcn names
(`background`, `foreground`, `muted`, `ring`, `popover`, ...), so a registry
component and a nimbus component read the same value. Dark is the default;
`html[data-theme]` switches the set. Never write a `dark:` class and never
write a raw palette class; add or change a token instead.

The type scale is reset in `@theme`: xs 12, sm 13, base 14, md 16, lg 20,
xl 24, 2xl 32. Radius, fonts (Geist, Geist Mono), and shadows are tokens
too.

## Registry ownership

- Add a component: `npx shadcn@latest add <name>` from `packages/nimbus-ui`.
  The CLI reads `components.json` and writes `src/components/ui/<name>.tsx`.
- Check drift: `npx shadcn@latest diff`. Expected output: `No updates found`.
- Do not hand-edit a registry file. To change a look, add a `cva` variant in
  a wrapper under `src/components/`, or extend the variant in the registry
  file only when upstream has no equivalent and the commit says why.
- Base UI, not Radix: compose with the `render` prop, never `asChild`.
  Dialog and Sheet need a Title. Prefer the Base UI `toast` for new
  projects; this console keeps `sonner` until a migration plan owns the
  switch (see `DESIGN.md` → Toast).

## Styling contract (the lint gate)

`DESIGN.md` → "Styling Contract" is the rule text. `@shadcn/lint` enforces
it through ESLint; Biome still owns general lint and formatting.

```sh
npm run lint -w packages/nimbus-ui          # biome check src && eslint . --max-warnings <cap>
npm run lint:design -w packages/nimbus-ui   # design rules only
npx eslint src/routes/operator              # one directory while iterating
```

- Errors: `no-raw-colors`, `no-unknown-classes`. Fix them; do not disable.
- Warnings behind the cap: `no-restyle`, `no-arbitrary-values`,
  `no-inline-styles`, `require-static-classes`. A change may lower the cap
  in `package.json`; it may not raise it.
- Exception: `// eslint-disable-next-line shadcn/<rule> -- <constraint>`.
- `src/components/ui/**` is exempt from the restyle, arbitrary-value, and
  static-class rules because the registry defines the variants.

Habits the linter cannot see but the shadcn rules require: `gap-*` not
`space-*`, `size-*` for equal dimensions, `truncate` plus `title`,
`data-icon` on icons inside registry components, no manual `z-index` on
overlays, and `Field`/`FieldGroup` for forms.

## Verification

```sh
npm run typecheck -w packages/nimbus-ui   # codegen + tsc
npm run test -w packages/nimbus-ui        # vitest
npm run lint -w packages/nimbus-ui        # biome + design rules
npm run build -w packages/nimbus-ui       # vite build (make ci-required runs it)
```

`make ci-required` runs `build-js typecheck-js lint-js test-js`. Playwright
smoke is configured in `packages/nimbus-ui/playwright.config.ts`; run it
only when a change touches routing, the shell, or a flow it covers. Every new component gets a
story and a spec; every state badge, empty state, and toast follows the
`DESIGN.md` mapping tables, not an improvised one.
