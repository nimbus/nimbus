import { Monitor, Moon, Sun } from "lucide-react";
import type { LucideIcon } from "lucide-react";
import { cn } from "../lib/cn";
import {
  PALETTES,
  type Palette,
  type Theme,
  type ThemeMode,
  useUiStore,
} from "../store/ui-store";
import { SegmentedControl } from "./segmented-control";

const MODE_OPTIONS: ReadonlyArray<{
  value: ThemeMode;
  label: string;
  icon: LucideIcon;
  description: string;
}> = [
  { value: "light", label: "Light", icon: Sun, description: "Always light" },
  { value: "dark", label: "Dark", icon: Moon, description: "Always dark" },
  {
    value: "system",
    label: "System",
    icon: Monitor,
    description: "Match OS",
  },
];

export function AppearanceSection() {
  const themeMode = useUiStore((s) => s.themeMode);
  const setThemeMode = useUiStore((s) => s.setThemeMode);
  const palette = useUiStore((s) => s.palette);
  const setPalette = useUiStore((s) => s.setPalette);
  const resolvedTheme = useUiStore((s) => s.theme);

  return (
    <section
      data-testid="settings-appearance"
      className="rounded-md border border-app bg-surface p-4"
    >
      <header className="mb-3">
        <h2
          className="text-sm text-default"
          style={{ fontSize: "var(--text-base)" }}
        >
          Appearance
        </h2>
        <p className="text-xs text-muted">
          Pick a mode and color theme. Each theme pairs a light and dark variant
          from the Nimbus brand palette.
        </p>
      </header>

      <div className="flex flex-col gap-5">
        <div>
          <h3 className="label mb-2 text-muted">Mode</h3>
          <SegmentedControl<ThemeMode>
            label="Theme mode"
            value={themeMode}
            options={MODE_OPTIONS}
            onChange={setThemeMode}
            testid="appearance-mode"
          />
        </div>

        <div>
          <h3 className="label mb-2 text-muted">Color theme</h3>
          <div
            role="radiogroup"
            aria-label="Color theme"
            className="grid grid-cols-1 gap-2 sm:grid-cols-3"
            data-testid="appearance-palette"
          >
            {PALETTES.map((entry) => {
              const active = palette === entry.id;
              return (
                <button
                  key={entry.id}
                  type="button"
                  role="radio"
                  aria-checked={active}
                  onClick={() => setPalette(entry.id)}
                  data-testid={`appearance-palette-${entry.id}`}
                  data-active={active ? "true" : "false"}
                  className={cn(
                    "group flex flex-col items-stretch gap-2 rounded-md border p-3 text-left transition-colors",
                    active
                      ? "border-brand bg-surface-2"
                      : "border-app hover:bg-surface-2",
                  )}
                >
                  <PaletteSwatchRow
                    palette={entry.id}
                    resolvedTheme={resolvedTheme}
                  />
                  <div className="flex items-baseline justify-between gap-2">
                    <span className="text-sm text-default">{entry.label}</span>
                    {active ? (
                      <span className="label font-mono text-brand">active</span>
                    ) : null}
                  </div>
                  <p className="text-xs text-muted">{entry.description}</p>
                </button>
              );
            })}
          </div>
          {resolvedTheme === "dark" ? (
            <p
              className="mt-2 text-xs text-muted"
              data-testid="appearance-palette-dark-note"
            >
              Dark mode is Night Blue for Warm and Blue — their swatches match
              on purpose. Only Mono has its own dark variant. The palette you
              pick here still sets your light-mode identity.
            </p>
          ) : null}
        </div>
      </div>
    </section>
  );
}

/* The swatch is a live probe of the cascade, not a transcription of it: the
   wrapper carries `data-palette` + `data-theme` so `globals.css` re-declares
   the `--nimbus-*` tokens inside it, exactly as it does on <html>. Hand-copied
   hexes drifted from the tokens and previewed a Warm dark theme that does not
   exist; this cannot.

   Both attributes are required. `data-palette` alone is correct in light mode
   only — under a dark <html>, `[data-palette="blue"]` and
   `[data-palette="mono"]` would win over the inherited dark tokens and preview
   the *light* variant. With `data-theme` mirrored, source order gives Night
   Blue for warm and blue, and the two-attribute selector gives Reverse Mono.

   The tokens must be `--nimbus-*`, not `--color-*`: `@theme inline` inlines
   its values into generated utilities and emits no `--color-*` custom
   property, so `var(--color-surface)` resolves to nothing here. */
function PaletteSwatchRow({
  palette,
  resolvedTheme,
}: {
  palette: Palette;
  resolvedTheme: Theme;
}) {
  return (
    <div
      data-palette={palette}
      data-theme={resolvedTheme}
      data-testid={`appearance-swatch-${palette}`}
      className="flex h-12 overflow-hidden rounded border border-app"
      style={{ background: "var(--nimbus-surface)" }}
      aria-hidden
    >
      <div className="flex-1" />
      <div className="flex w-12 flex-col">
        <div
          className="h-1/2"
          data-testid={`appearance-swatch-${palette}-brand`}
          style={{ background: "var(--nimbus-brand)" }}
        />
        <div
          className="h-1/2"
          data-testid={`appearance-swatch-${palette}-accent`}
          style={{ background: "var(--nimbus-accent)" }}
        />
      </div>
      <div
        className="flex w-6 items-center justify-center text-xs font-semibold"
        style={{ color: "var(--nimbus-text)" }}
      >
        Aa
      </div>
    </div>
  );
}
