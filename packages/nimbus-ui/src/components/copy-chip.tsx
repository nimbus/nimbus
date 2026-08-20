import { useCallback, useState } from "react";
import { toast } from "sonner";

import { cn } from "../lib/cn";

export function CopyChip({
  label,
  value,
  testid,
  hideUntilHover = false,
  className,
  children,
}: {
  label: string;
  value: string;
  testid?: string;
  hideUntilHover?: boolean;
  className?: string;
  children?: React.ReactNode;
}) {
  const [copied, setCopied] = useState(false);
  const handle = useCallback(async () => {
    try {
      await navigator.clipboard.writeText(value);
      setCopied(true);
      toast(`Copied ${label}`, { description: value });
      setTimeout(() => setCopied(false), 1200);
    } catch {
      toast.error(`Failed to copy ${label}`);
    }
  }, [label, value]);
  return (
    <button
      type="button"
      onClick={handle}
      title={value}
      aria-label={`Copy ${label}: ${value}`}
      data-copy
      data-testid={testid}
      data-copied={copied || undefined}
      className={cn(
        "inline-flex max-w-[28ch] truncate rounded px-1 font-mono text-xs",
        "hover:bg-surface-2 hover:text-default focus-visible:bg-surface-2",
        // Collapse the box, do not just fade it: `opacity-0` alone keeps the
        // chip's full width reserved, which punches a hole into the row (and
        // detaches the breadcrumb chevron from its segment). The element stays
        // rendered — not `hidden` — so it keeps its place in the tab order and
        // can reveal itself on `focus-visible`.
        hideUntilHover && [
          "w-0 p-0 opacity-0 pointer-events-none",
          "transition-[width,opacity] duration-150",
          "hover:w-auto hover:px-1 hover:opacity-100 hover:pointer-events-auto",
          "focus-visible:w-auto focus-visible:px-1 focus-visible:opacity-100 focus-visible:pointer-events-auto",
          "group-hover:w-auto group-hover:px-1 group-hover:opacity-100 group-hover:pointer-events-auto",
          "group-focus-within:w-auto group-focus-within:px-1 group-focus-within:opacity-100 group-focus-within:pointer-events-auto",
        ],
        className,
      )}
    >
      {children ?? value}
    </button>
  );
}
