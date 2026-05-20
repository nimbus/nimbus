import type { Meta, StoryObj } from "@storybook/react";
import { useState } from "react";

import { Select } from "../components/select";

const meta: Meta<typeof Select> = {
  title: "Components/Select",
  component: Select,
};

export default meta;

type Story = StoryObj<typeof Select>;

const LEVEL_OPTIONS = [
  { value: "trace", label: "trace" },
  { value: "debug", label: "debug" },
  { value: "info", label: "info" },
  { value: "warn", label: "warn" },
  { value: "error", label: "error" },
];

const LONG_OPTIONS = Array.from({ length: 24 }, (_, i) => ({
  value: `option-${i + 1}`,
  label: `option ${String(i + 1).padStart(2, "0")}`,
}));

export const Default: Story = {
  render: () => {
    const [value, setValue] = useState("info");
    return (
      <Select
        label="LEVEL"
        value={value}
        options={LEVEL_OPTIONS}
        onChange={setValue}
        testid="story-level"
      />
    );
  },
};

export const WithPlaceholder: Story = {
  render: () => {
    const [value, setValue] = useState("");
    return (
      <Select
        label="LEVEL"
        value={value}
        options={LEVEL_OPTIONS}
        placeholder="(any)"
        onChange={setValue}
        testid="story-level-placeholder"
      />
    );
  },
};

export const LongList: Story = {
  render: () => {
    const [value, setValue] = useState("option-1");
    return (
      <Select
        label="OPTION"
        value={value}
        options={LONG_OPTIONS}
        onChange={setValue}
        testid="story-long-list"
      />
    );
  },
};
