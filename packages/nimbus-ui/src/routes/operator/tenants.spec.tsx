import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const { loaderDataRef, searchRef, invalidateMock, navigateMock } = vi.hoisted(
  () => ({
    loaderDataRef: { current: null as unknown },
    searchRef: { current: {} as Record<string, unknown> },
    invalidateMock: vi.fn(),
    navigateMock: vi.fn(),
  }),
);

vi.mock("@tanstack/react-router", () => ({
  createFileRoute: () => (config: Record<string, unknown>) => ({
    ...config,
    useLoaderData: () => loaderDataRef.current,
    useSearch: () => searchRef.current,
  }),
  useRouter: () => ({ invalidate: invalidateMock }),
  useNavigate: () => navigateMock,
  // The mock serialises `search` into the href: a link that carries the
  // tenant only in a prop the mock drops would look correct here while
  // navigating nowhere in the app.
  Link: ({
    to,
    search,
    children,
    "data-testid": testId,
    className,
  }: {
    to: string;
    search?: Record<string, string | undefined>;
    children: React.ReactNode;
    "data-testid"?: string;
    className?: string;
  }) => {
    const query = new URLSearchParams(
      Object.entries(search ?? {}).filter(
        (entry): entry is [string, string] => entry[1] !== undefined,
      ),
    ).toString();
    return (
      <a
        href={query ? `${to}?${query}` : to}
        data-testid={testId}
        className={className}
      >
        {children}
      </a>
    );
  },
}));

const { nimbusQueryMock } = vi.hoisted(() => ({
  nimbusQueryMock: vi.fn(),
}));

vi.mock("../../lib/nimbus-client", () => ({
  getNimbusClient: () => ({ query: nimbusQueryMock }),
}));

const { removeMock, createMock } = vi.hoisted(() => ({
  removeMock: vi.fn(),
  createMock: vi.fn(),
}));

vi.mock("../../lib/api-mutations", () => ({
  tenants: { remove: removeMock, create: createMock },
}));

const { subPanelSpecRef } = vi.hoisted(() => ({
  subPanelSpecRef: { current: null as { children?: React.ReactNode } | null },
}));

vi.mock("../../shell/sub-panel", () => ({
  useContributeSubPanel: (spec: { children?: React.ReactNode }) => {
    subPanelSpecRef.current = spec;
  },
}));

const { toastMock } = vi.hoisted(() => ({
  toastMock: Object.assign(vi.fn(), { success: vi.fn(), error: vi.fn() }),
}));

vi.mock("sonner", () => ({ toast: toastMock }));

import { routeComponent, routeLoader } from "../../test/route-internals";
import { Route, tenantRows } from "./tenants";

type LoaderResult =
  | { kind: "ok"; tenants: string[]; tables: unknown[] }
  | { kind: "error"; message: string };

const TenantsPage = routeComponent(Route);
const loader = routeLoader<{ abortController: AbortController }, LoaderResult>(
  Route,
);

type Deferred<T> = { promise: Promise<T>; resolve: (value: T) => void };

function deferred<T>(): Deferred<T> {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((r) => {
    resolve = r;
  });
  return { promise, resolve };
}

function loaded(tenants: string[], tables: unknown[] = []) {
  loaderDataRef.current = { kind: "ok", tenants, tables };
}

beforeEach(() => {
  loaderDataRef.current = null;
  searchRef.current = {};
  subPanelSpecRef.current = null;
  invalidateMock.mockReset();
  invalidateMock.mockResolvedValue(undefined);
  navigateMock.mockReset();
  nimbusQueryMock.mockReset();
  createMock.mockReset();
  removeMock.mockReset();
  toastMock.mockReset();
  toastMock.success.mockReset();
  toastMock.error.mockReset();
});

