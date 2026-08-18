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
 */
export function SkeletonRows({
  columns,
  head,
  rows = 8,
  rowContentHeight = 22,
  label,
  testid,
  className,
}: {
  columns: number;
  /** The caller's `<thead>` element, rendered verbatim. */
  head?: ReactNode;
  rows?: number;
  /**
   * Height in pixels of the content box inside each cell. A loaded row is
   * sized by the tallest control a cell carries, not by the text strut, so
   * this is the knob that makes a skeleton row match a real one. Values
   * measured against the live tables at 1440px, each within 0.4px of the
   * loaded row: documents 22 (the default), tenant tables 21, schedules 18,
   * machines 34 — machines is taller because its host-name cell wraps.
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
      <table aria-hidden className="w-full border-collapse text-sm">
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
