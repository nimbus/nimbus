import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { LoadingState } from "./loading-state";

describe("LoadingState", () => {
  it("renders the label text", () => {
    render(<LoadingState label="Loading tables…" testid="ls" />);
    expect(screen.getByTestId("ls")).toHaveTextContent("Loading tables…");
  });

  it("renders the label in font-mono per DESIGN.md", () => {
    render(<LoadingState label="Loading…" testid="ls" />);
    expect(screen.getByTestId("ls")).toHaveClass("font-mono");
  });

  it("omits the data-testid when none is given", () => {
    const { container } = render(<LoadingState label="Loading…" />);
    expect(container.querySelector("[data-testid]")).toBeNull();
  });
});
