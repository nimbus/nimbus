import { fireEvent, render, screen, within } from "@testing-library/react";
import type { ReactNode } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const { useQueryMock } = vi.hoisted(() => ({ useQueryMock: vi.fn() }));

vi.mock("@tanstack/react-router", () => ({
  useNavigate: () => vi.fn(),
  Link: ({
    to,
    children,
    "data-testid": testId,
    className,
  }: {
    to: string;
    children: ReactNode;
    "data-testid"?: string;
    className?: string;
  }) => (
    <a href={to} data-testid={testId} className={className}>
      {children}
    </a>
  ),
}));

vi.mock("@nimbus/nimbus/react", () => ({
  useQuery: (...args: unknown[]) => useQueryMock(...args),
  useNimbus: () => ({ url: "http://nimbus.example:9000/convex/_nimbus" }),
}));

vi.mock("../../../hooks/use-tenant-list", () => ({
  useTenantList: () => ({
    kind: "loaded",
    tenants: [{ id: "acme" }, { id: "beta" }],
    reload: () => {},
  }),
}));

import type { ObservabilityTabProps } from "./-facets";
import { TracesTab } from "./-traces";
import type { ObservabilitySearch, RunDoc } from "./-types";

const NOW = 1_700_000_000_000;

// A failed mutation: its own span, one ok read, and the insert that threw.
const FAILED_RUN: RunDoc = {
  _id: "run-1",
  functionPath: "messages:send",
  kind: "mutation",
  status: "error",
  durationMs: 1200,
  startedAt: NOW + 3_000,
  error: { message: "commit rejected", class: "function_thrown" },
  fingerprint: "0123456789abcdef",
  spans: [
    {
      name: "messages:send",
      kind: "function",
      parent: null,
      startMs: 0,
      durationMs: 1200,
      status: "error",
    },
    {
      name: "convex.ctx.db.get",
      kind: "db",
      parent: 0,
      startMs: 10,
      durationMs: 4,
      status: "ok",
    },
    {
      name: "convex.ctx.db.insert",
      kind: "db",
      parent: 0,
      startMs: 40,
      durationMs: 12,
      status: "error",
    },
  ],
};

// A run that predates span recording.
const BARE_RUN: RunDoc = {
  _id: "run-2",
  functionPath: "messages:list",
  kind: "query",
  status: "ok",
  durationMs: 40,
  startedAt: NOW + 2_000,
};

function renderTab(
  search: ObservabilitySearch,
  overrides: Partial<ObservabilityTabProps> = {},
) {
  const props: ObservabilityTabProps = {
    search,
    tenantId: "acme",
    allowAllTenants: false,
    setSearch: vi.fn(),
    setSearchAction: vi.fn(),
    ...overrides,
  };
  render(<TracesTab {...props} />);
  return props;
}

beforeEach(() => {
  useQueryMock.mockReset();
});

