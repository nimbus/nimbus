import type { LucideIcon } from "lucide-react";
import { Monitor, Moon, Sun } from "lucide-react";
import { type ThemeMode, useUiStore } from "../store/ui-store";
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

  return (
    <section
      data-testid="settings-appearance"
      className="rounded-md border border-border-2 bg-bg-panel p-4"
    >
      <header className="mb-3">
        <h2 className="text-base font-medium text-text-1">Appearance</h2>
        <p className="text-xs text-text-3">
          Light, dark, or follow the operating system. The console has one
          palette, so the choice is only about the ground.
        </p>
      </header>

      <div>
        <h3 className="mb-2 text-xs font-medium text-text-3">Mode</h3>
        <SegmentedControl<ThemeMode>
          label="Theme mode"
          value={themeMode}
          options={MODE_OPTIONS}
          onChange={setThemeMode}
          testid="appearance-mode"
        />
      </div>
    </section>
  );
}
