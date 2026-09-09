import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const {
  pathnameRef,
  navigateMock,
  invalidateMock,
  useQueryMock,
  tenantListRef,
} = vi.hoisted(() => ({
  pathnameRef: { current: "/developer/compute" },
  navigateMock: vi.fn(),
  invalidateMock: vi.fn(),
  useQueryMock: vi.fn(),
  tenantListRef: {
    current: {
      kind: "loaded",
      tenants: [
        { id: "acme", backend: "sqlite" },
        { id: "globex", backend: "libsql" },
      ],
    },
  },
}));

vi.mock("@tanstack/react-router", () => ({
  useNavigate: () => navigateMock,
  useRouter: () => ({ invalidate: invalidateMock }),
  useRouterState: ({
    select,
  }: {
    select: (s: { location: { pathname: string } }) => unknown;
  }) => select({ location: { pathname: pathnameRef.current } }),
}));

vi.mock("@nimbus/nimbus/react", () => ({
  useQuery: (ref: { name: string }, args: unknown) => useQueryMock(ref, args),
}));

vi.mock("../hooks/use-tenant-list", () => ({
  useTenantList: () => tenantListRef.current,
}));

import { useUiStore } from "../store/ui-store";
import { CommandPalette, RECENT_KEY } from "./command-palette";

const ROWS: Record<string, Array<Record<string, unknown>>> = {
  "tables:list": [
    { _id: "tbl_1", name: "documents" },
    { _id: "tbl_2", name: "sessions" },
  ],
  "functions:list": [{ _id: "fn_1", path: "messages:send", kind: "mutation" }],
  "services:list": [{ _id: "svc_1", name: "mailer", state: "running" }],
  "machines:list": [{ _id: "mch_1", name: "node-a", state: "running" }],
  "routes:list": [
    { _id: "rt_1", path: "/hello", method: "GET", adapter: "convex" },
  ],
};

const LOADED_TENANTS = {
  kind: "loaded",
  tenants: [
    { id: "acme", backend: "sqlite" },
    { id: "globex", backend: "libsql" },
  ],
};

beforeEach(() => {
  pathnameRef.current = "/developer/compute";
  navigateMock.mockReset();
  invalidateMock.mockReset().mockResolvedValue(undefined);
  window.localStorage.clear();
  useQueryMock.mockReset();
  useQueryMock.mockImplementation((ref: { name: string }, args: unknown) =>
    args === "skip" ? undefined : ROWS[ref.name],
  );
  tenantListRef.current = LOADED_TENANTS;
  useUiStore.setState({ paletteOpen: true, activeTenant: "acme" });
});

function type(value: string) {
  fireEvent.change(screen.getByTestId("command-palette-input"), {
    target: { value },
  });
}

function key(name: string) {
  fireEvent.keyDown(screen.getByTestId("command-palette-input"), {
    key: name,
  });
}

function selectedTestId(): string | null {
  const selected = document.querySelectorAll('[data-selected="true"]');
  expect(selected).toHaveLength(1);
  return selected[0]?.getAttribute("data-testid") ?? null;
}

