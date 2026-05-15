import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { StateDot } from "./state-dot";

describe("StateDot", () => {
  it("renders an accessible label for connected", () => {
    render(<StateDot state="connected" />);
    expect(screen.getByRole("img")).toHaveAccessibleName("Connected");
  });

  it("pulses in the reconnecting state", () => {
    render(<StateDot state="reconnecting" />);
    expect(screen.getByRole("img").className).toMatch(/animate-pulse/);
  });

  it("uses the danger palette label for offline", () => {
    render(<StateDot state="offline" />);
    expect(screen.getByRole("img")).toHaveAccessibleName("Offline");
  });
});
