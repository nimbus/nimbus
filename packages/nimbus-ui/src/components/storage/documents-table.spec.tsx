import { fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import type { PageResponse } from "../../lib/types/table";
import { DocumentsTable } from "./documents-table";

const PAGE: PageResponse = {
  data: [
    { _id: "doc_a", author: "ada", body: "first", tags: ["x", "y"] },
    { _id: "doc_b", author: "grace", body: "second", tags: [] },
  ],
  next_cursor: "c1",
  has_more: true,
};

const COLUMNS = ["_id", "author", "body", "tags"];

function renderTable(
  overrides: Partial<Parameters<typeof DocumentsTable>[0]> = {},
) {
  const props = {
    page: PAGE,
    columns: COLUMNS,
    selected: new Set<string>(),
    pageNumber: 1,
    loading: false,
    order: null,
    indexBacked: new Set(["_id", "author"]),
    onSort: vi.fn(),
    onSelectionChange: vi.fn(),
    onEdit: vi.fn(),
    onDelete: vi.fn(),
    onPrev: vi.fn(),
    onNext: vi.fn(),
    ...overrides,
  };
  render(<DocumentsTable {...props} />);
  return props;
}

/** The document rows on screen, excluding the row menu and skeleton rows. */
function documentRows() {
  return document.querySelectorAll(
    '[role="row"][data-testid^="documents-row-"]',
  );
}

describe("DocumentsTable rows", () => {
  it("opens the document when the row body is clicked", async () => {
    const user = userEvent.setup();
    const props = renderTable();
    await user.click(screen.getByText("first"));
    expect(props.onEdit).toHaveBeenCalledWith(PAGE.data[0]);
  });

  // The checkbox, the id chip and the inline buttons all live inside the row.
  // A row click that swallowed them would make selection impossible.
  it("leaves the interactive cells to themselves", async () => {
    const user = userEvent.setup();
    const props = renderTable();

    await user.click(screen.getByTestId("documents-select-doc_a"));
    expect(props.onSelectionChange).toHaveBeenCalledWith(["doc_a"]);
    expect(props.onEdit).not.toHaveBeenCalled();

    await user.click(screen.getByTestId("documents-delete-doc_b"));
    expect(props.onDelete).toHaveBeenCalledWith(["doc_b"]);
    expect(props.onEdit).not.toHaveBeenCalled();
  });

  it("reports the whole page from the select-all box", async () => {
    const user = userEvent.setup();
    const props = renderTable();
    await user.click(screen.getByTestId("documents-select-all"));
    expect(props.onSelectionChange).toHaveBeenCalledWith(["doc_a", "doc_b"]);
  });

  it("reports the remaining set when a selected row is unchecked", async () => {
    const user = userEvent.setup();
    const props = renderTable({ selected: new Set(["doc_a", "doc_b"]) });
    await user.click(screen.getByTestId("documents-select-doc_a"));
    expect(props.onSelectionChange).toHaveBeenCalledWith(["doc_b"]);
  });

  // DESIGN.md:1117 — right-click is a peer of click on every resource row.
  it("opens the peer menu on right-click", async () => {
    const user = userEvent.setup();
    const props = renderTable();

    fireEvent.contextMenu(screen.getByTestId("documents-row-doc_a"), {
      clientX: 40,
      clientY: 60,
    });
    const menu = screen.getByTestId("documents-row-menu");
    expect(menu).toBeInTheDocument();

    await user.click(screen.getByTestId("documents-row-menu-edit"));
    expect(props.onEdit).toHaveBeenCalledWith(PAGE.data[0]);
    expect(screen.queryByTestId("documents-row-menu")).not.toBeInTheDocument();
  });

  it("offers delete from the row menu", () => {
    const props = renderTable();
    fireEvent.contextMenu(screen.getByTestId("documents-row-doc_b"));
    fireEvent.click(screen.getByTestId("documents-row-menu-delete"));
    expect(props.onDelete).toHaveBeenCalledWith(["doc_b"]);
  });

  // Right-click is not reachable from the keyboard, so the same menu has to
  // answer Shift+F10 and the ContextMenu key, anchored to the focused row.
  it("raises the same menu from the keyboard", () => {
    renderTable();
    const row = screen.getByTestId("documents-row-doc_a");
    fireEvent.keyDown(row, { key: "F10", shiftKey: true });
    expect(screen.getByTestId("documents-row-menu")).toBeInTheDocument();
    fireEvent.keyDown(screen.getByTestId("documents-row-menu-edit"), {
      key: "Escape",
    });

    fireEvent.keyDown(row, { key: "ContextMenu" });
    expect(screen.getByTestId("documents-row-menu")).toBeInTheDocument();
  });

  it("activates a row with Enter and moves focus with the arrow keys", () => {
    const props = renderTable();
    const first = screen.getByTestId("documents-row-doc_a");
    first.focus();
    fireEvent.keyDown(first, { key: "Enter" });
    expect(props.onEdit).toHaveBeenCalledWith(PAGE.data[0]);

    fireEvent.keyDown(first, { key: "ArrowDown" });
    expect(screen.getByTestId("documents-row-doc_b")).toHaveFocus();
  });

  // A roving tabindex keeps the grid to one tab stop; 200 rows at tabIndex 0
  // would be worse for a keyboard user than none.
  it("keeps exactly one row in the tab order", () => {
    renderTable();
    const rows = PAGE.data.map((d) =>
      screen.getByTestId(`documents-row-${d._id}`),
    );
    expect(rows.filter((r) => r.getAttribute("tabindex") === "0")).toHaveLength(
      1,
    );
  });

  it("marks a selected row for assistive technology", () => {
    renderTable({ selected: new Set(["doc_a"]) });
    expect(screen.getByTestId("documents-row-doc_a")).toHaveAttribute(
      "aria-selected",
      "true",
    );
    expect(screen.getByTestId("documents-row-doc_b")).not.toHaveAttribute(
      "aria-selected",
      "true",
    );
    expect(screen.getByTestId("documents-pagination")).toHaveTextContent(
      "1 selected",
    );
  });
});

describe("DocumentsTable headers", () => {
  it("makes every column header a sort control", async () => {
    const user = userEvent.setup();
    const props = renderTable();
    await user.click(screen.getByTestId("documents-sort-author"));
    expect(props.onSort).toHaveBeenCalledWith("author");
  });

  // TanStack cycles a column asc, desc, off. The page owns the direction
  // and treats every click on the active column as a flip, so the "off"
  // step of the cycle still has to name the active field.
  it("names the active column again when the sort cycle would clear it", async () => {
    const user = userEvent.setup();
    const props = renderTable({
      order: { field: "author", direction: "desc" },
    });
    await user.click(screen.getByTestId("documents-sort-author"));
    expect(props.onSort).toHaveBeenCalledWith("author");
  });

  it("shows which column the page is sorted by", () => {
    renderTable({ order: { field: "author", direction: "desc" } });
    expect(screen.getByTestId("documents-sort-author")).toHaveAttribute(
      "data-active",
      "true",
    );
    expect(screen.getByTestId("documents-sort-body")).toHaveAttribute(
      "data-active",
      "false",
    );
    expect(
      screen.getByRole("columnheader", { name: /author/ }),
    ).toHaveAttribute("aria-sort", "descending");
  });

  // DESIGN.md:269 — the browser has to make index use visible rather than
  // letting an operator fall into a full-table scan by accident.
  it("says in the header title whether a sort is index-backed", () => {
    renderTable();
    expect(screen.getByTestId("documents-sort-author")).toHaveAttribute(
      "title",
      expect.stringContaining("index-backed"),
    );
    expect(screen.getByTestId("documents-sort-body")).toHaveAttribute(
      "title",
      expect.stringContaining("scans the table"),
    );
  });
});

// DESIGN.md:889 — a loading state preserves the table's geometry, and no row
// of the previous page may be readable under the incoming page's header.
describe("DocumentsTable loading", () => {
  it("replaces the rows with skeletons while a page is in flight", () => {
    renderTable({ loading: true });

    expect(screen.getAllByTestId("documents-table-skeleton-row").length).toBe(
      PAGE.data.length,
    );
    expect(screen.getByTestId("documents-table")).toHaveAttribute(
      "aria-busy",
      "true",
    );
    expect(screen.queryByTestId("documents-row-doc_a")).not.toBeInTheDocument();
    expect(screen.queryByText("first")).not.toBeInTheDocument();
    // The header and the pager stay: the table does not collapse and reflow.
    expect(screen.getByTestId("documents-sort-author")).toBeInTheDocument();
    expect(screen.getByTestId("documents-pagination")).toHaveTextContent(
      "loading…",
    );
  });

  it("falls back to a full page of skeletons when no rows are known yet", () => {
    renderTable({
      loading: true,
      page: { data: [], next_cursor: null, has_more: false },
    });
    expect(
      screen.getAllByTestId("documents-table-skeleton-row").length,
    ).toBeGreaterThan(1);
  });

  it("freezes the pager and the select-all box while loading", async () => {
    const user = userEvent.setup();
    const props = renderTable({ loading: true, pageNumber: 2 });

    expect(screen.getByTestId("documents-prev-page")).toBeDisabled();
    expect(screen.getByTestId("documents-next-page")).toBeDisabled();
    // Selecting "all" mid-fetch would select rows the operator cannot see.
    const all = screen.getByTestId("documents-select-all");
    expect(all).not.toBeChecked();
    await user.click(all);
    expect(props.onSelectionChange).not.toHaveBeenCalled();
  });

  it("reports the page number the URL names", () => {
    renderTable({ pageNumber: 3 });
    expect(screen.getByTestId("documents-pagination")).toHaveTextContent(
      "page 3",
    );
  });
});

describe("DocumentsTable virtualization", () => {
  // A 200-row page is the norm and a filter can return more. The grid
  // renders only the rows in and around the viewport past the threshold,
  // so a full page costs the same as a short one.
  it("renders at most 60 rows for a 1,000-row page", () => {
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
    try {
      const data = Array.from({ length: 1_000 }, (_, i) => ({
        _id: `doc_${i}`,
        author: `author-${i}`,
        body: `body-${i}`,
        tags: [],
      }));
      renderTable({
        page: { data, next_cursor: "c1", has_more: true },
      });
      const grid = screen.getByTestId("documents-table");
      expect(grid).toHaveAttribute("data-virtual", "true");
      expect(grid).toHaveAttribute("aria-rowcount", "1001");
      const rendered = documentRows().length;
      expect(rendered).toBeGreaterThan(0);
      expect(rendered).toBeLessThanOrEqual(60);
      expect(screen.getByTestId("documents-pagination")).toHaveTextContent(
        "1000 rows",
      );
    } finally {
      if (offsetHeight) {
        Object.defineProperty(
          HTMLElement.prototype,
          "offsetHeight",
          offsetHeight,
        );
      } else {
        delete (HTMLElement.prototype as { offsetHeight?: number })
          .offsetHeight;
      }
    }
  });

  it("renders every row of a short page", () => {
    renderTable();
    expect(screen.getByTestId("documents-table")).not.toHaveAttribute(
      "data-virtual",
    );
    expect(documentRows()).toHaveLength(PAGE.data.length);
  });
});
