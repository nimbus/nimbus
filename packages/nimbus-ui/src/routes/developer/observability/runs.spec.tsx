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

import { resolveStateKind } from "../../../components/state-dot";
import type { ObservabilityTabProps } from "./-facets";
import { RUN_STATUSES, RunsTab } from "./-runs";
import type { ObservabilitySearch, RunDoc } from "./-types";

const NOW = 1_700_000_000_000;

const RUNS: RunDoc[] = [
  {
    _id: "run-1",
    functionPath: "messages:send",
    kind: "mutation",
    status: "error",
    durationMs: 1200,
    startedAt: NOW + 3_000,
    error: { message: "commit rejected" },
  },
  {
    _id: "run-2",
    functionPath: "messages:list",
    kind: "query",
    status: "ok",
    durationMs: 40,
    startedAt: NOW + 2_000,
  },
];

function isRunsQuery(args: unknown): boolean {
  return typeof args === "object" && args !== null && "functionPath" in args;
}

// The runs read answers with `runs`; the sheet's correlated-events read
// answers with an empty list.
function answer(runs: RunDoc[] | undefined) {
  useQueryMock.mockImplementation((_ref: unknown, args: unknown) =>
    isRunsQuery(args) ? runs : [],
  );
}

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
  render(<RunsTab {...props} />);
  return props;
}

beforeEach(() => {
  useQueryMock.mockReset();
});

/**
 * The status dropdown is a closed list, so every option it offers is a promise
 * that rows exist behind it. `running` and `queued` were on that list and no
 * run has ever carried either: a row is written only after the invocation
 * returns, with `result.is_ok() ? "ok" : "error"`. Both options answered "No
 * runs" for every deployment that has ever existed, which reads as "your runs
 * are missing" rather than "this state does not occur here".
 */
describe("run status filter", () => {
  it("offers only the two values a run can carry", () => {
    expect([...RUN_STATUSES]).toEqual(["ok", "error"]);
  });

  it("names every offered value in the state palette", () => {
    for (const status of RUN_STATUSES) {
      expect(resolveStateKind(status)).toBe(status);
    }
  });
});

/**
 * A fresh install lands on this tab and its empty state was one 12px muted
 * line in a box. The same condition one nav entry away — Operator ->
 * Observability -> Runs — renders the full `EmptyState`, and DESIGN.md's
 * whole-tab scope calls for a mono title plus a two-line body plus a next
 * action. This tab is whole-tab scope and had none of it.
 */
describe("RunsTab read states", () => {
  it("distinguishes a read in flight from a settled empty result", () => {
    answer(undefined);
    renderTab({ tab: "runs" });

    const table = screen.getByTestId("observability-runs-table");
    expect(table).toHaveAttribute("aria-busy", "true");
    expect(
      screen.getAllByTestId("observability-runs-table-skeleton-row").length,
    ).toBeGreaterThan(0);
    expect(screen.queryByTestId("observability-runs-empty")).toBeNull();
  });

  it("gives the empty tab a title, a body, and a next action", () => {
    answer([]);
    renderTab({ tab: "runs" });

    const title = screen.getByTestId("observability-runs-empty-title");
    expect(title.tagName).toBe("H2");
    expect(title).toHaveTextContent("No runs yet");
    expect(
      screen.getByTestId("observability-runs-empty-body").textContent ?? "",
    ).not.toHaveLength(0);
    // The next action is a function call on this tenant, not another page.
    expect(
      screen.getByTestId("observability-runs-empty-snippet"),
    ).toHaveTextContent("nimbus run http://nimbus.example:9000 functions");
    expect(
      screen.getByTestId("observability-runs-empty-snippet"),
    ).toHaveTextContent("--tenant acme");
    expect(screen.queryByTestId("observability-runs-empty-cta")).toBeNull();
    expect(screen.queryByTestId("observability-runs-table")).toBeNull();
  });

  it("names the filters, and offers to clear them, only when some are set", () => {
    answer([]);
    const props = renderTab({ tab: "runs", status: "error" });

    expect(
      screen.getByTestId("observability-runs-empty-title"),
    ).toHaveTextContent("No runs match the current filters");
    const cta = screen.getByTestId("observability-runs-empty-cta");
    expect(cta).toHaveTextContent(/Clear filters/i);
    fireEvent.click(cta);
    expect(props.setSearchAction).toHaveBeenCalledWith({
      status: undefined,
      functionPath: undefined,
    });
  });

  it("does not blame filters for an unfiltered empty list", () => {
    answer([]);
    renderTab({ tab: "runs" });

    expect(
      screen.getByTestId("observability-runs-empty"),
    ).not.toHaveTextContent(/current filters/i);
  });
});

