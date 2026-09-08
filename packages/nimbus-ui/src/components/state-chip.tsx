import { cn } from "@/lib/utils";

/**
 * Every state the console can name. The DESIGN.md badge table fixes the
 * token and glyph per state *family*; the extra members here are aliases
 * that fold onto one of those families (`ok` onto Ready/Healthy,
 * `restarting` onto Starting/Provisioning, `deleting` onto
 * Draining/Stopping, and so on).
 *
 * A state string the UI can produce but this union omits renders as
 * `unknown` — a literal `?` glyph — which reads as "the console lost track
 * of this resource". Anything a route or a hook can put on screen belongs
 * here, including every state the server can write into a record:
 *
 * - scheduled jobs: `pending` | `completed` | `failed`
 * - cron jobs: `active` | `paused`
 * - machines: `uninitialized` | `stopped` | `starting` | `running` | `failed`
 * - runs: `ok` | `error`
 *
 * `completed`, `paused` and `uninitialized` were missing, so a finished
 * scheduled job read `? completed` on /developer/schedules. The server
 * vocabularies are locked against this table in state-chip.spec.tsx.
 */
export type StateKind =
  | "ready"
  | "healthy"
  | "ok"
  | "active"
  | "connected"
  | "completed"
  | "running"
  | "starting"
  | "provisioning"
  | "restarting"
  | "pending"
  | "queued"
  | "draining"
  | "stopping"
  | "deleting"
  | "stopped"
  | "created"
  | "idle"
  | "paused"
  | "uninitialized"
  | "notready"
  | "degraded"
  | "reconnecting"
  | "warning"
  | "warn"
  | "error"
  | "failed"
  | "crashed"
  | "danger"
  | "offline"
  | "stale"
  | "unknown";

type Glyph = "solid" | "pulsing" | "half" | "outline" | "question";

export const statePalette: Record<
  StateKind,
  { token: string; glyph: Glyph; strike?: boolean }
> = {
  ready: { token: "--success", glyph: "solid" },
  healthy: { token: "--success", glyph: "solid" },
  ok: { token: "--success", glyph: "solid" },
  active: { token: "--success", glyph: "solid" },
  connected: { token: "--success", glyph: "solid" },
  /* A scheduled job that ran to completion is the terminal success of a
     run, so it folds onto the Ready/OK family rather than earning a row of
     its own. */
  completed: { token: "--success", glyph: "solid" },
  /* Running owns `--info` (blue), never `--accent`. The accent is amber and
     `--warning`, which carries NotReady and Degraded, is orange, so binding
     Running to the accent would paint two opposite states in one hue family,
     separated only by lightness. Semantic colours never use the accent hue. */
  running: { token: "--info", glyph: "pulsing" },
  starting: { token: "--warning", glyph: "half" },
  provisioning: { token: "--warning", glyph: "half" },
  restarting: { token: "--warning", glyph: "half" },
  draining: { token: "--text-3", glyph: "half" },
  stopping: { token: "--text-3", glyph: "half" },
  deleting: { token: "--text-3", glyph: "half" },
  pending: { token: "--text-3", glyph: "outline" },
  queued: { token: "--text-3", glyph: "outline" },
  stopped: { token: "--text-3", glyph: "outline" },
  created: { token: "--text-3", glyph: "outline" },
  idle: { token: "--text-3", glyph: "outline" },
  /* Paused (a disabled cron) and uninitialized (a machine with no host yet)
     are both "exists, not doing anything", which is the Stopped family. */
  paused: { token: "--text-3", glyph: "outline" },
  uninitialized: { token: "--text-3", glyph: "outline" },
  notready: { token: "--warning", glyph: "solid" },
  degraded: { token: "--warning", glyph: "solid" },
  reconnecting: { token: "--warning", glyph: "solid" },
  warning: { token: "--warning", glyph: "solid" },
  warn: { token: "--warning", glyph: "solid" },
  error: { token: "--error", glyph: "solid" },
  failed: { token: "--error", glyph: "solid" },
  crashed: { token: "--error", glyph: "solid" },
  danger: { token: "--error", glyph: "solid" },
  offline: { token: "--error", glyph: "solid" },
  stale: { token: "--text-3", glyph: "solid", strike: true },
  unknown: { token: "--text-3", glyph: "question" },
};

export function resolveStateKind(value: string | null | undefined): StateKind {
  return resolveKind(value);
}

function resolveKind(value: string | null | undefined): StateKind {
  if (!value) return "unknown";
  const key = value.toLowerCase().replace(/[-_\s]/g, "");
  if (key in statePalette) return key as StateKind;
  if (key.startsWith("err")) return "error";
  if (key === "info" || key === "debug" || key === "trace") return "idle";
  return "unknown";
}

function StateGlyph({ glyph, color }: { glyph: Glyph; color: string }) {
  if (glyph === "question") {
    return (
      <span
        aria-hidden
        className="inline-flex size-2 items-center justify-center font-mono text-xs leading-none"
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
  const entry = statePalette[kind];
  const colorVar = `var(${entry.token})`;
  const label = state ?? "—";
  return (
    <span
      className={cn(
        "inline-flex items-center gap-1.5 text-xs font-medium tabular text-text-1",
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
