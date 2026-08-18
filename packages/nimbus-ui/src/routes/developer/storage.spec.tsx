import { act, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@tanstack/react-router", () => ({
  createFileRoute: () => (config: Record<string, unknown>) => config,
  useNavigate: () => () => undefined,
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

describe("StoragePage loading state", () => {
  it("holds the table geometry with skeleton rows instead of a centered label", async () => {
    mockTenants(["demo"]);
    useQueryMock.mockReturnValue(undefined);

    const { StoragePage } = await loadPage("demo");
    // `useTenantList` resolves its fetch after mount; settle it so the render
    // under assertion is the committed one.
    await act(async () => {
      render(<StoragePage />);
    });

    const loading = screen.getByTestId("tenant-tables-loading");
    expect(
      loading.querySelectorAll('[data-testid="skeleton-row"]'),
    ).toHaveLength(8);
    // The header is what makes the swap invisible: it must survive the load.
    expect(loading).toHaveTextContent("Last write");
    expect(screen.queryByTestId("tenant-tables-empty")).toBeNull();
  });

  it("still shows the empty state once the query settles on zero tables", async () => {
    mockTenants(["demo"]);
    useQueryMock.mockReturnValue([]);

    const { StoragePage } = await loadPage("demo");
    render(<StoragePage />);

    await waitFor(() => {
      expect(screen.getByTestId("tenant-tables-empty")).toHaveTextContent(
        /No tables/i,
      );
    });
    expect(screen.queryByTestId("tenant-tables-loading")).toBeNull();
  });

  it("keeps the skeleton header in step with the loaded table header", async () => {
    mockTenants(["demo"]);
    const { StoragePage } = await loadPage("demo");

    useQueryMock.mockReturnValue(undefined);
    const loading = render(<StoragePage />);
    const skeletonHeader = headerLabels(loading.container);
    loading.unmount();

    useQueryMock.mockReturnValue([
      { _id: "t1", name: "messages", rowCount: 2, lastWriteAt: 1 },
    ]);
    const loaded = await act(async () => render(<StoragePage />));

    expect(skeletonHeader).toHaveLength(5);
    expect(headerLabels(loaded.container)).toEqual(skeletonHeader);
  });
});

// The loading header lives in the route and the loaded header lives in
// `TablesListTable`, so drift between them is the one failure this route's
// skeleton can hide.
function headerLabels(container: HTMLElement) {
  return Array.from(container.querySelectorAll("thead th")).map((cell) =>
    cell.textContent?.trim(),
  );
}

// `vi.resetModules()` gives the route a fresh `ui-store` module, so the active
// tenant has to be set on that instance rather than on the one this file
// imported.
async function loadPage(activeTenant: string | null = null) {
  vi.resetModules();
  const mod = await import("./storage");
  const store = await import("../../store/ui-store");
  store.useUiStore.setState({ activeTenant });
  return { StoragePage: routeComponent(mod.Route) };
}
