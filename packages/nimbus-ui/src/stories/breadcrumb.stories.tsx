import type { Meta, StoryObj } from "@storybook/react";

import { Breadcrumb } from "../components/breadcrumb";

const meta: Meta<typeof Breadcrumb> = {
  title: "Components/Breadcrumb",
  component: Breadcrumb,
};

export default meta;

type Story = StoryObj<typeof Breadcrumb>;

export const SingleSegment: Story = {
  args: { segments: [{ label: "Storage", active: true }] },
};

export const TwoSegment: Story = {
  args: {
    segments: [
      { label: "Storage", href: "/storage" },
      { label: "demo", active: true, copyValue: "tnt_demo" },
    ],
  },
};

export const ThreeSegment: Story = {
  args: {
    segments: [
      { label: "Storage", href: "/storage" },
      { label: "demo", href: "/storage/demo", copyValue: "tnt_demo" },
      { label: "machines", active: true },
    ],
  },
};

export const LongPathTruncation: Story = {
  args: {
    segments: [
      { label: "Storage", href: "/storage" },
      {
        label: "very-long-tenant-name-that-should-truncate-cleanly",
        href: "/storage/long",
        copyValue: "tnt_very_long_tenant_identifier_abcdef0123456789",
      },
      {
        label: "machines/with/an/unusually-long-trailing-segment-label",
        active: true,
      },
    ],
  },
  render: (args) => (
    <div className="w-[420px] rounded border border-app bg-surface px-3 py-2">
      <Breadcrumb {...args} />
    </div>
  ),
};
