import { render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const { notFoundMock } = vi.hoisted(() => ({
  notFoundMock: vi.fn(() => new Error("__NOT_FOUND__")),
}));

vi.mock("@tanstack/react-router", () => ({
  createFileRoute: () => (config: Record<string, unknown>) => config,
  notFound: notFoundMock,
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

import { useUiStore } from "../../store/ui-store";
import { Route, ServiceDetailLoaderError } from "./services_.$service";

type LoaderArgs = {
  params: { service: string };
  deps: { activeTenant: string | null };
};

const routeInternals = Route as unknown as {
  loaderDeps: () => { activeTenant: string | null };
  loader: (args: LoaderArgs) => Promise<{
    service: unknown;
    services: unknown[];
    bundles: unknown[];
    activeTenant: string | null;
  }>;
};

beforeEach(() => {
  nimbusQueryMock.mockReset();
  notFoundMock.mockClear();
  useUiStore.setState({ activeTenant: null });
});

afterEach(() => {
  useUiStore.setState({ activeTenant: null });
  vi.restoreAllMocks();
});

describe("app/services/$service loaderDeps + loader", () => {
  it("loaderDeps snapshots activeTenant from the Zustand store", () => {
    useUiStore.setState({ activeTenant: "acme" });
    expect(routeInternals.loaderDeps()).toEqual({ activeTenant: "acme" });
  });

  it("returns service + tenant-scoped services + bundles + active tenant", async () => {
    const target = { _id: "svc-1", name: "api", tenantId: "acme" };
    const tenantServices = [target];
    const allBundles = [{ _id: "bun-1", sha256: "deadbeef", status: "ready" }];
    nimbusQueryMock
      .mockResolvedValueOnce(target)
      .mockResolvedValueOnce(tenantServices)
      .mockResolvedValueOnce(allBundles);

    const result = await routeInternals.loader({
      params: { service: "svc-1" },
      deps: { activeTenant: "acme" },
    });

    expect(result.service).toEqual(target);
    expect(result.services).toEqual(tenantServices);
    expect(result.bundles).toEqual(allBundles);
    expect(result.activeTenant).toBe("acme");
    expect(nimbusQueryMock).toHaveBeenCalledTimes(3);
    expect(nimbusQueryMock.mock.calls[0]?.[1]).toMatchObject({ id: "svc-1" });
    expect(nimbusQueryMock.mock.calls[1]?.[1]).toMatchObject({
      tenantId: "acme",
      machineId: null,
      state: null,
      limit: 200,
    });
    expect(nimbusQueryMock.mock.calls[2]?.[1]).toMatchObject({
      status: null,
      limit: 50,
    });
  });

  it("throws notFound() when the service is missing", async () => {
    nimbusQueryMock
      .mockResolvedValueOnce(null)
      .mockResolvedValueOnce([])
      .mockResolvedValueOnce([]);

    await expect(
      routeInternals.loader({
        params: { service: "missing" },
        deps: { activeTenant: null },
      }),
    ).rejects.toThrow("__NOT_FOUND__");
    expect(notFoundMock).toHaveBeenCalledTimes(1);
  });

  it("passes activeTenant=null when deps.activeTenant is null", async () => {
    const target = { _id: "svc-1", name: "api" };
    nimbusQueryMock
      .mockResolvedValueOnce(target)
      .mockResolvedValueOnce([target])
      .mockResolvedValueOnce([]);

    const result = await routeInternals.loader({
      params: { service: "svc-1" },
      deps: { activeTenant: null },
    });

    expect(result.activeTenant).toBeNull();
    expect(nimbusQueryMock.mock.calls[1]?.[1]?.tenantId).toBeNull();
  });
});

describe("app/services/$service errorComponent", () => {
  it("renders the diagnostic envelope with the loader-error message and a Retry CTA wired to reset", () => {
    const reset = vi.fn();
    render(
      <ServiceDetailLoaderError
        error={new Error("convex down")}
        reset={reset}
      />,
    );
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
    screen.getByTestId("storage-server-error-envelope-cta").click();
    expect(reset).toHaveBeenCalledTimes(1);
  });
});
