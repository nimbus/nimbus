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

import { useUiStore } from "../../store/ui-store";
import { Route, ServicesLoaderError } from "./services";

type LoaderArgs = Record<string, unknown>;

const routeInternals = Route as unknown as {
  loader: (args: LoaderArgs) => Promise<{
    services: unknown[];
    activeTenant: string | null;
  }>;
};

beforeEach(() => {
  nimbusQueryMock.mockReset();
  invalidateMock.mockReset();
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

describe("app/services errorComponent", () => {
  it("renders the diagnostic envelope with the loader-error message and a Retry CTA", () => {
    render(<ServicesLoaderError error={new Error("convex down")} />);
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
