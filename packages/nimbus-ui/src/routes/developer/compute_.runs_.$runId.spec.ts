import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const { notFoundMock } = vi.hoisted(() => ({
  notFoundMock: vi.fn(() => new Error("__NOT_FOUND__")),
}));

vi.mock("@tanstack/react-router", () => ({
  createFileRoute: () => (config: Record<string, unknown>) => config,
  notFound: notFoundMock,
}));

const { nimbusQueryMock } = vi.hoisted(() => ({
  nimbusQueryMock: vi.fn(),
}));

vi.mock("../../lib/nimbus-client", () => ({
  getNimbusClient: () => ({ query: nimbusQueryMock }),
}));

import { Route } from "./compute_.runs_.$runId";

type LoaderArgs = { params: { runId: string } };

const routeInternals = Route as unknown as {
  loader: (args: LoaderArgs) => Promise<{
    run: unknown;
    events: unknown[];
  }>;
};

beforeEach(() => {
  nimbusQueryMock.mockReset();
  notFoundMock.mockClear();
});

afterEach(() => {
  vi.restoreAllMocks();
});

describe("app/compute/runs/$runId loader", () => {
  it("fetches run + correlated events in parallel and returns them", async () => {
    const run = {
      _id: "run-1",
      status: "ok",
      functionPath: "messages:list",
      kind: "query",
      durationMs: 12,
      _creationTime: 1700000000000,
    };
    const events = [
      { _id: "evt-1", message: "started", createdAt: 1700000000000 },
      { _id: "evt-2", message: "done", createdAt: 1700000000100 },
    ];
    nimbusQueryMock
      .mockResolvedValueOnce(run)
      .mockResolvedValueOnce(events);

    const result = await routeInternals.loader({ params: { runId: "run-1" } });

    expect(result.run).toEqual(run);
    expect(result.events).toEqual(events);
    expect(nimbusQueryMock).toHaveBeenCalledTimes(2);
    expect(nimbusQueryMock.mock.calls[0]?.[1]).toMatchObject({ id: "run-1" });
    expect(nimbusQueryMock.mock.calls[1]?.[1]).toMatchObject({
      source: null,
      level: null,
      category: null,
      correlationId: "run-1",
      limit: 200,
    });
    expect(notFoundMock).not.toHaveBeenCalled();
  });

  it("throws notFound() when the run is missing", async () => {
    nimbusQueryMock
      .mockResolvedValueOnce(null)
      .mockResolvedValueOnce([]);

    await expect(
      routeInternals.loader({ params: { runId: "missing" } }),
    ).rejects.toThrow("__NOT_FOUND__");
    expect(notFoundMock).toHaveBeenCalledTimes(1);
  });
});
