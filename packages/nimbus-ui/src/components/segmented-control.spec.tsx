import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { SegmentedControl } from "./segmented-control";

type Mode = "light" | "dark" | "system";

const MODE_OPTIONS = [
  { value: "light" as const, label: "Light", description: "Always light" },
  { value: "dark" as const, label: "Dark", description: "Always dark" },
  { value: "system" as const, label: "System", description: "Match OS" },
];

function renderControl(initial: Mode = "light") {
  const onChange = vi.fn();
  let current: Mode = initial;
  const { rerender } = render(
    <SegmentedControl<Mode>
      label="Theme mode"
      value={current}
      options={MODE_OPTIONS}
      onChange={(next) => {
        current = next;
        onChange(next);
      }}
      testid="mode"
    />,
  );
  return {
    onChange,
    setValue(next: Mode) {
      current = next;
      rerender(
        <SegmentedControl<Mode>
          label="Theme mode"
          value={current}
          options={MODE_OPTIONS}
          onChange={(n) => {
            current = n;
            onChange(n);
          }}
          testid="mode"
        />,
      );
    },
  };
}

describe("SegmentedControl", () => {
  it("renders one radio per option with role=radiogroup", () => {
    renderControl();
    const group = screen.getByTestId("mode");
    expect(group).toHaveAttribute("role", "radiogroup");
    expect(group).toHaveAttribute("aria-label", "Theme mode");
    expect(screen.getAllByRole("radio")).toHaveLength(3);
  });

  it("does not clip the focus ring off its segments", () => {
    renderControl();
    // The focus ring is drawn 2px outside each segment, so the group must not
    // clip its own children; the end segments carry the radius instead.
    expect(screen.getByTestId("mode")).not.toHaveClass("overflow-hidden");
    expect(screen.getByTestId("mode-light")).toHaveClass("rounded-l-[5px]");
    expect(screen.getByTestId("mode-system")).toHaveClass("rounded-r-[5px]");
    expect(screen.getByTestId("mode-dark")).not.toHaveClass("rounded-l-[5px]");
    expect(screen.getByTestId("mode-dark")).not.toHaveClass("rounded-r-[5px]");
  });

  it("marks the active segment via aria-checked + data-active + tabindex", () => {
    renderControl("dark");
    const light = screen.getByTestId("mode-light");
    const dark = screen.getByTestId("mode-dark");
    expect(light).toHaveAttribute("aria-checked", "false");
    expect(light).toHaveAttribute("tabindex", "-1");
    expect(light.dataset.active).toBe("false");
    expect(dark).toHaveAttribute("aria-checked", "true");
    expect(dark).toHaveAttribute("tabindex", "0");
    expect(dark.dataset.active).toBe("true");
  });

  it("clicking a segment fires onChange with that value", () => {
    const harness = renderControl("light");
    fireEvent.click(screen.getByTestId("mode-system"));
    expect(harness.onChange).toHaveBeenCalledWith("system");
  });

  it("ArrowRight/ArrowLeft move focus through the group", () => {
    const harness = renderControl("light");
    const light = screen.getByTestId("mode-light");
    const dark = screen.getByTestId("mode-dark");
    const system = screen.getByTestId("mode-system");
    light.focus();
    fireEvent.keyDown(light, { key: "ArrowRight" });
    expect(document.activeElement).toBe(dark);
    expect(harness.onChange).toHaveBeenLastCalledWith("dark");
    fireEvent.keyDown(dark, { key: "ArrowRight" });
    expect(document.activeElement).toBe(system);
    expect(harness.onChange).toHaveBeenLastCalledWith("system");
    fireEvent.keyDown(system, { key: "ArrowLeft" });
    expect(document.activeElement).toBe(dark);
    expect(harness.onChange).toHaveBeenLastCalledWith("dark");
  });

  it("ArrowRight wraps from the last segment to the first", () => {
    renderControl("system");
    const light = screen.getByTestId("mode-light");
    const system = screen.getByTestId("mode-system");
    system.focus();
    fireEvent.keyDown(system, { key: "ArrowRight" });
    expect(document.activeElement).toBe(light);
  });

  it("Home and End jump to the first and last segment", () => {
    const harness = renderControl("dark");
    const light = screen.getByTestId("mode-light");
    const dark = screen.getByTestId("mode-dark");
    const system = screen.getByTestId("mode-system");
    dark.focus();
    fireEvent.keyDown(dark, { key: "End" });
    expect(document.activeElement).toBe(system);
    expect(harness.onChange).toHaveBeenLastCalledWith("system");
    fireEvent.keyDown(system, { key: "Home" });
    expect(document.activeElement).toBe(light);
    expect(harness.onChange).toHaveBeenLastCalledWith("light");
  });

  it("Enter and Space on a focused segment commit the selection", () => {
    const harness = renderControl("light");
    const dark = screen.getByTestId("mode-dark");
    dark.focus();
    fireEvent.keyDown(dark, { key: "Enter" });
    expect(harness.onChange).toHaveBeenLastCalledWith("dark");
    fireEvent.keyDown(screen.getByTestId("mode-system"), { key: " " });
    expect(harness.onChange).toHaveBeenLastCalledWith("system");
  });
});
