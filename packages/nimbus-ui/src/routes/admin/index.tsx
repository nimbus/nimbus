import { createFileRoute } from "@tanstack/react-router";
import { useQuery } from "nimbus/react";
import { useEffect, useState } from "react";

import { api } from "../../../convex/_generated/api";
import { RelativeTime } from "../../components/time";

export const Route = createFileRoute("/admin/")({
  component: SystemOverviewPage,
});

type SystemStatus = {
  version?: string;
  health?: string;
  startedAt?: number;
  updatedAt?: number;
  details?: { listenAddress?: string } | null;
};

type MachineDoc = { _id: string; state?: string };
type ServiceDoc = { _id: string };
type ListenerDoc = { _id: string; adapter?: string; state?: string };

function SystemOverviewPage() {
  const status = useQuery(api.system.status, {}) as SystemStatus | null | undefined;
  const machines = useQuery(api.machines.list, {
    state: null,
    provider: null,
    limit: 500,
  }) as MachineDoc[] | undefined;
  const services = useQuery(api.services.list, {
    tenantId: null,
    machineId: null,
    state: null,
    limit: 500,
  }) as ServiceDoc[] | undefined;
  const listeners = useQuery(api.listeners.list, {
    adapter: null,
    state: null,
    limit: 100,
  }) as ListenerDoc[] | undefined;

  const tenantCount = useTenantCount();

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
          {status?.version ?? "—"}
        </Field>
        <Field label="Health" testid="system-overview-health">
          {status?.health ?? "—"}
        </Field>
        <Field label="Server uptime" testid="system-overview-uptime">
          {typeof status?.startedAt === "number" ? (
            <RelativeTime epochMs={status.startedAt} />
          ) : (
            "—"
          )}
        </Field>
        <Field label="Listen address" testid="system-overview-listen">
          {status?.details?.listenAddress ?? "—"}
        </Field>
        <Field label="Tenants" testid="system-overview-tenants">
          {tenantCount === undefined ? "…" : tenantCount.toString()}
        </Field>
        <Field label="Machines" testid="system-overview-machines">
          {machines === undefined ? "…" : machines.length.toString()}
        </Field>
        <Field label="Services" testid="system-overview-services">
          {services === undefined ? "…" : services.length.toString()}
        </Field>
        <Field label="Listeners" testid="system-overview-listeners">
          {listeners === undefined ? (
            "…"
          ) : (
            <ListenersValue listeners={listeners} />
          )}
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

function useTenantCount(): number | undefined {
  const [count, setCount] = useState<number | undefined>(undefined);
  useEffect(() => {
    const controller = new AbortController();
    fetch("/api/tenants", {
      credentials: "include",
      signal: controller.signal,
    })
      .then((response) => (response.ok ? response.json() : null))
      .then((body: { tenants?: unknown[] } | null) => {
        if (controller.signal.aborted) return;
        if (body && Array.isArray(body.tenants)) {
          setCount(body.tenants.length);
        }
      })
      .catch(() => {
        /* surfaced elsewhere; render — for the field */
      });
    return () => controller.abort();
  }, []);
  return count;
}