describe("RunsTab table", () => {
  it("scopes the read to the tenant and the facets", () => {
    answer(RUNS);
    renderTab({ tab: "runs", status: "error", functionPath: "messages:send" });

    const runsCall = useQueryMock.mock.calls.find(([, args]) =>
      isRunsQuery(args),
    );
    expect(runsCall?.[1]).toEqual({
      tenantId: "acme",
      bundleId: null,
      functionPath: "messages:send",
      status: "error",
      limit: 200,
    });
  });

  it("renders one row per run on the shared DataTable", () => {
    answer(RUNS);
    renderTab({ tab: "runs" });

    const table = screen.getByTestId("observability-runs-table");
    expect(table).toHaveAttribute("role", "table");
    expect(
      within(table)
        .getAllByRole("columnheader")
        .map((th) => th.textContent?.trim()),
    ).toEqual(["Started", "Function", "Status", "Kind", "Duration", "Run id"]);
    for (const run of RUNS) {
      const row = screen.getByTestId(`observability-run-row-${run._id}`);
      expect(row).toHaveTextContent(run.functionPath ?? "");
    }
  });

  it("opens the run sheet through a push navigation when a row activates", () => {
    answer(RUNS);
    const props = renderTab({ tab: "runs" });

    fireEvent.click(screen.getByTestId("observability-run-row-run-1"));
    expect(props.setSearchAction).toHaveBeenCalledWith({ run: "run-1" });
  });

  it("sends the honesty note's cross-adapter pointer to the Logs tab", () => {
    answer(RUNS);
    const props = renderTab({ tab: "runs" });

    fireEvent.click(
      screen.getByTestId("observability-adapter-honesty-events-link"),
    );
    expect(props.setSearchAction).toHaveBeenCalledWith({ tab: "logs" });
  });
});

describe("RunsTab detail sheet", () => {
  it("stays closed until the address names a run", () => {
    answer(RUNS);
    renderTab({ tab: "runs" });

    expect(screen.queryByTestId("observability-run-sheet")).toBeNull();
  });

  it("shows the named run's summary, error, and a hand-off to the Logs tab", () => {
    answer(RUNS);
    const props = renderTab({ tab: "runs", run: "run-1" });

    const sheet = screen.getByTestId("observability-run-sheet");
    expect(
      within(sheet).getByTestId("observability-run-sheet-head-status"),
    ).toHaveTextContent("error");
    expect(
      within(sheet).getByTestId("observability-run-sheet-function"),
    ).toHaveTextContent("messages:send");
    expect(
      within(sheet).getByTestId("observability-run-sheet-error"),
    ).toHaveTextContent("commit rejected");
    expect(
      within(sheet).getByTestId("observability-run-sheet-open-run"),
    ).toHaveAttribute("href", "/developer/compute/runs/$runId");

    fireEvent.click(
      within(sheet).getByTestId("observability-run-sheet-show-logs"),
    );
    expect(props.setSearchAction).toHaveBeenCalledWith({
      tab: "logs",
      correlationId: "run-1",
      run: undefined,
    });
  });

  it("reads the run's correlated lines under the tenant scope", () => {
    answer(RUNS);
    renderTab({ tab: "runs", run: "run-2" });

    const eventsCall = useQueryMock.mock.calls.find(
      ([, args]) => !isRunsQuery(args),
    );
    expect(eventsCall?.[1]).toEqual({
      correlationId: "run-2",
      tenantId: "acme",
      source: null,
      level: null,
      category: null,
      limit: 200,
    });
    expect(
      screen.getByTestId("observability-run-sheet-events-empty"),
    ).toBeInTheDocument();
  });

  it("says when the named run is not on the page and still offers the run page", () => {
    answer(RUNS);
    renderTab({ tab: "runs", run: "run-missing" });

    expect(
      screen.getByTestId("observability-run-sheet-missing"),
    ).toHaveTextContent(/not in the current page/i);
    expect(
      screen.getByTestId("observability-run-sheet-open-run"),
    ).toBeInTheDocument();
  });
});
