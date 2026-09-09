import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { Moon, Sun } from "lucide-react";
import { describe, expect, it, vi } from "vitest";

import { SegmentedControl } from "./segmented-control";

type Mode = "light" | "dark" | "system";

const OPTIONS = [
  { value: "light", label: "Light", icon: Sun, description: "Light theme" },
  { value: "dark", label: "Dark", icon: Moon },
  { value: "system", label: "System" },
] as const;

function mount(value: Mode = "light") {
  const onChange = vi.fn();
  render(
    <SegmentedControl<Mode>
      label="Theme"
      value={value}
      options={OPTIONS}
      onChange={onChange}
      testid="mode"
    />,
  );
  return onChange;
}

describe("SegmentedControl", () => {
  it("is a radio group of real radios, one per option, with testids", () => {
    mount();
    expect(
      screen.getByRole("radiogroup", { name: "Theme" }),
    ).toBeInTheDocument();
    const radios = screen.getAllByRole("radio");
    expect(radios).toHaveLength(3);
    expect(radios.map((r) => r.dataset.testid)).toEqual([
      "mode-light",
      "mode-dark",
      "mode-system",
    ]);
    expect(screen.getByRole("radio", { name: "Light" })).toHaveAttribute(
      "aria-description",
      "Light theme",
    );
  });

  it("marks the active option with aria-checked and data-active", () => {
    mount("dark");
    expect(screen.getByTestId("mode-dark")).toHaveAttribute(
      "aria-checked",
      "true",
    );
    expect(screen.getByTestId("mode-dark").dataset.active).toBe("true");
    expect(screen.getByTestId("mode-light")).toHaveAttribute(
      "aria-checked",
      "false",
    );
    expect(screen.getByTestId("mode-light").dataset.active).toBe("false");
  });

  it("calls onChange on click", async () => {
    const user = userEvent.setup();
    const onChange = mount();
    await user.click(screen.getByTestId("mode-system"));
    expect(onChange).toHaveBeenCalledWith("system");
  });

  it("moves with the arrow keys and wraps", async () => {
    const user = userEvent.setup();
    const onChange = mount("light");
    await user.tab();
    expect(screen.getByTestId("mode-light")).toHaveFocus();
    await user.keyboard("{ArrowRight}");
    expect(onChange).toHaveBeenLastCalledWith("dark");
    expect(screen.getByTestId("mode-dark")).toHaveFocus();
    await user.keyboard("{ArrowLeft}{ArrowLeft}");
    expect(onChange).toHaveBeenLastCalledWith("system");
    expect(screen.getByTestId("mode-system")).toHaveFocus();
  });

  it("keeps one tab stop for the whole group", async () => {
    const user = userEvent.setup();
    render(
      <>
        <SegmentedControl<Mode>
          label="Theme"
          value="dark"
          options={OPTIONS}
          onChange={() => {}}
          testid="mode"
        />
        <button type="button">after</button>
      </>,
    );
    await user.tab();
    expect(screen.getByTestId("mode-dark")).toHaveFocus();
    await user.tab();
    expect(screen.getByRole("button", { name: "after" })).toHaveFocus();
  });
});
