import type { ReactNode } from "react";

import { cn } from "../lib/cn";

/**
 * Canonical table cells for the console's resource lists: hairline row borders,
 * normal-weight headers, tabular/right-align and mono opt-ins. This is the one
 * table-cell primitive — every resource table imports `Th`/`Td` from here.
 *
 * `align` right-aligns numeric columns; `mono` renders cell content in the
 * monospace tabular face. `className` still composes for one-off needs.
 */
export function Th({
  children,
  align = "left",
  className,
}: {
  children: ReactNode;
  align?: "left" | "right";
  className?: string;
}) {
  return (
    <th
      className={cn(
        "border-b border-app px-3 py-2 font-normal",
        align === "right" ? "text-right" : "text-left",
        className,
      )}
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
        "px-3 py-2 align-middle",
        align === "right" && "text-right",
        mono && "font-mono tabular",
        className,
      )}
    >
      {children}
    </td>
  );
}
