import type { Meta, StoryObj } from "@storybook/react";
import { useEffect } from "react";

import { CopyChip } from "../components/copy-chip";

const meta: Meta<typeof CopyChip> = {
  title: "Components/CopyChip",
  component: CopyChip,
};

export default meta;

type Story = StoryObj<typeof CopyChip>;

export const Value: Story = {
  args: { label: "bundle sha", value: "8a1f1cc4a9d4ecd6" },
};

export const WithChildren: Story = {
  args: {
    label: "tenant id",
    value: "tnt_demo",
    children: "copy",
  },
};

export const HiddenUntilHover: Story = {
  args: {
    label: "tenant id",
    value: "tnt_demo",
    hideUntilHover: true,
    children: "copy",
  },
  render: (args) => (
    <div className="group inline-flex items-center gap-1 text-xs text-muted">
      <span>tnt_demo</span>
      <CopyChip {...args} />
    </div>
  ),
};

export const ClipboardDenied: Story = {
  args: { label: "bundle sha", value: "8a1f1cc4a9d4ecd6" },
  render: (args) => <ClipboardDeniedFrame {...args} />,
};

function ClipboardDeniedFrame(args: {
  label: string;
  value: string;
  testid?: string;
  hideUntilHover?: boolean;
  className?: string;
  children?: React.ReactNode;
}) {
  useEffect(() => {
    const original = Object.getOwnPropertyDescriptor(
      Object.getPrototypeOf(navigator),
      "clipboard",
    );
    Object.defineProperty(navigator, "clipboard", {
      configurable: true,
      value: {
        writeText: () => Promise.reject(new Error("clipboard denied")),
      },
    });
    return () => {
      if (original) {
        Object.defineProperty(navigator, "clipboard", original);
      } else {
        Reflect.deleteProperty(navigator as object, "clipboard");
      }
    };
  }, []);
  return <CopyChip {...args} />;
}