describe("CommandPalette", () => {
  it("holds no query subscriptions while it is closed", () => {
    useUiStore.setState({ paletteOpen: false });
    render(<CommandPalette />);
    expect(useQueryMock).not.toHaveBeenCalled();
    expect(screen.queryByTestId("command-palette")).toBeNull();
  });

  it("holds no query subscriptions until the operator types", () => {
    render(<CommandPalette />);
    expect(screen.getByTestId("command-palette")).toBeInTheDocument();
    expect(useQueryMock).not.toHaveBeenCalled();
  });

  it("is a modal dialog with a 640px surface", () => {
    render(<CommandPalette />);
    const dialog = screen.getByRole("dialog");
    expect(dialog).toHaveAccessibleName("Command palette");
    expect(dialog.className.split(/\s+/)).toContain("sm:max-w-[640px]");
  });

  // The three groups are the palette's fixed shape. Resources join them only
  // once the operator types, so an open palette is the console map, not a
  // dump of every row on the server.
  it("opens on the Routes, Tenants and Actions groups", () => {
    render(<CommandPalette />);
    const routes = screen.getByTestId("palette-group-routes");
    const tenants = screen.getByTestId("palette-group-tenants");
    const actions = screen.getByTestId("palette-group-actions");
    expect(routes).toHaveTextContent("Routes");
    expect(tenants).toHaveTextContent("Tenants");
    expect(actions).toHaveTextContent("Actions");
    // The current view's pages lead; the other console follows.
    const rows = Array.from(
      routes.querySelectorAll('[data-testid^="palette-item-"]'),
    ).map((row) => row.getAttribute("data-testid"));
    expect(rows[0]).toBe("palette-item-developer:overview");
    expect(rows).toContain("palette-item-operator:machines");
    expect(rows.indexOf("palette-item-operator:nodes")).toBeGreaterThan(
      rows.indexOf("palette-item-developer:settings"),
    );
    expect(screen.getByTestId("palette-item-tenant:acme")).toHaveAttribute(
      "data-checked",
      "true",
    );
    expect(
      screen.getByTestId("palette-item-tenant:globex"),
    ).not.toHaveAttribute("data-checked");
    expect(
      screen.getByTestId("palette-action-Refresh current view"),
    ).toBeInTheDocument();
    expect(
      screen.getByTestId("palette-action-Open system tenant lens"),
    ).toBeInTheDocument();
    expect(
      screen.getByTestId("palette-action-Switch to Operator console"),
    ).toBeInTheDocument();
  });

  it("moves the selection with the arrow keys and opens it with Enter", () => {
    render(<CommandPalette />);
    expect(selectedTestId()).toBe("palette-item-developer:overview");
    key("ArrowDown");
    expect(selectedTestId()).toBe("palette-item-developer:compute");
    key("ArrowDown");
    expect(selectedTestId()).toBe("palette-item-developer:deploys");
    key("ArrowUp");
    expect(selectedTestId()).toBe("palette-item-developer:compute");
    key("Enter");
    expect(navigateMock).toHaveBeenCalledWith({ to: "/developer/compute" });
    expect(useUiStore.getState().paletteOpen).toBe(false);
  });

  it("closes through the dialog and hands focus back to the store", () => {
    render(<CommandPalette />);
    fireEvent.keyDown(screen.getByRole("dialog"), { key: "Escape" });
    expect(useUiStore.getState().paletteOpen).toBe(false);
  });

  it("writes the keyboard contract in the footer", () => {
    render(<CommandPalette />);
    const footer = screen.getByTestId("command-palette-footer");
    expect(footer).toHaveTextContent("palette");
    expect(footer).toHaveTextContent("tenant lens");
    expect(footer).toHaveTextContent("filter page");
    const keys = Array.from(footer.querySelectorAll("[data-slot=kbd]")).map(
      (node) => node.textContent,
    );
    expect(keys).toContain("K");
    expect(keys).toContain("\\");
    expect(keys).toContain("/");
  });

  it("keeps the tenant lens chord off the operator console", () => {
    pathnameRef.current = "/operator/machines";
    render(<CommandPalette />);
    expect(screen.getByTestId("command-palette-footer")).not.toHaveTextContent(
      "tenant lens",
    );
    expect(
      screen.queryByTestId("palette-action-Open system tenant lens"),
    ).toBeNull();
    expect(
      screen.getByTestId("palette-action-Switch to Developer console"),
    ).toBeInTheDocument();
  });

  it("reaches a table by name, not only the console pages", async () => {
    render(<CommandPalette />);
    type("documents");
    await waitFor(() => {
      expect(
        screen.getByTestId("palette-item-table:tbl_1"),
      ).toBeInTheDocument();
    });
    // "documents" must not also match the Storage page, or the resource row
    // would be buried under the pages it was added to complement.
    expect(screen.queryByTestId("palette-item-developer:storage")).toBeNull();
  });

  it("reaches a resource by id as well as by name", async () => {
    render(<CommandPalette />);
    type("svc_1");
    await waitFor(() => {
      expect(
        screen.getByTestId("palette-item-service:svc_1"),
      ).toBeInTheDocument();
    });
  });

  it("marks exactly one row selected so Enter has a visible subject", async () => {
    render(<CommandPalette />);
    type("documents");
    await waitFor(() => {
      expect(
        screen.getByTestId("palette-item-table:tbl_1"),
      ).toBeInTheDocument();
    });
    const selected = document.querySelectorAll('[data-selected="true"]');
    expect(selected).toHaveLength(1);
    expect(selected[0]).toHaveAttribute("aria-selected", "true");
    expect(selected[0]).toHaveAttribute(
      "data-testid",
      "palette-item-table:tbl_1",
    );
  });

  it("navigates to a resolved href and remembers the target", async () => {
    render(<CommandPalette />);
    type("documents");
    await waitFor(() => {
      expect(
        screen.getByTestId("palette-item-table:tbl_1"),
      ).toBeInTheDocument();
    });
    fireEvent.click(screen.getByTestId("palette-item-table:tbl_1"));
    expect(navigateMock).toHaveBeenCalledWith({
      to: "/developer/storage/documents",
    });
    // The whole target is persisted, not a bare key: a key alone could only be
    // resolved against the nav list, so every stored resource would come back
    // as a dead row.
    const stored = JSON.parse(window.localStorage.getItem(RECENT_KEY) ?? "[]");
    expect(stored[0]).toMatchObject({
      kind: "table",
      key: "table:tbl_1",
      href: "/developer/storage/documents",
    });
  });

  it("lists recents ahead of the routes on the next open", () => {
    window.localStorage.setItem(
      RECENT_KEY,
      JSON.stringify([
        {
          kind: "table",
          key: "table:tbl_1",
          label: "documents",
          href: "/developer/storage/documents",
        },
      ]),
    );
    render(<CommandPalette />);
    expect(screen.getByTestId("palette-group-recent")).toHaveTextContent(
      "documents",
    );
    expect(selectedTestId()).toBe("palette-item-table:tbl_1");
  });

  it("skips the tenant-scoped table read when no tenant is active", () => {
    useUiStore.setState({ activeTenant: null });
    render(<CommandPalette />);
    type("documents");
    const tableCall = useQueryMock.mock.calls.find(
      ([ref]) => ref.name === "tables:list",
    );
    expect(tableCall?.[1]).toBe("skip");
  });

  it("makes a tenant the active scope from the developer console", () => {
    render(<CommandPalette />);
    fireEvent.click(screen.getByTestId("palette-item-tenant:globex"));
    expect(useUiStore.getState().activeTenant).toBe("globex");
    expect(useUiStore.getState().paletteOpen).toBe(false);
    expect(navigateMock).not.toHaveBeenCalled();
  });

  it("lands on the developer console when a tenant is picked elsewhere", () => {
    pathnameRef.current = "/operator/machines";
    render(<CommandPalette />);
    fireEvent.click(screen.getByTestId("palette-item-tenant:globex"));
    expect(useUiStore.getState().activeTenant).toBe("globex");
    expect(navigateMock).toHaveBeenCalledWith({ to: "/developer" });
  });

  // The registry input hides the element outline in the utilities layer,
  // beneath the console's unlayered `:focus-visible` rule, so the outline
  // still paints. What this holds is that the palette never re-adds a bare
  // `outline-none` and names no token other than the accent in its place.
  it("keeps the console-wide focus outline on the palette input", () => {
    render(<CommandPalette />);
    const input = screen.getByTestId("command-palette-input");
    expect(input.className).not.toMatch(/(^|[\s:])outline-none(?![\w-])/);
    for (const [, token] of input.className.matchAll(
      /var\((--[a-z0-9-]+)\)/g,
    )) {
      expect(token).toBe("--accent");
    }
  });

  // "Refresh current view" used to call window.location.reload(), which is a
  // different action than the one it names: the socket drops, and the panel
  // state and the query bar's filters go with it.
  it("refreshes the view in place rather than reloading the app", () => {
    const reload = vi.fn();
    const location = Object.getOwnPropertyDescriptor(window, "location");
    Object.defineProperty(window, "location", {
      value: { ...window.location, reload },
      configurable: true,
    });
    try {
      render(<CommandPalette />);
      fireEvent.click(
        screen.getByTestId("palette-action-Refresh current view"),
      );
      expect(invalidateMock).toHaveBeenCalledTimes(1);
      expect(reload).not.toHaveBeenCalled();
      expect(useUiStore.getState().paletteOpen).toBe(false);
    } finally {
      if (location) Object.defineProperty(window, "location", location);
    }
  });

  it("switches the console through the view switcher rule", () => {
    render(<CommandPalette />);
    fireEvent.click(
      screen.getByTestId("palette-action-Switch to Operator console"),
    );
    expect(navigateMock).toHaveBeenCalledWith({ to: "/operator" });
    expect(useUiStore.getState().paletteOpen).toBe(false);
  });

  it("flips the theme from the actions group", () => {
    useUiStore.setState({ themeMode: "light", theme: "light" });
    render(<CommandPalette />);
    fireEvent.click(screen.getByTestId("palette-action-Switch to dark theme"));
    expect(useUiStore.getState().theme).toBe("dark");
    useUiStore.getState().setThemeMode("light");
  });

  it("says so when the tenant list fails instead of dropping the group", () => {
    tenantListRef.current = { kind: "error", message: "boom" } as never;
    render(<CommandPalette />);
    expect(screen.getByTestId("palette-group-tenants-error")).toHaveTextContent(
      "boom",
    );
    type("acme");
    expect(screen.getByTestId("palette-group-tenants-error")).toHaveTextContent(
      "boom",
    );
  });
});
