import type { Meta, StoryObj } from "@storybook/react";

import { StateDot, statePalette } from "../components/state-dot";

const meta: Meta<typeof StateDot> = {
  title: "Components/StateDot",
  component: StateDot,
  args: { state: "connected" },
  argTypes: {
    state: { control: "select", options: Object.keys(statePalette) },
  },
};

export default meta;

type Story = StoryObj<typeof StateDot>;

export const Connected: Story = { args: { state: "connected" } };
export const Reconnecting: Story = { args: { state: "reconnecting" } };
export const Offline: Story = { args: { state: "offline" } };
export const Unknown: Story = { args: { state: "something-new" } };

// Every state token the palette knows, grouped by the glyph it draws, so a
// new token lands next to the ones it must be told apart from.
export const Gallery: Story = {
  render: () => {
    const kinds = Object.keys(statePalette) as Array<keyof typeof statePalette>;
    const glyphs = Array.from(new Set(kinds.map((k) => statePalette[k].glyph)));
    return (
      <div className="flex flex-col gap-4 text-xs text-text-3">
        {glyphs.map((glyph) => (
          <div key={glyph} className="flex flex-col gap-2">
            <span className="font-mono text-text-4">{glyph}</span>
            <div className="flex flex-wrap gap-x-4 gap-y-2">
              {kinds
                .filter((k) => statePalette[k].glyph === glyph)
                .map((kind) => (
                  <span key={kind} className="inline-flex items-center gap-1.5">
                    <StateDot state={kind} /> {kind}
                  </span>
                ))}
            </div>
          </div>
        ))}
      </div>
    );
  },
};
