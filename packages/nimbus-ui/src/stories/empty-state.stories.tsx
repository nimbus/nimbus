import type { Meta, StoryObj } from "@storybook/react";
import { Boxes, Database } from "lucide-react";

import { EmptyState } from "../components/empty-state";

const meta: Meta<typeof EmptyState> = {
  title: "Components/EmptyState",
  component: EmptyState,
  decorators: [
    (Story) => (
      <div className="h-64 w-full max-w-xl rounded-md border border-border-2 bg-bg-panel">
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
    cta: { label: "Open Developer", to: "/developer" },
  },
};

export const WithIcon: Story = {
  args: {
    icon: Boxes,
    title: "No sandboxes",
    body: "A sandbox starts when an agent or a service asks for one.",
  },
};

// The mascot stands in for the icon on the shell's own screens: a crash, a
// lost connection, a first run. Solid at 56px, so it reads as a sticker.
export const WithMascot: Story = {
  args: {
    mascot: "error",
    title: "The console shell failed to render",
    body: "Moving to another view clears this screen.",
  },
};

// The snippet is the first command an operator runs from the empty state,
// with the copy button the CLI examples share.
export const WithSnippet: Story = {
  args: {
    icon: Database,
    title: "No tables yet",
    body: "Create a table from the CLI, or write a document and Nimbus creates the table for you.",
    snippet: "nimbus storage tables create users",
    cta: { label: "Read the storage guide", to: "/developer/docs" },
  },
};
