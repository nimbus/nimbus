import type { Meta, StoryObj } from "@storybook/react";

import {
  CategoryPill,
  Pill,
  type PillTone,
  StatePill,
} from "../components/pill";

const TONES: ReadonlyArray<PillTone> = [
  "success",
  "warning",
  "error",
  "info",
  "neutral",
];

const STATES = [
  "ready",
  "running",
  "starting",
  "pending",
  "stopped",
  "paused",
  "degraded",
  "reconnecting",
  "error",
  "crashed",
  "offline",
  "unknown",
];

const meta: Meta<typeof Pill> = {
  title: "Components/Pill",
  component: Pill,
  args: { tone: "neutral", children: "label" },
  argTypes: { tone: { control: "select", options: TONES } },
};

export default meta;

type Story = StoryObj<typeof Pill>;

export const Playground: Story = {};

export const Tones: Story = {
  render: () => (
    <div className="flex flex-wrap items-center gap-2">
      {TONES.map((tone) => (
        <Pill key={tone} tone={tone}>
          {tone}
        </Pill>
      ))}
    </div>
  ),
};

// StatePill maps a state token onto a tone and a StateDot glyph, so every
// resource table shows the same state the same way.
export const States: Story = {
  render: () => (
    <div className="flex flex-wrap items-center gap-2">
      {STATES.map((state) => (
        <StatePill key={state} state={state} />
      ))}
    </div>
  ),
};

export const Categories: Story = {
  render: () => (
    <div className="flex flex-wrap items-center gap-2">
      {["service", "sandbox", "session", "cron", undefined].map((value) => (
        <CategoryPill key={value ?? "unknown"} value={value} />
      ))}
    </div>
  ),
};
