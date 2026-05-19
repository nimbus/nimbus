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

import { Route } from "./services";

type LoaderArgs = Record<string, unknown>;

const routeInternals = Route as unknown as {
  loader: (args: LoaderArgs) => Promise<{ services: unknown[] }>;
};

beforeEach(() => {
  nimbusQueryMock.mockReset();
});

afterEach(() => {
  vi.restoreAllMocks();
});

describe("admin/services loader", () => {
  it("queries every service with tenantId=null and returns the snapshot", async () => {
    const services = [
      { _id: "s1", name: "api", tenantId: "alpha", state: "running" },
      { _id: "s2", name: "web", tenantId: "beta", state: "idle" },
    ];
    nimbusQueryMock.mockResolvedValue(services);

    const result = await routeInternals.loader({});

    expect(nimbusQueryMock).toHaveBeenCalledTimes(1);
    const args = nimbusQueryMock.mock.calls[0]?.[1];
    expect(args).toMatchObject({
      tenantId: null,
      machineId: null,
      state: null,
      limit: 200,
    });
    expect(result.services).toEqual(services);
  });

  it("propagates query errors so the route can show its error UI", async () => {
    nimbusQueryMock.mockRejectedValue(new Error("convex down"));
    await expect(routeInternals.loader({})).rejects.toThrow("convex down");
  });
});
