import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { AppearanceMenu } from "./appearance-menu";

/**
 * DESIGN.md §Spacing And Shape: "Icon button: 32px square, 36px on touch
 * surfaces."
 *
 * The trigger shipped at 28px around a 14px glyph. It is the only control in
 * the nav with no visible label -- the name is a tooltip -- so the box is the
 * whole target, and it was the smallest one in the row.
 *
 * happy-dom performs no layout, so the size utilities and the glyph's own
 * width attribute are the only sizes a test can read back.
 */
describe("AppearanceMenu trigger hit target", () => {
  it("sizes the trigger 32px square", () => {
    render(<AppearanceMenu />);
    const classes = screen
      .getByTestId("appearance-menu-trigger")
      .className.split(" ");
    expect(classes).toEqual(expect.arrayContaining(["h-8", "w-8"]));
    // Two conflicting Tailwind sizes are settled by stylesheet order, not by
    // the class list, so a leftover 28px rule has to be absent rather than
    // outvoted.
    expect(classes).not.toContain("h-7");
    expect(classes).not.toContain("w-7");
  });

  it("scales the mode glyph with the box", () => {
    const { container } = render(<AppearanceMenu />);
    const glyph = container.querySelector("svg");
    // A 14px glyph in a 32px box reads as a mis-set icon rather than a bigger
    // button, so the two sizes move together.
    expect(glyph).toHaveAttribute("width", "16");
    expect(glyph).toHaveAttribute("height", "16");
  });
});
