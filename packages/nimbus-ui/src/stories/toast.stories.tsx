import type { Meta, StoryObj } from "@storybook/react";

import { toast } from "../components/toast";
import { Toaster } from "../components/toaster";
import { Button } from "../components/ui/button";

const meta: Meta<typeof Toaster> = {
  title: "Components/Toast",
  component: Toaster,
  decorators: [
    (Story) => (
      <div className="relative flex min-h-[320px] flex-col gap-2 p-4">
        <Story />
        <Toaster />
      </div>
    ),
  ],
};

export default meta;

type Story = StoryObj<typeof Toaster>;

// Every kind the console raises. A success expires on its own; an error
// stays until the operator closes it; a message has no icon.
export const Kinds: Story = {
  render: () => (
    <>
      <Button
        variant="outline"
        size="sm"
        onClick={() => toast.success("Started machine-01")}
      >
        Success
      </Button>
      <Button
        variant="outline"
        size="sm"
        onClick={() =>
          toast.error("Start failed", {
            description: "machine-01 refused the request: already running.",
          })
        }
      >
        Error
      </Button>
      <Button
        variant="outline"
        size="sm"
        onClick={() =>
          toast.message("Copied bundle", { description: "abc123" })
        }
      >
        Message
      </Button>
    </>
  ),
};

// One follow-up per toast. Taking it closes the toast; closing the toast
// without taking it runs `onDismiss`.
export const WithAction: Story = {
  render: () => (
    <Button
      variant="outline"
      size="sm"
      onClick={() =>
        toast.message("Nimbus 0.1.46 available", {
          description: "Update from 0.1.45.",
          action: { label: "Update", onClick: () => undefined },
          onDismiss: () => undefined,
          timeout: 0,
        })
      }
    >
      Update available
    </Button>
  ),
};

// Past three, the stack keeps the rest mounted but off screen and the shell
// draws a "+N more" line above it.
export const Overflow: Story = {
  render: () => (
    <Button
      variant="outline"
      size="sm"
      onClick={() => {
        for (let n = 1; n <= 5; n += 1) toast.error(`write ${n} failed`);
      }}
    >
      Raise five errors
    </Button>
  ),
};
