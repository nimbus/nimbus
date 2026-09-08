import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import { Select } from "./select";

type Op = "eq" | "gte" | "lt";

const OPTIONS = [
  { value: "eq", label: "=" },
  { value: "gte", label: ">=" },
  { value: "lt", label: "<" },
] as const;

function mount(value: Op = "eq") {
  const onChange = vi.fn();
  render(
    <Select<Op>
      label="Op"
      value={value}
      options={OPTIONS}
      onChange={onChange}
      testid="op"
    />,
  );
  return onChange;
}

describe("Select", () => {
  it("is a combobox named by its label that shows the current option", () => {
    mount("gte");
    const trigger = screen.getByTestId("op");
    expect(trigger).toHaveAttribute("role", "combobox");
    expect(trigger).toHaveAccessibleName("Op");
    expect(trigger).toHaveTextContent(">=");
    expect(trigger).toHaveClass("font-mono");
  });

  it("opens a listbox with one option per entry and reports the choice", async () => {
    const user = userEvent.setup();
    const onChange = mount();
    await user.click(screen.getByTestId("op"));
    const options = await screen.findAllByRole("option");
    expect(options).toHaveLength(3);
    await user.click(screen.getByTestId("op-option-lt"));
    expect(onChange).toHaveBeenCalledWith("lt");
  });

  it("paints the highlighted option with the hover ground, never the accent", async () => {
    const user = userEvent.setup();
    mount();
    await user.click(screen.getByTestId("op"));
    const option = await screen.findByTestId("op-option-gte");
    expect(option.className).not.toMatch(/bg-accent(?![-])/);
    expect(option.className).toContain("bg-bg-hover");
  });
});
