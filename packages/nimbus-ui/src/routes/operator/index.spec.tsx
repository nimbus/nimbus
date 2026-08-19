import { render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@tanstack/react-router", () => ({
  createFileRoute: () => (config: Record<string, unknown>) => config,
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

const { useQueryMock, connStateMock } = vi.hoisted(() => ({
  useQueryMock: vi.fn(),
  connStateMock: vi.fn(),
}));

vi.mock("@nimbus/nimbus/react", () => ({
  useQuery: (..._args: unknown[]) => useQueryMock(),
  useNimbusConnectionState: () => connStateMock(),
}));

import { routeComponent } from "../../test/route-internals";
import { Route } from "./index";

const NodesPage = routeComponent(Route);

beforeEach(() => {
  useQueryMock.mockReturnValue([]);
  connStateMock.mockReturnValue({
    isWebSocketConnected: true,
    hasEverConnected: true,
  });
});

afterEach(() => {
  vi.unstubAllGlobals();
});

/**
 * The Tenants tile is the one count on this page that does not ride the
 * WebSocket — it is a REST read of `/api/tenants`. Both of its failure modes
 * used to be swallowed, leaving the tile on the muted loading dot for the life
 * of the page while its three siblings resolved normally. That is the worst of
 * the three states: it promises the number is still on its way.
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
    vi.stubGlobal(
      "fetch",
      vi.fn().mockResolvedValue({
        ok: true,
        json: async () => ({ tenants: ["acme", "demo"] }),
      }),
    );

    render(<NodesPage />);

    await waitFor(() => {
      expect(screen.getByTestId("nodes-hosted-tenants")).toHaveTextContent("2");
    });
    expect(screen.queryByTestId("nodes-hosted-tenants-loading")).toBeNull();
    expect(screen.queryByTestId("nodes-hosted-tenants-error")).toBeNull();
  });
});
