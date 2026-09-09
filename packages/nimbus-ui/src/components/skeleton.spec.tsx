import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import {
  CardGridSkeleton,
  CardSkeleton,
  DetailSkeleton,
  LoadingStatus,
  Skeleton,
  StatSkeleton,
  TableSkeleton,
} from "./skeleton";

describe("Skeleton", () => {
  it("is hidden from assistive tech and shimmers unless motion is reduced", () => {
    const { container } = render(<Skeleton className="h-3 w-10" />);
    const bar = container.firstElementChild as HTMLElement;
    expect(bar).toHaveAttribute("aria-hidden", "true");
    expect(bar.className).toContain("animate-shimmer");
    expect(bar.className).toContain("motion-reduce:animate-none");
  });
});

describe("LoadingStatus", () => {
  it("announces one polite busy status with a readable label", () => {
    render(
      <LoadingStatus label="Loading machines" testid="loading">
        <Skeleton />
      </LoadingStatus>,
    );
    const status = screen.getByRole("status");
    expect(status).toHaveAttribute("aria-live", "polite");
    expect(status).toHaveAttribute("aria-busy", "true");
    expect(status).toHaveTextContent("Loading machines");
    expect(screen.getByTestId("loading")).toBe(status);
  });
});

describe("shaped skeletons", () => {
  it("TableSkeleton draws the header and the asked rows", () => {
    render(<TableSkeleton rows={3} columns={2} testid="table" />);
    expect(screen.getAllByTestId("skeleton-row")).toHaveLength(3);
    expect(screen.getByRole("status")).toHaveTextContent("Loading");
  });

  it("each shape is one status region", () => {
    const shapes = [
      <CardSkeleton key="card" lines={2} />,
      <CardGridSkeleton key="grid" count={3} />,
      <DetailSkeleton key="detail" />,
      <StatSkeleton key="stat" count={2} />,
    ];
    for (const shape of shapes) {
      const { unmount } = render(shape);
      expect(screen.getAllByRole("status")).toHaveLength(1);
      unmount();
    }
  });
});
