import { Popover } from "@base-ui/react/popover";
import type { LucideIcon } from "lucide-react";
import { Monitor, Moon, Sun } from "lucide-react";
import { cn } from "@/lib/utils";
import { SegmentedControl } from "../components/segmented-control";
import { type ThemeMode, useUiStore } from "../store/ui-store";

const MODE_OPTIONS: ReadonlyArray<{
  value: ThemeMode;
  label: string;
  icon: LucideIcon;
}> = [
  { value: "light", label: "Light", icon: Sun },
  { value: "dark", label: "Dark", icon: Moon },
  { value: "system", label: "System", icon: Monitor },
];

const MODE_ICONS: Record<ThemeMode, LucideIcon> = {
  light: Sun,
  dark: Moon,
  system: Monitor,
};

// Appearance is a per-user preference, not server administration, so it has to
// be reachable from both consoles. The full AppearanceSection card stays on the
// operator settings page; this is the same control bound to the same store
// slice.
export function AppearanceMenu() {
  const themeMode = useUiStore((s) => s.themeMode);
  const setThemeMode = useUiStore((s) => s.setThemeMode);
  const TriggerIcon = MODE_ICONS[themeMode];

  return (
    <Popover.Root>
      {/* 32px square, per DESIGN.md §Spacing And Shape. It is the one shell
          control with no text to aim at: the label is a tooltip, so the box is
          the whole target. */}
      <Popover.Trigger
        render={
          <button
            type="button"
            aria-label="Appearance"
            title="Appearance"
            data-testid="appearance-menu-trigger"
            className="flex h-8 w-8 items-center justify-center rounded-md text-text-3 transition-colors hover:bg-bg-hover hover:text-text-1"
          >
            <TriggerIcon size={16} aria-hidden />
          </button>
        }
      />
      <Popover.Portal>
        <Popover.Positioner sideOffset={8} side="bottom" align="end">
          <Popover.Popup
            data-testid="appearance-menu"
            className={cn(
              "z-50 w-60 rounded-md border border-border-2 bg-bg-raised p-3 shadow-overlay",
              "flex flex-col gap-3 text-xs text-text-1 outline-none",
            )}
          >
            <div>
              <h2 className="mb-1.5 text-xs font-medium text-text-3">Mode</h2>
              <SegmentedControl<ThemeMode>
                label="Theme mode"
                value={themeMode}
                options={MODE_OPTIONS}
                onChange={setThemeMode}
                testid="appearance-menu-mode"
              />
            </div>
          </Popover.Popup>
        </Popover.Positioner>
      </Popover.Portal>
    </Popover.Root>
  );
}