afterEach(() => {
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

describe("operator/tenants loader", () => {
  it("returns kind=error when /api/tenants is non-OK", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn().mockResolvedValue({ ok: false, status: 503 }),
    );
    const result = await loader({ abortController: new AbortController() });
    expect(result).toEqual({
      kind: "error",
      message: "Tenants endpoint returned a non-OK response.",
    });
  });

  it("returns kind=error when fetch throws", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn().mockRejectedValue(new Error("The operation was aborted")),
    );
    const result = await loader({ abortController: new AbortController() });
    expect(result).toEqual({
      kind: "error",
      message: "The operation was aborted",
    });
  });

  it("returns kind=ok with sorted tenants and the table inventory", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn().mockResolvedValue({
        ok: true,
        json: async () => ({ tenants: ["zeta", "alpha"] }),
      }),
    );
    nimbusQueryMock.mockResolvedValue([{ tenantId: "alpha", rowCount: 3 }]);
    const result = await loader({ abortController: new AbortController() });
    expect(result).toEqual({
      kind: "ok",
      tenants: ["alpha", "zeta"],
      tables: [{ tenantId: "alpha", rowCount: 3 }],
    });
  });

  it("forwards the abort signal to fetch", async () => {
    const controller = new AbortController();
    const fetchMock = vi.fn().mockResolvedValue({
      ok: true,
      json: async () => ({ tenants: [] }),
    });
    vi.stubGlobal("fetch", fetchMock);
    nimbusQueryMock.mockResolvedValue([]);
    await loader({ abortController: controller });
    expect(fetchMock.mock.calls[0]?.[1]).toMatchObject({
      signal: controller.signal,
    });
  });
});

describe("tenantRows", () => {
  it("joins the tenant list with the table inventory and keeps unlisted owners", () => {
    expect(
      tenantRows(["beta", "alpha"], [
        { tenantId: "alpha", rowCount: 3 },
        { tenantId: "alpha", rowCount: 2 },
        { tenantId: "ghost", rowCount: 1 },
        { tenantId: null },
      ] as never),
    ).toEqual([
      { tenantId: "alpha", tableCount: 2, totalRows: 5 },
      { tenantId: "beta", tableCount: 0, totalRows: 0 },
      { tenantId: "ghost", tableCount: 1, totalRows: 1 },
    ]);
  });
});

describe("operator/tenants render", () => {
  it("renders the diagnostic envelope with a Retry that reloads", () => {
    loaderDataRef.current = { kind: "error", message: "boom" };
    render(<TenantsPage />);
    expect(screen.getByTestId("tenants-error-envelope")).toBeInTheDocument();
    expect(screen.getByTestId("tenants-error")).toHaveTextContent("boom");
    expect(screen.queryByTestId("tenants-table")).toBeNull();
    fireEvent.click(screen.getByTestId("tenants-error-envelope-cta"));
    expect(invalidateMock).toHaveBeenCalledTimes(1);
  });

  it("renders the table on the happy path", () => {
    loaded(["alpha"]);
    render(<TenantsPage />);
    expect(screen.queryByTestId("tenants-error-envelope")).toBeNull();
    expect(screen.getByTestId("tenants-table")).toBeInTheDocument();
    expect(screen.getByTestId("tenants-row-alpha")).toBeInTheDocument();
  });

  it("offers Create tenant from the empty state", () => {
    loaded([]);
    render(<TenantsPage />);
    expect(screen.queryByTestId("tenants-create-dialog")).toBeNull();
    fireEvent.click(screen.getByTestId("tenants-empty-cta"));
    expect(screen.getByTestId("tenants-create-dialog")).toBeInTheDocument();
  });

  it("keeps Create as the one header action; delete is not inline", () => {
    loaded(["alpha"]);
    render(<TenantsPage />);
    expect(screen.getByTestId("tenants-create")).toHaveTextContent(
      "Create tenant",
    );
    expect(
      within(screen.getByTestId("tenants-row-alpha")).queryByText(/delete/i),
    ).toBeNull();
  });
});

