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
