import { fireEvent, render, screen, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { FacetBar, FacetButton, FacetInput, FacetToggle } from "./facet-bar";

describe("FacetBar", () => {
  it("is a named toolbar that wraps instead of clipping", () => {
    render(
      <FacetBar label="Log facets" testid="facets">
        <span>facet</span>
      </FacetBar>,
    );
    const bar = screen.getByTestId("facets");
    expect(bar).toHaveAttribute("role", "toolbar");
    expect(bar).toHaveAccessibleName("Log facets");
    expect(bar.className).toContain("flex-wrap");
  });

  it("keeps the trailing cluster inside the toolbar, pinned to the end", () => {
    render(
      <FacetBar
        label="Log facets"
        testid="facets"
        trailing={
          <FacetButton onClick={() => {}} testid="clear">
            clear
          </FacetButton>
        }
      >
        <span>facet</span>
      </FacetBar>,
    );
    const clear = within(screen.getByTestId("facets")).getByTestId("clear");
    expect(clear.parentElement?.className).toContain("ml-auto");
  });
});

describe("FacetInput", () => {
  it("bounds its width and reports every keystroke", () => {
    const onChange = vi.fn();
    render(
      <FacetInput
        id="source"
        label="Source"
        value=""
        onChange={onChange}
        testid="source"
      />,
    );
    const input = screen.getByTestId("source");
    expect(input.className).toContain("w-[14ch]");
    expect(input.className).toContain("min-w-0");
    expect(screen.getByLabelText("Source")).toBe(input);
    fireEvent.change(input, { target: { value: "mac" } });
    expect(onChange).toHaveBeenCalledWith("mac");
  });
});

describe("FacetToggle", () => {
  it("is a switch that flips its value on click", () => {
    const onChange = vi.fn();
    render(
      <FacetToggle
        label="Follow"
        value={false}
        onChange={onChange}
        testid="follow"
      />,
    );
    const toggle = screen.getByTestId("follow");
    expect(toggle).toHaveAttribute("role", "switch");
    expect(toggle).toHaveAttribute("aria-checked", "false");
    fireEvent.click(toggle);
    expect(onChange).toHaveBeenCalledWith(true);
  });
});

describe("FacetButton", () => {
  it("paints the danger tone in the error color", () => {
    render(
      <FacetButton tone="danger" onClick={() => {}} testid="resume">
        paused · resume
      </FacetButton>,
    );
    expect(screen.getByTestId("resume").className).toContain("text-error");
  });
});
