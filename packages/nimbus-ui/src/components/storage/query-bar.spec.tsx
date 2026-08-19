import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import { QueryBar } from "./query-bar";
import type { DocumentFilter } from "./table-query";

/**
 * Every size and padding utility on an element, variant prefixes stripped.
 *
 * happy-dom does no layout and loads no stylesheet, so a hit target has no
 * measurable geometry here — the utility class list is the only readable
 * observable. Asserting the intended pair is the *only* sizing present is the
 * part that matters: Tailwind settles a duplicate `h-*` by stylesheet order,
 * not by class-list membership, so an outvoted class is not a removed one, and
 * a stray `p-*` inside a fixed box shrinks the glyph's cell rather than
 * widening the target.
 */
function sizingClasses(el: Element) {
  return el.className
    .split(/\s+/)
    .map((token) => token.slice(token.lastIndexOf(":") + 1))
    .filter((name) =>
      /^(?:h|w|size|min-h|min-w|max-h|max-w|p|px|py|pt|pr|pb|pl)-/.test(name),
    )
    .sort();
}

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

  // DESIGN.md:877 asks for a 32px square icon button. The remove control used
  // to carry no width, height or padding at all — an 11px glyph — so dropping
  // a filter needed an 11px hit. 26px is the largest square this row allows:
  // the chip is 22px and the row is 26px (`documents-add-filter` is
  // `h-[26px]`), so 32px would overflow the row and, once chips wrap, overlap
  // the remove target of the line above through the 6px `gap-1.5`.
  it("gives the chip's remove control the largest hit target the row allows", () => {
    renderBar({ filters: [{ field: "author", op: "eq", value: "ada" }] });

    const button = screen.getByRole("button", {
      name: "Remove filter author = ada",
    });
    expect(sizingClasses(button)).toEqual(["h-[26px]", "w-[26px]"]);
    // Sizing alone parks the glyph in a corner of the new box.
    expect(button.className.split(" ")).toEqual(
      expect.arrayContaining([
        "flex",
        "items-center",
        "justify-center",
        // The chip is capped at 32ch; without this a long label squeezes the
        // box back towards the glyph it is meant to be bigger than.
        "shrink-0",
      ]),
    );

    // The bigger target must not grow what it sits in: the chip stays 22px
    // and the row stays 26px, the button overflowing 2px into the row's own
    // slack.
    expect(
      screen.getByTestId("documents-filter-chip-author").className.split(" "),
    ).toEqual(expect.arrayContaining(["h-[22px]", "items-center"]));
    expect(
      screen.getByTestId("documents-add-filter").className.split(" "),
    ).toEqual(expect.arrayContaining(["h-[26px]"]));
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

  // The value field used to cancel the console-wide focus outline and leave
  // the `--border` -> `--border-strong` tint as the whole cue: 1.35:1 -> 1.74:1
  // on this white field in warm light, which a sighted keyboard user cannot
  // see. Every other input in the console (select.tsx, -filters.tsx,
  // tenants.tsx) keeps the outline and adds the tint on top.
  it("keeps the console-wide focus outline on the value field", async () => {
    const user = userEvent.setup();
    renderBar();
    await user.click(screen.getByTestId("documents-add-filter"));

    const field = screen.getByTestId("documents-filter-value");
    // The anchor set has to include whitespace: a class list is
    // space-separated, so the ordinary form of the regression is a bare
    // `outline-none` sitting *between* two other utilities. Anchoring on
    // start-of-string and the variant colon alone misses exactly that case and
    // leaves this guard green through the regression it exists to catch. The
    // `(?![\w-])` tail keeps a longer utility that merely starts the same way
    // from matching.
    expect(field.className).not.toMatch(/(^|[\s:])outline-none(?![\w-])/);
    // The tint stays — as emphasis on top of the ring, not instead of it.
    expect(field.className).toContain("focus-visible:border-strong");
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
