import type { CSSProperties } from "react";

import { cn } from "@/lib/utils";

// StateDot reports liveness: is this thing up, coming up, going down, or
// gone. The console keeps the two state idioms apart. A dot reports liveness
// and a Pill reports lifecycle (see pill.tsx), and both resolve a server's
// free-form state string through the one palette below, so the same word
// paints the same colour in every table, header, and status bar.
//
// One accent, four jobs: no state ever paints the amber. Liveness colours
// are the semantic set (success, warning, error, info) plus the neutral
// text tone for states that are at rest.

export type StateKind =
  | "ready"
  | "healthy"
  | "ok"
  | "active"
  | "connected"
  | "completed"
  | "enabled"
  | "running"
  | "backfilling"
  | "starting"
  | "provisioning"
  | "restarting"
  | "draining"
  | "stopping"
  | "deleting"
  | "pending"
  | "queued"
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

// solid: the state is settled. pulsing: work is happening now (running
// alone). half: a transition, filled to the half that is done. outline: at
// rest, nothing happening. question: the console does not know this word.
export type StateGlyph = "solid" | "pulsing" | "half" | "outline" | "question";

export type StateStyle = {
  token: "--success" | "--warning" | "--error" | "--info" | "--text-3";
  glyph: StateGlyph;
  strike?: true;
};

const SUCCESS: StateStyle = { token: "--success", glyph: "solid" };
const TRANSITION_UP: StateStyle = { token: "--warning", glyph: "half" };
const TRANSITION_DOWN: StateStyle = { token: "--text-3", glyph: "half" };
const REST: StateStyle = { token: "--text-3", glyph: "outline" };
const WARNING: StateStyle = { token: "--warning", glyph: "solid" };
const ERROR: StateStyle = { token: "--error", glyph: "solid" };

export const statePalette: Record<StateKind, StateStyle> = {
  ready: SUCCESS,
  healthy: SUCCESS,
  ok: SUCCESS,
  active: SUCCESS,
  connected: SUCCESS,
  completed: SUCCESS,
  enabled: SUCCESS,
  running: { token: "--info", glyph: "pulsing" },
  // An index the server is still building: work is under way, so it takes
  // the transition treatment, not the pulsing one, which `running` owns.
  backfilling: TRANSITION_UP,
  starting: TRANSITION_UP,
  provisioning: TRANSITION_UP,
  restarting: TRANSITION_UP,
  draining: TRANSITION_DOWN,
  stopping: TRANSITION_DOWN,
  deleting: TRANSITION_DOWN,
  pending: REST,
  queued: REST,
  stopped: REST,
  created: REST,
  idle: REST,
  paused: REST,
  uninitialized: REST,
  notready: WARNING,
  degraded: WARNING,
  reconnecting: WARNING,
  warning: WARNING,
  warn: WARNING,
  error: ERROR,
  failed: ERROR,
  crashed: ERROR,
  danger: ERROR,
  offline: ERROR,
  stale: { token: "--text-3", glyph: "solid", strike: true },
  unknown: { token: "--text-3", glyph: "question" },
};

// resolveStateKind maps a server's spelling onto the palette: case, dashes,
// underscores, and spaces are noise; any `err…` word is an error; a log
// level that is not a fault is at rest; a word the palette does not know is
// unknown, which draws the question glyph instead of guessing.
export function resolveStateKind(value: string | null | undefined): StateKind {
  if (!value) return "unknown";
  const key = value.toLowerCase().replace(/[-_\s]/g, "");
  if (key.startsWith("err")) return "error";
  if (key === "info" || key === "debug" || key === "trace") return "idle";
  return key in statePalette ? (key as StateKind) : "unknown";
}

// ConnState is the status bar's vocabulary for the socket; it names three
// palette kinds that never pulse, because the status bar is always on
// screen and a permanent pulse there is the opposite of calm.
export type ConnState = "connected" | "reconnecting" | "offline";

function glyphStyle(style: StateStyle): CSSProperties {
  const color = `var(${style.token})`;
  switch (style.glyph) {
    case "outline":
      return { boxShadow: `inset 0 0 0 1.5px ${color}` };
    case "half":
      return {
        background: `conic-gradient(from 270deg, ${color} 0 50%, transparent 50% 100%)`,
        boxShadow: `inset 0 0 0 1px ${color}`,
      };
    case "question":
      return { color };
    default:
      return { background: color };
  }
}

function capitalize(value: string): string {
  return value.charAt(0).toUpperCase() + value.slice(1);
}

// StateDot is an 8px glyph with the state's name as its accessible name.
// `label` overrides the name when the raw state is not a word a reader
// should hear.
export function StateDot({
  state,
  label,
  className,
}: {
  state: string | null | undefined;
  label?: string;
  className?: string;
}) {
  const kind = resolveStateKind(state);
  const style = statePalette[kind];
  return (
    <span
      role="img"
      aria-label={label ?? (state ? capitalize(state) : "Unknown")}
      data-state={kind}
      data-glyph={style.glyph}
      className={cn(
        "inline-flex size-2 shrink-0 items-center justify-center rounded-full font-mono text-[9px] leading-none",
        style.glyph === "pulsing" && "animate-pulse motion-reduce:animate-none",
        className,
      )}
      style={glyphStyle(style)}
    >
      {style.glyph === "question" ? "?" : null}
    </span>
  );
}
