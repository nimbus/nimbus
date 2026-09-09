import type { Meta, StoryObj } from "@storybook/react";
import { useEffect } from "react";

import { AppearanceSection } from "../components/appearance-section";
import { type ThemeMode, useUiStore } from "../store/ui-store";

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

type StoryArgs = { mode: ThemeMode };
type Story = StoryObj<StoryArgs>;

function Frame({ mode }: StoryArgs) {
  const setThemeMode = useUiStore((s) => s.setThemeMode);
  useEffect(() => {
    const previousMode = useUiStore.getState().themeMode;
    setThemeMode(mode);
    return () => {
      setThemeMode(previousMode);
    };
  }, [mode, setThemeMode]);
  return <AppearanceSection />;
}

export const System: Story = {
  args: { mode: "system" },
  render: (args) => <Frame {...args} />,
};

export const Dark: Story = {
  args: { mode: "dark" },
  render: (args) => <Frame {...args} />,
};

export const Light: Story = {
  args: { mode: "light" },
  render: (args) => <Frame {...args} />,
};
