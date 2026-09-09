import { useEffect, useState } from "react";

import { useApiRead } from "../../../hooks/use-api-read";
import type { ApiResult } from "../../../lib/api-mutations";
import type {
  SandboxCollection,
  SandboxResource,
} from "../../../lib/types/sandbox";
import { sandboxIsTransitional } from "../../../lib/types/sandbox";
import type { LoadingValue } from "../../../shell/loading-value";

const enc = encodeURIComponent;

/** How often a page re-reads a sandbox that is still moving. */
export const SANDBOX_POLL_MS = 2000;

// What a sandbox list read resolves to. The service-control routes are
// mounted only when the server runs a service manager; without one every
// sandbox route is 404, which the pages show as one plain state instead
// of an empty list.
export type SandboxListRead =
  | { kind: "list"; items: SandboxResource[] }
  | { kind: "unavailable"; message: string };

export type SandboxRead =
  | { kind: "sandbox"; sandbox: SandboxResource }
  | { kind: "missing"; message: string };

function selectList(
  result: ApiResult<SandboxCollection>,
): LoadingValue<SandboxListRead> {
  if (result.ok)
    return { kind: "ok", value: { kind: "list", items: result.data.items } };
  if (result.status === 404) {
    return {
      kind: "ok",
      value: { kind: "unavailable", message: result.error },
    };
  }
  return { kind: "error", message: result.error };
}

function selectOne(
  result: ApiResult<SandboxResource>,
): LoadingValue<SandboxRead> {
  if (result.ok)
    return { kind: "ok", value: { kind: "sandbox", sandbox: result.data } };
  if (result.status === 404) {
    return { kind: "ok", value: { kind: "missing", message: result.error } };
  }
  return { kind: "error", message: result.error };
}

// Poll while any sandbox in the read is transitional, so a pending or
// stopping row settles on screen without a reload. `revision` is the
// caller's own bump after a write; the poll adds its own ticks to it.
function usePollingRevision(
  revision: number,
  moving: boolean,
  pollMs: number,
): number {
  const [ticks, setTicks] = useState(0);
  useEffect(() => {
    if (!moving) return;
    const timer = setInterval(() => setTicks((n) => n + 1), pollMs);
    return () => clearInterval(timer);
  }, [moving, pollMs]);
  return revision * 1_000_000 + ticks;
}

export function useSandboxList(
  tenant: string,
  revision: number,
  pollMs = SANDBOX_POLL_MS,
): LoadingValue<SandboxListRead> {
  const [moving, setMoving] = useState(false);
  const combined = usePollingRevision(revision, moving, pollMs);
  const read = useApiRead<SandboxListRead, SandboxCollection>(
    `/api/tenants/${enc(tenant)}/sandboxes?limit=200`,
    selectList,
    combined,
  );
  useEffect(() => {
    setMoving(
      read.kind === "ok" &&
        read.value.kind === "list" &&
        read.value.items.some((item) =>
          sandboxIsTransitional(item.status.lifecycleState),
        ),
    );
  }, [read]);
  return read;
}

export function useSandbox(
  tenant: string,
  id: string,
  revision: number,
  pollMs = SANDBOX_POLL_MS,
): LoadingValue<SandboxRead> {
  const [moving, setMoving] = useState(false);
  const combined = usePollingRevision(revision, moving, pollMs);
  const read = useApiRead<SandboxRead, SandboxResource>(
    `/api/tenants/${enc(tenant)}/sandboxes/${enc(id)}`,
    selectOne,
    combined,
  );
  useEffect(() => {
    setMoving(
      read.kind === "ok" &&
        read.value.kind === "sandbox" &&
        sandboxIsTransitional(read.value.sandbox.status.lifecycleState),
    );
  }, [read]);
  return read;
}
