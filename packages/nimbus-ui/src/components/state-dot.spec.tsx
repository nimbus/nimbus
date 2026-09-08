import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import {
  type ConnState,
  resolveStateKind,
  StateDot,
  type StateKind,
  statePalette,
} from "./state-dot";

describe("resolveStateKind", () => {
  it("normalizes case, dashes, underscores, and spaces", () => {
    expect(resolveStateKind("RUNNING")).toBe("running");
    expect(resolveStateKind("not_ready")).toBe("notready");
    expect(resolveStateKind("Not Ready")).toBe("notready");
    expect(resolveStateKind("not-ready")).toBe("notready");
  });

  it("treats every err… word as an error", () => {
    expect(resolveStateKind("erroring")).toBe("error");
    expect(resolveStateKind("Errored")).toBe("error");
  });

  it("treats a non-fault log level as at rest", () => {
    expect(resolveStateKind("info")).toBe("idle");
    expect(resolveStateKind("debug")).toBe("idle");
  });

  it("maps a word the palette does not know, or nothing, to unknown", () => {
    expect(resolveStateKind("quantum")).toBe("unknown");
    expect(resolveStateKind(null)).toBe("unknown");
    expect(resolveStateKind("")).toBe("unknown");
  });

  it("never resolves a server vocabulary to the question glyph", () => {
    const vocab = [
      "ready",
      "running",
      "starting",
      "stopping",
      "stopped",
      "failed",
      "degraded",
      "pending",
      "completed",
      "queued",
      "paused",
      "offline",
    ];
    for (const word of vocab) {
      expect(statePalette[resolveStateKind(word)].glyph).not.toBe("question");
    }
  });
});

describe("statePalette", () => {
  it("never paints the accent: one accent, four jobs", () => {
    for (const style of Object.values(statePalette)) {
      expect(style.token).not.toBe("--accent");
    }
  });

  it("keeps running and degraded apart", () => {
    expect(statePalette.running.token).not.toBe(statePalette.degraded.token);
    expect(statePalette.running.glyph).toBe("pulsing");
    expect(statePalette.degraded.glyph).toBe("solid");
  });

  it("gives the pulse to running alone", () => {
    const pulsing = (Object.keys(statePalette) as StateKind[]).filter(
      (kind) => statePalette[kind].glyph === "pulsing",
    );
    expect(pulsing).toEqual(["running"]);
  });
});

describe("StateDot", () => {
  it("names the state for a screen reader", () => {
    render(<StateDot state="connected" />);
    expect(screen.getByRole("img")).toHaveAccessibleName("Connected");
  });

  it("takes an explicit label over the raw state", () => {
    render(<StateDot state="notready" label="Not ready" />);
    expect(screen.getByRole("img")).toHaveAccessibleName("Not ready");
  });

  it("stamps the resolved kind and glyph", () => {
    render(<StateDot state="Not_Ready" />);
    const dot = screen.getByRole("img");
    expect(dot).toHaveAttribute("data-state", "notready");
    expect(dot).toHaveAttribute("data-glyph", "solid");
  });

  it("draws the question glyph for a word it does not know", () => {
    render(<StateDot state="quantum" />);
    const dot = screen.getByRole("img");
    expect(dot).toHaveAttribute("data-glyph", "question");
    expect(dot).toHaveTextContent("?");
  });

  it("pulses for running only, and respects reduced motion", () => {
    const { unmount } = render(<StateDot state="running" />);
    expect(screen.getByRole("img").className).toMatch(/animate-pulse/);
    expect(screen.getByRole("img").className).toMatch(
      /motion-reduce:animate-none/,
    );
    unmount();
    render(<StateDot state="degraded" />);
    expect(screen.getByRole("img").className).not.toMatch(/animate-/);
  });

  it("never animates a connection state: the status bar is always on screen", () => {
    const states: ConnState[] = ["connected", "reconnecting", "offline"];
    for (const state of states) {
      const { container, unmount } = render(<StateDot state={state} />);
      expect(container.innerHTML).not.toMatch(/animate-/);
      unmount();
    }
  });

  it("takes its colour from the shared palette, not a private copy", () => {
    const expected: Array<[ConnState, StateKind, string]> = [
      ["connected", "connected", "--success"],
      ["reconnecting", "reconnecting", "--warning"],
      ["offline", "offline", "--error"],
    ];
    for (const [state, kind, token] of expected) {
      const { container, unmount } = render(<StateDot state={state} />);
      const dot = container.querySelector("[data-state]") as HTMLElement;
      expect(dot.dataset.state).toBe(kind);
      expect(dot.style.background).toBe(`var(${token})`);
      expect(statePalette[kind].token).toBe(token);
      unmount();
    }
  });

  it("draws a transition as a half glyph and rest as an outline", () => {
    const { unmount } = render(<StateDot state="starting" />);
    let dot = screen.getByRole("img");
    expect(dot).toHaveAttribute("data-glyph", "half");
    expect(dot.style.background).toContain("conic-gradient");
    unmount();
    render(<StateDot state="stopped" />);
    dot = screen.getByRole("img");
    expect(dot).toHaveAttribute("data-glyph", "outline");
    expect(dot.style.boxShadow).toContain("var(--text-3)");
  });
});
