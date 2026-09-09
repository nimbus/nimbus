# nimbus-ui component catalog

The catalog is a Storybook-based fixture surface for the reusable
components shared across the operator console.

## Why Storybook (not Ladle)

Ladle was the default candidate "unless a constraint surfaces". The constraint
here is that Storybook was already wired into `package.json`
(`@storybook/react-vite`, `@storybook/addon-a11y`) before the catalog work
started, and
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

Each reusable component has a `*.stories.tsx` file under `src/stories/`:

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
| Kbd                   | `kbd.stories.tsx`                |
| SubPanel (host)      | `sub-panel.stories.tsx`         |

The sub-panel stories render a story-only `FakeSubPanelHost` and
`FakeSubPanelRail` at fixed widths. They repeat the panel's visual contract
(header, search threshold, rows, rail) without the router or the resizable
group that `SubPanelLayout` needs in the shell.

## Adding a story

1. Put the story file beside the others under `src/stories/`.
2. Use the existing `@storybook/react` `Meta` / `StoryObj` pattern.
3. Avoid router context unless you need it — most components are
   prop-driven and render cleanly in isolation.
4. Run `npm run catalog:build` and confirm the build is clean.
