import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import { BulkToolbar } from "./bulk-toolbar";
import { ColumnChooser } from "./column-chooser";

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

function renderChooser(
  overrides: Partial<Parameters<typeof ColumnChooser>[0]> = {},
) {
  const props = {
    available: ["author", "body", "tags"],
    visible: ["_id", "author", "body", "tags"],
    fromSchema: true,
    onToggle: vi.fn(),
    onMove: vi.fn(),
    onReset: vi.fn(),
    ...overrides,
  };
  render(<ColumnChooser {...props} />);
  return props;
}

describe("ColumnChooser", () => {
  it("reports how many columns are shown out of how many exist", () => {
    renderChooser();
    expect(screen.getByTestId("documents-column-chooser")).toHaveTextContent(
      "columns 3/3",
    );
    expect(
      screen.queryByTestId("documents-columns-hidden"),
    ).not.toBeInTheDocument();
  });

  // Hidden columns are how an operator loses a field without noticing, so the
  // closed trigger has to carry the count.
  it("says how many columns are hidden without opening the panel", () => {
    renderChooser({ visible: ["_id", "author"] });
    expect(screen.getByTestId("documents-columns-hidden")).toHaveTextContent(
      "+2 hidden",
    );
  });

  it("toggles a column's visibility", async () => {
    const user = userEvent.setup();
    const props = renderChooser();
    await user.click(screen.getByTestId("documents-column-chooser"));
    await user.click(screen.getByTestId("documents-column-toggle-body"));
    expect(props.onToggle).toHaveBeenCalledWith("body", false);
  });

  it("reorders a visible column and refuses to move it out of the row", async () => {
    const user = userEvent.setup();
    const props = renderChooser();
    await user.click(screen.getByTestId("documents-column-chooser"));

    await user.click(screen.getByRole("button", { name: "Move body left" }));
    expect(props.onMove).toHaveBeenCalledWith("body", -1);

    // `author` sits directly after the pinned `_id`; `tags` is last.
    expect(
      screen.getByRole("button", { name: "Move author left" }),
    ).toBeDisabled();
    expect(
      screen.getByRole("button", { name: "Move tags right" }),
    ).toBeDisabled();
  });

  // DESIGN.md:877 — icon button, 32px square. Both reorder controls used to
  // carry no width, height or padding at all, so the hit target was the 12px
  // chevron: a near miss lands on the row behind it and reorders nothing.
  it("gives the move-left control a 32px square hit target", async () => {
    const user = userEvent.setup();
    renderChooser();
    await user.click(screen.getByTestId("documents-column-chooser"));

    const button = screen.getByRole("button", { name: "Move body left" });
    expect(sizingClasses(button)).toEqual(["h-8", "w-8"]);
    // Sizing alone parks the chevron in a corner of the new box.
    expect(button.className.split(" ")).toEqual(
      expect.arrayContaining(["flex", "items-center", "justify-center"]),
    );

    // The row is a fixed `h-8`, so a 32px button fills it exactly instead of
    // growing it. The parent's height is this fix's headroom.
    expect((button.parentElement as HTMLElement).className.split(" ")).toEqual(
      expect.arrayContaining(["flex", "h-8", "items-center"]),
    );
  });

  it("gives the move-right control a 32px square hit target", async () => {
    const user = userEvent.setup();
    renderChooser();
    await user.click(screen.getByTestId("documents-column-chooser"));

    const button = screen.getByRole("button", { name: "Move body right" });
    expect(sizingClasses(button)).toEqual(["h-8", "w-8"]);
    expect(button.className.split(" ")).toEqual(
      expect.arrayContaining(["flex", "items-center", "justify-center"]),
    );
  });

  it("cannot move or reorder a hidden column", async () => {
    const user = userEvent.setup();
    renderChooser({ visible: ["_id", "author"] });
    await user.click(screen.getByTestId("documents-column-chooser"));
    expect(
      screen.getByRole("button", { name: "Move body left" }),
    ).toBeDisabled();
    expect(
      screen.getByRole("button", { name: "Move body right" }),
    ).toBeDisabled();
  });

  // Without a schema the field list is only what the visited pages contained.
  // Presenting that as the table's field list is how an operator concludes a
  // field does not exist.
  it("says where the field list came from", async () => {
    const user = userEvent.setup();
    const { rerender } = render(
      <ColumnChooser
        available={["author"]}
        visible={["_id", "author"]}
        fromSchema
        onToggle={vi.fn()}
        onMove={vi.fn()}
        onReset={vi.fn()}
      />,
    );
    await user.click(screen.getByTestId("documents-column-chooser"));
    expect(
      screen.getByTestId("documents-column-chooser-panel"),
    ).toHaveTextContent("schema fields");

    rerender(
      <ColumnChooser
        available={["author"]}
        visible={["_id", "author"]}
        fromSchema={false}
        onToggle={vi.fn()}
        onMove={vi.fn()}
        onReset={vi.fn()}
      />,
    );
    const panel = screen.getByTestId("documents-column-chooser-panel");
    expect(panel).toHaveTextContent("fields seen so far");
    expect(panel).toHaveTextContent("grows as you page through documents");
  });

  it("resets the saved layout", async () => {
    const user = userEvent.setup();
    const props = renderChooser();
    await user.click(screen.getByTestId("documents-column-chooser"));
    await user.click(screen.getByTestId("documents-column-reset"));
    expect(props.onReset).toHaveBeenCalledTimes(1);
  });

  it("closes on Escape and on an outside click", async () => {
    const user = userEvent.setup();
    renderChooser();
    await user.click(screen.getByTestId("documents-column-chooser"));
    await user.keyboard("{Escape}");
    expect(
      screen.queryByTestId("documents-column-chooser-panel"),
    ).not.toBeInTheDocument();

    await user.click(screen.getByTestId("documents-column-chooser"));
    await user.click(document.body);
    expect(
      screen.queryByTestId("documents-column-chooser-panel"),
    ).not.toBeInTheDocument();
  });
});

describe("BulkToolbar", () => {
  // DESIGN.md:1120-1123 — a bulk selection gets its own toolbar with the count,
  // the action, and the way out, not a `(n)` suffix on a corner button.
  it("carries the count, the destructive action and the escape hatch", async () => {
    const user = userEvent.setup();
    const onDelete = vi.fn();
    const onClear = vi.fn();
    render(<BulkToolbar count={3} onDelete={onDelete} onClear={onClear} />);

    const bar = screen.getByRole("toolbar", { name: "Bulk document actions" });
    expect(bar).toHaveTextContent("3 selected");
    expect(bar).toHaveTextContent("clears");

    await user.click(screen.getByTestId("documents-bulk-delete"));
    expect(onDelete).toHaveBeenCalledTimes(1);
    await user.click(screen.getByTestId("documents-bulk-clear"));
    expect(onClear).toHaveBeenCalledTimes(1);
  });
});
