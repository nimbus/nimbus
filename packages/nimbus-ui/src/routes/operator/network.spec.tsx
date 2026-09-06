import { render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@tanstack/react-router", () => ({
  createFileRoute: () => (config: Record<string, unknown>) => config,
  redirect: vi.fn(),
}));

const { useQueryMock } = vi.hoisted(() => ({ useQueryMock: vi.fn() }));

vi.mock("@nimbus/nimbus/react", () => ({
  useQuery: (...args: unknown[]) => useQueryMock(...args),
}));

vi.mock("../../shell/sub-drawer", () => ({
  useContributeSubDrawer: vi.fn(),
}));

import { routeComponent } from "../../test/route-internals";
import { Route } from "./network";

type Section = "routes" | "ws" | "ports" | "listeners" | "security";

const ROWS: Record<string, Array<Record<string, unknown>>> = {
  "routes:list": [],
  "subscriptions:list": [
    {
      _id: "subscription-1",
      tenantId: "acme",
      adapter: "convex",
      queryKey: "messages:list",
      clientCount: 2,
    },
  ],
  "ports:list": [
    {
      _id: "port-1",
      serviceId: "service-api",
      hostPort: 8443,
      guestPort: 443,
      protocol: "tcp",
      actualAddress: "127.0.0.1:8443",
      observedPhase: "ready",
    },
  ],
  "listeners:list": [
    {
      _id: "listener-1",
      adapter: "s3",
      protocol: "http",
      actualAddress: "127.0.0.1:9000",
      observedPhase: "ready",
      version: "v1",
    },
  ],
  "adapter_capabilities:list": [
    {
      _id: "capability-1",
      adapter: "firebase",
      feature: "transaction isolation",
      status: "supported",
      evidence: "adapter contract suite",
    },
  ],
};

function renderSection(section: Section) {
  (Route as unknown as { useSearch: () => { section: Section } }).useSearch =
    () => ({ section });
  const Component = routeComponent(Route);
  render(<Component />);
}

beforeEach(() => {
  useQueryMock.mockReset();
  useQueryMock.mockImplementation(
    (ref: { name: string }) => ROWS[ref.name] ?? [],
  );
});

describe("operator network sections", () => {
  it("defaults invalid or absent search input to routes", () => {
    const validateSearch = (
      Route as unknown as {
        validateSearch: (search: Record<string, unknown>) => {
          section: Section;
        };
      }
    ).validateSearch;

    expect(validateSearch({}).section).toBe("routes");
    expect(validateSearch({ section: "unknown" }).section).toBe("routes");
  });

  it.each([
    ["ws", "network-ws-table", "messages:list", "subscriptions:list"],
    ["ports", "network-ports-table", "8443", "ports:list"],
    [
      "listeners",
      "network-listeners-table",
      "127.0.0.1:9000",
      "listeners:list",
    ],
    [
      "security",
      "network-security-table",
      "transaction isolation",
      "adapter_capabilities:list",
    ],
  ] as const)("renders the %s inventory instead of the routes panel", (section, tableTestId, visibleValue, queryName) => {
    renderSection(section);

    expect(screen.getByTestId("page-network")).toHaveAttribute(
      "data-section",
      section,
    );
    expect(screen.getByTestId(tableTestId)).toHaveTextContent(visibleValue);
    expect(screen.queryByTestId("network-routes-table")).toBeNull();
    expect(useQueryMock).toHaveBeenCalledTimes(1);
    expect(useQueryMock.mock.calls[0]?.[0]).toMatchObject({ name: queryName });
  });
});
