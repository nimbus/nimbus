import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import {
  DataTable,
  DataTableFooter,
  dataColumns,
  selectionColumn,
  VIRTUAL_THRESHOLD,
} from "./data-table";

type Machine = { id: string; name: string; cpu: number };

const col = dataColumns<Machine>();
const COLUMNS = [
  col.accessor("name", {
    header: "Name",
    cell: (ctx) => <a href={`/m/${ctx.row.original.id}`}>{ctx.getValue()}</a>,
  }),
  col.accessor("cpu", { header: "CPU", cell: (ctx) => ctx.getValue() }),
];

function rows(count: number): Machine[] {
  return Array.from({ length: count }, (_, i) => ({
    id: `m${i}`,
    name: `machine-${String(i).padStart(3, "0")}`,
    cpu: (i * 7) % 100,
  }));
}

describe("DataTable", () => {
  it("is an ARIA table with a header row and one row per record", () => {
    render(
      <DataTable
        columns={COLUMNS}
        data={rows(3)}
        getRowId={(m) => m.id}
        ariaLabel="Machines"
        testid="t"
      />,
    );
    const grid = screen.getByRole("table", { name: "Machines" });
    expect(grid).toHaveAttribute("aria-rowcount", "4");
    expect(grid).toHaveAttribute("aria-colcount", "2");
    expect(screen.getAllByTestId("t-row")).toHaveLength(3);
    expect(screen.getAllByTestId("t-row")[0]).toHaveAttribute(
      "aria-rowindex",
      "2",
    );
  });

  it("cycles aria-sort on the header button and reorders rows", async () => {
    const user = userEvent.setup();
    render(
      <DataTable
        columns={COLUMNS}
        data={rows(3)}
        getRowId={(m) => m.id}
        ariaLabel="Machines"
        testid="t"
      />,
    );
    const header = screen.getByRole("columnheader", { name: /CPU/ });
    expect(header).toHaveAttribute("aria-sort", "none");
    await user.click(within(header).getByRole("button"));
    expect(header).toHaveAttribute("aria-sort", "ascending");
    expect(screen.getAllByTestId("t-row")[0]).toHaveTextContent("machine-000");
    await user.click(within(header).getByRole("button"));
    expect(header).toHaveAttribute("aria-sort", "descending");
    expect(screen.getAllByTestId("t-row")[0]).toHaveTextContent("machine-002");
    await user.click(within(header).getByRole("button"));
    expect(header).toHaveAttribute("aria-sort", "none");
  });

  it("sorts from the keyboard", async () => {
    const user = userEvent.setup();
    render(
      <DataTable
        columns={COLUMNS}
        data={rows(3)}
        getRowId={(m) => m.id}
        ariaLabel="Machines"
      />,
    );
    const header = screen.getByRole("columnheader", { name: /Name/ });
    within(header).getByRole("button").focus();
    await user.keyboard("{Enter}");
    expect(header).toHaveAttribute("aria-sort", "ascending");
  });

  it("activates a row on click, Enter, and Space, but not through an inner link", async () => {
    const user = userEvent.setup();
    const onRowActivate = vi.fn();
    render(
      <DataTable
        columns={COLUMNS}
        data={rows(2)}
        getRowId={(m) => m.id}
        ariaLabel="Machines"
        onRowActivate={onRowActivate}
        testid="t"
      />,
    );
    const [first] = screen.getAllByTestId("t-row");
    await user.click(
      within(first).getByText("machine-000").parentElement as HTMLElement,
    );
    first.focus();
    await user.keyboard("{Enter}");
    await user.keyboard(" ");
    expect(onRowActivate).toHaveBeenCalledTimes(3);
    expect(onRowActivate).toHaveBeenLastCalledWith(
      expect.objectContaining({ id: "m0" }),
    );
    await user.click(within(first).getByRole("link"));
    expect(onRowActivate).toHaveBeenCalledTimes(3);
  });

  it("renders every row below the threshold and only a window above it", () => {
    // happy-dom lays nothing out, so the scroller has no offsetHeight and the
    // virtualizer would render an empty window. Give it a viewport.
    const offsetHeight = Object.getOwnPropertyDescriptor(
      HTMLElement.prototype,
      "offsetHeight",
    );
    Object.defineProperty(HTMLElement.prototype, "offsetHeight", {
      configurable: true,
      get: () => 400,
    });
    const { unmount } = render(
      <DataTable
        columns={COLUMNS}
        data={rows(VIRTUAL_THRESHOLD)}
        getRowId={(m) => m.id}
        ariaLabel="Machines"
        testid="t"
      />,
    );
    expect(screen.getByRole("table")).not.toHaveAttribute("data-virtual");
    expect(screen.getAllByTestId("t-row")).toHaveLength(VIRTUAL_THRESHOLD);
    unmount();
    render(
      <DataTable
        columns={COLUMNS}
        data={rows(1_000)}
        getRowId={(m) => m.id}
        ariaLabel="Machines"
        testid="t"
        maxHeight={400}
      />,
    );
    const grid = screen.getByRole("table");
    expect(grid).toHaveAttribute("data-virtual", "true");
    expect(grid).toHaveAttribute("aria-rowcount", "1001");
    expect(screen.getAllByTestId("t-row").length).toBeLessThan(1_000);
    if (offsetHeight) {
      Object.defineProperty(
        HTMLElement.prototype,
        "offsetHeight",
        offsetHeight,
      );
    } else {
      delete (HTMLElement.prototype as { offsetHeight?: number }).offsetHeight;
    }
  });

  it("selects rows through the selection column and reports them", async () => {
    const user = userEvent.setup();
    const onRowSelectionChange = vi.fn();
    render(
      <DataTable
        columns={[selectionColumn<Machine>((m) => m.name), ...COLUMNS]}
        data={rows(2)}
        getRowId={(m) => m.id}
        ariaLabel="Machines"
        rowSelection={{}}
        onRowSelectionChange={onRowSelectionChange}
      />,
    );
    await user.click(
      screen.getByRole("checkbox", { name: "Select machine-001" }),
    );
    const updater = onRowSelectionChange.mock.calls[0][0];
    const next = typeof updater === "function" ? updater({}) : updater;
    expect(next).toEqual({ m1: true });
    expect(
      screen.getByRole("checkbox", { name: "Select all rows" }),
    ).toBeInTheDocument();
  });

  it("shows the empty message in one row when there is nothing to list", () => {
    render(
      <DataTable
        columns={COLUMNS}
        data={[]}
        getRowId={(m) => m.id}
        ariaLabel="Machines"
        emptyMessage="No machines yet."
        testid="t"
      />,
    );
    expect(screen.getByTestId("t-empty")).toHaveTextContent("No machines yet.");
    expect(screen.getByRole("table")).toHaveAttribute("aria-rowcount", "1");
  });
});

describe("DataTableFooter", () => {
  it("states the count against the request bound", () => {
    const { unmount } = render(
      <DataTableFooter
        loaded={100}
        pageSize={100}
        noun={{ one: "record", many: "records" }}
      />,
    );
    expect(screen.getByTestId("data-table-footer")).toHaveTextContent(
      "100 records loaded · 100 per request",
    );
    unmount();
    render(
      <DataTableFooter
        loaded={1}
        pageSize={1000}
        hasMore
        noun={{ one: "file", many: "files" }}
      />,
    );
    expect(screen.getByTestId("data-table-footer")).toHaveTextContent(
      "1 file loaded · 1,000 per request · more exist past the bound",
    );
  });
});
