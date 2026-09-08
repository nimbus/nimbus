import type { Meta, StoryObj } from "@storybook/react";
import { type ComponentType, useEffect } from "react";

import { FunctionRunner } from "../components/function-runner/function-runner";
import { useUiStore } from "../store/ui-store";

// The runner reads the active tenant from the ui store, the way the function
// page does, so the decorator pins one before the story renders.
function withTenant(tenant: string | null) {
  return (Story: ComponentType) => {
    useEffect(() => {
      useUiStore.setState({ activeTenant: tenant });
    }, []);
    return (
      <div className="w-[720px] rounded-lg border border-border-1 bg-bg-panel">
        <Story />
      </div>
    );
  };
}

const meta: Meta<typeof FunctionRunner> = {
  title: "Components/FunctionRunner",
  component: FunctionRunner,
  parameters: { layout: "centered" },
  decorators: [withTenant("demo")],
};

export default meta;

type Story = StoryObj<typeof FunctionRunner>;

const twoStrings = {
  kind: "object",
  fields: {
    conversationId: { kind: "string" },
    text: { kind: "string" },
  },
};

export const WithValidator: Story = {
  name: "With validator (form mode)",
  args: {
    fn: {
      _id: "functions:send",
      path: "agent:send",
      kind: "mutation",
      adapter: "nimbus",
      argsSchema: twoStrings,
    },
  },
};

export const WithoutValidator: Story = {
  name: "Without validator (JSON only)",
  args: {
    fn: {
      _id: "functions:list",
      path: "messages:list",
      kind: "query",
      adapter: "convex",
    },
  },
};

export const NotRunnable: Story = {
  name: "Non-runnable kind",
  args: {
    fn: {
      _id: "functions:http",
      path: "http:webhook",
      kind: "http",
      adapter: "nimbus",
    },
  },
};

export const NoTenant: Story = {
  name: "No active tenant",
  decorators: [withTenant(null)],
  args: {
    fn: {
      _id: "functions:send",
      path: "agent:send",
      kind: "mutation",
      adapter: "nimbus",
      argsSchema: twoStrings,
    },
  },
};
