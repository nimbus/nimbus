import { fireEvent, render, screen, waitFor } from "@testing-library/react";
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
}));

import { api } from "../../../convex/_generated/api";
import { routeComponent } from "../../test/route-internals";
import { Route, readFacts, readNodeHeadline } from "./index";

const NodesPage = routeComponent(Route);

type Doc = Record<string, unknown>;

type Fixture = {
  status?: Doc | null;
  machines?: Doc[];
  services?: Doc[];
  listeners?: Doc[];
  events?: Doc[];
};

function mockQueries(fixture: Fixture) {
  const byRef = new Map<unknown, unknown>([
    [api.system.status, fixture.status ?? null],
    [api.machines.list, fixture.machines ?? []],
    [api.services.list, fixture.services ?? []],
    [api.listeners.list, fixture.listeners ?? []],
    [api.events.recent, fixture.events ?? []],
  ]);
  useQueryMock.mockImplementation((ref: unknown) => {
    if (!byRef.has(ref)) throw new Error("unexpected query on the nodes page");
    return byRef.get(ref);
  });
}

function mockTenants(tenants: string[]) {
  vi.stubGlobal(
    "fetch",
    vi.fn().mockResolvedValue({
      ok: true,
      json: async () => ({ tenants }),
    }),
  );
}

const STATUS = {
  version: "0.4.1",
  buildHash: "abcdef1234567890",
  health: "healthy",
  startedAt: Date.now() - 3 * 60 * 60 * 1000,
  updatedAt: Date.now() - 5_000,
  details: { listenAddress: "127.0.0.1:3210", dataDir: "/srv/nimbus/data" },
};

beforeEach(() => {
  navigateMock.mockReset();
  mockQueries({});
  mockTenants([]);
  connStateMock.mockReturnValue({
    isWebSocketConnected: true,
    hasEverConnected: true,
  });
});

afterEach(() => {
  vi.unstubAllGlobals();
});

const ok = <T,>(value: T) => ({ kind: "ok" as const, value });
const loading = { kind: "loading" as const };
const offline = { kind: "offline" as const };
const hosted = (services: Doc[] = []) =>
  ok({ machines: [], services, listeners: [] } as never);

describe("readNodeHeadline", () => {
  it("puts a dropped connection before a healthy status document", () => {
    const reading = readNodeHeadline(ok({ health: "healthy" }), offline);
    expect(reading.mascot).toBe("error");
    expect(reading.sentence).toContain("connection to the node dropped");
  });

  it("reports a status read error in its own words", () => {
    expect(
      readNodeHeadline({ kind: "error", message: "status: 503" }, hosted()),
    ).toEqual({ mascot: "error", sentence: "status: 503" });
  });

  it("works while either read is in flight", () => {
    expect(readNodeHeadline(loading, hosted()).mascot).toBe("working");
    expect(readNodeHeadline(ok({}), loading).mascot).toBe("working");
  });

  it("names an unhealthy node", () => {
    expect(readNodeHeadline(ok({ health: "degraded" }), hosted())).toEqual({
      mascot: "error",
      sentence: "The node reports its health as degraded.",
    });
  });

  it("counts failing services ahead of running ones", () => {
    const reading = readNodeHeadline(
      ok({ health: "healthy" }),
      hosted([{ state: "running" }, { state: "failed" }, { state: "error" }]),
    );
    expect(reading).toEqual({
      mascot: "error",
      sentence: "The node is up. 2 services are failing.",
    });
  });

  it("says when nothing is placed on the node", () => {
    expect(readNodeHeadline(ok({ health: "healthy" }), hosted())).toEqual({
      mascot: "idle",
      sentence: "The node is up. No services are placed on it yet.",
    });
  });

  it("says every service is running when it is, and the ratio when not", () => {
    expect(
      readNodeHeadline(ok({}), hosted([{ state: "running" }])).sentence,
    ).toBe("The node is up and every service is running.");
    expect(
      readNodeHeadline(
        ok({}),
        hosted([
          { state: "running" },
          { state: "stopped" },
          { state: "stopped" },
        ]),
      ).sentence,
    ).toBe("The node is up. 1 of 3 services is running.");
  });
});

