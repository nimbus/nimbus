import {
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { HttpResponse, http } from "msw";
import { setupServer } from "msw/node";
import type { ReactNode } from "react";
import {
  afterAll,
  afterEach,
  beforeAll,
  beforeEach,
  describe,
  expect,
  it,
  vi,
} from "vitest";

const { navigateMock, useQueryMock } = vi.hoisted(() => ({
  navigateMock: vi.fn(),
  useQueryMock: vi.fn(),
}));

vi.mock("@tanstack/react-router", () => ({
  createFileRoute: () => (config: Record<string, unknown>) => config,
  useNavigate: () => navigateMock,
  Link: ({
    to,
    children,
    "data-testid": testId,
    "aria-current": current,
    className,
  }: {
    to: string;
    children: ReactNode;
    "data-testid"?: string;
    "aria-current"?: "page";
    className?: string;
  }) => (
    <a
      href={to}
      data-testid={testId}
      aria-current={current}
      className={className}
    >
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

import { useUiStore } from "../../../store/ui-store";
import { routeComponent } from "../../../test/route-internals";
import { Route } from "../observability";
import type { EventDoc, LogSearchPage, RunDoc } from "./-types";

// The search page is an HTTP read, not a subscription, so it goes through
// msw. Other reads on the page are the mocked useQuery above.
const server = setupServer();
beforeAll(() => server.listen({ onUnhandledRequest: "bypass" }));
afterEach(() => server.resetHandlers());
afterAll(() => server.close());

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
    durationMs: 4,
    startedAt: NOW + 2_000,
  },
  {
    _id: "run-3",
    functionPath: "agent:tick",
    kind: "action",
    status: "ok",
    durationMs: 80,
    startedAt: NOW + 1_000,
  },
];

const EVENTS: EventDoc[] = [
  {
    _id: "evt-1",
    createdAt: NOW + 3_100,
    level: "error",
    source: "nimbus-engine::committer",
    category: "mutation",
    message: "commit rejected",
    correlationId: "run-1",
  },
  {
    _id: "evt-2",
    createdAt: NOW + 500,
    level: "info",
    source:
      "nimbus-server::adapter::convex::websocket::subscription::dispatcher",
    category: "websocket",
    message:
      "a message long enough that intrinsic column sizing would move every other row's left edge",
  },
];

type QueryArgs = Record<string, unknown>;

// The page asks two questions of the server, runs and events, through the
// same hook. The mock tells them apart by the argument the runs query alone
// carries.
function isRunsQuery(args: unknown): boolean {
  return typeof args === "object" && args !== null && "functionPath" in args;
}

function answer(runs: RunDoc[] | undefined, events: EventDoc[] | undefined) {
  useQueryMock.mockImplementation((_ref: unknown, args: unknown) =>
    isRunsQuery(args) ? runs : events,
  );
}

function queryArgs(pick: "runs" | "events"): QueryArgs | undefined {
  const call = useQueryMock.mock.calls.find(([, args]) =>
    pick === "runs" ? isRunsQuery(args) : !isRunsQuery(args),
  );
  return call?.[1] as QueryArgs | undefined;
}

function renderPage(search: Record<string, unknown> = { tab: "logs" }) {
  const validateSearch = (
    Route as unknown as {
      validateSearch: (s: Record<string, unknown>) => Record<string, unknown>;
    }
  ).validateSearch;
  const resolved = validateSearch(search);
  (Route as unknown as { useSearch: () => Record<string, unknown> }).useSearch =
    () => resolved;
  const Component = routeComponent(Route);
  return render(<Component />);
}

function runGroups() {
  return document.querySelectorAll(
    '[data-testid^="observability-log-group-"][data-kind="run"]',
  );
}

beforeEach(() => {
  navigateMock.mockReset();
  useQueryMock.mockReset();
  useUiStore.setState({ activeTenant: "acme", lensOpen: false });
});

// DESIGN.md: Observability (Developer) defaults to the active tenant and is
// never cross-tenant. The stream used to read the `_nimbus` system tenant
// with no tenant in the query at all, so a run in the operator's own tenant
// never showed up under Logs.
describe("LogsTab tenant scope", () => {
  it("defaults the tenant facet to the active tenant and scopes both reads to it", () => {
    answer(RUNS, EVENTS);
    renderPage();

    expect(screen.getByTestId("observability-filter-tenant")).toHaveTextContent(
      "acme",
    );
    expect(queryArgs("events")?.tenantId).toBe("acme");
    expect(queryArgs("runs")?.tenantId).toBe("acme");
  });

  it("lets ?tenant= override the active tenant without touching the store", () => {
    answer(RUNS, EVENTS);
    renderPage({ tab: "logs", tenant: "beta" });

    expect(screen.getByTestId("observability-filter-tenant")).toHaveTextContent(
      "beta",
    );
    expect(queryArgs("events")?.tenantId).toBe("beta");
    expect(useUiStore.getState().activeTenant).toBe("acme");
  });

  it("opens the system tenant lens from the facet bar", () => {
    answer(RUNS, EVENTS);
    renderPage();

    fireEvent.click(screen.getByTestId("observability-open-lens"));
    expect(useUiStore.getState().lensOpen).toBe(true);
  });
});

// A function invocation writes a run row and, today, no event rows, so a
// flat event stream stays empty after the operator's own functions ran. The
// stream is grouped by run: each run is one group with its correlated lines
// beneath it, and lines that belong to no run sit under the server group.
describe("LogsTab run groups", () => {
  it("makes one log group per run: three runs, three groups", () => {
    answer(RUNS, EVENTS);
    renderPage();

    expect(runGroups()).toHaveLength(3);
    expect(
      [...runGroups()].map((el) => el.getAttribute("data-testid")),
    ).toEqual([
      "observability-log-group-run-1",
      "observability-log-group-run-2",
      "observability-log-group-run-3",
    ]);
  });

  it("attaches a line to its run by correlation id and parks the rest under the server group", () => {
    answer(RUNS, EVENTS);
    renderPage();

    const first = screen.getByTestId("observability-log-group-run-1");
    expect(
      within(first).getByTestId("observability-log-row-evt-1"),
    ).toBeInTheDocument();
    expect(
      within(first).queryByTestId("observability-log-row-evt-2"),
    ).toBeNull();

    const server = screen.getByTestId("observability-log-group-server");
    expect(server).toHaveAttribute("data-kind", "server");
    expect(
      within(server).getByTestId("observability-log-row-evt-2"),
    ).toBeInTheDocument();
  });

  it("says when a run recorded no lines instead of rendering a bare header", () => {
    answer(RUNS, EVENTS);
    renderPage();

    const quiet = screen.getByTestId("observability-log-group-run-2");
    expect(
      within(quiet).getByTestId("observability-log-group-empty-run-2"),
    ).toHaveTextContent(/no log lines/i);
  });

  it("heads each group with the run's state, function, and a link to the run page", () => {
    answer(RUNS, EVENTS);
    renderPage();

    const head = screen.getByTestId("observability-log-group-head-run-1");
    expect(head.querySelector('[data-slot="pill"]')).toHaveAttribute(
      "data-state",
      "error",
    );
    expect(head).toHaveTextContent("messages:send");
    expect(
      within(head).getByTestId("observability-log-group-open-run-1"),
    ).toBeInTheDocument();
  });

  it("narrows to one run's group when a correlation id is set", () => {
    answer(RUNS, EVENTS);
    renderPage({ tab: "logs", correlationId: "run-1" });

    expect(runGroups()).toHaveLength(1);
    expect(screen.queryByTestId("observability-log-group-server")).toBeNull();
  });

  it("drops runs with no matching line while a line filter is set", () => {
    answer(RUNS, [EVENTS[0]]);
    renderPage({ tab: "logs", level: "error" });

    expect(runGroups()).toHaveLength(1);
    expect(
      screen.getByTestId("observability-log-group-run-1"),
    ).toBeInTheDocument();
  });
});

describe("LogsTab layout contract", () => {
  it("renders the stream as a fixed-column grid so every line shares a left edge", () => {
    answer(RUNS, EVENTS);
    renderPage();

    const table = screen.getByTestId("observability-log-table");
    expect(
      within(table)
        .getAllByRole("columnheader")
        .map((th) => th.textContent?.trim()),
    ).toEqual(["Time", "Level", "Source", "Message", "Run"]);

    for (const event of EVENTS) {
      const row = screen.getByTestId(`observability-log-row-${event._id}`);
      expect(within(table).getAllByRole("row")).toContain(row);
      expect(within(row).getAllByRole("cell")).toHaveLength(5);
    }
  });

  it("keeps the whole action cluster inside a facet bar that degrades by wrapping", () => {
    answer(RUNS, EVENTS);
    renderPage();

    const toolbar = screen.getByTestId("observability-log-filters");
    // happy-dom has no layout, so the class is the proxy for the measured
    // behaviour: the bar's ancestor is `overflow-hidden`, so the bar has to
    // wrap or its trailing controls are clipped with no scrollbar.
    expect(toolbar.className).toContain("flex-wrap");
    for (const testid of [
      "observability-filter-tenant",
      "observability-log-follow",
      "observability-log-pause-on-error",
      "observability-filter-clear",
      "observability-open-lens",
    ]) {
      expect(within(toolbar).getByTestId(testid)).toBeInTheDocument();
    }
  });

  it("bounds every text facet so the bar cannot be widened past its container", () => {
    answer(RUNS, EVENTS);
    renderPage();

    for (const testid of [
      "observability-filter-category",
      "observability-filter-source",
      "observability-filter-correlation",
    ]) {
      const input = screen.getByTestId(testid);
      expect(input.className).toContain("w-[14ch]");
      expect(input.className).toContain("min-w-0");
    }
  });
});

// `useQuery` is `undefined` until it answers. Two reads feed the stream, and
// the pane says "loading" until both have landed, so a slow read is never
// reported as an empty log.
describe("LogsTab read states", () => {
  it("says the stream is loading while either read is in flight", () => {
    answer(undefined, EVENTS);
    const { unmount } = renderPage();
    expect(screen.getByTestId("observability-log-loading")).toHaveTextContent(
      /Loading/i,
    );
    expect(screen.queryByTestId("observability-log-empty")).toBeNull();
    unmount();

    answer(RUNS, undefined);
    renderPage();
    expect(screen.getByTestId("observability-log-loading")).toBeInTheDocument();
  });

  it("blames no filter when a settled read is genuinely empty", () => {
    answer([], []);
    renderPage();

    expect(
      screen.getByTestId("observability-log-empty-title"),
    ).toHaveTextContent("No runs or log lines yet");
    expect(screen.getByTestId("observability-log-empty")).not.toHaveTextContent(
      /current filters/i,
    );
    expect(screen.queryByTestId("observability-log-loading")).toBeNull();
    // The next action is a function call on this tenant, not another page.
    expect(
      screen.getByTestId("observability-log-empty-snippet"),
    ).toHaveTextContent("nimbus run http://nimbus.example:9000 functions");
    expect(
      screen.getByTestId("observability-log-empty-snippet"),
    ).toHaveTextContent("--tenant acme");
    expect(screen.queryByTestId("observability-log-empty-cta")).toBeNull();
  });

  it("names the filters, and offers to clear them, only when some are set", () => {
    answer([], []);
    renderPage({ tab: "logs", level: "error" });

    expect(
      screen.getByTestId("observability-log-empty-title"),
    ).toHaveTextContent("Nothing matches the current filters");
    expect(screen.getByTestId("observability-log-empty-cta")).toHaveTextContent(
      /Clear filters/i,
    );
  });

  it("keeps the stream's frame mounted in every state so the swap moves nothing", () => {
    answer(undefined, undefined);
    const { rerender } = renderPage();
    const loadingFrame = screen.getByTestId("observability-log-stream");

    answer(RUNS, EVENTS);
    const Component = routeComponent(Route);
    rerender(<Component />);

    expect(screen.getByTestId("observability-log-stream").className).toBe(
      loadingFrame.className,
    );
  });
});

// The search page is the UIR19 acceptance: `?q=` reads GET /api/console/logs
// with the same facets the stream reads, the matches group under their
// runs, and the count line says how many matched and how far the server
// looked. The field commits after a quiet gap, not per keystroke.
describe("LogsTab search", () => {
  function answerSearch(
    page: Partial<LogSearchPage>,
    onRequest?: (url: URL) => void,
  ) {
    server.use(
      http.get("*/api/console/logs", ({ request }) => {
        onRequest?.(new URL(request.url));
        return HttpResponse.json({
          lines: [],
          matched: 0,
          scanned: 0,
          exhaustive: true,
          limit: 200,
          ...page,
        });
      }),
    );
  }

  // The committed search lands in the address through the replace
  // navigation; the mock keeps the reducer, so the test applies it.
  function committedSearch(
    prev: Record<string, unknown> = { tab: "logs" },
  ): Record<string, unknown> | undefined {
    const call = navigateMock.mock.calls.at(-1)?.[0] as
      | { search: (prev: Record<string, unknown>) => Record<string, unknown> }
      | undefined;
    return call?.search(prev);
  }

  it("reads the search page on the tenant scope, groups the matches, and counts them", async () => {
    answer(RUNS, EVENTS);
    let requested: URL | undefined;
    answerSearch(
      { lines: [EVENTS[0]], matched: 2, scanned: 2000, exhaustive: false },
      (url) => {
        requested = url;
      },
    );
    renderPage({ tab: "logs", q: "commit" });

    const count = await screen.findByTestId("observability-log-search-count");
    expect(count).toHaveTextContent("2 lines match");
    expect(count).toHaveTextContent("“commit”");
    expect(count).toHaveTextContent("in the newest 2,000 lines");
    expect(count).toHaveTextContent("The newest 1 are shown.");
    expect(requested?.searchParams.get("q")).toBe("commit");
    expect(requested?.searchParams.get("tenant")).toBe("acme");
    expect(requested?.searchParams.get("limit")).toBe("200");
    expect(requested?.searchParams.get("run")).toBeNull();
    // The one returned line sits under its run; the other runs are not
    // groups, because a search that names no line under them says nothing.
    expect(runGroups()).toHaveLength(1);
    expect(
      within(screen.getByTestId("observability-log-group-run-1")).getByTestId(
        "observability-log-row-evt-1",
      ),
    ).toBeInTheDocument();
    expect(screen.getByTestId("observability-log-search")).toHaveValue(
      "commit",
    );
  });

  it("scopes the search to the run named by ?correlationId= and says so in the field", async () => {
    answer(RUNS, EVENTS);
    let requested: URL | undefined;
    answerSearch(
      { lines: [EVENTS[0]], matched: 1, scanned: 1, exhaustive: true },
      (url) => {
        requested = url;
      },
    );
    renderPage({
      tab: "logs",
      q: "commit",
      correlationId: "run-1",
      level: "error",
    });

    const count = await screen.findByTestId("observability-log-search-count");
    expect(count).toHaveTextContent("1 line matches");
    expect(count).toHaveTextContent("in every recorded line");
    expect(count).not.toHaveTextContent("are shown");
    expect(requested?.searchParams.get("run")).toBe("run-1");
    expect(requested?.searchParams.get("level")).toBe("error");
    expect(screen.getByTestId("observability-log-search")).toHaveAttribute(
      "placeholder",
      "search this run",
    );
  });

  it("commits the field once, after the last keystroke, not per letter", async () => {
    answer(RUNS, EVENTS);
    renderPage();
    const field = screen.getByTestId("observability-log-search");

    fireEvent.change(field, { target: { value: "pay" } });
    fireEvent.change(field, { target: { value: "paym" } });
    fireEvent.change(field, { target: { value: "payment" } });
    expect(navigateMock).not.toHaveBeenCalled();
    expect(field).toHaveValue("payment");

    await waitFor(() => expect(navigateMock).toHaveBeenCalledTimes(1));
    expect(committedSearch()?.q).toBe("payment");
    expect(navigateMock.mock.calls[0][0]).toMatchObject({ replace: true });
  });

  it("shows the filtered empty state under a zero count", async () => {
    answer(RUNS, EVENTS);
    answerSearch({ lines: [], matched: 0, scanned: 12, exhaustive: true });
    renderPage({ tab: "logs", q: "nothing" });

    const count = await screen.findByTestId("observability-log-search-count");
    expect(count).toHaveTextContent("0 lines match");
    expect(
      screen.getByTestId("observability-log-empty-title"),
    ).toHaveTextContent("Nothing matches the current filters");
  });

  it("reports a failed search read and retries on Try again", async () => {
    answer(RUNS, EVENTS);
    let calls = 0;
    server.use(
      http.get("*/api/console/logs", () => {
        calls += 1;
        if (calls === 1) {
          return HttpResponse.json({ error: "engine busy" }, { status: 503 });
        }
        return HttpResponse.json({
          lines: [],
          matched: 0,
          scanned: 0,
          exhaustive: true,
          limit: 200,
        });
      }),
    );
    renderPage({ tab: "logs", q: "commit" });

    const failed = await screen.findByTestId("observability-log-search-failed");
    expect(failed).toHaveTextContent("Could not load log search");
    fireEvent.click(within(failed).getByRole("button", { name: /try again/i }));
    await screen.findByTestId("observability-log-search-count");
    expect(calls).toBe(2);
  });

  it("clears the search from the count line and returns to the stream", async () => {
    answer(RUNS, EVENTS);
    answerSearch({ lines: [], matched: 0, scanned: 0, exhaustive: true });
    renderPage({ tab: "logs", q: "commit", level: "error" });

    await screen.findByTestId("observability-log-search-count");
    fireEvent.click(screen.getByTestId("observability-log-search-clear"));
    const next = committedSearch({ tab: "logs", q: "commit", level: "error" });
    expect(next?.q).toBeUndefined();
    // Only the search clears; the other facets stay.
    expect(next?.level).toBe("error");
  });
});
