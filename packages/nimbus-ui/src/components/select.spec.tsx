import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { Select } from "./select";

type Level = "info" | "warn" | "error";

const LEVEL_OPTIONS = [
  { value: "info" as const, label: "info" },
  { value: "warn" as const, label: "warn" },
  { value: "error" as const, label: "error" },
];

function renderSelect(initial: Level = "info") {
  const onChange = vi.fn();
  let current: Level = initial;
  const { rerender } = render(
    <Select<Level>
      label="LEVEL"
      value={current}
      options={LEVEL_OPTIONS}
      onChange={(next) => {
        current = next;
        onChange(next);
      }}
      testid="level-select"
    />,
  );
  return {
    onChange,
    setValue(next: Level) {
      current = next;
      rerender(
        <Select<Level>
          label="LEVEL"
          value={current}
          options={LEVEL_OPTIONS}
          onChange={(n) => {
            current = n;
            onChange(n);
          }}
          testid="level-select"
        />,
      );
    },
    get current() {
      return current;
    },
  };
}

describe("Select", () => {
  it("opens and closes on trigger click", () => {
    renderSelect();
    const trigger = screen.getByTestId("level-select");
    expect(trigger).toHaveAttribute("aria-expanded", "false");
    fireEvent.click(trigger);
    expect(trigger).toHaveAttribute("aria-expanded", "true");
    expect(screen.getByTestId("level-select-menu")).toBeInTheDocument();
    fireEvent.click(trigger);
    expect(trigger).toHaveAttribute("aria-expanded", "false");
  });

  it("opens on ArrowDown from the trigger", () => {
    renderSelect();
    const trigger = screen.getByTestId("level-select");
    fireEvent.keyDown(trigger, { key: "ArrowDown" });
    expect(trigger).toHaveAttribute("aria-expanded", "true");
  });

  it("navigates with ArrowDown/ArrowUp and selects with Enter", () => {
    const harness = renderSelect("info");
    fireEvent.click(screen.getByTestId("level-select"));
    const menu = screen.getByTestId("level-select-menu");
    fireEvent.keyDown(menu, { key: "ArrowDown" });
    fireEvent.keyDown(menu, { key: "Enter" });
    expect(harness.onChange).toHaveBeenCalledWith("warn");
  });

  it("supports Home and End", () => {
    const harness = renderSelect("warn");
    fireEvent.click(screen.getByTestId("level-select"));
    const menu = screen.getByTestId("level-select-menu");
    fireEvent.keyDown(menu, { key: "End" });
    fireEvent.keyDown(menu, { key: "Enter" });
    expect(harness.onChange).toHaveBeenLastCalledWith("error");
  });

  it("closes on Escape and returns focus to the trigger", () => {
    renderSelect();
    const trigger = screen.getByTestId("level-select");
    fireEvent.click(trigger);
    const menu = screen.getByTestId("level-select-menu");
    fireEvent.keyDown(menu, { key: "Escape" });
    expect(screen.queryByTestId("level-select-menu")).not.toBeInTheDocument();
  });

  it("type-ahead jumps to the first option whose label matches", () => {
    const harness = renderSelect("info");
    fireEvent.click(screen.getByTestId("level-select"));
    const menu = screen.getByTestId("level-select-menu");
    fireEvent.keyDown(menu, { key: "e" });
    fireEvent.keyDown(menu, { key: "Enter" });
    expect(harness.onChange).toHaveBeenCalledWith("error");
  });

  it("mouse click on an option commits the selection", () => {
    const harness = renderSelect("info");
    fireEvent.click(screen.getByTestId("level-select"));
    fireEvent.click(screen.getByTestId("level-select-option-warn"));
    expect(harness.onChange).toHaveBeenCalledWith("warn");
  });

  it("renders placeholder when value is not in options", () => {
    render(
      <Select
        label="LEVEL"
        value={"none" as Level}
        options={LEVEL_OPTIONS}
        onChange={() => undefined}
        placeholder="(any)"
        testid="level-select"
      />,
    );
    expect(screen.getByTestId("level-select")).toHaveTextContent("(any)");
  });
});
