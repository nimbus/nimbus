import { cn } from "../lib/cn";

type StateKind =
  | "ready"
  | "running"
  | "ok"
  | "healthy"
  | "active"
  | "starting"
  | "pending"
  | "queued"
  | "draining"
  | "stopping"
  | "stopped"
  | "idle"
  | "error"
  | "failed"
  | "danger"
  | "warning"
  | "warn"
  | "stale"
  | "unknown";

const palette: Record<StateKind, { color: string }> = {
  ready: { color: "var(--color-success)" },
  running: { color: "var(--color-success)" },
  ok: { color: "var(--color-success)" },
  healthy: { color: "var(--color-success)" },
  active: { color: "var(--color-success)" },
  starting: { color: "var(--color-starting)" },
  pending: { color: "var(--color-queued)" },
  queued: { color: "var(--color-queued)" },
  draining: { color: "var(--color-draining)" },
  stopping: { color: "var(--color-draining)" },
  stopped: { color: "var(--color-stale)" },
  idle: { color: "var(--color-stale)" },
  error: { color: "var(--color-danger)" },
  failed: { color: "var(--color-danger)" },
  danger: { color: "var(--color-danger)" },
  warning: { color: "var(--color-warning)" },
  warn: { color: "var(--color-warning)" },
  stale: { color: "var(--color-stale)" },
  unknown: { color: "var(--color-stale)" },
};

function resolveKind(value: string | null | undefined): StateKind {
  if (!value) return "unknown";
  const key = value.toLowerCase();
  if (key in palette) return key as StateKind;
  if (key.startsWith("err")) return "error";
  if (key === "info" || key === "debug" || key === "trace") return "idle";
  return "unknown";
}

export function StateChip({
  state,
  className,
  showDot = true,
}: {
  state: string | null | undefined;
  className?: string;
  showDot?: boolean;
}) {
  const kind = resolveKind(state);
  const label = state ?? "—";
  const color = palette[kind].color;
  return (
    <span
      className={cn(
        "inline-flex items-center gap-1 rounded px-1.5 py-px text-[11px] uppercase tracking-wide font-mono",
        className,
      )}
      style={{ color }}
      data-state={kind}
    >
      {showDot ? (
        <span
          aria-hidden
          className="inline-block size-1.5 rounded-full"
          style={{ background: color }}
        />
      ) : null}
      {label}
    </span>
  );
}
