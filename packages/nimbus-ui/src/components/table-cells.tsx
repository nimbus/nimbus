import type { ReactNode } from "react";

import { cn } from "../lib/cn";

export function Th({
  children,
  align = "left",
}: {
  children: ReactNode;
  align?: "left" | "right";
}) {
  return (
    <th
      className={cn(
        "px-3 py-2 font-semibold",
        align === "right" ? "text-right" : "text-left",
      )}
    >
      {children}
    </th>
  );
}

export function Td({
  children,
  align = "left",
  mono,
}: {
  children: ReactNode;
  align?: "left" | "right";
  mono?: boolean;
}) {
  return (
    <td
      className={cn(
        "px-3 py-2 text-default",
        align === "right" ? "text-right" : "text-left",
        mono && "font-mono tabular",
      )}
    >
      {children}
    </td>
  );
}