describe("TracesTab list", () => {
  it("reads the same run list as the Runs tab, scoped to the tenant and facets", () => {
    useQueryMock.mockReturnValue([FAILED_RUN, BARE_RUN]);
    renderTab({ tab: "traces", status: "error" });

    expect(useQueryMock.mock.calls[0]?.[1]).toEqual({
      tenantId: "acme",
      bundleId: null,
      functionPath: null,
      status: "error",
      fingerprint: null,
      limit: 200,
    });
    const table = screen.getByTestId("observability-traces-table");
    expect(
      within(table).getByTestId("observability-trace-row-run-1"),
    ).toBeInTheDocument();
    expect(
      within(table).getByTestId("observability-trace-row-run-2"),
    ).toBeInTheDocument();
  });

  it("counts a run's spans in the list so a bare run is visible before it is picked", () => {
    useQueryMock.mockReturnValue([FAILED_RUN, BARE_RUN]);
    renderTab({ tab: "traces" });

    expect(
      screen.getByTestId("observability-trace-row-run-1"),
    ).toHaveTextContent("3");
    expect(
      screen.getByTestId("observability-trace-row-run-2"),
    ).toHaveTextContent("0");
  });

  it("names the picked run in the address, as the Runs tab does", () => {
    useQueryMock.mockReturnValue([FAILED_RUN, BARE_RUN]);
    const props = renderTab({ tab: "traces" });

    fireEvent.click(screen.getByTestId("observability-trace-row-run-1"));
    expect(props.setSearchAction).toHaveBeenCalledWith({ run: "run-1" });
  });

  it("asks the reader to pick a run before it draws anything", () => {
    useQueryMock.mockReturnValue([FAILED_RUN]);
    renderTab({ tab: "traces" });

    expect(
      screen.getByTestId("observability-trace-empty-title"),
    ).toHaveTextContent("Pick a run to read its trace");
    expect(screen.queryByTestId("observability-trace-waterfall")).toBeNull();
  });

  it("gives the empty tab a title and a body, and blames filters only when some are set", () => {
    useQueryMock.mockReturnValue([]);
    renderTab({ tab: "traces" });
    expect(
      screen.getByTestId("observability-traces-empty-title"),
    ).toHaveTextContent("No traces yet");
    expect(screen.queryByTestId("observability-traces-empty-cta")).toBeNull();
  });

  it("offers to clear the filters when they empty the list", () => {
    useQueryMock.mockReturnValue([]);
    const props = renderTab({ tab: "traces", functionPath: "nope:none" });

    fireEvent.click(screen.getByTestId("observability-traces-empty-cta"));
    expect(props.setSearchAction).toHaveBeenCalledWith({
      status: undefined,
      functionPath: undefined,
      fingerprint: undefined,
    });
  });
});

// The waterfall is the point of the tab: one time axis, the function's own
// span as the bar, then each host call nested under its parent with the
// error glyph on any span that failed.
describe("TracesTab waterfall", () => {
  it("draws the picked run's spans nested under the run bar", () => {
    useQueryMock.mockReturnValue([FAILED_RUN, BARE_RUN]);
    renderTab({ tab: "traces", run: "run-1" });

    const waterfall = screen.getByTestId("observability-trace-waterfall");
    expect(waterfall).toHaveTextContent("3 spans");
    const bar = within(screen.getByTestId("observability-trace-waterfall-bar"));
    expect(bar.getByRole("img", { name: "error" })).toHaveTextContent("✗");

    const read = screen.getByTestId("observability-trace-waterfall-span-1");
    expect(read).toHaveTextContent("convex.ctx.db.get");
    expect(within(read).queryByRole("img")).toBeNull();

    const insert = screen.getByTestId("observability-trace-waterfall-span-2");
    expect(
      within(insert).getByRole("img", { name: "error" }),
    ).toHaveTextContent("✗");
    expect(insert).toHaveAttribute("data-depth", "0");
  });

  it("names the run above the waterfall and links to its page", () => {
    useQueryMock.mockReturnValue([FAILED_RUN]);
    renderTab({ tab: "traces", run: "run-1" });

    const pane = screen.getByTestId("observability-trace-pane");
    expect(pane).toHaveTextContent("messages:send");
    expect(screen.getByTestId("observability-trace-open-run")).toHaveAttribute(
      "href",
      "/developer/compute/runs/$runId",
    );
  });

  it("says a run recorded no spans instead of drawing an empty chart", () => {
    useQueryMock.mockReturnValue([FAILED_RUN, BARE_RUN]);
    renderTab({ tab: "traces", run: "run-2" });

    expect(
      screen.getByTestId("observability-trace-waterfall-empty"),
    ).toHaveTextContent("No spans were recorded for this run.");
  });

  it("says when the named run is not in the page rather than showing nothing", () => {
    useQueryMock.mockReturnValue([FAILED_RUN]);
    renderTab({ tab: "traces", run: "run-gone" });

    expect(
      screen.getByTestId("observability-trace-missing-title"),
    ).toHaveTextContent("This run is not in the current page");
    expect(
      screen.getByTestId("observability-trace-missing-open"),
    ).toHaveAttribute("href", "/developer/compute/runs/$runId");
  });
});
