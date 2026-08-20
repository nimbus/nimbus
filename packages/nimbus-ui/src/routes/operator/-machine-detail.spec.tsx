import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { MachineDetail } from "./-machine-detail";

const { useQueryMock } = vi.hoisted(() => ({ useQueryMock: vi.fn(() => []) }));

vi.mock("@nimbus/nimbus/react", () => ({
  useQuery: (..._args: unknown[]) => useQueryMock(),
}));

const machine = {
  _id: "machines:1",
  name: "kvm-01",
  kind: "machine",
  state: "running",
} as Parameters<typeof MachineDetail>[0]["machine"];

describe("MachineDetail", () => {
  // jsdom does not lay out, so this locks the constraint, not the width it
  // resolves to. `w-[420px]` is the inspector's preferred width; with
  // `shrink-0` it was also its floor, so a narrow window kept all 420px and
  // the machines row cut off the panel's right edge — the close button
  // included — leaving no way to dismiss it.
  it("treats 420px as a preferred width, not a floor", () => {
    render(<MachineDetail machine={machine} onClose={() => {}} />);
    const panel = screen.getByTestId("machines-detail");
    expect(panel.className).not.toContain("shrink-0");
    // min-w-0 is what lets a flex item shrink below its min-content width.
    expect(panel.className).toContain("min-w-0");
  });
});
