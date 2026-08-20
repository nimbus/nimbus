import { render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@tanstack/react-router", () => ({
  createFileRoute: () => (config: Record<string, unknown>) => config,
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

const { useQueryMock, connStateMock } = vi.hoisted(() => ({
  useQueryMock: vi.fn(),
  connStateMock: vi.fn(),
}));

vi.mock("@nimbus/nimbus/react", () => ({
  useQuery: (ref: unknown, args: unknown) => useQueryMock(ref, args),
  useNimbusConnectionState: () => connStateMock(),
}));

import { api } from "../../../convex/_generated/api";
import { useUiStore } from "../../store/ui-store";
import { routeComponent } from "../../test/route-internals";
import { Route } from "./index";

const OverviewPage = routeComponent(Route);

type Doc = Record<string, unknown>;

type Fixture = {
  status?: Doc | null;
  machines?: Doc[];
  services?: Doc[];
  tables?: Doc[];
  functions?: Doc[];
  runs?: Doc[];
  events?: Doc[];
};

function mockQueries(fixture: Fixture) {
  const byRef = new Map<unknown, unknown>([
    [api.system.status, fixture.status ?? null],
    [api.machines.list, fixture.machines ?? []],
    [api.services.list, fixture.services ?? []],
    [api.tables.list, fixture.tables ?? []],
    [api.functions.list, fixture.functions ?? []],
    [api.runs.recent, fixture.runs ?? []],
    [api.events.recent, fixture.events ?? []],
  ]);
  useQueryMock.mockImplementation((ref: unknown) => byRef.get(ref));
}

const SIX_FUNCTIONS: Doc[] = [
  { _id: "f1", kind: "query" },
  { _id: "f2", kind: "query" },
  { _id: "f3", kind: "query" },
  { _id: "f4", kind: "mutation" },
  { _id: "f5", kind: "mutation" },
  { _id: "f6", kind: "mutation" },
];

const THIRTEEN_TABLES: Doc[] = Array.from({ length: 13 }, (_, i) => ({
  _id: `t${i}`,
  tenantId: i % 2 === 0 ? "demo" : "acme",
}));

beforeEach(() => {
  useQueryMock.mockReset();
  connStateMock.mockReturnValue({
    isWebSocketConnected: true,
    hasEverConnected: true,
  });
  useUiStore.setState({ activeTenant: "demo" });
});

afterEach(() => {
  useUiStore.setState({ activeTenant: null });
});

describe("Overview state vocabulary", () => {
  it("renders function kinds as category pills, never the unknown ? glyph", () => {
    mockQueries({ functions: SIX_FUNCTIONS, tables: THIRTEEN_TABLES });
    render(<OverviewPage />);

    const tile = screen.getByTestId("overview-count-functions");
    const pills = Array.from(tile.querySelectorAll("[data-category]")).map(
      (el) => el.getAttribute("data-category"),
    );
    expect(pills.sort()).toEqual(["mutation", "query"]);
    // Not a state: the tile must contain no state chip at all, and so no `?`.
    expect(tile.querySelector("[data-state]")).toBeNull();
    expect(tile.textContent).not.toContain("?");
    expect(tile).toHaveTextContent("query");
    expect(tile).toHaveTextContent("mutation");
  });

  it("draws no unknown glyph anywhere on the page", () => {
    mockQueries({
      functions: SIX_FUNCTIONS,
      tables: THIRTEEN_TABLES,
      machines: [{ _id: "m1", state: "running" }],
      services: [{ _id: "s1", state: "ready", tenantId: "demo" }],
      runs: [{ _id: "r1", status: "ok", functionPath: "a:b" }],
      events: [{ _id: "e1", level: "info", source: "system", message: "hi" }],
      status: { health: "ok", version: "0.1.0" },
    });
    const { container } = render(<OverviewPage />);

    const unknown = Array.from(container.querySelectorAll("[data-state]"))
      .filter((el) => el.getAttribute("data-glyph") === "question")
      .map((el) => el.textContent);
    expect(unknown).toEqual([]);
  });

  it("keeps state chips for the state-grouped tiles", () => {
    mockQueries({ runs: [{ _id: "r1", status: "ok" }] });
    render(<OverviewPage />);
    const tile = screen.getByTestId("overview-count-runs");
    expect(tile.querySelector("[data-state]")).toHaveAttribute(
      "data-state",
      "ok",
    );
  });
});

describe("Overview count tile sublines", () => {
  it("never prints 'No state breakdown'", () => {
    mockQueries({ functions: SIX_FUNCTIONS, tables: THIRTEEN_TABLES });
    const { container } = render(<OverviewPage />);
    expect(container.textContent).not.toContain("No state breakdown");
  });

  it("gives the ungroupable Tables tile a real fact instead of an apology", () => {
    mockQueries({ tables: THIRTEEN_TABLES });
    render(<OverviewPage />);
    expect(
      screen.getByTestId("overview-count-tables-subline"),
    ).toHaveTextContent("across 2 tenants");
  });

  it("gives the Tenants tile a real fact", () => {
    mockQueries({
      tables: THIRTEEN_TABLES,
      services: [
        { _id: "s1", state: "ready", tenantId: "demo" },
        { _id: "s2", state: "ready", tenantId: "demo" },
      ],
    });
    render(<OverviewPage />);
    expect(
      screen.getByTestId("overview-count-tenants-subline"),
    ).toHaveTextContent("1 with services");
  });

  it("renders an em dash, not a sentence, for a groupable tile with zero rows", () => {
    mockQueries({ machines: [] });
    render(<OverviewPage />);
    const subline = screen.getByTestId("overview-count-machines-subline");
    expect(subline.textContent?.trim()).toBe("—");
  });

  it("still reaches the loading branch for an ungroupable tile", () => {
    connStateMock.mockReturnValue({
      isWebSocketConnected: true,
      hasEverConnected: false,
    });
    useQueryMock.mockImplementation(() => undefined);
    render(<OverviewPage />);
    expect(
      screen.getByTestId("overview-count-tables-subline"),
    ).toHaveTextContent("Loading…");
  });

  it("reserves the subline slot on every tile so the six stay flush", () => {
    mockQueries({ functions: SIX_FUNCTIONS, tables: THIRTEEN_TABLES });
    render(<OverviewPage />);
    for (const id of [
      "machines",
      "services",
      "tenants",
      "tables",
      "functions",
      "runs",
    ]) {
      expect(
        screen.getByTestId(`overview-count-${id}-subline`).className,
      ).toContain("min-h-5");
    }
  });
});

describe("Overview top strip", () => {
  it("refuses to shrink so the page scrolls instead of clipping the header", () => {
    mockQueries({});
    render(<OverviewPage />);
    // The strip is `overflow-hidden`, so without `shrink-0` flexbox silently
    // eats it — at 1440px it had collapsed to a 2px hairline.
    expect(screen.getByTestId("overview-top-strip").className).toContain(
      "shrink-0",
    );
  });

  it("prints no licence at all until the server has reported one", () => {
    // The strip renders on mount, before any query answers. It used to fall
    // through to the literal "developer" — a value the server never sent,
    // rendered in the same face and colour as a real reading.
    useQueryMock.mockImplementation(() => undefined);
    render(<OverviewPage />);

    expect(screen.getByTestId("overview-license-loading")).toBeInTheDocument();
    expect(screen.getByTestId("overview-top-strip")).not.toHaveTextContent(
      /developer/i,
    );
  });

  it("shows the loading marker, not a fabricated reading, for every status cell", () => {
    useQueryMock.mockImplementation(() => undefined);
    render(<OverviewPage />);

    for (const cell of [
      "overview-server",
      "overview-version",
      "overview-uptime",
      "overview-storage",
      "overview-license",
      "overview-started",
      "overview-updated",
    ]) {
      expect(screen.getByTestId(`${cell}-loading`)).toBeInTheDocument();
    }
  });

  it("says offline rather than loading once the socket has dropped", () => {
    connStateMock.mockReturnValue({
      isWebSocketConnected: false,
      hasEverConnected: true,
    });
    useQueryMock.mockImplementation(() => undefined);
    render(<OverviewPage />);

    expect(screen.getByTestId("overview-license-offline")).toBeInTheDocument();
  });

  it("renders an em dash for a field a settled status omits", () => {
    // `null` is an answer — the deployment has no status row — so the strip
    // must settle rather than sit on the loading marker for the life of the
    // page. A field the server did not report is still not a value.
    mockQueries({ status: null });
    render(<OverviewPage />);

    expect(screen.queryByTestId("overview-license-loading")).toBeNull();
    expect(screen.getByTestId("overview-top-strip")).not.toHaveTextContent(
      /developer/i,
    );
  });

  it("reports the licence the server actually sent", () => {
    mockQueries({
      status: { version: "1.2.3", details: { license: "enterprise" } },
    });
    render(<OverviewPage />);

    expect(screen.getByTestId("overview-top-strip")).toHaveTextContent(
      "enterprise",
    );
  });

  it("reserves each value line so the strip does not resize when status lands", () => {
    // The loading marker is a bare `·` at text-sm and the loaded cells are
    // text-xs chips. Without a floor the eight-cell grid changed height under
    // itself the moment the query answered.
    mockQueries({});
    render(<OverviewPage />);

    const values = screen
      .getByTestId("overview-top-strip")
      .querySelectorAll(":scope > div > span:last-child");
    expect(values).toHaveLength(8);
    for (const value of values) {
      expect(value.className).toContain("min-h-5");
    }
  });
});

describe("Overview activity feeds", () => {
  it("gives an empty events feed a two-line message and a next action", () => {
    mockQueries({ events: [] });
    render(<OverviewPage />);
    const empty = screen.getByTestId("overview-events-empty");
    expect(empty).toHaveTextContent("No events recorded yet");
    expect(empty).toHaveTextContent(
      "Server, scheduler, and function activity streams here live.",
    );
    expect(screen.getByTestId("overview-events-empty-cta")).toHaveAttribute(
      "href",
      "/developer/compute",
    );
  });

  it("centres the empty state in the height the stretched grid hands it", () => {
    mockQueries({ runs: [] });
    render(<OverviewPage />);
    const empty = screen.getByTestId("overview-runs-empty");
    expect(empty.className).toContain("flex-1");
    expect(empty.className).toContain("justify-center");
    expect(empty.className).toContain("text-center");
  });

  it("does not duplicate the header's View all link as the empty-state action", () => {
    mockQueries({ events: [] });
    render(<OverviewPage />);
    const cta = screen.getByTestId("overview-events-empty-cta");
    expect(cta.getAttribute("href")).not.toBe("/developer/observability");
  });
});
