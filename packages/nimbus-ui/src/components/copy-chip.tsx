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
        hideUntilHover &&
          "opacity-0 transition-opacity duration-150 hover:opacity-100 focus-visible:opacity-100 group-hover:opacity-100",
        className,
      )}
    >
      {children ?? value}
    </button>
  );
}
