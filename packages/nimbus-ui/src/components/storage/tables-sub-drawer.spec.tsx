import { render, screen } from "@testing-library/react";
import type { ReactNode } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const { pathname, searchText, contributed } = vi.hoisted(() => ({
  pathname: { value: "/developer/storage" },
  searchText: { value: "" },
  contributed: { spec: null as null | Record<string, unknown> },
}));

vi.mock("@tanstack/react-router", () => ({
  Link: ({
    params,
    children,
    ...rest
  }: {
    params?: { table: string };
    children: ReactNode;
  } & Record<string, unknown>) => (
    <a href={`/developer/storage/${params?.table ?? ""}`} {...rest}>
      {children}
    </a>
  ),
  useRouterState: ({ select }: { select: (s: unknown) => unknown }) =>
    select({ location: { pathname: pathname.value } }),
}));

vi.mock("../../shell/sub-drawer", () => ({
  useContributeSubDrawer: (spec: Record<string, unknown>) => {
    contributed.spec = spec;
  },
  useSubDrawerSearch: () => searchText.value,
}));

import type { TableDoc } from "../../lib/types/table";
import { useTablesSubDrawer } from "./tables-sub-drawer";

const TABLES: TableDoc[] = [
  { _id: "t2", name: "users", rowCount: 3 },
  { _id: "t1", name: "messages", rowCount: 25 },
  { _id: "t3", name: "sessions" },
];

function Harness(props: {
  tenant: string | null;
  tables: TableDoc[] | undefined;
  hasTenants?: boolean;
}) {
  useTablesSubDrawer({
    tenant: props.tenant,
    tables: props.tables,
    hasTenants: props.hasTenants,
  });
  return null;
}

function readContributedSpec(): Record<string, unknown> | null {
  return contributed.spec;
}

// The hook only contributes a spec; the shell renders its children. The tests
// mirror that split: mount the hook, then render what it handed the shell.
function renderDrawer(props: {
  tenant: string | null;
  tables: TableDoc[] | undefined;
  hasTenants?: boolean;
}) {
  contributed.spec = null;
  render(<Harness {...props} />);
  // Read through a call so control-flow analysis keeps the declared union:
  // the assignment above would otherwise narrow `spec` to `null`, because
  // TypeScript cannot see that rendering the harness writes it back.
  const spec = readContributedSpec();
  if (spec === null) {
    throw new Error("Expected the sub-drawer hook to contribute a spec.");
  }
  return render(<>{spec.children as ReactNode}</>);
}

beforeEach(() => {
  pathname.value = "/developer/storage";
  searchText.value = "";
});

describe("useTablesSubDrawer", () => {
  it("contributes a searchable Tables drawer", () => {
    render(<Harness tenant="demo" tables={TABLES} />);
    expect(contributed.spec).toMatchObject({
      kind: "dynamic",
      title: "Tables",
      search: { placeholder: "Filter tables" },
    });
  });

  it("lists the tenant's tables in name order", () => {
    renderDrawer({ tenant: "demo", tables: TABLES });
    const names = screen
      .getAllByRole("link")
      .map((el) => el.getAttribute("data-testid"));
    expect(names).toEqual([
      "sub-drawer-item-dev-messages",
      "sub-drawer-item-dev-sessions",
      "sub-drawer-item-dev-users",
    ]);
  });

  // The drawer is the section's navigator, so it has to say which table the
  // detail pane is showing.
  it("marks the open table as the current page", () => {
    pathname.value = "/developer/storage/messages";
    renderDrawer({ tenant: "demo", tables: TABLES });

    const open = screen.getByTestId("sub-drawer-item-dev-messages");
    expect(open).toHaveAttribute("aria-current", "page");
    expect(open).toHaveAttribute("data-active", "true");
    expect(screen.getByTestId("sub-drawer-item-dev-users")).toHaveAttribute(
      "data-active",
      "false",
    );
  });

  it("decodes a percent-encoded table name before matching", () => {
    pathname.value = "/developer/storage/my%20table";
    renderDrawer({
      tenant: "demo",
      tables: [{ _id: "t9", name: "my table" }],
    });
    expect(screen.getByTestId("sub-drawer-item-dev-my table")).toHaveAttribute(
      "aria-current",
      "page",
    );
  });

  // The drawer's search box used to be decorative: it accepted text and
  // filtered nothing.
  it("filters the list by the drawer's search text", () => {
    searchText.value = "ses";
    renderDrawer({ tenant: "demo", tables: TABLES });
    expect(screen.getAllByRole("link")).toHaveLength(1);
    expect(
      screen.getByTestId("sub-drawer-item-dev-sessions"),
    ).toBeInTheDocument();
  });

  it("says so when the search matches nothing", () => {
    searchText.value = "zzz";
    renderDrawer({ tenant: "demo", tables: TABLES });
    expect(screen.getByTestId("sub-drawer-tables-no-match")).toHaveTextContent(
      "zzz",
    );
    expect(screen.queryAllByRole("link")).toHaveLength(0);
  });

  it("explains the empty and no-tenant states instead of showing a bare list", () => {
    const empty = renderDrawer({ tenant: "demo", tables: [] });
    expect(empty.container).toHaveTextContent("No tables yet.");
    empty.unmount();

    const none = renderDrawer({ tenant: null, tables: undefined });
    expect(none.container).toHaveTextContent("Select a tenant.");
    none.unmount();

    const noTenants = renderDrawer({
      tenant: null,
      tables: undefined,
      hasTenants: false,
    });
    expect(noTenants.container).toHaveTextContent("No tenants yet.");
  });

  // `useContributeSubDrawer` resets the search box whenever the spec identity
  // changes, so a spec rebuilt on every render would erase the operator's
  // filter text as they typed it.
  it("keeps one spec identity across renders with the same tables", () => {
    const { rerender } = render(<Harness tenant="demo" tables={TABLES} />);
    const first = contributed.spec;
    rerender(<Harness tenant="demo" tables={TABLES} />);
    expect(contributed.spec).toBe(first);
  });
});
