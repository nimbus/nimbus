import type { Meta, StoryObj } from "@storybook/react";

import { EmptyState } from "../components/empty-state";

const meta: Meta<typeof EmptyState> = {
  title: "Components/EmptyState",
  component: EmptyState,
  decorators: [
    (Story) => (
      <div className="h-64 w-full max-w-xl rounded-md border border-app bg-surface">
        <Story />
      </div>
    ),
  ],
};

export default meta;

type Story = StoryObj<typeof EmptyState>;

export const TitleOnly: Story = {
  args: { title: "No machines" },
};

export const TitleAndBody: Story = {
  args: {
    title: "No services",
    body: "Author a compose.yaml and run nimbus compose up to register services.",
  },
};

export const WithButtonCta: Story = {
  args: {
    title: "Tenants endpoint unavailable",
    body: "This deployment can't reach /api/tenants: Request failed: 404",
    cta: { label: "Retry", onClick: () => undefined },
  },
};

export const WithLinkCta: Story = {
  args: {
    title: "Welcome to Nimbus",
    body: "Get started by visiting the developer console.",
    cta: { label: "Open Developer", to: "/app" },
  },
};
