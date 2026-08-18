import type { ReactNode } from "react";

import { cn } from "../lib/cn";

/**
 * Canonical table cells for the console's resource lists: hairline row borders,
 * normal-weight headers, tabular/right-align and mono opt-ins. This is the one
 * table-cell primitive — every resource table imports `Th`/`Td` from here.
 *
 * `align` right-aligns numeric columns; `mono` renders cell content in the
 * monospace tabular face. `className` still composes for one-off needs.
 *
 * Cells do not wrap. A dense table's value is that every row is the same
 * height and the eye can scan a column without re-finding the baseline; one
 * cell wrapping to a second line breaks the whole grid. Long values overflow
 * into the table's own horizontal scroll instead. A cell that genuinely needs
 * to wrap opts out with `className="whitespace-normal"`.
 *
 * `h-10` is a floor, not a fixed height: on a table cell the CSS `height`
 * property behaves as a minimum. It pins every row to the same 40px dense
 * step whether or not the row carries action buttons, and still lets a row
 * grow when a cell stacks an error under its value.
 *
 * `width` declares a column's share of the table under `table-fixed`. Put it
 * on the header, not the body cells: a table's header component is the one
 * artefact its skeleton and its loaded state both render, so a column plan
 * declared there is the same plan in both and the two cannot fall out of
 * alignment. See `SkeletonRows`' `fixed` prop.
 */
export function Th({
  children,
  align = "left",
  width,
  className,
}: {
  children: ReactNode;
  align?: "left" | "right";
  /** CSS width for the column, honoured under `table-fixed`. */
  width?: string;
  className?: string;
}) {
  return (
    <th
      className={cn(
        "whitespace-nowrap border-b border-app px-3 py-2 font-normal",
        align === "right" ? "text-right" : "text-left",
        className,
      )}
      style={width ? { width } : undefined}
    >
      {children}
    </th>
  );
}

export function Td({
  children,
  align,
  mono = false,
  className,
}: {
  children: ReactNode;
  align?: "left" | "right";
  mono?: boolean;
  className?: string;
}) {
  return (
    <td
      className={cn(
        "h-10 whitespace-nowrap px-3 py-2 align-middle",
        align === "right" && "text-right",
        mono && "font-mono tabular",
        className,
      )}
    >
      {children}
    </td>
  );
}
