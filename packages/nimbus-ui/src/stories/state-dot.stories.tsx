import type { Meta, StoryObj } from "@storybook/react";

import { StateDot } from "../components/state-dot";

const meta: Meta<typeof StateDot> = {
  title: "Components/StateDot",
  component: StateDot,
};

export default meta;

type Story = StoryObj<typeof StateDot>;

export const Connected: Story = { args: { state: "connected" } };
export const Reconnecting: Story = { args: { state: "reconnecting" } };
export const Offline: Story = { args: { state: "offline" } };

export const All: Story = {
  render: () => (
    <div className="flex items-center gap-4 text-xs text-muted">
      <span className="inline-flex items-center gap-1">
        <StateDot state="connected" /> connected
      </span>
      <span className="inline-flex items-center gap-1">
        <StateDot state="reconnecting" /> reconnecting
      </span>
      <span className="inline-flex items-center gap-1">
        <StateDot state="offline" /> offline
      </span>
    </div>
  ),
};
