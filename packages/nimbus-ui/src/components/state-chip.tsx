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
  ready: { token: "--nimbus-success", glyph: "solid" },
  healthy: { token: "--nimbus-success", glyph: "solid" },
  ok: { token: "--nimbus-success", glyph: "solid" },
  active: { token: "--nimbus-success", glyph: "solid" },
  running: { token: "--nimbus-accent", glyph: "pulsing" },
  starting: { token: "--nimbus-starting", glyph: "half" },
  provisioning: { token: "--nimbus-starting", glyph: "half" },
  draining: { token: "--nimbus-draining", glyph: "half" },
  stopping: { token: "--nimbus-draining", glyph: "half" },
  pending: { token: "--nimbus-queued", glyph: "outline" },
  queued: { token: "--nimbus-queued", glyph: "outline" },
  stopped: { token: "--nimbus-muted", glyph: "outline" },
  idle: { token: "--nimbus-muted", glyph: "outline" },
  notready: { token: "--nimbus-warning", glyph: "solid" },
  degraded: { token: "--nimbus-warning", glyph: "solid" },
  reconnecting: { token: "--nimbus-warning", glyph: "solid" },
  warning: { token: "--nimbus-warning", glyph: "solid" },
  warn: { token: "--nimbus-warning", glyph: "solid" },
  error: { token: "--nimbus-danger", glyph: "solid" },
  failed: { token: "--nimbus-danger", glyph: "solid" },
  crashed: { token: "--nimbus-danger", glyph: "solid" },
  danger: { token: "--nimbus-danger", glyph: "solid" },
  stale: { token: "--nimbus-stale", glyph: "solid", strike: true },
  unknown: { token: "--nimbus-muted", glyph: "question" },
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
