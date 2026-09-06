import { render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const { notFoundMock, invalidateMock } = vi.hoisted(() => ({
  notFoundMock: vi.fn(() => new Error("__NOT_FOUND__")),
  invalidateMock: vi.fn(),
}));

vi.mock("@tanstack/react-router", () => ({
  createFileRoute: () => (config: Record<string, unknown>) => config,
  notFound: notFoundMock,
  useRouter: () => ({ invalidate: invalidateMock }),
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

const { nimbusQueryMock } = vi.hoisted(() => ({
  nimbusQueryMock: vi.fn(),
}));

vi.mock("../../lib/nimbus-client", () => ({
  getNimbusClient: () => ({ query: nimbusQueryMock }),
}));

import { AdminServiceDetailLoaderError } from "../../components/service-loader-errors";
import { routeLoader } from "../../test/route-internals";
import { isTab, Route, TABS } from "./services_.$service";

type LoaderArgs = { params: { service: string } };
type LoaderResult = {
  service: unknown;
  services: unknown[];
  bundles: unknown[];
  machines: unknown[];
};

const loader = routeLoader<LoaderArgs, LoaderResult>(Route);

beforeEach(() => {
  nimbusQueryMock.mockReset();
  notFoundMock.mockClear();
  invalidateMock.mockReset();
});

afterEach(() => {
  vi.restoreAllMocks();
});

describe("Admin service detail tabs (DR6 / F6)", () => {
  it("TABS has exactly one entry: Placement", () => {
    expect(TABS).toHaveLength(1);
    expect(TABS[0]).toEqual({ id: "placement", label: "Placement" });
  });

  it("isTab accepts only 'placement'", () => {
    expect(isTab("placement")).toBe(true);
    expect(isTab("restarts")).toBe(false);
    expect(isTab("density")).toBe(false);
    expect(isTab("drift")).toBe(false);
    expect(isTab(undefined)).toBe(false);
    expect(isTab(null)).toBe(false);
    expect(isTab(42)).toBe(false);
  });
});

describe("admin/services/$service loader", () => {
  it("returns service + services + bundles + machines from one parallel fetch", async () => {
    const targetService = { _id: "svc-1", name: "api", tenantId: "alpha" };
    const allServices = [
      targetService,
      { _id: "svc-2", name: "web", tenantId: "beta" },
    ];
    const allBundles = [{ _id: "bun-1", sha256: "deadbeef", status: "ready" }];
    const allMachines = [{ _id: "mac-1", name: "alpha-1", state: "running" }];
    nimbusQueryMock
      .mockResolvedValueOnce(targetService)
      .mockResolvedValueOnce(allServices)
      .mockResolvedValueOnce(allBundles)
      .mockResolvedValueOnce(allMachines);

    const result = await loader({
      params: { service: "svc-1" },
    });

    expect(result.service).toEqual(targetService);
    expect(result.services).toEqual(allServices);
    expect(result.bundles).toEqual(allBundles);
    expect(result.machines).toEqual(allMachines);
    expect(nimbusQueryMock).toHaveBeenCalledTimes(4);
    expect(nimbusQueryMock.mock.calls[0]?.[1]).toMatchObject({ id: "svc-1" });
    expect(nimbusQueryMock.mock.calls[1]?.[1]).toMatchObject({
      tenantId: null,
      machineId: null,
      state: null,
      limit: 200,
    });
    expect(nimbusQueryMock.mock.calls[2]?.[1]).toMatchObject({
      status: null,
      limit: 50,
    });
    expect(nimbusQueryMock.mock.calls[3]?.[1]).toMatchObject({
      state: null,
      provider: null,
      limit: 200,
    });
  });

  it("throws notFound() when the service is missing", async () => {
    nimbusQueryMock
      .mockResolvedValueOnce(null)
      .mockResolvedValueOnce([])
      .mockResolvedValueOnce([])
      .mockResolvedValueOnce([]);

    await expect(loader({ params: { service: "missing" } })).rejects.toThrow(
      "__NOT_FOUND__",
    );
    expect(notFoundMock).toHaveBeenCalledTimes(1);
  });
});

describe("admin/services/$service errorComponent", () => {
  it("renders the diagnostic envelope with the loader-error message and a Retry CTA", () => {
    render(<AdminServiceDetailLoaderError error={new Error("convex down")} />);
    expect(
      screen.getByTestId("storage-server-error-envelope"),
    ).toBeInTheDocument();
    expect(
      screen.getByTestId("storage-server-error-envelope-title"),
    ).toHaveTextContent("Service detail unavailable");
    expect(
      screen.getByTestId("storage-server-error-envelope-cta"),
    ).toHaveTextContent("Retry");
    expect(screen.getByTestId("storage-server-error")).toHaveTextContent(
      "convex down",
    );
  });
});
