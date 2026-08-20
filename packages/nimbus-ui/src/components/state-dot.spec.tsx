import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { type StateKind, statePalette } from "./state-chip";
import { type ConnState, StateDot } from "./state-dot";

describe("StateDot", () => {
  it("renders an accessible label for connected", () => {
    render(<StateDot state="connected" />);
    expect(screen.getByRole("img")).toHaveAccessibleName("Connected");
  });

  it("renders a solid dot for reconnecting (DESIGN.md gives the pulse to running alone)", () => {
    render(<StateDot state="reconnecting" />);
    const dot = screen.getByRole("img");
    expect(dot.className).not.toMatch(/animate-pulse/);
    expect(dot).toHaveAttribute("data-state", "reconnecting");
  });

  it("never animates: the status bar and overlay are always on screen", () => {
    const states: ConnState[] = ["connected", "reconnecting", "offline"];
    for (const state of states) {
      const { container, unmount } = render(<StateDot state={state} />);
      expect(container.innerHTML).not.toMatch(/animate-/);
      unmount();
    }
  });

  it("uses the danger palette label for offline", () => {
    render(<StateDot state="offline" />);
    expect(screen.getByRole("img")).toHaveAccessibleName("Offline");
  });

  it("takes its colour from the shared state palette, not a private copy", () => {
    const expected: Array<[ConnState, StateKind, string]> = [
      ["connected", "connected", "--nimbus-success"],
      ["reconnecting", "reconnecting", "--nimbus-warning"],
      ["offline", "offline", "--nimbus-danger"],
    ];
    for (const [state, kind, token] of expected) {
      const { container, unmount } = render(<StateDot state={state} />);
      const dot = container.querySelector("[data-state]") as HTMLElement;
      expect(dot.dataset.state).toBe(kind);
      expect(dot.style.background).toBe(`var(${token})`);
      // The dot and the chip must resolve the same state to the same token.
      expect(statePalette[kind].token).toBe(token);
      unmount();
    }
  });
});
