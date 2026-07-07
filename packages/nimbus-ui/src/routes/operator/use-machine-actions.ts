import { useCallback, useState } from "react";
import { toast } from "sonner";

import { machines } from "../../lib/api-mutations";
import type { MachineDoc } from "./machine-types";

export type LifecycleAction = "start" | "stop" | "restart" | "delete";

// State a machine optimistically shows while its lifecycle request is in
// flight, before the reactive query reports the settled state.
export const OPTIMISTIC_STATES: Record<LifecycleAction, string> = {
  start: "starting",
  stop: "stopping",
  restart: "restarting",
  delete: "deleting",
};

// Which lifecycle actions are offered for a machine in a given state.
export function actionsForState(state: string | undefined): LifecycleAction[] {
  const value = (state ?? "").toLowerCase();
  if (value === "running" || value === "ready" || value === "ok") {
    return ["stop", "restart"];
  }
  if (value === "failed" || value === "error") {
    return ["start", "restart", "delete"];
  }
  if (
    value === "stopped" ||
    value === "created" ||
    value === "idle" ||
    value === "pending"
  ) {
    return ["start", "delete"];
  }
  if (value === "starting" || value === "restarting" || value === "stopping") {
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
