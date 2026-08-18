import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { LoadingState, SkeletonRows } from "./loading-state";

const HEAD = (
  <thead data-testid="skeleton-head">
    <tr>
      <th>Table</th>
      <th>Schema</th>
      <th>Rows</th>
    </tr>
  </thead>
);

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

describe("SkeletonRows", () => {
  it("renders the caller's thead so the header keeps its geometry", () => {
    render(
      <SkeletonRows
        columns={3}
        head={HEAD}
        label="Loading tables…"
        testid="sk"
      />,
    );
    const head = screen.getByTestId("skeleton-head");
    expect(head.tagName).toBe("THEAD");
    expect(head.closest("table")).not.toBeNull();
  });

  it("renders the requested row count with one cell per column", () => {
    const { container } = render(
      <SkeletonRows columns={4} head={HEAD} rows={5} label="Loading…" />,
    );
    const rows = container.querySelectorAll("tbody tr");
    expect(rows).toHaveLength(5);
    expect(rows[0].querySelectorAll("td")).toHaveLength(4);
  });

  it("defaults to 8 rows", () => {
    const { container } = render(
      <SkeletonRows columns={2} head={HEAD} label="Loading…" />,
    );
    expect(container.querySelectorAll("tbody tr")).toHaveLength(8);
  });

  it("hides the placeholder table and announces the label once", () => {
    const { container } = render(
      <SkeletonRows columns={3} head={HEAD} label="Loading tables…" />,
    );
    expect(container.querySelector("table")).toHaveAttribute("aria-hidden");
    const status = screen.getByRole("status");
    expect(status).toHaveTextContent("Loading tables…");
    expect(status).toHaveClass("sr-only");
  });

  it("pulses only when the user has not asked for reduced motion", () => {
    const { container } = render(
      <SkeletonRows columns={3} head={HEAD} label="Loading…" />,
    );
    const body = container.querySelector("tbody");
    expect(body).toHaveClass("animate-pulse");
    expect(body).toHaveClass("motion-reduce:animate-none");
  });

  it("sizes the cell content box so a row matches the loaded row", () => {
    const { container, rerender } = render(
      <SkeletonRows columns={2} head={HEAD} label="Loading…" />,
    );
    const box = () =>
      container.querySelector("tbody td > span") as HTMLElement | null;
    // The storage tables carry a ~22px inline control per row; the taller
    // machines table passes its own measurement instead.
    expect(box()).toHaveStyle({ height: "22px" });
    rerender(
      <SkeletonRows
        columns={2}
        head={HEAD}
        label="Loading…"
        rowContentHeight={34}
      />,
    );
    expect(box()).toHaveStyle({ height: "34px" });
  });

  it("uses the same dense cell padding and hairline as a loaded row", () => {
    const { container } = render(
      <SkeletonRows columns={3} head={HEAD} label="Loading…" />,
    );
    const row = container.querySelector("tbody tr");
    expect(row).toHaveClass("border-t", "border-app");
    expect(row?.querySelector("td")).toHaveClass(
      "px-3",
      "py-2",
      "align-middle",
    );
  });
});
