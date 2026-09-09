import { fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const { useQueryMock, connStateMock, navigateMock } = vi.hoisted(() => ({
  useQueryMock: vi.fn(),
  connStateMock: vi.fn(),
  navigateMock: vi.fn(),
}));

vi.mock("@tanstack/react-router", () => ({
  createFileRoute: () => (config: Record<string, unknown>) => config,
  useNavigate: () => navigateMock,
  Link: ({
    to,
    search,
    children,
    "data-testid": testId,
    className,
  }: {
    to: string;
    search?: Record<string, string>;
    children: React.ReactNode;
    "data-testid"?: string;
    className?: string;
  }) => (
    <a
      href={search ? `${to}?${new URLSearchParams(search)}` : to}
      data-testid={testId}
      className={className}
    >
      {children}
    </a>
  ),
}));

vi.mock("@nimbus/nimbus/react", () => ({
  useQuery: (ref: unknown, args: unknown) => useQueryMock(ref, args),
  useNimbusConnectionState: () => connStateMock(),
  useNimbus: () => ({ url: "http://nimbus.example:9000/convex/_nimbus" }),
}));

import { api } from "../../../convex/_generated/api";
import { useUiStore } from "../../store/ui-store";
import { routeComponent } from "../../test/route-internals";
import {
  connectSnippets,
  hourlyBuckets,
  Route,
  type RunRow,
  readHeadline,
  readStats,
} from "./index";

const OverviewPage = routeComponent(Route);

type Doc = Record<string, unknown>;

type Fixture = {
  status?: Doc | null;
  tables?: Doc[];
  functions?: Doc[];
  runs?: Doc[];
};

const NOW = Date.UTC(2026, 8, 8, 12, 0, 0);
const HOUR = 60 * 60 * 1000;

function mockQueries(fixture: Fixture) {
  const byRef = new Map<unknown, unknown>([
    [api.system.status, fixture.status ?? null],
    [api.tables.list, fixture.tables ?? []],
    [api.functions.list, fixture.functions ?? []],
    [api.runs.recent, fixture.runs ?? []],
  ]);
  useQueryMock.mockImplementation((ref: unknown) => {
    if (!byRef.has(ref)) throw new Error("unexpected query on the overview");
    return byRef.get(ref);
  });
}

function connected() {
  connStateMock.mockReturnValue({
    isWebSocketConnected: true,
    hasEverConnected: true,
  });
}

function run(overrides: Partial<Doc> & { _id: string }): Doc {
  return {
    status: "ok",
    functionPath: "messages:list",
    durationMs: 12,
    startedAt: NOW - 5 * 60 * 1000,
    ...overrides,
  };
}

// A populated server in the shape of the agent-chat example: three
// functions, two tables, a day of runs with one failure.
const POPULATED: Fixture = {
  status: { name: "nimbus", version: "0.4.1", health: "ok", startedAt: NOW },
  functions: [
    { _id: "f1", path: "messages:list", kind: "query" },
    { _id: "f2", path: "messages:send", kind: "mutation" },
    { _id: "f3", path: "agent:reply", kind: "mutation" },
  ],
  tables: [
    { _id: "t1", name: "messages", tenantId: "demo" },
    { _id: "t2", name: "agentMemory", tenantId: "demo" },
  ],
  runs: [
    run({ _id: "r1", functionPath: "agent:reply", durationMs: 48 }),
    run({
      _id: "r2",
      status: "error",
      functionPath: "agent:reply",
      error: "boom",
      startedAt: NOW - 2 * HOUR,
    }),
    run({ _id: "r3", startedAt: NOW - 3 * HOUR }),
    run({ _id: "r4", startedAt: NOW - 4 * HOUR }),
    run({ _id: "r5", startedAt: NOW - 5 * HOUR }),
    run({ _id: "r6", startedAt: NOW - 6 * HOUR }),
    run({ _id: "r7", startedAt: NOW - 3 * 24 * HOUR }),
  ],
};

beforeEach(() => {
  vi.useFakeTimers({ now: NOW, toFake: ["Date"] });
  useUiStore.setState({ activeTenant: "demo" });
  connected();
});

afterEach(() => {
  vi.useRealTimers();
  useQueryMock.mockReset();
  connStateMock.mockReset();
  navigateMock.mockReset();
});

describe("headline", () => {
  it("reads a healthy server with clean runs as idle", () => {
    mockQueries({
      ...POPULATED,
      runs: POPULATED.runs?.filter((r) => r.status === "ok"),
    });
    render(<OverviewPage />);
    expect(screen.getByTestId("overview-mascot")).toHaveAttribute(
      "data-state",
      "idle",
    );
    expect(screen.getByTestId("overview-sentence")).toHaveTextContent(
      "The server is up and every recent run succeeded.",
    );
  });

  it("names the failures of the last day in the sentence", () => {
    mockQueries(POPULATED);
    render(<OverviewPage />);
    expect(screen.getByTestId("overview-mascot")).toHaveAttribute(
      "data-state",
      "error",
    );
    expect(screen.getByTestId("overview-sentence")).toHaveTextContent(
      "1 run failed in the last 24 hours",
    );
  });

  it("puts the tenant, endpoint, and version on one fact line", () => {
    mockQueries(POPULATED);
    render(<OverviewPage />);
    const facts = screen.getByTestId("overview-facts");
    expect(within(facts).getByTestId("overview-fact-tenant")).toHaveTextContent(
      "tenant demo",
    );
    expect(
      within(facts).getByTestId("overview-fact-endpoint"),
    ).toHaveTextContent("http://nimbus.example:9000");
    expect(
      within(facts).getByTestId("overview-fact-version"),
    ).toHaveTextContent("v0.4.1");
  });

  it("leaves a version the server did not report off the line", () => {
    mockQueries({ ...POPULATED, status: { health: "ok" } });
    render(<OverviewPage />);
    expect(screen.queryByTestId("overview-fact-version")).toBeNull();
    expect(screen.getByTestId("overview-facts")).not.toHaveTextContent("—");
  });

  it("shows the working face until the status and the lists load", () => {
    useQueryMock.mockReturnValue(undefined);
    render(<OverviewPage />);
    expect(screen.getByTestId("overview-mascot")).toHaveAttribute(
      "data-state",
      "working",
    );
    expect(screen.queryByTestId("overview-onboarding")).toBeNull();
    expect(screen.getByTestId("overview-stats-loading")).toBeInTheDocument();
  });

  it("prefers a dropped connection over a healthy status document", () => {
    connStateMock.mockReturnValue({
      isWebSocketConnected: false,
      hasEverConnected: true,
    });
    // The status document arrived before the socket dropped; the lists
    // never did.
    useQueryMock.mockImplementation((ref: unknown) =>
      ref === api.system.status ? POPULATED.status : undefined,
    );
    render(<OverviewPage />);
    expect(screen.getByTestId("overview-mascot")).toHaveAttribute(
      "data-state",
      "error",
    );
    expect(screen.getByTestId("overview-sentence")).toHaveTextContent(
      "connection to the server dropped",
    );
  });

  it("repeats a health the server reports as anything but ok", () => {
    const reading = readHeadline(
      { kind: "ok", value: { health: "degraded" } },
      { kind: "ok", value: { functions: [], tables: [], runs: [] } },
      false,
    );
    expect(reading).toEqual({
      mascot: "error",
      sentence: "The server reports its health as degraded.",
    });
  });
});

describe("connect panel", () => {
  it("offers curl, the TypeScript SDK, and the Convex client as tabs", () => {
    mockQueries(POPULATED);
    render(<OverviewPage />);
    const tabs = screen.getByTestId("overview-connect-tabs");
    expect(
      within(tabs)
        .getAllByRole("tab")
        .map((t) => t.textContent),
    ).toEqual(["curl", "TypeScript SDK", "Convex client"]);
    const snippet = screen.getByTestId("overview-connect-snippet");
    expect(snippet).toHaveAttribute("data-snippet", "curl");
    expect(snippet).toHaveTextContent(
      "http://nimbus.example:9000/api/tenants/demo/query",
    );
    expect(snippet).toHaveTextContent('"table": "messages"');

    fireEvent.click(screen.getByTestId("overview-connect-tab-sdk"));
    expect(snippet).toHaveAttribute("data-snippet", "sdk");
    expect(snippet).toHaveTextContent(
      'new NimbusClient("http://nimbus.example:9000/convex/demo")',
    );
    expect(snippet).toHaveTextContent("api.messages.list");

    fireEvent.click(screen.getByTestId("overview-connect-tab-convex"));
    expect(snippet).toHaveAttribute("data-snippet", "convex");
    expect(snippet).toHaveTextContent("ConvexReactClient");
  });

  it("falls back to the quick start names on an empty server", () => {
    const snippets = connectSnippets({
      serverUrl: "",
      tenant: null,
      functionPath: null,
      table: null,
    });
    expect(snippets.curl).toContain(
      "http://localhost:3210/api/tenants/demo/query",
    );
    expect(snippets.sdk).toContain("api.messages.list");
  });
});

describe("stats", () => {
  it("shows functions, tables, runs, and errors for the last day", () => {
    mockQueries(POPULATED);
    render(<OverviewPage />);
    const stats = screen.getByTestId("overview-stats");
    expect(
      within(stats).getByTestId("overview-stat-functions-value"),
    ).toHaveTextContent("3");
    expect(
      within(stats).getByTestId("overview-stat-functions"),
    ).toHaveTextContent("2 mutation · 1 query");
    expect(
      within(stats).getByTestId("overview-stat-tables-value"),
    ).toHaveTextContent("2");
    expect(
      within(stats).getByTestId("overview-stat-runs-value"),
    ).toHaveTextContent("6");
    expect(
      within(stats).getByTestId("overview-stat-errors-value"),
    ).toHaveTextContent("1");
    expect(within(stats).getByTestId("overview-stat-errors")).toHaveAttribute(
      "href",
      "/developer/observability?tab=runs&status=error",
    );
  });

  it("draws a sparkline for runs and errors only", () => {
    mockQueries(POPULATED);
    render(<OverviewPage />);
    const stats = screen.getByTestId("overview-stats");
    expect(
      within(stats).queryByRole("img", { name: "Functions by hour" }),
    ).toBeNull();
    expect(
      within(stats).getByRole("img", { name: "Runs, 24h by hour" }),
    ).toBeInTheDocument();
    expect(
      within(stats).getByRole("img", { name: "Errors, 24h by hour" }),
    ).toBeInTheDocument();
  });

  it("buckets a day of runs by hour, oldest first", () => {
    const runs: RunRow[] = [
      {
        id: "a",
        status: "ok",
        functionPath: "x",
        durationMs: 1,
        startedAt: NOW - 30 * 60 * 1000,
      },
      {
        id: "b",
        status: "ok",
        functionPath: "x",
        durationMs: 1,
        startedAt: NOW - 23 * HOUR - 30 * 60 * 1000,
      },
      {
        id: "c",
        status: "ok",
        functionPath: "x",
        durationMs: 1,
        startedAt: NOW - 25 * HOUR,
      },
    ];
    const points = hourlyBuckets(runs, () => true, NOW);
    expect(points).toHaveLength(24);
    expect(points[0].value).toBe(1);
    expect(points[23].value).toBe(1);
    expect(points.reduce((sum, p) => sum + p.value, 0)).toBe(2);
  });

  it("hides a stat that has no value", () => {
    const stats = readStats(
      {
        functions: [{ _id: "f1", path: "a:b", kind: "query" }],
        tables: [],
        runs: [],
      },
      "demo",
      NOW,
    );
    expect(stats.map((s) => s.id)).toEqual(["functions"]);
  });
});

describe("recent runs", () => {
  it("lists the five newest runs and links to the rest", () => {
    mockQueries(POPULATED);
    render(<OverviewPage />);
    const table = screen.getByTestId("overview-runs-table");
    const rows = within(table).getAllByRole("row").slice(1);
    expect(rows).toHaveLength(5);
    expect(rows[0]).toHaveTextContent("agent:reply");
    expect(rows[1]).toHaveTextContent(/error/i);
    expect(screen.getByTestId("overview-runs-all")).toHaveAttribute(
      "href",
      "/developer/observability?tab=runs",
    );
  });

  it("opens the run on activation", () => {
    mockQueries(POPULATED);
    render(<OverviewPage />);
    const table = screen.getByTestId("overview-runs-table");
    fireEvent.click(within(table).getAllByRole("row")[1]);
    expect(navigateMock).toHaveBeenCalledWith({
      to: "/developer/compute/runs/$runId",
      params: { runId: "r1" },
    });
  });
});

describe("empty tenant", () => {
  it("renders the first-run panel instead of empty tiles", () => {
    mockQueries({ status: { health: "ok", version: "0.4.1" } });
    render(<OverviewPage />);
    expect(screen.getByTestId("overview-mascot")).toHaveAttribute(
      "data-state",
      "empty",
    );
    const panel = screen.getByTestId("overview-onboarding");
    expect(
      within(panel).getByTestId("overview-onboarding-title"),
    ).toHaveTextContent("Nothing here yet");
    expect(
      within(panel).getByTestId("overview-onboarding-progress"),
    ).toHaveTextContent("0 of 3 steps done");
    expect(
      within(panel).getByTestId("overview-onboarding-step-install"),
    ).toHaveTextContent("brew install nimbus/tap/nimbus");
    expect(screen.queryByTestId("overview-stats")).toBeNull();
    expect(screen.queryByTestId("overview-runs")).toBeNull();
    // The connect panel stays: it is how the numbers start moving.
    expect(screen.getByTestId("overview-connect")).toBeInTheDocument();
  });

  it("marks steps done from the same queries and retires on the first run", () => {
    mockQueries({
      status: { health: "ok" },
      functions: [{ _id: "f1", path: "messages:list", kind: "query" }],
    });
    const { rerender } = render(<OverviewPage />);
    const panel = screen.getByTestId("overview-onboarding");
    expect(
      within(panel).getByTestId("overview-onboarding-step-install"),
    ).toHaveAttribute("data-done", "true");
    expect(
      within(panel).getByTestId("overview-onboarding-step-run"),
    ).toHaveAttribute("data-done", "false");
    expect(
      within(panel).getByTestId("overview-onboarding-progress"),
    ).toHaveTextContent("2 of 3 steps done");

    mockQueries({
      status: { health: "ok" },
      functions: [{ _id: "f1", path: "messages:list", kind: "query" }],
      runs: [run({ _id: "r1" })],
    });
    rerender(<OverviewPage />);
    expect(screen.queryByTestId("overview-onboarding")).toBeNull();
    expect(screen.getByTestId("overview-stats")).toBeInTheDocument();
    expect(screen.getByTestId("overview-runs")).toBeInTheDocument();
  });
});
