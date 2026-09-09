import { fireEvent, render, screen, within } from "@testing-library/react";
import type { ReactNode } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const { navigateMock, useQueryMock } = vi.hoisted(() => ({
  navigateMock: vi.fn(),
  useQueryMock: vi.fn(),
}));

vi.mock("@tanstack/react-router", () => ({
  createFileRoute: () => (config: Record<string, unknown>) => config,
  useNavigate: () => navigateMock,
  useSearch: () => ({}),
  useRouter: () => ({
    buildLocation: () => ({ href: "#" }),
    navigate: navigateMock,
  }),
  Link: ({
    to,
    children,
    "data-testid": testId,
    className,
  }: {
    to?: string;
    children?: ReactNode;
    "data-testid"?: string;
    className?: string;
  }) => (
    <a href={to ?? "#"} data-testid={testId} className={className}>
      {children}
    </a>
  ),
}));
vi.mock("@nimbus/nimbus/react", () => ({
  useQuery: (..._args: unknown[]) => useQueryMock(),
}));
vi.mock("../../shell/sub-panel", () => ({
  useContributeSubPanel: () => undefined,
  useSubPanelSearch: () => "",
}));

import { RunsTab } from "./compute_.$function";

const fn = { _id: "functions:1", path: "messages:list" };

const RUNS = [
  { _id: "runs:1", status: "ok", durationMs: 4, startedAt: 1_700_000_000_000 },
  {
    _id: "runs:2",
    status: "error",
    durationMs: 1200,
    startedAt: 1_700_000_001_000,
  },
];

beforeEach(() => {
  navigateMock.mockReset();
});

describe("RunsTab", () => {
  it("shows skeleton rows while the runs are in flight", () => {
    useQueryMock.mockReturnValue(undefined);
    render(<RunsTab fn={fn} />);

    expect(screen.getAllByTestId("skeleton-row")).toHaveLength(8);
    expect(screen.getByRole("status").textContent).toBe("Loading runs…");
  });

  it("shows the empty state, not skeletons, once an empty result lands", () => {
    useQueryMock.mockReturnValue([]);
    render(<RunsTab fn={fn} />);

    expect(screen.getByTestId("function-tab-runs-empty")).toBeTruthy();
    expect(screen.getByText("No runs yet")).toBeTruthy();
    expect(screen.queryAllByTestId("skeleton-row")).toHaveLength(0);
  });

  it("renders the runs as a DataTable with a state pill and a mono duration", () => {
    useQueryMock.mockReturnValue(RUNS);
    render(<RunsTab fn={fn} />);

    const table = screen.getByTestId("function-tab-runs");
    expect(table.getAttribute("role")).toBe("table");
    expect(table.getAttribute("aria-label")).toBe("Runs of messages:list");
    const rows = within(table).getAllByRole("row");
    // The header row plus one row per run.
    expect(rows).toHaveLength(3);

    const first = rows[1];
    const pill = first.querySelector('[data-slot="pill"]');
    expect(pill?.getAttribute("data-state")).toBe("ok");
    expect(first.textContent).toContain("4ms");
    expect(
      within(first).getByTestId("function-tab-runs-link-runs:1").className,
    ).toContain("font-mono");
  });

  it("opens the run when its row is activated", () => {
    useQueryMock.mockReturnValue(RUNS);
    render(<RunsTab fn={fn} />);

    const rows = within(screen.getByTestId("function-tab-runs")).getAllByRole(
      "row",
    );
    fireEvent.click(rows[2]);
    expect(navigateMock).toHaveBeenCalledWith({
      to: "/developer/compute/runs/$runId",
      params: { runId: "runs:2" },
    });
  });
});
