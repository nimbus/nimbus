import { render, screen, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

vi.mock("@tanstack/react-router", () => ({
  useNavigate: () => vi.fn(),
  Link: ({
    to,
    children,
    "data-testid": testId,
    className,
  }: {
    to: string;
    children: React.ReactNode;
    "data-testid"?: string;
    className?: string;
  }) => (
    <a href={to} data-testid={testId} className={className}>
      {children}
    </a>
  ),
}));

const { useQueryMock } = vi.hoisted(() => ({ useQueryMock: vi.fn() }));

vi.mock("@nimbus/nimbus/react", () => ({
  useQuery: (..._args: unknown[]) => useQueryMock(),
}));

import { LogsTab } from "./logs";
import type { EventDoc } from "./types";

const events: EventDoc[] = [
  {
    _id: "evt-1",
    createdAt: 1_700_000_000_000,
    level: "error",
    source: "nimbus-engine::committer",
    category: "mutation",
    message: "commit rejected",
    correlationId: "run-abcdef0123456789",
  },
  {
    _id: "evt-2",
    createdAt: 1_700_000_001_000,
    level: "info",
    source:
      "nimbus-server::adapter::convex::websocket::subscription::dispatcher",
    category: "websocket",
    message:
      "a message long enough that intrinsic column sizing would move every other row's left edge",
  },
];

describe("LogsTab layout contract", () => {
  it("renders the stream as a fixed-column table so every row shares a left edge", () => {
    useQueryMock.mockReturnValue(events);
    render(<LogsTab search={{ tab: "logs" }} />);

    const table = screen.getByTestId("observability-log-table");
    // Fixed tracks are the whole point: intrinsic per-row sizing is what made
    // the columns walk left and right as messages changed length.
    expect(table.className).toContain("table-fixed");
    expect(
      within(table)
        .getAllByRole("columnheader")
        .map((th) => th.textContent?.trim()),
    ).toEqual(["Time", "Level", "Source", "Message", "Run"]);
    expect(table.querySelectorAll("colgroup col")).toHaveLength(5);

    for (const event of events) {
      const row = screen.getByTestId(`observability-log-row-${event._id}`);
      expect(row.tagName).toBe("TR");
      expect(within(row).getAllByRole("cell")).toHaveLength(5);
    }
  });

  it("keeps the whole action cluster inside a toolbar that degrades by wrapping", () => {
    useQueryMock.mockReturnValue(events);
    render(<LogsTab search={{ tab: "logs" }} />);

    const toolbar = screen.getByTestId("observability-log-filters");
    // jsdom has no layout, so the class is the proxy for the measured
    // behaviour: the toolbar's ancestor is `overflow-hidden`, so the bar has
    // to wrap. A non-compressible grid pushed these three controls past the
    // clipped edge with no scrollbar to reach them.
    expect(toolbar.className).toContain("flex-wrap");
    expect(toolbar.className).not.toContain("grid-cols");
    for (const testid of [
      "observability-log-follow",
      "observability-log-pause-on-error",
      "observability-filter-clear",
    ]) {
      expect(within(toolbar).getByTestId(testid)).toBeInTheDocument();
    }
  });

  it("bounds every filter input so the toolbar cannot be widened past its container", () => {
    useQueryMock.mockReturnValue(events);
    render(<LogsTab search={{ tab: "logs" }} />);

    for (const testid of [
      "observability-filter-category",
      "observability-filter-source",
      "observability-filter-correlation",
    ]) {
      const input = screen.getByTestId(testid);
      // A text input's intrinsic width comes from its `size` attribute
      // (~20ch), and `min-width: auto` pins a flex item there unless both an
      // explicit width and `min-w-0` override it.
      expect(input.className).toContain("w-[14ch]");
      expect(input.className).toContain("min-w-0");
    }
  });
});
