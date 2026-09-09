import { useRouter } from "@tanstack/react-router";
import { useCallback, useState } from "react";
import { toast } from "sonner";

import { services as serviceApi } from "../../../lib/api-mutations";
import type { ServiceDoc } from "../../../lib/types/service";

export type ServiceAction = "start" | "stop" | "restart";

// The state a service row shows while its lifecycle request is in flight,
// before the loader reads the settled state back. Every value resolves to a
// named state in `components/state-dot.tsx`, so a row never answers a click
// with a question mark; `-service-lifecycle.spec.ts` locks the two together.
export const OPTIMISTIC_STATES: Record<ServiceAction, string> = {
  start: "starting",
  stop: "stopping",
  restart: "restarting",
};

// The sandbox status vocabulary the service manager writes into `state`
// (`starting`, `ready`, `not_ready`, `stopping`, `stopped`, `failed`),
// plus the spellings the older smoke fixtures use (`running`, `error`).
// Grouped by the actions they allow, so a test can enumerate every state
// the console branches on.
const LIVE_STATES = ["ready", "running", "ok", "healthy"] as const;
const FAULTED_STATES = ["failed", "error", "crashed"] as const;
const HALTED_STATES = [
  "stopped",
  "created",
  "idle",
  "pending",
  "not_ready",
  "notready",
] as const;
const IN_FLIGHT_STATES = ["starting", "stopping", "restarting"] as const;

export const BRANCHED_SERVICE_STATES: readonly string[] = [
  ...LIVE_STATES,
  ...FAULTED_STATES,
  ...HALTED_STATES,
  ...IN_FLIGHT_STATES,
];

function includes(states: readonly string[], value: string): boolean {
  return states.includes(value);
}

// Which lifecycle actions a service in a given state offers.
export function actionsForState(state: string | undefined): ServiceAction[] {
  const value = (state ?? "").toLowerCase();
  if (includes(LIVE_STATES, value)) return ["stop", "restart"];
  if (includes(FAULTED_STATES, value)) return ["start", "restart"];
  if (includes(HALTED_STATES, value)) return ["start"];
  // A request is already in flight; a second one would race it.
  if (includes(IN_FLIGHT_STATES, value)) return [];
  return ["start", "stop", "restart"];
}

export const ACTION_LABELS: Record<ServiceAction, string> = {
  start: "Start",
  stop: "Stop",
  restart: "Restart",
};

// The service row carries the definition generation the manager last
// applied; the restart route refuses a restart against a generation the
// operator did not see. The system schema stores it as a string.
export function sourceGenerationOf(service: ServiceDoc): number {
  const raw = (service as { sourceGeneration?: unknown }).sourceGeneration;
  const parsed = typeof raw === "string" ? Number(raw) : raw;
  return typeof parsed === "number" && Number.isFinite(parsed) && parsed > 0
    ? Math.floor(parsed)
    : 1;
}

export type ServiceActions = {
  // The in-flight action per service id.
  pending: Record<string, ServiceAction>;
  // The last refusal per service id, cleared by the next request.
  errors: Record<string, string>;
  runAction: (service: ServiceDoc, action: ServiceAction) => Promise<void>;
};

function displayNameOf(service: ServiceDoc): string {
  return service.name ?? service._id;
}

// Owns the service lifecycle side effects for the services list and both
// detail pages: the in-flight action per service, the refusal per service,
// and the request that drives them. A settled request invalidates the
// router so the loader reads the new state back.
export function useServiceActions(): ServiceActions {
  const router = useRouter();
  const [pending, setPending] = useState<Record<string, ServiceAction>>({});
  const [errors, setErrors] = useState<Record<string, string>>({});

  const runAction = useCallback(
    async (service: ServiceDoc, action: ServiceAction) => {
      const key = service._id;
      const name = displayNameOf(service);
      const tenant = service.tenantId;
      if (!tenant) {
        setErrors((prev) => ({
          ...prev,
          [key]:
            "This service names no tenant, so the console cannot address it.",
        }));
        return;
      }
      setPending((prev) => ({ ...prev, [key]: action }));
      setErrors((prev) => {
        if (!(key in prev)) return prev;
        const next = { ...prev };
        delete next[key];
        return next;
      });
      const result =
        action === "start"
          ? await serviceApi.start(tenant, name)
          : action === "stop"
            ? await serviceApi.stop(tenant, name)
            : await serviceApi.restart(tenant, name, {
                sourceGeneration: sourceGenerationOf(service),
                requestId: crypto.randomUUID(),
              });
      if (!result.ok) {
        setErrors((prev) => ({ ...prev, [key]: result.error }));
        toast.error(`${ACTION_LABELS[action]} refused for ${name}`, {
          description: result.error,
        });
      } else {
        toast.success(`${ACTION_LABELS[action]} sent to ${name}`);
        await router.invalidate();
      }
      setPending((prev) => {
        const next = { ...prev };
        delete next[key];
        return next;
      });
    },
    [router],
  );

  return { pending, errors, runAction };
}
