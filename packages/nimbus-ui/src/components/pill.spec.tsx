import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { Pill, type PillTone, StatePill, toneOfKind } from "./pill";

describe("Pill", () => {
  it("renders its children in one recipe with the tone stamped", () => {
    render(<Pill tone="success">Completed</Pill>);
    const pill = screen.getByText("Completed");
    expect(pill).toHaveAttribute("data-slot", "pill");
    expect(pill).toHaveAttribute("data-tone", "success");
    expect(pill).toHaveClass("rounded-full", "text-xs", "font-medium");
  });

  it("paints each tone with its tint and text, never the accent", () => {
    const tones: PillTone[] = [
      "success",
      "warning",
      "error",
      "info",
      "neutral",
    ];
    for (const tone of tones) {
      const { unmount } = render(<Pill tone={tone}>{tone}</Pill>);
      const pill = screen.getByText(tone);
      expect(pill.className).not.toMatch(/accent/);
      if (tone !== "neutral")
        expect(pill.className).toContain(`bg-${tone}-tint`);
      unmount();
    }
  });

  it("passes through extra attributes", () => {
    render(
      <Pill tone="neutral" data-testid="kind" data-category="query">
        query
      </Pill>,
    );
    expect(screen.getByTestId("kind")).toHaveAttribute(
      "data-category",
      "query",
    );
  });
});

describe("toneOfKind", () => {
  it("derives the tone from the shared palette token", () => {
    expect(toneOfKind("completed")).toBe("success");
    expect(toneOfKind("degraded")).toBe("warning");
    expect(toneOfKind("failed")).toBe("error");
    expect(toneOfKind("running")).toBe("info");
    expect(toneOfKind("stopped")).toBe("neutral");
    expect(toneOfKind("unknown")).toBe("neutral");
  });
});

describe("StatePill", () => {
  it("keeps the server's spelling as the label and stamps the resolved kind", () => {
    render(<StatePill state="RUNNING" />);
    const pill = screen.getByText("RUNNING");
    expect(pill).toHaveAttribute("data-state", "running");
    expect(pill).toHaveAttribute("data-tone", "info");
  });

  it("renders the dash for a missing state", () => {
    render(<StatePill state={null} data-testid="pill" />);
    const pill = screen.getByTestId("pill");
    expect(pill).toHaveTextContent("—");
    expect(pill).toHaveAttribute("data-state", "unknown");
  });

  it("strikes a stale state", () => {
    render(<StatePill state="stale" />);
    expect(screen.getByText("stale")).toHaveClass("line-through");
  });
});
