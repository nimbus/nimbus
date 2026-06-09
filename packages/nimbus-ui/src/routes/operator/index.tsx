import { createFileRoute } from "@tanstack/react-router";
import { useNimbusConnectionState, useQuery } from "@nimbus/nimbus/react";
import { useEffect, useState } from "react";

import { api } from "../../../convex/_generated/api";
import type { Doc } from "../../../convex/_generated/dataModel";
import { LoadingCell } from "../../components/loading-cell";
import { RelativeTime } from "../../components/time";
import {
  type ConnectionSnapshot,
  toLoadingValue,
} from "../../shell/loading-value";
import { fetchTenants } from "../../shell/tenants-fetch";

export const Route = createFileRoute("/operator/")({
  component: SystemOverviewPage,
});

type SystemStatus = {
  version?: string;
  health?: string;
  startedAt?: number;
  updatedAt?: number;
  details?: { listenAddress?: string } | null;
};

type ListenerDoc = Doc<"listeners">;

function SystemOverviewPage() {
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

  const tenantCount = useTenantCount();

  const statusLv = toLoadingValue(status, conn);
  const machinesLv = toLoadingValue(machines, conn);
  const servicesLv = toLoadingValue(services, conn);
  const listenersLv = toLoadingValue(listeners, conn);
  const tenantsLv = toLoadingValue(tenantCount, conn);

  return (
    <section
      className="flex h-full flex-col gap-4 overflow-auto px-6 py-5"
      data-testid="page-admin-system"
    >
      <header>
        <h1 className="text-default" style={{ fontSize: "var(--text-xl)" }}>
          System
        </h1>
        <p className="text-sm text-muted">
          Server-wide health, runtime, and live counts across every tenant.
        </p>
      </header>

      <div
        className="grid grid-cols-1 gap-3 md:grid-cols-2"
        data-testid="system-overview"
      >
        <Field label="Nimbus version" testid="system-overview-version">
          <LoadingCell value={statusLv} testid="system-overview-version">
            {(s) => s.version ?? "—"}
          </LoadingCell>
        </Field>
        <Field label="Health" testid="system-overview-health">
          <LoadingCell value={statusLv} testid="system-overview-health">
            {(s) => s.health ?? "—"}
          </LoadingCell>
        </Field>
        <Field label="Server uptime" testid="system-overview-uptime">
          <LoadingCell value={statusLv} testid="system-overview-uptime">
            {(s) =>
              typeof s.startedAt === "number" ? (
                <RelativeTime epochMs={s.startedAt} />
              ) : (
                "—"
              )
            }
          </LoadingCell>
        </Field>
        <Field label="Listen address" testid="system-overview-listen">
          <LoadingCell value={statusLv} testid="system-overview-listen">
            {(s) => s.details?.listenAddress ?? "—"}
          </LoadingCell>
        </Field>
        <Field label="Tenants" testid="system-overview-tenants">
          <LoadingCell value={tenantsLv} testid="system-overview-tenants">
            {(n) => n.toString()}
          </LoadingCell>
        </Field>
        <Field label="Machines" testid="system-overview-machines">
          <LoadingCell value={machinesLv} testid="system-overview-machines">
            {(m) => m.length.toString()}
          </LoadingCell>
        </Field>
        <Field label="Services" testid="system-overview-services">
          <LoadingCell value={servicesLv} testid="system-overview-services">
            {(s) => s.length.toString()}
          </LoadingCell>
        </Field>
        <Field label="Listeners" testid="system-overview-listeners">
          <LoadingCell value={listenersLv} testid="system-overview-listeners">
            {(items) => <ListenersValue listeners={items} />}
          </LoadingCell>
        </Field>
      </div>
    </section>
  );
}

function Field({
  label,
  children,
  testid,
}: {
  label: string;
  children: React.ReactNode;
  testid: string;
}) {
  return (
    <div
      className="flex flex-col gap-1 rounded-md border border-app bg-surface px-3 py-2"
      data-testid={testid}
    >
      <span className="font-mono text-[10px] uppercase tracking-[0.18em] text-muted">
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
    adapters.size === 0
      ? ""
      : ` · ${Array.from(adapters).sort().join(", ")}`;
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

function useTenantCount(): number | undefined {
  const [count, setCount] = useState<number | undefined>(undefined);
  useEffect(() => {
    const controller = new AbortController();
    fetchTenants(controller.signal)
      .then((ids) => {
        if (controller.signal.aborted) return;
        if (ids !== null) setCount(ids.length);
      })
      .catch(() => {
        /* surfaced elsewhere; render — for the field */
      });
    return () => controller.abort();
  }, []);
  return count;
}
