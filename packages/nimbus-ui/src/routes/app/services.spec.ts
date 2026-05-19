import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@tanstack/react-router", () => ({
  createFileRoute: () => (config: Record<string, unknown>) => config,
}));

const { nimbusQueryMock } = vi.hoisted(() => ({
  nimbusQueryMock: vi.fn(),
}));

vi.mock("../../lib/nimbus-client", () => ({
  getNimbusClient: () => ({ query: nimbusQueryMock }),
}));

import { useUiStore } from "../../store/ui-store";
import { Route } from "./services";

type LoaderArgs = Record<string, unknown>;

const routeInternals = Route as unknown as {
  loader: (args: LoaderArgs) => Promise<{
    services: unknown[];
    activeTenant: string | null;
  }>;
};

beforeEach(() => {
  nimbusQueryMock.mockReset();
  useUiStore.setState({ activeTenant: null });
});

afterEach(() => {
  useUiStore.setState({ activeTenant: null });
  vi.restoreAllMocks();
});

describe("app/services loader", () => {
  it("queries services scoped to the active tenant from the Zustand store", async () => {
    useUiStore.setState({ activeTenant: "acme" });
    const services = [{ _id: "s1", name: "api", tenantId: "acme" }];
    nimbusQueryMock.mockResolvedValue(services);

    const result = await routeInternals.loader({});

    expect(nimbusQueryMock.mock.calls[0]?.[1]).toMatchObject({
      tenantId: "acme",
      machineId: null,
      state: null,
      limit: 200,
    });
    expect(result.services).toEqual(services);
    expect(result.activeTenant).toBe("acme");
  });

  it("passes activeTenant=null when no tenant is selected", async () => {
    nimbusQueryMock.mockResolvedValue([]);

    const result = await routeInternals.loader({});

    expect(nimbusQueryMock.mock.calls[0]?.[1]?.tenantId).toBeNull();
    expect(result.activeTenant).toBeNull();
  });

  it("captures the snapshot of activeTenant taken at load time", async () => {
    useUiStore.setState({ activeTenant: "alpha" });
    nimbusQueryMock.mockResolvedValue([]);

    const result = await routeInternals.loader({});
    useUiStore.setState({ activeTenant: "beta" });

    expect(result.activeTenant).toBe("alpha");
  });
});
