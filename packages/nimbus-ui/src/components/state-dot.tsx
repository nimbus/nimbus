import { cn } from "../lib/cn";
import { type StateKind, statePalette } from "./state-chip";

export type ConnState = "connected" | "reconnecting" | "offline";

/**
 * The bare dot for the status bar and the disconnected overlay, where the
 * label is already printed beside it. The colour comes from `statePalette`
 * rather than a private table: two components owning the same
 * state→token binding is how they drift apart.
 *
 * No state here pulses. DESIGN.md grants the pulse to `Running` alone and
 * assigns `Reconnecting` a solid `--warning` dot; a permanent unstoppable
 * pulse in the one element that is always on screen is the opposite of
 * calm, and it ignored `prefers-reduced-motion` besides.
 */
const CONN_STATES: Record<ConnState, { kind: StateKind; label: string }> = {
  connected: { kind: "connected", label: "Connected" },
  reconnecting: { kind: "reconnecting", label: "Reconnecting" },
  offline: { kind: "offline", label: "Offline" },
};

export function StateDot({
  state,
  className,
}: {
  state: ConnState;
  className?: string;
}) {
  const entry = CONN_STATES[state];
  return (
    <span
      aria-label={entry.label}
      role="img"
      data-state={entry.kind}
      className={cn("inline-block size-2 rounded-full", className)}
      style={{ background: `var(${statePalette[entry.kind].token})` }}
    />
  );
}
