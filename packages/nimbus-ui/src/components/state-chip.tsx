import { cn } from "../lib/cn";

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
  ready: { token: "--nimbus-success", glyph: "solid" },
  healthy: { token: "--nimbus-success", glyph: "solid" },
  ok: { token: "--nimbus-success", glyph: "solid" },
  active: { token: "--nimbus-success", glyph: "solid" },
  connected: { token: "--nimbus-success", glyph: "solid" },
  /* A scheduled job that ran to completion is the terminal success of a
     run, so it folds onto the Ready/OK family rather than earning a row of
     its own. */
  completed: { token: "--nimbus-success", glyph: "solid" },
  /* Running owns `--running` (teal, hue 207), not `--accent`. In the warm
     palette `--accent` is hue 70 and `--warning` — which carries NotReady and
     Degraded — is hue 72, so binding Running to the accent painted two
     semantically opposite states in one hue family, separated only by
     lightness. `--running` is also palette-stable, where `--accent` shifts with
     the brand, so a palette switch can no longer change what Running looks
     like relative to Degraded. */
  running: { token: "--nimbus-running", glyph: "pulsing" },
  starting: { token: "--nimbus-starting", glyph: "half" },
  provisioning: { token: "--nimbus-starting", glyph: "half" },
  restarting: { token: "--nimbus-starting", glyph: "half" },
  draining: { token: "--nimbus-draining", glyph: "half" },
  stopping: { token: "--nimbus-draining", glyph: "half" },
  deleting: { token: "--nimbus-draining", glyph: "half" },
  pending: { token: "--nimbus-queued", glyph: "outline" },
  queued: { token: "--nimbus-queued", glyph: "outline" },
  stopped: { token: "--nimbus-muted", glyph: "outline" },
  created: { token: "--nimbus-muted", glyph: "outline" },
  idle: { token: "--nimbus-muted", glyph: "outline" },
  /* Paused (a disabled cron) and uninitialized (a machine with no host yet)
     are both "exists, not doing anything", which is the Stopped family. */
  paused: { token: "--nimbus-muted", glyph: "outline" },
  uninitialized: { token: "--nimbus-muted", glyph: "outline" },
  notready: { token: "--nimbus-warning", glyph: "solid" },
  degraded: { token: "--nimbus-warning", glyph: "solid" },
  reconnecting: { token: "--nimbus-warning", glyph: "solid" },
  warning: { token: "--nimbus-warning", glyph: "solid" },
  warn: { token: "--nimbus-warning", glyph: "solid" },
  error: { token: "--nimbus-danger", glyph: "solid" },
  failed: { token: "--nimbus-danger", glyph: "solid" },
  crashed: { token: "--nimbus-danger", glyph: "solid" },
  danger: { token: "--nimbus-danger", glyph: "solid" },
  offline: { token: "--nimbus-danger", glyph: "solid" },
  stale: { token: "--nimbus-stale", glyph: "solid", strike: true },
  unknown: { token: "--nimbus-muted", glyph: "question" },
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
        "inline-flex items-center gap-1.5 font-mono text-xs uppercase tracking-wide tabular text-default",
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