describe("operator/tenants navigation", () => {
  it("points the sub-panel entry at the tenant's data", () => {
    loaded(["alpha"]);
    render(<TenantsPage />);
    render(<>{subPanelSpecRef.current?.children}</>);
    expect(screen.getByTestId("sub-panel-item-op-alpha")).toHaveAttribute(
      "href",
      "/developer/storage?as=alpha",
    );
  });

  it("navigates to the tenant's storage on a row click", () => {
    loaded(["alpha"]);
    render(<TenantsPage />);
    fireEvent.click(screen.getByTestId("tenants-row-alpha"));
    expect(navigateMock).toHaveBeenCalledWith({
      to: "/developer/storage",
      search: { as: "alpha" },
    });
  });

  it("leaves the row's own controls to themselves", () => {
    loaded(["alpha"]);
    vi.stubGlobal("navigator", {
      clipboard: { writeText: vi.fn().mockResolvedValue(undefined) },
    });
    render(<TenantsPage />);
    fireEvent.click(screen.getByTestId("tenants-copy-alpha"));
    fireEvent.click(screen.getByTestId("tenants-row-actions-alpha"));
    expect(navigateMock).not.toHaveBeenCalled();
  });
});

describe("operator/tenants create", () => {
  it("shows pending, then a toast, then the row", async () => {
    loaded(["alpha"]);
    const write = deferred<{ ok: true; data: { id: string } }>();
    createMock.mockReturnValue(write.promise);
    render(<TenantsPage />);

    fireEvent.click(screen.getByTestId("tenants-create"));
    const dialog = screen.getByTestId("tenants-create-dialog");
    const submit = within(dialog).getByTestId("tenants-create-submit");
    // Nothing typed: Create is inert, and pressing it writes nothing.
    expect(submit).toHaveAttribute("aria-disabled", "true");
    fireEvent.click(submit);
    expect(createMock).not.toHaveBeenCalled();

    fireEvent.change(within(dialog).getByTestId("tenants-create-input"), {
      target: { value: " beta " },
    });
    expect(submit).toHaveAttribute("aria-disabled", "false");
    fireEvent.click(submit);
    expect(createMock).toHaveBeenCalledWith("beta");

    // Pending: the dialog stays open, both buttons are inert but focusable,
    // and the verb says what is happening.
    expect(submit).toHaveTextContent("Creating…");
    expect(submit).toHaveAttribute("aria-disabled", "true");
    expect(within(dialog).getByTestId("tenants-create-cancel")).toHaveAttribute(
      "aria-disabled",
      "true",
    );
    fireEvent.click(within(dialog).getByTestId("tenants-create-cancel"));
    expect(screen.getByTestId("tenants-create-dialog")).toBeInTheDocument();
    expect(toastMock.success).not.toHaveBeenCalled();

    // The loader answers the reload with the new tenant.
    loaded(["alpha", "beta"]);
    await act(async () => {
      write.resolve({ ok: true, data: { id: "beta" } });
      await write.promise;
    });
    expect(toastMock.success).toHaveBeenCalledWith("Created tenant beta");
    expect(invalidateMock).toHaveBeenCalledTimes(1);
    await waitFor(() => {
      expect(screen.queryByTestId("tenants-create-dialog")).toBeNull();
    });
    expect(screen.getByTestId("tenants-row-beta")).toBeInTheDocument();
  });

  it("keeps the dialog open with the server's refusal next to the field", async () => {
    loaded(["alpha"]);
    createMock.mockResolvedValue({
      ok: false,
      error: "tenant already exists: alpha",
      status: 409,
    });
    render(<TenantsPage />);
    fireEvent.click(screen.getByTestId("tenants-create"));
    const input = screen.getByTestId("tenants-create-input");
    fireEvent.change(input, { target: { value: "alpha" } });
    await act(async () => {
      fireEvent.click(screen.getByTestId("tenants-create-submit"));
    });
    const dialog = screen.getByTestId("tenants-create-dialog");
    expect(within(dialog).getByRole("alert")).toHaveTextContent(
      "tenant already exists: alpha",
    );
    expect(input).toHaveValue("alpha");
    expect(input).toHaveAttribute("aria-invalid", "true");
    expect(toastMock.error).not.toHaveBeenCalled();
    expect(invalidateMock).not.toHaveBeenCalled();
  });

  it("submits on Enter from the field", () => {
    loaded([]);
    createMock.mockReturnValue(new Promise(() => undefined));
    render(<TenantsPage />);
    fireEvent.click(screen.getByTestId("tenants-create"));
    fireEvent.change(screen.getByTestId("tenants-create-input"), {
      target: { value: "gamma" },
    });
    fireEvent.submit(screen.getByTestId("tenants-create-form"));
    expect(createMock).toHaveBeenCalledWith("gamma");
  });

  it("opens on ?create=1 and clears the flag when dismissed", () => {
    loaded([]);
    searchRef.current = { create: 1 };
    render(<TenantsPage />);
    expect(screen.getByTestId("tenants-create-dialog")).toBeInTheDocument();
    fireEvent.click(screen.getByTestId("tenants-create-cancel"));
    expect(screen.queryByTestId("tenants-create-dialog")).toBeNull();
    expect(navigateMock).toHaveBeenCalledWith({
      to: "/operator/tenants",
      search: {},
      replace: true,
    });
  });
});

