import type { Meta, StoryObj } from "@storybook/react";

import { RelativeTime, Uptime } from "../components/time";

const meta: Meta = {
  title: "Components/Time",
};

export default meta;

type Story = StoryObj;

const NOW = Date.now();

export const RelativeRecent: Story = {
  render: () => <RelativeTime epochMs={NOW - 90_000} />,
};

export const RelativeHoursOld: Story = {
  render: () => <RelativeTime epochMs={NOW - 3 * 3_600_000} />,
};

export const UptimeStripe: Story = {
  render: () => (
    <div className="flex gap-4 text-xs text-muted">
      <Uptime startedAtMs={NOW - 5 * 60_000} />
      <Uptime startedAtMs={NOW - (3 * 3_600_000 + 12 * 60_000)} />
      <Uptime startedAtMs={NOW - (2 * 86_400_000 + 4 * 3_600_000)} />
    </div>
  ),
};
