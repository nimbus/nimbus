import { useEffect, useState } from "react";

import {
  formatAbsoluteTime,
  formatRelativeTime,
  formatUptime,
} from "@/lib/format";
import { cn } from "@/lib/utils";

// RelativeTime is the one element for a moment in time: the relative phrase
// that never wraps, the exact instant in the tooltip, and the machine stamp
// on the element. It re-renders on a 15 s tick so "just now" ages. An
// absent stamp renders the dash with no tooltip.
export function RelativeTime({
  epochMs,
  className,
}: {
  epochMs: number | null | undefined;
  className?: string;
}) {
  const now = useNow(15_000);
  if (epochMs === null || epochMs === undefined || !Number.isFinite(epochMs)) {
    return <span className={cn("tabular text-text-3", className)}>—</span>;
  }
  return (
    <time
      className={cn("whitespace-nowrap tabular text-text-3", className)}
      dateTime={new Date(epochMs).toISOString()}
      title={formatAbsoluteTime(epochMs)}
    >
      {formatRelativeTime(epochMs, now)}
    </time>
  );
}

export function Uptime({
  startedAtMs,
  className,
}: {
  startedAtMs: number;
  className?: string;
}) {
  const now = useNow(30_000);
  return (
    <span className={cn("tabular", className)}>
      {formatUptime(startedAtMs, now)}
    </span>
  );
}

function useNow(intervalMs: number) {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    const id = window.setInterval(() => setNow(Date.now()), intervalMs);
    return () => window.clearInterval(id);
  }, [intervalMs]);
  return now;
}
