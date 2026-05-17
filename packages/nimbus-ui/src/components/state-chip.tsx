import { cn } from "../lib/cn";

type StateKind =
  | "ready"
  | "healthy"
  | "ok"
  | "active"
  | "running"
  | "starting"
  | "provisioning"
  | "pending"
  | "queued"
  | "draining"
  | "stopping"
  | "stopped"
  | "idle"
  | "notready"
  | "degraded"
  | "reconnecting"
  | "warning"
  | "warn"
  | "error"
  | "failed"
  | "crashed"
  | "danger"
  | "stale"
  | "unknown";

type Glyph = "solid" | "pulsing" | "half" | "outline" | "question";

const palette: Record<
  StateKind,
  { token: string; glyph: Glyph; strike?: boolean }
> = {
  ready: { token: "--color-success", glyph: "solid" },
  healthy: { token: "--color-success", glyph: "solid" },
  ok: { token: "--color-success", glyph: "solid" },
  active: { token: "--color-success", glyph: "solid" },
  running: { token: "--color-accent", glyph: "pulsing" },
  starting: { token: "--color-starting", glyph: "half" },
  provisioning: { token: "--color-starting", glyph: "half" },
  draining: { token: "--color-draining", glyph: "half" },
  stopping: { token: "--color-draining", glyph: "half" },
  pending: { token: "--color-queued", glyph: "outline" },
  queued: { token: "--color-queued", glyph: "outline" },
  stopped: { token: "--color-muted", glyph: "outline" },
  idle: { token: "--color-muted", glyph: "outline" },
  notready: { token: "--color-warning", glyph: "solid" },
  degraded: { token: "--color-warning", glyph: "solid" },
  reconnecting: { token: "--color-warning", glyph: "solid" },
  warning: { token: "--color-warning", glyph: "solid" },
  warn: { token: "--color-warning", glyph: "solid" },
  error: { token: "--color-danger", glyph: "solid" },
  failed: { token: "--color-danger", glyph: "solid" },
  crashed: { token: "--color-danger", glyph: "solid" },
  danger: { token: "--color-danger", glyph: "solid" },
  stale: { token: "--color-stale", glyph: "solid", strike: true },
  unknown: { token: "--color-muted", glyph: "question" },
};

function resolveKind(value: string | null | undefined): StateKind {
  if (!value) return "unknown";
  const key = value.toLowerCase().replace(/[-_\s]/g, "");
  if (key in palette) return key as StateKind;
  if (key.startsWith("err")) return "error";
  if (key === "info" || key === "debug" || key === "trace") return "idle";
  return "unknown";
}

function StateGlyph({ glyph, color }: { glyph: Glyph; color: string }) {
  if (glyph === "question") {
    return (
      <span
        aria-hidden
        className="inline-flex size-2 items-center justify-center font-mono text-[10px] leading-none"
        style={{ color }}
      >
        ?
      </span>
    );
  }
  if (glyph === "outline") {
    return (
      <span
        aria-hidden
        className="inline-block size-2 rounded-full"
        style={{ border: `1.5px solid ${color}`, background: "transparent" }}
      />
    );
  }
  if (glyph === "half") {
    return (
      <span
        aria-hidden
        className="inline-block size-2 rounded-full"
        style={{
          background: `conic-gradient(from 270deg, ${color} 0 50%, transparent 50% 100%)`,
          border: `1px solid ${color}`,
        }}
      />
    );
  }
  if (glyph === "pulsing") {
    return (
      <span
        aria-hidden
        className="inline-block size-2 rounded-full animate-pulse motion-reduce:animate-none"
        style={{ background: color }}
      />
    );
  }
  return (
    <span
      aria-hidden
      className="inline-block size-2 rounded-full"
      style={{ background: color }}
    />
  );
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
  const entry = palette[kind];
  const colorVar = `var(${entry.token})`;
  const label = state ?? "—";
  return (
    <span
      className={cn(
        "inline-flex items-center gap-1.5 font-mono text-[11px] uppercase tracking-wide tabular text-default",
        className,
      )}
      data-state={kind}
      data-glyph={entry.glyph}
    >
      {showDot ? <StateGlyph glyph={entry.glyph} color={colorVar} /> : null}
      <span className={cn(entry.strike && "line-through decoration-from-font")}>
        {label}
      </span>
    </span>
  );
}
