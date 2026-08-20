import { useNimbusConnectionState, useQuery } from "@nimbus/nimbus/react";
import { createFileRoute, Link } from "@tanstack/react-router";
import { useEffect, useState } from "react";

import { api } from "../../../convex/_generated/api";
import type { Doc } from "../../../convex/_generated/dataModel";
import { CopyChip } from "../../components/copy-chip";
import { LoadingCell } from "../../components/loading-cell";
import { PageHeader } from "../../components/page-header";
import { StateChip } from "../../components/state-chip";
import { RelativeTime, Uptime } from "../../components/time";
import { loadTenantList } from "../../hooks/use-tenant-list";
import {
  type ConnectionSnapshot,
  type LoadingValue,
  toLoadingValue,
} from "../../shell/loading-value";

export const Route = createFileRoute("/operator/")({
  component: NodesPage,
});

type SystemStatus = {
  version?: string;
  buildHash?: string;
  health?: string;
  startedAt?: number;
  updatedAt?: number;
  details?: { listenAddress?: string } | null;
};

type ListenerDoc = Doc<"listeners">;

// A "node" is a host running the Nimbus binary (the `nimbus node` lifecycle).
// Multi-node clustering is not wired yet, so this deployment is exactly one
// node — the local host, sourced from system status. The page is shaped as a
// node list so it scales to a real cluster without a redesign. This is
// distinct from a "machine" (the outer dev VM under Operator → Machines).
function NodesPage() {
  const conn = useConnSnapshot();
  const status = useQuery(api.system.status, {}) as
    | SystemStatus
    | null
    | undefined;
  const machines = useQuery(api.machines.list, {
    state: null,
    provider: null,
    limit: 500,
  });
  const services = useQuery(api.services.list, {
    tenantId: null,
    machineId: null,
    state: null,
    limit: 500,
  });
  const listeners = useQuery(api.listeners.list, {
    adapter: null,
    state: null,
    limit: 100,
  });

  const tenantsLv = useTenantCount();

  const statusLv = toLoadingValue(status, conn);
  const machinesLv = toLoadingValue(machines, conn);
  const servicesLv = toLoadingValue(services, conn);
  const listenersLv = toLoadingValue(listeners, conn);

  return (
    <section
      className="flex h-full flex-col gap-4 overflow-auto px-6 py-5"
      data-testid="page-operator-nodes"
    >
      <PageHeader
        title="Nodes"
        subtitle="Hosts running the Nimbus binary. This deployment is a single node — clustering is not active yet."
      />

      <NodeCard status={statusLv} />

      <section className="flex flex-col gap-2">
        <h2 className="font-mono text-xs uppercase tracking-[0.18em] text-muted">
          Hosted on this node
        </h2>
        <div
          className="grid grid-cols-2 gap-3 md:grid-cols-4"
          data-testid="nodes-hosted"
        >
          <Field
            label="Tenants"
            testid="nodes-hosted-tenants"
            to="/operator/tenants"
          >
            <LoadingCell value={tenantsLv} testid="nodes-hosted-tenants">
              {(n) => n.toString()}
            </LoadingCell>
          </Field>
          <Field
            label="Machines"
            testid="nodes-hosted-machines"
            to="/operator/machines"
          >
            <LoadingCell value={machinesLv} testid="nodes-hosted-machines">
              {(m) => m.length.toString()}
            </LoadingCell>
          </Field>
          <Field
            label="Services"
            testid="nodes-hosted-services"
            to="/operator/services"
          >
            <LoadingCell value={servicesLv} testid="nodes-hosted-services">
              {(s) => s.length.toString()}
            </LoadingCell>
          </Field>
          <Field
            label="Listeners"
            testid="nodes-hosted-listeners"
            to="/operator/network"
          >
            <LoadingCell value={listenersLv} testid="nodes-hosted-listeners">
              {(items) => <ListenersValue listeners={items} />}
            </LoadingCell>
          </Field>
        </div>
      </section>
    </section>
  );
}

