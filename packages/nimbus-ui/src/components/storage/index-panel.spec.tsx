import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { IndexPanel } from "./index-panel";

describe("IndexPanel", () => {
  // jsdom does not lay out, so this locks the constraint, not the width it
  // resolves to. `w-[420px]` is the panel's preferred width; with `shrink-0`
  // it was also its floor, so in a window under ~564px the row's
  // `overflow-hidden` cut off the panel's right edge — the close button
  // included — and left no way to dismiss it. The schema panel beside it in
  // the same toolbar carries the identical constraint.
  it("treats 420px as a preferred width, not a floor", () => {
    render(<IndexPanel schema={null} onClose={() => {}} />);
    const panel = screen.getByTestId("documents-indexes-panel");
    expect(panel.className).not.toContain("shrink-0");
    // min-w-0 is what lets a flex item shrink below its min-content width.
    expect(panel.className).toContain("min-w-0");
  });
});
