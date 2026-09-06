import { render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const { invalidateMock } = vi.hoisted(() => ({
  invalidateMock: vi.fn(),
}));

vi.mock("@tanstack/react-router", () => ({
  createFileRoute: () => (config: Record<string, unknown>) => config,
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

import { AdminServicesLoaderError } from "../../components/service-loader-errors";
import { routeLoader } from "../../test/route-internals";
import { Route } from "./services";

type LoaderArgs = Record<string, unknown>;

const loader = routeLoader<LoaderArgs, { services: unknown[] }>(Route);

beforeEach(() => {
  nimbusQueryMock.mockReset();
  invalidateMock.mockReset();
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

    const result = await loader({});

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
    await expect(loader({})).rejects.toThrow("convex down");
  });
});

describe("admin/services errorComponent", () => {
  it("renders the diagnostic envelope with the loader-error message and a Retry CTA", () => {
    render(<AdminServicesLoaderError error={new Error("convex down")} />);
    expect(
      screen.getByTestId("storage-server-error-envelope"),
    ).toBeInTheDocument();
    expect(
      screen.getByTestId("storage-server-error-envelope-title"),
    ).toHaveTextContent("Services endpoint unavailable");
    expect(
      screen.getByTestId("storage-server-error-envelope-cta"),
    ).toHaveTextContent("Retry");
    expect(screen.getByTestId("storage-server-error")).toHaveTextContent(
      "convex down",
    );
  });
});
