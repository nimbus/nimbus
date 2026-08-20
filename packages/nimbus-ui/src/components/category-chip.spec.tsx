import { render, screen } from "@testing-library/react";
import axe from "axe-core";
import { describe, expect, it } from "vitest";

import { CategoryChip } from "./category-chip";
import { StateChip } from "./state-chip";

describe("CategoryChip", () => {
  it("renders the raw value and exposes it as a category", () => {
    const { container } = render(<CategoryChip value="query" />);
    expect(screen.getByText("query")).toBeInTheDocument();
    expect(container.querySelector("[data-category]")).toHaveAttribute(
      "data-category",
      "query",
    );
  });

  it("lowercases the category attribute but leaves the label alone", () => {
    const { container } = render(<CategoryChip value="HTTP" />);
    expect(screen.getByText("HTTP")).toBeInTheDocument();
    expect(container.querySelector("[data-category]")).toHaveAttribute(
      "data-category",
      "http",
    );
  });

  it.each([
    "query",
    "mutation",
    "action",
    "http",
    "scheduled",
    "cron",
  ])("never draws the unknown ? glyph for the function kind %s", (kind) => {
    const { container } = render(<CategoryChip value={kind} />);
    // The whole point of the component: `StateChip` resolves these to
    // `unknown` and prints a literal `?`, which is what put
    // "? QUERY 3  ? MUTATION 3" on the developer overview.
    expect(container.querySelector("[data-state]")).toBeNull();
    expect(container.textContent).not.toContain("?");
  });

  it("is a filled pill, not a labeled dot (DESIGN.md categorical badges)", () => {
    const { container } = render(<CategoryChip value="convex" />);
    const chip = container.querySelector("[data-category]");
    expect(chip?.className).toContain("bg-surface-2");
    expect(chip?.className).toContain("font-mono");
    expect(chip?.className).toContain("text-xs");
    // No dot: a category says what a thing is, not how it is doing.
    expect(container.querySelector("[aria-hidden=true]")).toBeNull();
  });

  it("shows the state chip's ? for the same value, proving the two vocabularies differ", () => {
    const { container } = render(<StateChip state="query" />);
    expect(container.querySelector("[data-state]")).toHaveAttribute(
      "data-glyph",
      "question",
    );
  });

  it("falls back to a readable label for an empty value", () => {
    render(<CategoryChip value={null} />);
    expect(screen.getByText("unknown")).toBeInTheDocument();
  });

  it("has no axe-core a11y violations", async () => {
    const { container } = render(
      <div>
        {["query", "mutation", "action", "http"].map((k) => (
          <CategoryChip key={k} value={k} />
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
