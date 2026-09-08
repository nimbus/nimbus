import type { Meta, StoryObj } from "@storybook/react";

import { Kbd } from "../components/kbd";

const meta: Meta<typeof Kbd> = {
  title: "Components/Kbd",
  component: Kbd,
};

export default meta;

type Story = StoryObj<typeof Kbd>;

export const Single: Story = { args: { children: "⌘" } };

export const Combo: Story = {
  render: () => (
    <span className="inline-flex items-center gap-1 text-xs text-text-3">
      <Kbd>⌘</Kbd>
      <span aria-hidden>+</span>
      <Kbd>K</Kbd>
    </span>
  ),
};

export const Escape: Story = { args: { children: "Esc" } };
