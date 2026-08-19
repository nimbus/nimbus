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
    current: { kind: "loaded", tenants: [{ id: "acme", backend: "sqlite" }] },
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
import { CommandPalette } from "./command-palette";

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

beforeEach(() => {
  pathnameRef.current = "/developer/compute";
  navigateMock.mockReset();
  invalidateMock.mockReset().mockResolvedValue(undefined);
  window.localStorage.clear();
  useQueryMock.mockReset();
  useQueryMock.mockImplementation((ref: { name: string }, args: unknown) =>
    args === "skip" ? undefined : ROWS[ref.name],
  );
  useUiStore.setState({ paletteOpen: true, activeTenant: "acme" });
});

function type(value: string) {
  fireEvent.change(screen.getByTestId("command-palette-input"), {
    target: { value },
  });
}

describe("CommandPalette", () => {
  it("holds no query subscriptions while it is closed", () => {
    useUiStore.setState({ paletteOpen: false });
    render(<CommandPalette />);
    expect(useQueryMock).not.toHaveBeenCalled();
  });

  it("holds no query subscriptions until the operator types", () => {
    render(<CommandPalette />);
    expect(screen.getByTestId("command-palette")).toBeInTheDocument();
    expect(useQueryMock).not.toHaveBeenCalled();
  });

  it("reaches a table by name, not only the drawer sections", async () => {
    render(<CommandPalette />);
    type("documents");
    await waitFor(() => {
      expect(
        screen.getByTestId("palette-item-table:tbl_1"),
      ).toBeInTheDocument();
    });
    // "documents" must not also match the Storage section, or the resource row
    // would be buried under the 15 sections it was added to complement.
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
    const stored = JSON.parse(
      window.localStorage.getItem("nimbus-ui:palette:recent") ?? "[]",
    );
    expect(stored[0]).toMatchObject({
      kind: "table",
      key: "table:tbl_1",
      href: "/developer/storage/documents",
    });
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

  // The palette's one text field cancelled the console-wide outline and put
  // nothing in its place. `autoFocus` masks that on open; tab to the mode
  // toggle and back and the caret is the only thing left saying where focus
  // is. Vitest runs with `css: false`, so there is no cascade here to measure —
  // what this holds is that the input does not opt out of the outline the base
  // layer paints, and that it names no other token in its place. The
  // repo-wide version of the rule lives in styles/contrast.spec.ts.
  it("keeps the console-wide focus outline on the palette input", () => {
    render(<CommandPalette />);
    const input = screen.getByTestId("command-palette-input");
    // Anchored on whitespace as well as on `:` and the start of the string:
    // the two precedents for this assertion (documents-table.spec.tsx,
    // query-bar.spec.tsx) match `(^|:)outline-none`, which reads a variant
    // prefix but walks straight past a bare `outline-none` sitting between two
    // other utilities — which is exactly the form this input carried.
    expect(input.className).not.toMatch(/(^|[\s:])outline-none(?![\w-])/);
    for (const [, token] of input.className.matchAll(
      /var\((--nimbus-[a-z0-9-]+)\)/g,
    )) {
      expect(token).toBe("--nimbus-focus");
    }
  });

  // "Refresh current view" used to call window.location.reload(), which is a
  // different action than the one it names: the socket drops, and the drawer
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
      fireEvent.click(screen.getByTestId("palette-mode-run"));
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

  it("says so when the tenant list fails instead of dropping the group", async () => {
    tenantListRef.current = { kind: "error", message: "boom" } as never;
    render(<CommandPalette />);
    type("acme");
    await waitFor(() => {
      expect(
        screen.getByTestId("palette-group-tenants-error"),
      ).toHaveTextContent("boom");
    });
    tenantListRef.current = {
      kind: "loaded",
      tenants: [{ id: "acme", backend: "sqlite" }],
    };
  });
});
