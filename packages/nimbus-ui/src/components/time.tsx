import { useEffect, useState } from "react";

import {
  formatAbsoluteTime,
  formatRelativeTime,
  formatUptime,
} from "../lib/format";

export function RelativeTime({ epochMs }: { epochMs: number }) {
  const now = useNow(15_000);
  return (
    <time
      className="tabular text-muted"
      dateTime={new Date(epochMs).toISOString()}
      title={formatAbsoluteTime(epochMs)}
    >
      {formatRelativeTime(epochMs, now)}
    </time>
  );
}

export function Uptime({ startedAtMs }: { startedAtMs: number }) {
  const now = useNow(30_000);
  return <span className="tabular">{formatUptime(startedAtMs, now)}</span>;
}

function useNow(intervalMs: number) {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    const id = window.setInterval(() => setNow(Date.now()), intervalMs);
    return () => window.clearInterval(id);
  }, [intervalMs]);
  return now;
}
