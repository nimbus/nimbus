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

const { useQueryMock } = vi.hoisted(() => ({
  useQueryMock: vi.fn(),
}));

vi.mock("@nimbus/nimbus/react", () => ({
  useQuery: (..._args: unknown[]) => useQueryMock(),
}));

vi.mock("../../shell/sub-drawer", () => ({
  useContributeSubDrawer: () => undefined,
}));

import { useUiStore } from "../../store/ui-store";
import { routeComponent } from "../../test/route-internals";

function mockTenants(tenants: string[]) {
  vi.stubGlobal(
    "fetch",
    vi.fn().mockResolvedValue({
      ok: true,
      json: async () => ({ tenants }),
    }),
  );
}

beforeEach(() => {
  useQueryMock.mockReset();
  useUiStore.setState({ activeTenant: null });
});

afterEach(() => {
  vi.unstubAllGlobals();
  useUiStore.setState({ activeTenant: null });
});

describe("StoragePage empty states", () => {
  it("renders the create-tenant CTA when no tenant is selected and zero tenants exist", async () => {
    mockTenants([]);
    useQueryMock.mockReturnValue(undefined);

    const { StoragePage } = await loadPage();
    render(<StoragePage />);

    await waitFor(() => {
      expect(screen.getByTestId("tenant-tables-empty")).toHaveTextContent(
        /No tenants yet/i,
      );
    });
    expect(screen.getByTestId("tenant-tables-empty")).toHaveTextContent(
      /CREATE TENANT/i,
    );
  });

  it("renders the pick-a-tenant copy when no tenant is selected but tenants exist", async () => {
    mockTenants(["acme"]);
    useQueryMock.mockReturnValue(undefined);

    const { StoragePage } = await loadPage();
    render(<StoragePage />);

    await waitFor(() => {
      expect(screen.getByTestId("tenant-tables-empty")).toHaveTextContent(
        /Select a tenant/i,
      );
    });
    expect(screen.getByTestId("tenant-tables-empty")).toHaveTextContent(
      /Pick a tenant from the top-nav selector/i,
    );
    expect(screen.getByTestId("tenant-tables-empty")).not.toHaveTextContent(
      /CREATE TENANT/i,
    );
  });
});

async function loadPage() {
  vi.resetModules();
  const mod = await import("./storage");
  return { StoragePage: routeComponent(mod.Route) };
}
