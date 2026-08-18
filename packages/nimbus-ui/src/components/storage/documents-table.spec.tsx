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
    cursorStack: [null] as Array<string | null>,
    order: null,
    indexBacked: new Set(["_id", "author"]),
    onSort: vi.fn(),
    onToggleAll: vi.fn(),
    onToggleOne: vi.fn(),
    onEdit: vi.fn(),
    onDelete: vi.fn(),
    onPrev: vi.fn(),
    onNext: vi.fn(),
    ...overrides,
  };
  render(<DocumentsTable {...props} />);
  return props;
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
    expect(props.onToggleOne).toHaveBeenCalledWith("doc_a", true);
    expect(props.onEdit).not.toHaveBeenCalled();

    await user.click(screen.getByTestId("documents-delete-doc_b"));
    expect(props.onDelete).toHaveBeenCalledWith(["doc_b"]);
    expect(props.onEdit).not.toHaveBeenCalled();
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

  // A roving tabindex keeps the grid to one tab stop; 25 rows at tabIndex 0
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
    expect(screen.getByTestId("documents-row-doc_b")).toHaveAttribute(
      "aria-selected",
      "false",
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
