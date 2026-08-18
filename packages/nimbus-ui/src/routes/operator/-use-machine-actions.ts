import { useCallback, useState } from "react";
import { toast } from "sonner";

import { machines } from "../../lib/api-mutations";
import type { MachineDoc } from "./-machine-types";

export type LifecycleAction = "start" | "stop" | "restart" | "delete";

// State a machine optimistically shows while its lifecycle request is in
// flight, before the reactive query reports the settled state. Every value
// here must resolve to a named state in `components/state-chip.tsx`, or the
// row answers a lifecycle click with a question mark; `use-machine-actions.spec.tsx`
// locks the two files together.
export const OPTIMISTIC_STATES: Record<LifecycleAction, string> = {
  start: "starting",
  stop: "stopping",
  restart: "restarting",
  delete: "deleting",
};

// The machine states this page branches on, grouped by the actions they
// allow. Named arrays instead of inline `||` chains so a test can enumerate
// every state the console can display and assert the badge can name it.
const LIVE_STATES = ["running", "ready", "ok"] as const;
const FAULTED_STATES = ["failed", "error"] as const;
const HALTED_STATES = ["stopped", "created", "idle", "pending"] as const;
const IN_FLIGHT_STATES = [
  "starting",
  "restarting",
  "stopping",
  "deleting",
] as const;

export const BRANCHED_MACHINE_STATES: readonly string[] = [
  ...LIVE_STATES,
  ...FAULTED_STATES,
  ...HALTED_STATES,
  ...IN_FLIGHT_STATES,
];

function includes(states: readonly string[], value: string): boolean {
  return states.includes(value);
}

// Which lifecycle actions are offered for a machine in a given state.
export function actionsForState(state: string | undefined): LifecycleAction[] {
  const value = (state ?? "").toLowerCase();
  if (includes(LIVE_STATES, value)) {
    return ["stop", "restart"];
  }
  if (includes(FAULTED_STATES, value)) {
    return ["start", "restart", "delete"];
  }
  if (includes(HALTED_STATES, value)) {
    return ["start", "delete"];
  }
  // A lifecycle request is in flight. Offer nothing rather than a row of
  // buttons that would race the one already running — `deleting` included,
  // which used to fall through to the catch-all below and render Start /
  // Stop / Restart / Delete for a machine on its way out.
  if (includes(IN_FLIGHT_STATES, value)) {
    return [];
  }
  return ["start", "stop", "restart", "delete"];
}

function capitalize(value: string): string {
  if (value.length === 0) return value;
  return value[0].toUpperCase() + value.slice(1);
}

export type MachineActions = {
  pending: Record<string, LifecycleAction>;
  errors: Record<string, string>;
  confirmDelete: MachineDoc | null;
  setConfirmDelete: (machine: MachineDoc | null) => void;
  runAction: (machine: MachineDoc, action: LifecycleAction) => Promise<void>;
  handleAction: (machine: MachineDoc, action: LifecycleAction) => void;
};

// Owns the machine lifecycle side effects for the operator machines page:
// per-machine in-flight action, per-machine error, the delete-confirmation
// target, and the fetch that drives them. `handleAction` routes delete through
// confirmation; every other action fires immediately.
export function useMachineActions(): MachineActions {
  const [pending, setPending] = useState<Record<string, LifecycleAction>>({});
  const [errors, setErrors] = useState<Record<string, string>>({});
  const [confirmDelete, setConfirmDelete] = useState<MachineDoc | null>(null);

  const runAction = useCallback(
    async (machine: MachineDoc, action: LifecycleAction) => {
      const key = machine._id;
      setPending((prev) => ({ ...prev, [key]: action }));
      setErrors((prev) => {
        if (!(key in prev)) return prev;
        const next = { ...prev };
        delete next[key];
        return next;
      });
      const result =
        action === "delete"
          ? await machines.remove(machine.name)
          : await machines.action(machine.name, action);
      if (!result.ok) {
        setErrors((prev) => ({ ...prev, [key]: result.error }));
      } else {
        toast(`${capitalize(action)} sent to ${machine.name}`);
      }
      setPending((prev) => {
        const next = { ...prev };
        delete next[key];
        return next;
      });
    },
    [],
  );

  const handleAction = useCallback(
    (machine: MachineDoc, action: LifecycleAction) => {
      if (action === "delete") {
        setConfirmDelete(machine);
        return;
      }
      void runAction(machine, action);
    },
    [runAction],
  );

  return {
    pending,
    errors,
    confirmDelete,
    setConfirmDelete,
    runAction,
    handleAction,
  };
}
