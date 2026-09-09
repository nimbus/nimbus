import type { ReactNode } from "react";

import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import { cn } from "@/lib/utils";

// One row recipe for everything the sidebar stacks: nav links, the footer
// controls, the rail's scope buttons. The expanded row is a 36px line with
// an icon and a label; the rail row is the same line with the label gone and
// the icon centred, so the rail is the sidebar with its words removed, not
// a second design. The sheet gets 44px rows because a thumb is not a cursor.
export function rowClass({
  collapsed,
  inSheet = false,
  active = false,
  className,
}: {
  collapsed: boolean;
  inSheet?: boolean;
  active?: boolean;
  className?: string;
}): string {
  return cn(
    "relative flex items-center gap-2.5 rounded-sm text-sm font-medium outline-none transition-colors duration-150 ease-standard",
    inSheet ? "h-11" : "h-9",
    collapsed ? "w-10 justify-center px-0" : "w-full px-3",
    active
      ? "bg-bg-hover text-text-1"
      : "text-text-3 hover:bg-bg-hover hover:text-text-2",
    className,
  );
}

// RailTooltip names a rail row for a pointer; the row's own aria-label names
// it for a screen reader. The tooltip opens to the right because that is the
// only side with room.
export function RailTooltip({
  label,
  children,
}: {
  label: ReactNode;
  children: React.ReactElement;
}) {
  return (
    <Tooltip>
      <TooltipTrigger render={children} />
      <TooltipContent side="right" sideOffset={8}>
        {label}
      </TooltipContent>
    </Tooltip>
  );
}
