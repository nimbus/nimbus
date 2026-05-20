# nimbus-ui component catalog

The catalog is a Storybook-based fixture surface for the eleven reusable
components called out in the desktop-ui architecture-hardening plan
(`docs/plans/desktop-ui-architecture-hardening-plan.md`, phase A5).

## Why Storybook (not Ladle)

The plan defaults to Ladle "unless a constraint surfaces". The constraint
here is that Storybook was already wired into `package.json`
(`@storybook/react-vite`, `@storybook/addon-a11y`) before A5 started, and
six stories (StateChip, StateDot, CopyChip, Breadcrumb, Time, Kbd) were
already authored against it. Switching to Ladle would have required
rewriting the existing stories and removing two devDependencies, which
is a higher cost than the "lighter, Vite-native" win Ladle promises. We
stayed on Storybook.

## Running

From `packages/nimbus-ui/`:

```sh
# Dev server with HMR (alias of `npm run storybook`)
npm run catalog

# Static build for review / CI artifacts (alias of `npm run storybook:build`)
npm run catalog:build
```

The dev server runs on port 6006. The build writes a static bundle to
`storybook-static/`.

## Story coverage

The eleven plan-mandated components each have a `*.stories.tsx` file
under `src/stories/`:

| Component             | Story file                       |
|-----------------------|----------------------------------|
| StateChip             | `state-chip.stories.tsx`         |
| StateDot              | `state-dot.stories.tsx`          |
| EmptyState            | `empty-state.stories.tsx`        |
| CopyChip              | `copy-chip.stories.tsx`          |
| LoadingCell           | `loading-cell.stories.tsx`       |
| RelativeTime + Uptime | `time.stories.tsx`               |
| Breadcrumb            | `breadcrumb.stories.tsx`         |
| UpgradePopover        | `upgrade-popover.stories.tsx`    |
| AppearanceSection     | `appearance-section.stories.tsx` |
| Select                | `select.stories.tsx`             |
| SegmentedControl      | `segmented-control.stories.tsx`  |
| SubDrawer (host)      | `sub-drawer.stories.tsx`         |

`SubDrawer` is rendered through a story-only `FakeSubDrawerHost` wrapper
that re-implements the visual shell without depending on the router
context. This is the pattern the plan calls out under A5.

## Adding a story

1. Put the story file beside the others under `src/stories/`.
2. Use the existing `@storybook/react` `Meta` / `StoryObj` pattern.
3. Avoid router context unless you need it — most components are
   prop-driven and render cleanly in isolation.
4. Run `npm run catalog:build` and confirm the build is clean.
