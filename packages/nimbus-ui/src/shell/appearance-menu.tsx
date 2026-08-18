import { Popover } from "@base-ui/react/popover";
import type { LucideIcon } from "lucide-react";
import { Monitor, Moon, Sun } from "lucide-react";

import { SegmentedControl } from "../components/segmented-control";
import { cn } from "../lib/cn";
import {
  PALETTES,
  type Palette,
  type Theme,
  type ThemeMode,
  useUiStore,
} from "../store/ui-store";

const MODE_OPTIONS: ReadonlyArray<{
  value: ThemeMode;
  label: string;
  icon: LucideIcon;
}> = [
  { value: "light", label: "Light", icon: Sun },
  { value: "dark", label: "Dark", icon: Moon },
  { value: "system", label: "System", icon: Monitor },
];

const PALETTE_OPTIONS = PALETTES.map((entry) => ({
  value: entry.id,
  label: entry.label,
  description: entry.description,
}));

const MODE_ICONS: Record<ThemeMode, LucideIcon> = {
  light: Sun,
  dark: Moon,
  system: Monitor,
};

// Appearance is a per-user preference, not server administration, so it has to
// be reachable from both consoles. The full AppearanceSection card stays on the
// operator settings page; this is the same two controls bound to the same store
// slices, sized for a 40px nav.
export function AppearanceMenu() {
  const themeMode = useUiStore((s) => s.themeMode);
  const setThemeMode = useUiStore((s) => s.setThemeMode);
  const palette = useUiStore((s) => s.palette);
  const setPalette = useUiStore((s) => s.setPalette);
  const resolvedTheme = useUiStore((s) => s.theme);
  const TriggerIcon = MODE_ICONS[themeMode];

  return (
    <Popover.Root>
      <Popover.Trigger
        render={
          <button
            type="button"
            aria-label="Appearance"
            title="Appearance"
            data-testid="appearance-menu-trigger"
            className="flex h-7 w-7 items-center justify-center rounded-md text-muted transition-colors hover:bg-surface-2 hover:text-default"
          >
            <TriggerIcon size={14} aria-hidden />
          </button>
        }
      />
      <Popover.Portal>
        <Popover.Positioner sideOffset={8} side="bottom" align="end">
          <Popover.Popup
            data-testid="appearance-menu"
            className={cn(
              "z-50 w-60 rounded-md border border-app bg-surface p-3 shadow-lg",
              "flex flex-col gap-3 text-xs text-default outline-none",
            )}
          >
            <div>
              <h2 className="label mb-1.5 text-muted">Mode</h2>
              <SegmentedControl<ThemeMode>
                label="Theme mode"
                value={themeMode}
                options={MODE_OPTIONS}
                onChange={setThemeMode}
                testid="appearance-menu-mode"
              />
            </div>
            <div>
              <h2 className="label mb-1.5 text-muted">Color theme</h2>
              {/* SegmentedControl already owns the radiogroup semantics and the
                  roving tabindex, so the palette picker inherits arrow-key and
                  Home/End navigation instead of re-implementing it. */}
              <SegmentedControl<Palette>
                label="Color theme"
                value={palette}
                options={PALETTE_OPTIONS}
                onChange={setPalette}
                testid="appearance-menu-palette"
                className="w-full"
                segmentClassName="flex-1 flex-col items-stretch gap-1 px-2 py-1.5"
                renderSegment={(option) => (
                  <>
                    <PaletteSwatch
                      palette={option.value}
                      resolvedTheme={resolvedTheme}
                    />
                    <span className="truncate text-xs">{option.label}</span>
                  </>
                )}
              />
            </div>
          </Popover.Popup>
        </Popover.Positioner>
      </Popover.Portal>
    </Popover.Root>
  );
}

/* Same live-cascade probe the settings card uses: the wrapper carries both
   `data-palette` and `data-theme` so globals.css re-declares the `--nimbus-*`
   tokens inside it. Hand-written hexes drift from the tokens; this cannot.
   The tokens must be `--nimbus-*` — `@theme inline` emits no `--color-*`
   custom property, so `var(--color-brand)` would resolve to nothing here. */
function PaletteSwatch({
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
      data-testid={`appearance-menu-swatch-${palette}`}
      className="flex h-4 overflow-hidden rounded-sm border border-app"
      style={{ background: "var(--nimbus-surface)" }}
      aria-hidden
    >
      <div className="flex-1" />
      <div className="w-2" style={{ background: "var(--nimbus-brand)" }} />
      <div className="w-2" style={{ background: "var(--nimbus-accent)" }} />
    </div>
  );
}
