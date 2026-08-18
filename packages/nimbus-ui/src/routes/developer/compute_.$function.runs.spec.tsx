import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

vi.mock("@tanstack/react-router", () => ({
  createFileRoute: () => (config: Record<string, unknown>) => config,
  useNavigate: () => () => undefined,
  Link: ({
    to,
    children,
    "data-testid": testId,
  }: {
    to?: string;
    children: React.ReactNode;
    "data-testid"?: string;
  }) => (
    <a href={to ?? "#"} data-testid={testId}>
      {children}
    </a>
  ),
}));

const { useQueryMock } = vi.hoisted(() => ({ useQueryMock: vi.fn() }));
vi.mock("@nimbus/nimbus/react", () => ({
  useQuery: (..._args: unknown[]) => useQueryMock(),
}));
vi.mock("../../shell/sub-drawer", () => ({
  useContributeSubDrawer: () => undefined,
}));

import { RunsTab } from "./compute_.$function";

const fn = { path: "messages:list" } as Parameters<typeof RunsTab>[0]["fn"];

describe("RunsTab loading state", () => {
  it("keeps the table header mounted while runs are in flight", () => {
    useQueryMock.mockReturnValue(undefined);
    render(<RunsTab fn={fn} />);

    // The header is the point: swapping the whole table out is what made the
    // panel jump twice per load.
    expect(screen.getByText("Run ID")).toBeTruthy();
    expect(screen.getByText("Status")).toBeTruthy();
    expect(screen.getAllByTestId("skeleton-row")).toHaveLength(8);
    expect(screen.getByRole("status").textContent).toBe("Loading runs…");
  });

  it("renders the skeleton as its own table, never nested in one", () => {
    useQueryMock.mockReturnValue(undefined);
    const { container } = render(<RunsTab fn={fn} />);

    // `SkeletonRows` carries its own <table>. Nesting it inside another one
    // is invalid markup the browser silently repairs, so assert on structure.
    const tables = container.querySelectorAll("table");
    expect(tables).toHaveLength(1);
    expect(tables[0].querySelector("table")).toBeNull();
  });

  it("shows the empty state, not skeletons, once an empty result lands", () => {
    useQueryMock.mockReturnValue([]);
    render(<RunsTab fn={fn} />);

    expect(screen.getByText("No runs yet")).toBeTruthy();
    expect(screen.queryAllByTestId("skeleton-row")).toHaveLength(0);
  });
});