describe("readFacts", () => {
  it("reads the address, version, start, and data directory off the status", () => {
    expect(readFacts(STATUS)).toEqual({
      listenAddress: "127.0.0.1:3210",
      version: "0.4.1",
      startedAt: STATUS.startedAt,
      dataDir: "/srv/nimbus/data",
    });
  });

  it("leaves out what the server did not report", () => {
    expect(readFacts({})).toEqual({
      listenAddress: null,
      version: null,
      startedAt: null,
      dataDir: null,
    });
  });
});

describe("Nodes page", () => {
  it("leads with the headline sentence and the facts line", () => {
    mockQueries({
      status: STATUS,
      services: [{ _id: "s1", state: "running" }],
    });
    render(<NodesPage />);
    expect(screen.getByTestId("nodes-mascot")).toHaveAttribute(
      "data-state",
      "idle",
    );
    expect(screen.getByTestId("nodes-sentence")).toHaveTextContent(
      "The node is up and every service is running.",
    );
    expect(screen.getByTestId("nodes-fact-address")).toHaveTextContent(
      "127.0.0.1:3210",
    );
    expect(screen.getByTestId("nodes-fact-version")).toHaveTextContent(
      "v0.4.1",
    );
    expect(screen.getByTestId("nodes-fact-uptime")).toHaveTextContent(/^up /);
    expect(screen.getByTestId("nodes-fact-data-dir")).toHaveTextContent(
      "/srv/nimbus/data",
    );
  });

  it("keeps the working face while the status is in flight", () => {
    mockQueries({ status: undefined });
    useQueryMock.mockImplementation((ref: unknown) =>
      ref === api.system.status ? undefined : [],
    );
    render(<NodesPage />);
    expect(screen.getByTestId("nodes-mascot")).toHaveAttribute(
      "data-state",
      "working",
    );
    expect(screen.getByTestId("nodes-facts-loading")).toBeInTheDocument();
  });

  it("treats a missing status document as an answer, not a load", () => {
    mockQueries({ status: null });
    render(<NodesPage />);
    expect(screen.getByTestId("nodes-mascot")).toHaveAttribute(
      "data-state",
      "idle",
    );
    expect(screen.queryByTestId("nodes-facts-loading")).toBeNull();
  });

  it("shows the node card with health and timestamps", () => {
    mockQueries({ status: STATUS });
    render(<NodesPage />);
    expect(screen.getByTestId("node-health")).toHaveTextContent("healthy");
    expect(screen.getByTestId("node-build")).toHaveTextContent("abcdef1");
    expect(screen.getByTestId("node-row")).toHaveTextContent("standalone");
  });

  it("labels every hosted count with what it is made of", () => {
    mockQueries({
      status: STATUS,
      machines: [
        { _id: "m1", state: "running" },
        { _id: "m2", state: "stopped" },
      ],
      services: [{ _id: "s1", state: "running" }],
      listeners: [
        { _id: "l1", adapter: "http" },
        { _id: "l2", adapter: "ws" },
        { _id: "l3", adapter: "ws" },
      ],
    });
    render(<NodesPage />);
    expect(screen.getByTestId("nodes-hosted-machines-count")).toHaveTextContent(
      "2",
    );
    expect(
      screen.getByTestId("nodes-hosted-machines-subline"),
    ).toHaveTextContent("1 running · 1 stopped");
    expect(
      screen.getByTestId("nodes-hosted-services-subline"),
    ).toHaveTextContent("1 running");
    expect(
      screen.getByTestId("nodes-hosted-listeners-count"),
    ).toHaveTextContent("3");
    expect(
      screen.getByTestId("nodes-hosted-listeners-subline"),
    ).toHaveTextContent("http, ws");
    expect(screen.getByTestId("nodes-hosted-listeners")).toHaveAttribute(
      "href",
      "/operator/network",
    );
  });

  it("lists the recent events and opens the logs narrowed to a correlation", () => {
    mockQueries({
      status: STATUS,
      events: [
        {
          _id: "e1",
          level: "error",
          source: "machine",
          message: "boot failed",
          correlationId: "run-9",
          createdAt: Date.now() - 1_000,
        },
        {
          _id: "e2",
          level: "info",
          source: "http",
          message: "GET /api/health 200",
          createdAt: Date.now() - 2_000,
        },
      ],
    });
    render(<NodesPage />);
    const first = screen.getByTestId("nodes-event-row-e1");
    expect(first).toHaveTextContent("boot failed");
    expect(first.querySelector('[data-state="error"]')).toBeInTheDocument();
    expect(screen.getByTestId("nodes-events-all")).toHaveAttribute(
      "href",
      "/operator/observability?tab=logs",
    );
    fireEvent.click(first);
    expect(navigateMock).toHaveBeenCalledWith({
      to: "/operator/observability",
      search: { tab: "logs", correlationId: "run-9" },
    });
    fireEvent.click(screen.getByTestId("nodes-event-row-e2"));
    expect(navigateMock).toHaveBeenLastCalledWith({
      to: "/operator/observability",
      search: { tab: "logs" },
    });
  });

  it("says so when the node has recorded no events", () => {
    mockQueries({ status: STATUS, events: [] });
    render(<NodesPage />);
    expect(screen.getByTestId("nodes-events-empty")).toBeInTheDocument();
    expect(screen.queryByTestId("nodes-events-table")).toBeNull();
  });
});

