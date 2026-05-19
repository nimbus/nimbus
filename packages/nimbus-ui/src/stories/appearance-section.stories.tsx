import type { Meta, StoryObj } from "@storybook/react";
import { useEffect } from "react";

import { AppearanceSection } from "../components/appearance-section";
import { type Palette, type ThemeMode, useUiStore } from "../store/ui-store";

const meta: Meta<typeof AppearanceSection> = {
  title: "Components/AppearanceSection",
  component: AppearanceSection,
  decorators: [
    (Story) => (
      <div className="w-[480px]">
        <Story />
      </div>
    ),
  ],
};

export default meta;

type StoryArgs = { mode: ThemeMode; palette: Palette };
type Story = StoryObj<StoryArgs>;

function Frame({ mode, palette }: StoryArgs) {
  const setThemeMode = useUiStore((s) => s.setThemeMode);
  const setPalette = useUiStore((s) => s.setPalette);
  useEffect(() => {
    const previousMode = useUiStore.getState().themeMode;
    const previousPalette = useUiStore.getState().palette;
    setThemeMode(mode);
    setPalette(palette);
    return () => {
      setThemeMode(previousMode);
      setPalette(previousPalette);
    };
  }, [mode, palette, setThemeMode, setPalette]);
  return <AppearanceSection />;
}

export const Default: Story = {
  args: { mode: "system", palette: "blue" },
  render: (args) => <Frame {...args} />,
};

export const BlueDark: Story = {
  args: { mode: "dark", palette: "blue" },
  render: (args) => <Frame {...args} />,
};

export const MonoLight: Story = {
  args: { mode: "light", palette: "mono" },
  render: (args) => <Frame {...args} />,
};

export const WarmSystem: Story = {
  args: { mode: "system", palette: "warm" },
  render: (args) => <Frame {...args} />,
};
