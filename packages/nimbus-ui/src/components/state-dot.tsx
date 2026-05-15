import { cn } from "../lib/cn";

export type ConnState = "connected" | "reconnecting" | "offline";

const palette: Record<ConnState, { color: string; label: string }> = {
  connected: { color: "var(--color-success)", label: "Connected" },
  reconnecting: { color: "var(--color-warning)", label: "Reconnecting" },
  offline: { color: "var(--color-danger)", label: "Offline" },
};

export function StateDot({
  state,
  className,
}: {
  state: ConnState;
  className?: string;
}) {
  const entry = palette[state];
  return (
    <span
      aria-label={entry.label}
      role="img"
      className={cn(
        "inline-block size-2 rounded-full",
        state === "reconnecting" && "animate-pulse",
        className,
      )}
      style={{ background: entry.color }}
    />
  );
}
