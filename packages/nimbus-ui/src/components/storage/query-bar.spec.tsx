import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import { QueryBar } from "./query-bar";
import type { DocumentFilter } from "./table-query";

function renderBar(overrides: Partial<Parameters<typeof QueryBar>[0]> = {}) {
  const props = {
    fields: ["_id", "author", "body"],
    filters: [] as DocumentFilter[],
    order: null,
    indexBacked: new Set(["_id", "author"]),
    pendingScanSort: null,
    onFiltersChange: vi.fn(),
    onOrderChange: vi.fn(),
    onConfirmScanSort: vi.fn(),
    onCancelScanSort: vi.fn(),
    ...overrides,
  };
  render(<QueryBar {...props} />);
  return props;
}

describe("QueryBar", () => {
  it("says the view is unfiltered rather than showing an empty strip", () => {
    renderBar();
    expect(screen.getByTestId("documents-query-bar")).toHaveTextContent(
      "no filters · natural order",
    );
  });

  it("renders each active filter as a removable chip", async () => {
    const user = userEvent.setup();
    const filters: DocumentFilter[] = [
      { field: "author", op: "eq", value: "ada" },
      { field: "n", op: "gte", value: 3 },
    ];
    const props = renderBar({ filters });

    expect(
      screen.getByTestId("documents-filter-chip-author"),
    ).toHaveTextContent("author = ada");
    await user.click(
      screen.getByRole("button", { name: "Remove filter author = ada" }),
    );
    expect(props.onFiltersChange).toHaveBeenCalledWith([filters[1]]);
  });

  // Documents are JSON, so a filter on a numeric field must send a number.
  // Sending "42" as a string silently matches nothing.
  it("adds a filter with its value read as JSON", async () => {
    const user = userEvent.setup();
    const props = renderBar();

    await user.click(screen.getByTestId("documents-add-filter"));
    await user.click(screen.getByTestId("documents-filter-field"));
    await user.click(screen.getByTestId("documents-filter-field-option-body"));
    await user.click(screen.getByTestId("documents-filter-op"));
    await user.click(screen.getByTestId("documents-filter-op-option-gte"));
    await user.type(screen.getByTestId("documents-filter-value"), "42");
    await user.click(screen.getByTestId("documents-filter-apply"));

    expect(props.onFiltersChange).toHaveBeenCalledWith([
      { field: "body", op: "gte", value: 42 },
    ]);
    expect(
      screen.queryByTestId("documents-filter-editor"),
    ).not.toBeInTheDocument();
  });

  it("shows the sort as its own removable chip", async () => {
    const user = userEvent.setup();
    const props = renderBar({ order: { field: "author", direction: "desc" } });

    expect(screen.getByTestId("documents-sort-chip")).toHaveTextContent(
      "sort author ↓",
    );
    await user.click(screen.getByRole("button", { name: "Clear sort" }));
    expect(props.onOrderChange).toHaveBeenCalledWith(null);
  });

  // DESIGN.md:269 — make index use visible and refuse unbounded scans. An
  // unindexed sort is sorted in memory over the whole filtered table, so it
  // needs an explicit decision rather than a silent one.
  it("gates an unindexed sort behind an explicit scan confirmation", async () => {
    const user = userEvent.setup();
    const props = renderBar({ pendingScanSort: "body" });

    const warning = screen.getByRole("alert");
    expect(warning).toHaveTextContent("unindexed sort");
    expect(warning).toHaveTextContent("body");

    await user.click(screen.getByTestId("documents-scan-confirm"));
    expect(props.onConfirmScanSort).toHaveBeenCalledTimes(1);
    await user.click(screen.getByTestId("documents-scan-cancel"));
    expect(props.onCancelScanSort).toHaveBeenCalledTimes(1);
  });

  it("marks an already-applied unindexed sort as the scan it is", () => {
    renderBar({ order: { field: "body", direction: "asc" } });
    expect(screen.getByTestId("documents-sort-chip").className).toContain(
      "warning",
    );
  });

  it("renders the column chooser slot", () => {
    render(
      <QueryBar
        fields={["_id"]}
        filters={[]}
        order={null}
        indexBacked={new Set(["_id"])}
        pendingScanSort={null}
        onFiltersChange={vi.fn()}
        onOrderChange={vi.fn()}
        onConfirmScanSort={vi.fn()}
        onCancelScanSort={vi.fn()}
        trailing={<button type="button">columns</button>}
      />,
    );
    expect(screen.getByRole("button", { name: "columns" })).toBeInTheDocument();
  });
});
