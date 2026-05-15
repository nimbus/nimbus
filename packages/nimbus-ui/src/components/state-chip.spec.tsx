import { render, screen } from "@testing-library/react";
import axe from "axe-core";
import { describe, expect, it } from "vitest";

import { StateChip } from "./state-chip";

describe("StateChip", () => {
  it("renders the raw label when state is known", () => {
    render(<StateChip state="running" />);
    const chip = screen.getByText("running");
    expect(chip).toBeInTheDocument();
    expect(chip).toHaveAttribute("data-state", "running");
  });

  it("normalizes case before lookup", () => {
    render(<StateChip state="RUNNING" />);
    expect(screen.getByText("RUNNING")).toHaveAttribute(
      "data-state",
      "running",
    );
  });

  it("maps error-prefixed values to the error palette", () => {
    render(<StateChip state="erroring" />);
    expect(screen.getByText("erroring")).toHaveAttribute("data-state", "error");
  });

  it("maps log levels (info/debug/trace) to idle", () => {
    render(<StateChip state="info" />);
    expect(screen.getByText("info")).toHaveAttribute("data-state", "idle");
  });

  it("falls back to unknown for unrecognized states", () => {
    render(<StateChip state="quantum" />);
    expect(screen.getByText("quantum")).toHaveAttribute(
      "data-state",
      "unknown",
    );
  });

  it("renders an em-dash for null state and marks it unknown", () => {
    render(<StateChip state={null} />);
    expect(screen.getByText("—")).toHaveAttribute("data-state", "unknown");
  });

  it("hides the dot when showDot is false", () => {
    const { container } = render(<StateChip state="running" showDot={false} />);
    expect(container.querySelector("[aria-hidden=true]")).toBeNull();
  });

  it("has no axe-core a11y violations across every state tone", async () => {
    const states = [
      "ready",
      "starting",
      "draining",
      "stopped",
      "error",
      "warning",
      "stale",
      "unknown",
    ];
    const { container } = render(
      <div>
        {states.map((s) => (
          <StateChip key={s} state={s} />
        ))}
      </div>,
    );
    const results = await axe.run(container, {
      runOnly: { type: "tag", values: ["wcag2a", "wcag2aa"] },
    });
    expect(
      results.violations.filter(
        (v) => v.impact === "critical" || v.impact === "serious",
      ),
    ).toEqual([]);
  });
});