/**
 * The Tenants tile is the one count on this page that does not ride the
 * WebSocket: it is a REST read of `/api/tenants`. Both of its failure modes
 * used to be swallowed, leaving the tile on the muted loading dot for the
 * life of the page while its three siblings resolved normally. That is the
 * worst of the three states: it promises the number is still on its way.
 */
describe("Nodes tenant count", () => {
  it("reports a non-OK tenants response instead of loading forever", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn().mockResolvedValue({
        ok: false,
        status: 503,
        json: async () => ({ error: { message: "tenant store offline" } }),
      }),
    );

    render(<NodesPage />);

    await waitFor(() => {
      expect(
        screen.getByTestId("nodes-hosted-tenants-error"),
      ).toHaveTextContent("tenant store offline");
    });
    expect(screen.queryByTestId("nodes-hosted-tenants-loading")).toBeNull();
  });

  it("reports a rejected tenants fetch instead of loading forever", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn().mockRejectedValue(new Error("Failed to fetch")),
    );

    render(<NodesPage />);

    await waitFor(() => {
      expect(
        screen.getByTestId("nodes-hosted-tenants-error"),
      ).toHaveTextContent("Failed to fetch");
    });
  });

  it("holds the loading marker only while the read is genuinely in flight", () => {
    vi.stubGlobal(
      "fetch",
      vi.fn(() => new Promise(() => undefined)),
    );

    render(<NodesPage />);

    expect(
      screen.getByTestId("nodes-hosted-tenants-loading"),
    ).toBeInTheDocument();
    expect(screen.queryByTestId("nodes-hosted-tenants-error")).toBeNull();
  });

  it("shows the count once the read succeeds", async () => {
    mockTenants(["acme", "demo"]);

    render(<NodesPage />);

    await waitFor(() => {
      expect(
        screen.getByTestId("nodes-hosted-tenants-count"),
      ).toHaveTextContent("2");
    });
    expect(screen.queryByTestId("nodes-hosted-tenants-loading")).toBeNull();
    expect(screen.queryByTestId("nodes-hosted-tenants-error")).toBeNull();
  });
});
