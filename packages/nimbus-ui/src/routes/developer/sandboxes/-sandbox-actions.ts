import { useCallback, useState } from "react";
import { toast } from "sonner";

import { sandboxes as sandboxApi } from "../../../lib/api-mutations";
import type { SandboxResource } from "../../../lib/types/sandbox";
import { sandboxDisplayName } from "../../../lib/types/sandbox";

export type SandboxActions = {
  // Sandbox ids with a stop request in flight.
  pending: Record<string, true>;
  // The last refusal per sandbox id, cleared by the next request.
  errors: Record<string, string>;
  stop: (sandbox: SandboxResource) => Promise<void>;
};

// Owns the one lifecycle write a sandbox takes: stop. A stopped sandbox
// does not start again (there is no start route; a new sandbox is
// created instead), so the callers put the request behind a confirmation
// and this hook only sends it. A settled request calls `onChanged` so the
// page reads the new state back.
export function useSandboxActions(onChanged: () => void): SandboxActions {
  const [pending, setPending] = useState<Record<string, true>>({});
  const [errors, setErrors] = useState<Record<string, string>>({});

  const stop = useCallback(
    async (sandbox: SandboxResource) => {
      const key = sandbox.metadata.id;
      const name = sandboxDisplayName(sandbox);
      setPending((prev) => ({ ...prev, [key]: true }));
      setErrors((prev) => {
        if (!(key in prev)) return prev;
        const next = { ...prev };
        delete next[key];
        return next;
      });
      const result = await sandboxApi.stop(sandbox.metadata.tenantId, key);
      if (!result.ok) {
        setErrors((prev) => ({ ...prev, [key]: result.error }));
        toast.error(`Stop refused for ${name}`, { description: result.error });
      } else {
        toast.success(`Stop sent to ${name}`);
        onChanged();
      }
      setPending((prev) => {
        const next = { ...prev };
        delete next[key];
        return next;
      });
    },
    [onChanged],
  );

  return { pending, errors, stop };
}
