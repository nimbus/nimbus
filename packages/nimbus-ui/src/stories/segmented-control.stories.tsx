import type { Meta, StoryObj } from "@storybook/react";
import { Monitor, Moon, Sun } from "lucide-react";
import { useState } from "react";

import { SegmentedControl } from "../components/segmented-control";

const meta: Meta<typeof SegmentedControl> = {
  title: "Components/SegmentedControl",
  component: SegmentedControl,
};

export default meta;

type Story = StoryObj<typeof SegmentedControl>;

const VIEW_OPTIONS = [
  { value: "developer", label: "Developer" },
  { value: "operator", label: "Operator" },
];

const MODE_OPTIONS = [
  { value: "light", label: "Light", description: "Always light", icon: Sun },
  { value: "dark", label: "Dark", description: "Always dark", icon: Moon },
  { value: "system", label: "System", description: "Match OS", icon: Monitor },
];

export const TwoOption: Story = {
  render: () => {
    const [value, setValue] = useState("developer");
    return (
      <SegmentedControl
        label="Console view"
        value={value}
        options={VIEW_OPTIONS}
        onChange={setValue}
        testid="story-view"
        className="h-7"
        segmentClassName="h-7 px-3 py-0 font-mono uppercase tracking-[0.12em] text-xs"
      />
    );
  },
};

export const ThreeOptionWithIcons: Story = {
  render: () => {
    const [value, setValue] = useState("light");
    return (
      <SegmentedControl
        label="Theme mode"
        value={value}
        options={MODE_OPTIONS}
        onChange={setValue}
        testid="story-mode"
      />
    );
  },
};
