import type { Meta, StoryObj } from "@storybook/react";

import { StateChip } from "../components/state-chip";

const meta: Meta<typeof StateChip> = {
  title: "Components/StateChip",
  component: StateChip,
};

export default meta;

type Story = StoryObj<typeof StateChip>;

const ALL_STATES = [
  "ready",
  "running",
  "starting",
  "queued",
  "draining",
  "stopping",
  "stopped",
  "stale",
  "warning",
  "error",
  "failed",
  "unknown",
];

export const StateMatrix: Story = {
  render: () => (
    <div className="flex flex-wrap gap-2">
      {ALL_STATES.map((state) => (
        <StateChip key={state} state={state} />
      ))}
    </div>
  ),
};

export const Running: Story = { args: { state: "running" } };
export const Stopped: Story = { args: { state: "stopped" } };
export const ErrorState: Story = { args: { state: "error" } };
export const Warning: Story = { args: { state: "warning" } };
export const Unknown: Story = { args: { state: null } };
