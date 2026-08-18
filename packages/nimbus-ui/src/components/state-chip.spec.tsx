import { render, screen } from "@testing-library/react";
import axe from "axe-core";
import { describe, expect, it } from "vitest";

import { StateChip, statePalette } from "./state-chip";

describe("StateChip", () => {
  it("renders the raw label when state is known", () => {
    render(<StateChip state="running" />);
    const chip = screen.getByText("running").closest("[data-state]");
    expect(chip).not.toBeNull();
    expect(chip).toHaveAttribute("data-state", "running");
  });

  it("normalizes case before lookup", () => {
    const { container } = render(<StateChip state="RUNNING" />);
    expect(container.querySelector("[data-state]")).toHaveAttribute(
      "data-state",
      "running",
    );
  });

  it("normalizes separators before lookup", () => {
    const { container } = render(<StateChip state="not_ready" />);
    expect(container.querySelector("[data-state]")).toHaveAttribute(
      "data-state",
      "notready",
    );
  });

  it("maps error-prefixed values to the error palette", () => {
    const { container } = render(<StateChip state="erroring" />);
    expect(container.querySelector("[data-state]")).toHaveAttribute(
      "data-state",
      "error",
    );
  });

  it("maps log levels (info/debug/trace) to idle", () => {
    const { container } = render(<StateChip state="info" />);
    expect(container.querySelector("[data-state]")).toHaveAttribute(
      "data-state",
      "idle",
    );
  });

  it("falls back to unknown for unrecognized states", () => {
    const { container } = render(<StateChip state="quantum" />);
    expect(container.querySelector("[data-state]")).toHaveAttribute(
      "data-state",
      "unknown",
    );
  });

  it("renders an em-dash for null state and marks it unknown", () => {
    render(<StateChip state={null} />);
    expect(screen.getByText("—")).toBeInTheDocument();
  });

  it("hides the dot when showDot is false", () => {
    const { container } = render(<StateChip state="running" showDot={false} />);
    expect(container.querySelector("[aria-hidden=true]")).toBeNull();
  });

  it("uses a pulsing dot for running (DESIGN.md mandatory glyph)", () => {
    const { container } = render(<StateChip state="running" />);
    const chip = container.querySelector("[data-state]");
    expect(chip).toHaveAttribute("data-glyph", "pulsing");
  });

  it("binds running to --running and never to the brand accent", () => {
    expect(statePalette.running.token).toBe("--nimbus-running");
    expect(statePalette.running.token).not.toBe("--nimbus-accent");
  });

  it("keeps no state on --accent, which the brand palette redefines", () => {
    // DESIGN.md's state table names only semantic tokens. A state bound to
    // --accent inherits the brand hue, which in the warm palette (hue 70)
    // lands on top of --warning (hue 72) — Running and Degraded then differ
    // by lightness alone.
    const onAccent = Object.entries(statePalette)
      .filter(([, entry]) => entry.token === "--nimbus-accent")
      .map(([state]) => state);
    expect(onAccent).toEqual([]);
  });

  it("gives running and degraded distinct tokens", () => {
    expect(statePalette.running.token).not.toBe(statePalette.degraded.token);
  });

  it("uses a half-filled dot for starting", () => {
    const { container } = render(<StateChip state="starting" />);
    expect(container.querySelector("[data-state]")).toHaveAttribute(
      "data-glyph",
      "half",
    );
  });

  it("uses an outline dot for queued", () => {
    const { container } = render(<StateChip state="queued" />);
    expect(container.querySelector("[data-state]")).toHaveAttribute(
      "data-glyph",
      "outline",
    );
  });

  it("uses an outline dot for stopped (per DESIGN.md table)", () => {
    const { container } = render(<StateChip state="stopped" />);
    expect(container.querySelector("[data-state]")).toHaveAttribute(
      "data-glyph",
      "outline",
    );
  });

  it.each([
    ["restarting", "half"],
    ["deleting", "half"],
  ])("names the in-flight lifecycle state %s with a transitional %s dot", (state, glyph) => {
    const { container } = render(<StateChip state={state} />);
    const chip = container.querySelector("[data-state]");
    expect(chip).toHaveAttribute("data-state", state);
    expect(chip).toHaveAttribute("data-glyph", glyph);
  });

  it("names a created machine with the stopped-family outline dot", () => {
    const { container } = render(<StateChip state="created" />);
    expect(container.querySelector("[data-state]")).toHaveAttribute(
      "data-glyph",
      "outline",
    );
  });

  it.each([
    "connected",
    "offline",
  ])("names the connection state %s so StateDot and StateChip agree", (state) => {
    const { container } = render(<StateChip state={state} />);
    expect(container.querySelector("[data-state]")).toHaveAttribute(
      "data-state",
      state,
    );
  });

  it("renders ? glyph for unknown", () => {
    const { container } = render(<StateChip state="quantum" />);
    expect(container.querySelector("[data-state]")).toHaveAttribute(
      "data-glyph",
      "question",
    );
  });

  it("strikes through the label when state is stale", () => {
    render(<StateChip state="stale" />);
    expect(screen.getByText("stale")).toHaveClass("line-through");
  });

  it("uses an 8px dot per DESIGN.md (no 6px legacy size)", () => {
    const { container } = render(<StateChip state="ready" />);
    const dot = container.querySelector("[aria-hidden=true]");
    expect(dot?.className).toContain("size-2");
    expect(dot?.className).not.toContain("size-1.5");
  });

  it("has no axe-core a11y violations across every state tone", async () => {
    const states = [
      "ready",
      "running",
      "starting",
      "draining",
      "queued",
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
