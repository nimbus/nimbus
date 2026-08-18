import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const { pathnameRef, navigateMock, useQueryMock, tenantListRef } = vi.hoisted(
  () => ({
    pathnameRef: { current: "/developer/compute" },
    navigateMock: vi.fn(),
    useQueryMock: vi.fn(),
    tenantListRef: {
      current: { kind: "loaded", tenants: [{ id: "acme", backend: "sqlite" }] },
    },
  }),
);

vi.mock("@tanstack/react-router", () => ({
  useNavigate: () => navigateMock,
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