function NodeCard({ status }: { status: LoadingValue<SystemStatus> }) {
  return (
    <article
      className="flex flex-col overflow-hidden rounded-md border border-app bg-surface"
      data-testid="node-row"
    >
      <header className="flex items-start justify-between gap-3 border-b border-app px-4 py-3">
        <div className="flex flex-col gap-1">
          <LoadingCell value={status} testid="node-address">
            {(s) =>
              s.details?.listenAddress ? (
                <CopyChip
                  label="listen address"
                  value={s.details.listenAddress}
                  testid="node-address"
                >
                  {s.details.listenAddress}
                </CopyChip>
              ) : (
                <span className="font-mono text-sm text-default">
                  local node
                </span>
              )
            }
          </LoadingCell>
          <span className="font-mono text-xs uppercase tracking-[0.18em] text-muted">
            local host · standalone
          </span>
        </div>
        <LoadingCell value={status} testid="node-health">
          {(s) => <StateChip state={s.health ?? "unknown"} />}
        </LoadingCell>
      </header>
      <div className="grid grid-cols-2 gap-px bg-surface-2 md:grid-cols-4">
        <Cell label="Nimbus version">
          <LoadingCell value={status} testid="node-version">
            {(s) => (
              <>
                {s.version ?? "—"}
                {s.buildHash ? (
                  <span className="text-muted">
                    {" "}
                    +{s.buildHash.slice(0, 7)}
                  </span>
                ) : null}
              </>
            )}
          </LoadingCell>
        </Cell>
        <Cell label="Uptime">
          <LoadingCell value={status} testid="node-uptime">
            {(s) =>
              typeof s.startedAt === "number" ? (
                <Uptime startedAtMs={s.startedAt} />
              ) : (
                "—"
              )
            }
          </LoadingCell>
        </Cell>
        <Cell label="Started">
          <LoadingCell value={status} testid="node-started">
            {(s) =>
              typeof s.startedAt === "number" ? (
                <RelativeTime epochMs={s.startedAt} />
              ) : (
                "—"
              )
            }
          </LoadingCell>
        </Cell>
        <Cell label="Updated">
          <LoadingCell value={status} testid="node-updated">
            {(s) =>
              typeof s.updatedAt === "number" ? (
                <RelativeTime epochMs={s.updatedAt} />
              ) : (
                "—"
              )
            }
          </LoadingCell>
        </Cell>
      </div>
    </article>
  );
}

function Field({
  label,
  children,
  testid,
  to,
}: {
  label: string;
  children: React.ReactNode;
  testid: string;
  to?:
    | "/operator/tenants"
    | "/operator/machines"
    | "/operator/services"
    | "/operator/network";
}) {
  const body = (
    <>
      <span className="font-mono text-xs uppercase tracking-[0.18em] text-muted">
        {label}
      </span>
      <span className="font-mono text-sm text-default">{children}</span>
    </>
  );
  if (to) {
    return (
      <Link
        to={to}
        data-testid={testid}
        className="flex flex-col gap-1 rounded-md border border-app bg-surface px-3 py-2 hover:border-strong"
      >
        {body}
      </Link>
    );
  }
  return (
    <div
      className="flex flex-col gap-1 rounded-md border border-app bg-surface px-3 py-2"
      data-testid={testid}
    >
      {body}
    </div>
  );
}

function Cell({
  label,
  children,
}: {
  label: string;
  children: React.ReactNode;
}) {
  return (
    <div className="flex flex-col gap-1 bg-surface px-3 py-2">
      <span className="text-xs uppercase tracking-[0.14em] text-muted">
        {label}
      </span>
      <span className="font-mono text-sm text-default">{children}</span>
    </div>
  );
}

function ListenersValue({ listeners }: { listeners: ListenerDoc[] }) {
  if (listeners.length === 0) return <>—</>;
  const adapters = new Set<string>();
  for (const listener of listeners) {
    if (listener.adapter) adapters.add(listener.adapter);
  }
  const adapterLabel =
    adapters.size === 0 ? "" : ` · ${Array.from(adapters).sort().join(", ")}`;
  return (
    <>
      {listeners.length}
      {adapterLabel}
    </>
  );
}

function useConnSnapshot(): ConnectionSnapshot {
  const conn = useNimbusConnectionState();
  return {
    isWebSocketConnected: conn.isWebSocketConnected,
    hasEverConnected: conn.hasEverConnected,
  };
}

/**
 * The tenant count is the one tile on this page that does not ride the
 * WebSocket — it is a REST read of `/api/tenants` — so it reports its own
 * outcome rather than being folded through `toLoadingValue`, whose
 * undefined-while-connected branch means "loading" and has no way back out.
 * A failed read used to leave the tile on the loading marker for the life of
 * the page, which is the worst of the three states: it promises the number is
 * still coming.
 *
 * `loadTenantList` rather than `fetchTenants` because it rejects with the
 * error-envelope message instead of collapsing a non-OK response to `null`,
 * and an error state has to be able to say what failed. The tile links to
 * Operator → Tenants, which carries the same failure with a Retry.
 */
function useTenantCount(): LoadingValue<number> {
  const [value, setValue] = useState<LoadingValue<number>>({ kind: "loading" });
  useEffect(() => {
    const controller = new AbortController();
    loadTenantList(controller.signal)
      .then((tenants) => {
        if (controller.signal.aborted) return;
        setValue({ kind: "ok", value: tenants.length });
      })
      .catch((err) => {
        if (controller.signal.aborted) return;
        setValue({
          kind: "error",
          message: err instanceof Error ? err.message : String(err),
        });
      });
    return () => controller.abort();
  }, []);
  return value;
}
