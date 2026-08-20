import type { ReactNode } from "react";

import { cn } from "../lib/cn";
import { Td } from "./data-table";

// Full-panel loading placeholder: a single centered status line that fills its
// container. The sibling `LoadingCell` handles inline value-cell loading via a
// `LoadingValue<T>` switch; this is the panel-sized counterpart the routes
// reach for while a whole table or tab is still resolving.
export function LoadingState({
  label,
  testid,
  className,
}: {
  label: string;
  testid?: string;
  className?: string;
}) {
  return (
    <div
      className={cn(
        "flex h-full items-center justify-center font-mono text-xs text-muted",
        className,
      )}
      data-testid={testid}
    >
      {label}
    </div>
  );
}

// Deterministic per-column bar widths so a skeleton reads as tabular data
// instead of a uniform block, and so it does not reflow between renders.
const BAR_WIDTHS = ["w-3/4", "w-1/2", "w-2/3", "w-5/12", "w-7/12"] as const;

/**
 * Table-shaped loading placeholder (DESIGN.md: "Loading state preserves table
 * geometry with skeleton rows"). Callers pass their real `<thead>` through
 * `head` so the header keeps its exact geometry, and the body cells are the
 * canonical `Td` primitive, so a skeleton row keeps tracking the table's cell
 * density instead of re-declaring it here.
 *
 * The table itself is `aria-hidden`; the adjacent live region announces the
 * label once instead of N empty rows.
 *
 * The placeholder bars are sized in percentages, so they add nothing to a
 * column's intrinsic width: under auto layout the skeleton's columns are the
 * widths the header alone asks for. A table whose body is wider than its
 * header therefore lands its columns somewhere else once the data arrives.
 * Such a table declares a column plan with `Th`'s `width` and passes `fixed`
 * here, and both states resolve to the same grid.
 */
export function SkeletonRows({
  columns,
  head,
  rows = 8,
  rowContentHeight = 22,
  fixed = false,
  label,
  testid,
  className,
}: {
  columns: number;
  /** The caller's `<thead>` element, rendered verbatim. */
  head?: ReactNode;
  rows?: number;
  /**
   * Lay the placeholder out with `table-fixed`, matching a loaded table that
   * declares its columns on the header. Pass it wherever the loaded table is
   * fixed, or the two disagree about where the columns sit.
   */
  fixed?: boolean;
  /**
   * Height in pixels of the content box inside each cell.
   *
   * Every table built from the canonical `Td` is floored at 40px by its
   * `h-10`, leaving a 24px content box after `py-2`. So on those tables any
   * value <= 24 renders an identical row and the default already matches --
   * measured 0.00px delta on documents, tenant tables, schedules, machines,
   * and network. Only pass this for a hand-rolled table that does not use
   * `Td`, and measure the loaded row before choosing a number.
   */
  rowContentHeight?: number;
  label: string;
  testid?: string;
  className?: string;
}) {
  return (
    <div className={cn("overflow-auto", className)} data-testid={testid}>
      <span role="status" className="sr-only">
        {label}
      </span>
      <table
        aria-hidden
        className={cn("w-full border-collapse text-sm", fixed && "table-fixed")}
      >
        {head}
        <tbody className="animate-pulse motion-reduce:animate-none">
          {Array.from({ length: rows }, (_, row) => (
            <tr
              // biome-ignore lint/suspicious/noArrayIndexKey: placeholder rows are positional and never reorder
              key={row}
              className="border-t border-app"
              data-testid="skeleton-row"
            >
              {Array.from({ length: columns }, (_, col) => (
                <Td
                  // biome-ignore lint/suspicious/noArrayIndexKey: placeholder cells are positional and never reorder
                  key={col}
                >
                  {/* The bar sits in a fixed-height box so the row matches
                      the loaded row it replaces; the surrounding padding still
                      comes from `Td`. Inline because the height is a caller
                      measurement, and a computed Tailwind class would not be
                      in the generated CSS. */}
                  <span
                    className="flex items-center"
                    style={{ height: rowContentHeight }}
                  >
                    <span
                      className={cn(
                        "block h-3 rounded bg-surface-2",
                        BAR_WIDTHS[col % BAR_WIDTHS.length],
                      )}
                    />
                  </span>
                </Td>
              ))}
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}
