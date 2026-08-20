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

  // The other half of the same fix, and identical to the schema panel's: once
  // the documents table took its own `min-w-[20rem]` floor, the inspector
  // beside it became the side that yields. A browser measurement of the real
  // flex row put it at 6px wide on a 390px viewport; spanning the row until it
  // can afford 320 + 16 + 420 = 756px, with the row stacking below that, reads
  // 342px there and 420px once the row clears the threshold.
  //
  // The threshold is measured against the `documents-row` container on the
  // page section rather than the viewport, because the shell's drawers take
  // 80px to 480px in between and no single viewport breakpoint fits both.
  //
  // happy-dom performs no layout, so the utility class is the only observable
  // here. `w-[420px]` must be absent rather than merely outranked: conflicting
  // width utilities are settled by stylesheet order, not by class-list order.
  it("spans the row until it can afford 420px beside the table", () => {
    render(<IndexPanel schema={null} onClose={() => {}} />);
    const classes = screen
      .getByTestId("documents-indexes-panel")
      .className.split(" ");
    expect(classes).toContain("w-full");
    expect(classes).toContain("@min-[756px]/documents-row:w-[420px]");
    expect(classes).not.toContain("w-[420px]");
    expect(classes).not.toContain("lg:w-[420px]");
  });
});
