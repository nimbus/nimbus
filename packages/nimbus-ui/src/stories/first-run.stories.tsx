import type { Meta, StoryObj } from "@storybook/react";

import { FirstRun } from "../components/onboarding/first-run";

const meta: Meta<typeof FirstRun> = {
  title: "Components/FirstRun",
  component: FirstRun,
  decorators: [
    (Story) => (
      <div className="w-full max-w-3xl">
        <Story />
      </div>
    ),
  ],
};

export default meta;

type Story = StoryObj<typeof FirstRun>;

export const NothingYet: Story = {
  args: { progress: { functions: 0, runs: 0 } },
};

export const FunctionsDeployed: Story = {
  args: { progress: { functions: 3, runs: 0 } },
};

export const Complete: Story = {
  args: { progress: { functions: 3, runs: 1 } },
};
