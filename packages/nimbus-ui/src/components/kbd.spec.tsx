import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { Kbd } from "./kbd";

describe("Kbd", () => {
  it("renders children inside a <kbd> element", () => {
    render(<Kbd>⌘K</Kbd>);
    const node = screen.getByText("⌘K");
    expect(node.tagName).toBe("KBD");
  });

  it("merges additional class names through cn", () => {
    render(<Kbd className="text-emerald-500">Esc</Kbd>);
    expect(screen.getByText("Esc").className).toMatch(/text-emerald-500/);
  });

  it("forwards HTML attributes", () => {
    render(<Kbd data-testid="kbd-meta">⌘</Kbd>);
    expect(screen.getByTestId("kbd-meta")).toBeInTheDocument();
  });
});
