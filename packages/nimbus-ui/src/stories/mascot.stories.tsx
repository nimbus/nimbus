import type { Meta, StoryObj } from "@storybook/react";

import { MASCOT_STATES, Mascot, type MascotState } from "../components/mascot";

const meta: Meta<typeof Mascot> = {
  title: "Brand/Mascot",
  component: Mascot,
  args: { size: 48, state: "idle" },
  argTypes: {
    state: { control: "select", options: MASCOT_STATES },
    size: { control: { type: "range", min: 16, max: 160, step: 4 } },
  },
  decorators: [
    (Story) => (
      <div className="rounded-md bg-bg-panel p-6 text-text-1">
        <Story />
      </div>
    ),
  ],
};

export default meta;

type Story = StoryObj<typeof Mascot>;

export const Idle: Story = {};
export const Working: Story = { args: { state: "working" } };
export const ErrorState: Story = { args: { state: "error" } };
export const Empty: Story = { args: { state: "empty" } };
export const Celebrate: Story = { args: { state: "celebrate" } };

const SIZES = [16, 24, 32, 48] as const;

function StateGallery() {
  return (
    <div className="grid grid-cols-[auto_repeat(4,auto)] items-center gap-x-8 gap-y-4 text-xs text-text-3">
      <span />
      {SIZES.map((size) => (
        <span key={size} className="font-mono">
          {size}px
        </span>
      ))}
      {MASCOT_STATES.map((state: MascotState) => (
        <MascotRow key={state} state={state} />
      ))}
    </div>
  );
}

function MascotRow({ state }: { state: MascotState }) {
  return (
    <>
      <span className="capitalize">{state}</span>
      {SIZES.map((size) => (
        <span key={size} className="flex items-center text-text-1">
          <Mascot state={state} size={size} decorative />
        </span>
      ))}
    </>
  );
}

// Every state at the four sizes the console uses: 16 in a tab, 24 in the
// nav, 32 in a card header, 48 in an empty state.
export const Gallery: Story = {
  render: () => <StateGallery />,
};

// The mark next to the wordmark, as the top nav sets it.
export const Lockup: Story = {
  render: () => (
    <div className="flex items-center gap-2 text-text-1">
      <Mascot size={30} />
      <span className="text-sm font-semibold tracking-tight">nimbus</span>
    </div>
  ),
};
