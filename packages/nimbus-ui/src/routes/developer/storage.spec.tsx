import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
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

// The page hand-rolled the title/subtitle molecule instead of using
// `PageHeader`, and so missed the one thing that component exists for: the
// 68ch cap. Its 137-character description set a single unbroken line at
// 1440px, and the measure visibly changed on the step from this page to the
// table it links into, which does go through `PageHeader`. jsdom does no
// layout, so the cap utility is the only thing a test can read back.
describe("storage header", () => {
  it("caps the subtitle measure through the shared PageHeader", async () => {
    mockTenants([]);
    useQueryMock.mockReturnValue(undefined);

    const { StoragePage } = await loadPage();
    render(<StoragePage />);

    const subtitle = screen
      .getByTestId("tenant-tables-header")
      .querySelector("p");
    expect(subtitle?.textContent).toContain("Tables are reactive");
    expect(subtitle?.className.split(" ")).toContain("max-w-[68ch]");
  });

  it("names the selected tenant in the title", async () => {
    mockTenants(["demo"]);
    useQueryMock.mockReturnValue([]);

    const { StoragePage } = await loadPage("demo");
    await act(async () => {
      render(<StoragePage />);
    });

    expect(
      screen.getByTestId("tenant-tables-header").querySelector("h1")
        ?.textContent,
    ).toBe("Tables in demo");
  });

  it("falls back to the bare page name with no tenant selected", async () => {
    mockTenants(["demo"]);
    useQueryMock.mockReturnValue(undefined);

    const { StoragePage } = await loadPage();
    await act(async () => {
      render(<StoragePage />);
    });

    expect(
      screen.getByTestId("tenant-tables-header").querySelector("h1")
        ?.textContent,
    ).toBe("Storage");
  });

  it("keeps the breadcrumb above the header rather than inside it", async () => {
    mockTenants(["demo"]);
    useQueryMock.mockReturnValue([]);

    const { StoragePage } = await loadPage("demo");
    await act(async () => {
      render(<StoragePage />);
    });

    // The breadcrumb is the surviving mono treatment of the tenant id, so it
    // has to stay rendered and stay outside the header molecule.
    const header = screen.getByTestId("tenant-tables-header");
    expect(
      header.querySelector('[data-testid="tenant-breadcrumb"]'),
    ).toBeNull();
    expect(screen.getByTestId("tenant-breadcrumb")).toHaveTextContent("demo");
  });
});

/**
 * "Select a tenant" is an instruction, and it used to be the answer to all
 * three tenant-list states. A reader whose list was still in flight was told
 * to pick from a selector that had nothing in it yet, and a reader whose list
 * had failed outright was told the same thing permanently, with nothing on
 * this panel saying anything had gone wrong.
 */
describe("StoragePage tenant-list states", () => {
  it("says the tenants are loading instead of telling the reader to select one", async () => {
    // A fetch that never settles holds `useTenantList` on `{kind:"loading"}`,
    // which is the state the panel used to have no branch for.
    vi.stubGlobal(
      "fetch",
      vi.fn(() => new Promise(() => undefined)),
    );
    useQueryMock.mockReturnValue(undefined);

    const { StoragePage } = await loadPage();
    render(<StoragePage />);

    expect(
      screen.getByTestId("tenant-tables-tenants-loading"),
    ).toHaveTextContent(/Loading tenants/i);
    expect(screen.queryByTestId("tenant-tables-empty")).toBeNull();
  });

  it("reports the failed tenant read, names it, and offers a way out", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn().mockResolvedValue({
        ok: false,
        status: 503,
        json: async () => ({ error: { message: "tenant store offline" } }),
      }),
    );
    useQueryMock.mockReturnValue(undefined);

    const { StoragePage } = await loadPage();
    render(<StoragePage />);

    await waitFor(() => {
      expect(screen.getByTestId("tenant-tables-tenants-error")).toBeVisible();
    });
    // An error has to say what failed; a spinner or a directive does not.
    expect(
      screen.getByTestId("tenant-tables-tenants-error-message"),
    ).toHaveTextContent("tenant store offline");
    expect(
      screen.getByTestId("tenant-tables-tenants-error-cta"),
    ).toBeInTheDocument();
    expect(screen.queryByTestId("tenant-tables-empty")).toBeNull();
    expect(screen.queryByTestId("tenant-tables-tenants-loading")).toBeNull();
  });

  it("re-reads the endpoint when the reader retries", async () => {
    const fetchMock = vi
      .fn()
      .mockResolvedValueOnce({
        ok: false,
        status: 503,
        json: async () => ({ error: { message: "tenant store offline" } }),
      })
      .mockResolvedValue({
        ok: true,
        json: async () => ({ tenants: ["acme"] }),
      });
    vi.stubGlobal("fetch", fetchMock);
    useQueryMock.mockReturnValue(undefined);

    const { StoragePage } = await loadPage();
    render(<StoragePage />);

    await waitFor(() => {
      expect(screen.getByTestId("tenant-tables-tenants-error")).toBeVisible();
    });
    expect(fetchMock).toHaveBeenCalledTimes(1);

    await act(async () => {
      fireEvent.click(screen.getByTestId("tenant-tables-tenants-error-cta"));
    });

    // The retry has to reach the endpoint again. A button that only clears the
    // error locally is a worse dead end than the one it replaced, because it
    // looks like recovery.
    expect(fetchMock).toHaveBeenCalledTimes(2);
    await waitFor(() => {
      expect(screen.getByTestId("tenant-tables-empty")).toHaveTextContent(
        /Select a tenant/i,
      );
    });
    expect(screen.queryByTestId("tenant-tables-tenants-error")).toBeNull();
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