describe("operator/tenants delete", () => {
  it("lives in the row menu behind a ConfirmDialog", async () => {
    loaded(["alpha"]);
    const write = deferred<{ ok: true; data: undefined }>();
    removeMock.mockReturnValue(write.promise);
    render(<TenantsPage />);

    fireEvent.contextMenu(screen.getByTestId("tenants-row-alpha"));
    const menu = screen.getByTestId("tenants-row-menu");
    expect(within(menu).getByTestId("tenants-row-menu-open")).toBeTruthy();
    expect(within(menu).getByTestId("tenants-row-menu-copy")).toBeTruthy();
    fireEvent.click(within(menu).getByTestId("tenants-row-menu-delete"));

    const dialog = screen.getByTestId("tenants-delete-dialog");
    expect(dialog).toHaveTextContent('Delete tenant "alpha"?');
    // No tables: no typed phrase is asked for.
    expect(screen.queryByTestId("tenants-delete-dialog-typed")).toBeNull();
    expect(removeMock).not.toHaveBeenCalled();

    const confirm = screen.getByTestId("tenants-delete-dialog-confirm");
    fireEvent.click(confirm);
    expect(removeMock).toHaveBeenCalledWith("alpha");
    expect(confirm).toHaveTextContent("Working…");
    expect(confirm).toHaveAttribute("aria-disabled", "true");
    fireEvent.click(confirm);
    expect(removeMock).toHaveBeenCalledTimes(1);

    loaded([]);
    await act(async () => {
      write.resolve({ ok: true, data: undefined });
      await write.promise;
    });
    expect(toastMock.success).toHaveBeenCalledWith("Deleted tenant alpha");
    expect(invalidateMock).toHaveBeenCalledTimes(1);
    await waitFor(() => {
      expect(screen.queryByTestId("tenants-delete-dialog")).toBeNull();
    });
    expect(screen.queryByTestId("tenants-row-alpha")).toBeNull();
  });

  it("asks for the tenant id when the tenant owns tables", () => {
    loaded(["alpha"], [{ tenantId: "alpha", rowCount: 4 }]);
    render(<TenantsPage />);
    fireEvent.click(screen.getByTestId("tenants-row-actions-alpha"));
    fireEvent.click(screen.getByTestId("tenants-row-menu-delete"));
    expect(screen.getByTestId("tenants-delete-dialog")).toHaveTextContent(
      "This removes 1 table and every document in them.",
    );
    const confirm = screen.getByTestId("tenants-delete-dialog-confirm");
    expect(confirm).toHaveAttribute("aria-disabled", "true");
    fireEvent.change(screen.getByTestId("tenants-delete-dialog-typed"), {
      target: { value: "alpha" },
    });
    expect(confirm).toHaveAttribute("aria-disabled", "false");
  });

  it("keeps the dialog open with the refusal when the server says no", async () => {
    loaded(["alpha"]);
    removeMock.mockResolvedValue({ ok: false, error: "tenant is busy" });
    render(<TenantsPage />);
    fireEvent.contextMenu(screen.getByTestId("tenants-row-alpha"));
    fireEvent.click(screen.getByTestId("tenants-row-menu-delete"));
    await act(async () => {
      fireEvent.click(screen.getByTestId("tenants-delete-dialog-confirm"));
    });
    const dialog = screen.getByTestId("tenants-delete-dialog");
    expect(within(dialog).getByRole("alert")).toHaveTextContent(
      "tenant is busy",
    );
    expect(toastMock.error).not.toHaveBeenCalled();
    expect(invalidateMock).not.toHaveBeenCalled();
  });
});
