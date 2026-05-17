import { Monitor, Moon, Sun } from "lucide-react";
import type { LucideIcon } from "lucide-react";
import { cn } from "../lib/cn";
import {
  PALETTES,
  type Palette,
  type ThemeMode,
  useUiStore,
} from "../store/ui-store";

type PaletteSwatch = {
  brand: string;
  accent: string;
  surface: string;
  text: string;
};

const PALETTE_SWATCHES: Record<Palette, { light: PaletteSwatch; dark: PaletteSwatch }> = {
  blue: {
    light: { brand: "#3B82F6", accent: "#06B6D4", surface: "#F8FAFC", text: "#0F172A" },
    dark: { brand: "#60A5FA", accent: "#67E8F9", surface: "#0B1220", text: "#E2E8F0" },
  },
  mono: {
    light: { brand: "#111827", accent: "#4B5563", surface: "#F9FAFB", text: "#111827" },
    dark: { brand: "#FFFFFF", accent: "#D1D5DB", surface: "#111827", text: "#FFFFFF" },
  },
  warm: {
    light: { brand: "#F59E0B", accent: "#FFB84D", surface: "#FFFAF2", text: "#0F172A" },
    dark: { brand: "#FBBF24", accent: "#FDBA74", surface: "#1C1410", text: "#FBF5E8" },
  },
};

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
          Pick a mode and color theme. Each theme pairs a light and dark
          variant from the Nimbus brand palette.
        </p>
      </header>

      <div className="flex flex-col gap-5">
        <div>
          <h3 className="mb-2 text-[10px] uppercase tracking-[0.14em] text-muted">
            Mode
          </h3>
          <div
            role="radiogroup"
            aria-label="Theme mode"
            className="inline-flex overflow-hidden rounded-md border border-app"
            data-testid="appearance-mode"
          >
            {MODE_OPTIONS.map((opt, idx) => {
              const active = themeMode === opt.value;
              const Icon = opt.icon;
              return (
                <button
                  key={opt.value}
                  type="button"
                  role="radio"
                  aria-checked={active}
                  aria-label={opt.description}
                  onClick={() => setThemeMode(opt.value)}
                  data-testid={`appearance-mode-${opt.value}`}
                  data-active={active ? "true" : "false"}
                  className={cn(
                    "flex items-center gap-1.5 px-3 py-1.5 text-xs",
                    idx > 0 && "border-l border-app",
                    active
                      ? "bg-surface-2 text-default"
                      : "text-muted hover:bg-surface-2 hover:text-default",
                  )}
                >
                  <Icon size={14} aria-hidden className="shrink-0" />
                  <span>{opt.label}</span>
                </button>
              );
            })}
          </div>
        </div>

        <div>
          <h3 className="mb-2 text-[10px] uppercase tracking-[0.14em] text-muted">
            Color theme
          </h3>
          <div
            role="radiogroup"
            aria-label="Color theme"
            className="grid grid-cols-1 gap-2 sm:grid-cols-3"
            data-testid="appearance-palette"
          >
            {PALETTES.map((entry) => {
              const active = palette === entry.id;
              const swatch = PALETTE_SWATCHES[entry.id][resolvedTheme];
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
                  <PaletteSwatchRow swatch={swatch} />
                  <div className="flex items-baseline justify-between gap-2">
                    <span className="text-sm text-default">{entry.label}</span>
                    {active ? (
                      <span
                        className="font-mono text-[10px] uppercase tracking-[0.14em]"
                        style={{ color: "var(--color-brand)" }}
                      >
                        active
                      </span>
                    ) : null}
                  </div>
                  <p className="text-[11px] text-muted">{entry.description}</p>
                </button>
              );
            })}
          </div>
        </div>
      </div>
    </section>
  );
}

function PaletteSwatchRow({ swatch }: { swatch: PaletteSwatch }) {
  return (
    <div
      className="flex h-12 overflow-hidden rounded border border-app"
      aria-hidden
    >
      <div className="flex-1" style={{ background: swatch.surface }} />
      <div
        className="flex w-12 flex-col"
        style={{ background: swatch.surface }}
      >
        <div className="h-1/2" style={{ background: swatch.brand }} />
        <div className="h-1/2" style={{ background: swatch.accent }} />
      </div>
      <div
        className="flex w-6 items-center justify-center text-[10px] font-semibold"
        style={{ background: swatch.surface, color: swatch.text }}
      >
        Aa
      </div>
    </div>
  );
}
