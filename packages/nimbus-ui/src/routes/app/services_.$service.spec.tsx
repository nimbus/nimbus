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

import { useUiStore } from "../../store/ui-store";
import { Route, ServiceDetailLoaderError } from "./services_.$service";

type LoaderArgs = { params: { service: string } };

const routeInternals = Route as unknown as {
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
  invalidateMock.mockReset();
  useUiStore.setState({ activeTenant: null });
});

afterEach(() => {
  useUiStore.setState({ activeTenant: null });
  vi.restoreAllMocks();
});

describe("app/services/$service loader", () => {
  it("returns service + tenant-scoped services + bundles + active tenant", async () => {
    useUiStore.setState({ activeTenant: "acme" });
    const target = { _id: "svc-1", name: "api", tenantId: "acme" };
    const tenantServices = [target];
    const allBundles = [{ _id: "bun-1", sha256: "deadbeef", status: "ready" }];
    nimbusQueryMock
      .mockResolvedValueOnce(target)
      .mockResolvedValueOnce(tenantServices)
      .mockResolvedValueOnce(allBundles);

    const result = await routeInternals.loader({
      params: { service: "svc-1" },
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
      routeInternals.loader({ params: { service: "missing" } }),
    ).rejects.toThrow("__NOT_FOUND__");
    expect(notFoundMock).toHaveBeenCalledTimes(1);
  });

  it("passes activeTenant=null when no tenant is selected", async () => {
    const target = { _id: "svc-1", name: "api" };
    nimbusQueryMock
      .mockResolvedValueOnce(target)
      .mockResolvedValueOnce([target])
      .mockResolvedValueOnce([]);

    const result = await routeInternals.loader({
      params: { service: "svc-1" },
    });

    expect(result.activeTenant).toBeNull();
    expect(nimbusQueryMock.mock.calls[1]?.[1]?.tenantId).toBeNull();
  });
});

describe("app/services/$service errorComponent", () => {
  it("renders the diagnostic envelope with the loader-error message and a Retry CTA", () => {
    render(<ServiceDetailLoaderError error={new Error("convex down")} />);
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
