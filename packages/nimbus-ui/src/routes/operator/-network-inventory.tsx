import { useQuery } from "@nimbus/nimbus/react";
import type { ReactNode } from "react";

import { api } from "../../../convex/_generated/api";
import { Td, Th } from "../../components/data-table";
import { EmptyState } from "../../components/empty-state";
import { SkeletonRows } from "../../components/loading-state";
import { PageHeader } from "../../components/page-header";
import { ScrollRegion } from "../../components/scroll-region";
import { RelativeTime } from "../../components/time";
import { cn } from "../../lib/cn";

export type NetworkInventorySectionName =
  | "ws"
  | "ports"
  | "listeners"
  | "security";

type InventoryDoc = {
  _id: string;
};

type SubscriptionDoc = InventoryDoc & {
  tenantId?: string;
  adapter?: string;
  queryKey?: string;
  clientCount?: number;
  lastDeliveryAt?: number;
  error?: string;
};

type PortDoc = InventoryDoc & {
  machineId?: string;
  serviceId?: string;
  hostPort?: number;
  guestPort?: number;
  protocol?: string;
  actualAddress?: string;
  observedPhase?: string;
};

type ListenerDoc = InventoryDoc & {
  adapter?: string;
  protocol?: string;
  actualAddress?: string;
  observedPhase?: string;
  version?: string;
  error?: string;
};

type CapabilityDoc = InventoryDoc & {
  adapter?: string;
  feature?: string;
  status?: string;
  caveat?: string;
  evidence?: string;
};

type InventoryColumn<T> = {
  label: string;
  render: (record: T) => ReactNode;
};

type InventoryPageProps<T extends InventoryDoc> = {
  section: NetworkInventorySectionName;
  subtitle: string;
  records: T[] | undefined;
  noun: string;
  emptyTitle: string;
  emptyBody: string;
  columns: InventoryColumn<T>[];
};

export function NetworkInventorySection({
  section,
}: {
  section: NetworkInventorySectionName;
}) {
  switch (section) {
    case "ws":
      return <WebSocketSection />;
    case "ports":
      return <PortsSection />;
    case "listeners":
      return <ListenersSection />;
    case "security":
      return <SecuritySection />;
  }
}

function WebSocketSection() {
  const subscriptions = useQuery(api.subscriptions.list, {
    tenantId: null,
    adapter: null,
    limit: 500,
  }) as SubscriptionDoc[] | undefined;

  return (
    <InventoryPage
      section="ws"
      subtitle="Live WebSocket subscriptions and their latest delivery state."
      records={subscriptions}
      noun="subscriptions"
      emptyTitle="No WebSocket subscriptions"
      emptyBody="Active subscriptions appear here after a client connects and starts a query."
      columns={[
        { label: "Adapter", render: (row) => <Mono>{row.adapter}</Mono> },
        { label: "Tenant", render: (row) => <Mono>{row.tenantId}</Mono> },
        {
          label: "Query key",
          render: (row) => <Truncated value={row.queryKey} width="42ch" />,
        },
        {
          label: "Clients",
          render: (row) => (
            <span className="tabular text-default">
              {typeof row.clientCount === "number" ? row.clientCount : "—"}
            </span>
          ),
        },
        {
          label: "Last delivery",
          render: (row) =>
            typeof row.lastDeliveryAt === "number" ? (
              <RelativeTime epochMs={row.lastDeliveryAt} />
            ) : (
              <span className="text-muted">never</span>
            ),
        },
        { label: "Error", render: (row) => <ErrorText value={row.error} /> },
      ]}
    />
  );
}

function PortsSection() {
  const ports = useQuery(api.ports.list, {
    machineId: null,
    serviceId: null,
    observedPhase: null,
    limit: 500,
  }) as PortDoc[] | undefined;

  return (
    <InventoryPage
      section="ports"
      subtitle="Published host ports and their machine or service owners."
      records={ports}
      noun="ports"
      emptyTitle="No published ports"
      emptyBody="Ports appear here when a machine or service publishes a host endpoint."
      columns={[
        {
          label: "Host port",
          render: (row) => <NumberCell value={row.hostPort} />,
        },
        {
          label: "Guest port",
          render: (row) => <NumberCell value={row.guestPort} />,
        },
        { label: "Protocol", render: (row) => <Mono>{row.protocol}</Mono> },
        {
          label: "Address",
          render: (row) => <Truncated value={row.actualAddress} width="34ch" />,
        },
        {
          label: "Owner",
          render: (row) => (
            <Mono>{row.serviceId ?? row.machineId ?? undefined}</Mono>
          ),
        },
        {
          label: "Phase",
          render: (row) => <StatusText value={row.observedPhase} />,
        },
      ]}
    />
  );
}

function ListenersSection() {
  const listeners = useQuery(api.listeners.list, {
    adapter: null,
    observedPhase: null,
    limit: 500,
  }) as ListenerDoc[] | undefined;

  return (
    <InventoryPage
      section="listeners"
      subtitle="Bound protocol listeners from the live server registry."
      records={listeners}
      noun="listeners"
      emptyTitle="No listeners"
      emptyBody="Listeners appear here after a protocol surface binds successfully."
      columns={[
        { label: "Adapter", render: (row) => <Mono>{row.adapter}</Mono> },
        { label: "Protocol", render: (row) => <Mono>{row.protocol}</Mono> },
        {
          label: "Address",
          render: (row) => <Truncated value={row.actualAddress} width="34ch" />,
        },
        {
          label: "Phase",
          render: (row) => <StatusText value={row.observedPhase} />,
        },
        { label: "Version", render: (row) => <Mono>{row.version}</Mono> },
        { label: "Error", render: (row) => <ErrorText value={row.error} /> },
      ]}
    />
  );
}

function SecuritySection() {
  const capabilities = useQuery(api.adapter_capabilities.list, {
    adapter: null,
    status: null,
    limit: 500,
  }) as CapabilityDoc[] | undefined;

  return (
    <InventoryPage
      section="security"
      subtitle="Adapter capability status, caveats, and the evidence behind each claim."
      records={capabilities}
      noun="capabilities"
      emptyTitle="No capability records"
      emptyBody="Adapter security and compatibility claims appear here when they register."
      columns={[
        { label: "Adapter", render: (row) => <Mono>{row.adapter}</Mono> },
        {
          label: "Capability",
          render: (row) => <Truncated value={row.feature} width="30ch" />,
        },
        {
          label: "Status",
          render: (row) => <StatusText value={row.status} />,
        },
        {
          label: "Caveat",
          render: (row) => <Truncated value={row.caveat} width="42ch" />,
        },
        {
          label: "Evidence",
          render: (row) => <Truncated value={row.evidence} width="42ch" />,
        },
      ]}
    />
  );
}

function InventoryPage<T extends InventoryDoc>({
  section,
  subtitle,
  records,
  noun,
  emptyTitle,
  emptyBody,
  columns,
}: InventoryPageProps<T>) {
  return (
    <section
      className="flex h-full flex-col gap-4 overflow-hidden px-6 py-5"
      data-testid="page-network"
      data-section={section}
    >
      <PageHeader
        title="Network"
        subtitle={subtitle}
        trailing={
          <span
            className="font-mono text-xs text-muted"
            data-testid="network-total"
          >
            {records === undefined ? "loading…" : `${records.length} ${noun}`}
          </span>
        }
      />

      <div className="min-h-0 flex-1 overflow-hidden rounded-md border border-app bg-surface">
        {records === undefined ? (
          <SkeletonRows
            columns={columns.length}
            head={<InventoryTableHead columns={columns} />}
            label={`Loading ${noun}…`}
            testid={`network-${section}-loading`}
          />
        ) : records.length > 0 ? (
          <InventoryTable
            label={noun}
            section={section}
            records={records}
            columns={columns}
          />
        ) : (
          <EmptyState
            title={emptyTitle}
            body={emptyBody}
            testid={`network-${section}-empty`}
          />
        )}
      </div>
    </section>
  );
}

function InventoryTableHead<T>({ columns }: { columns: InventoryColumn<T>[] }) {
  return (
    <thead className="sticky top-0 bg-surface-2 text-xs uppercase tracking-[0.14em] text-muted">
      <tr>
        {columns.map((column) => (
          <Th key={column.label}>{column.label}</Th>
        ))}
      </tr>
    </thead>
  );
}

function InventoryTable<T extends InventoryDoc>({
  label,
  section,
  records,
  columns,
}: {
  label: string;
  section: NetworkInventorySectionName;
  records: T[];
  columns: InventoryColumn<T>[];
}) {
  return (
    <ScrollRegion label={label} className="h-full">
      <table
        className="w-full border-collapse text-base"
        data-testid={`network-${section}-table`}
      >
        <InventoryTableHead columns={columns} />
        <tbody>
          {records.map((record) => (
            <tr
              key={record._id}
              className="border-t border-app hover:bg-surface-2"
            >
              {columns.map((column) => (
                <Td key={column.label}>{column.render(record)}</Td>
              ))}
            </tr>
          ))}
        </tbody>
      </table>
    </ScrollRegion>
  );
}

function Mono({ children }: { children: string | undefined }) {
  return <span className="font-mono text-default">{children ?? "—"}</span>;
}

function NumberCell({ value }: { value: number | undefined }) {
  return (
    <span className="font-mono tabular text-default">
      {typeof value === "number" ? value : "—"}
    </span>
  );
}

function Truncated({
  value,
  width,
}: {
  value: string | undefined;
  width: "30ch" | "34ch" | "42ch";
}) {
  const widths = {
    "30ch": "max-w-[30ch]",
    "34ch": "max-w-[34ch]",
    "42ch": "max-w-[42ch]",
  } as const;
  return (
    <span
      className={cn("block truncate font-mono text-default", widths[width])}
      title={value}
    >
      {value ?? "—"}
    </span>
  );
}

function ErrorText({ value }: { value: string | undefined }) {
  return (
    <span
      className={cn(
        "block max-w-[34ch] truncate font-mono",
        value ? "text-danger" : "text-muted",
      )}
      title={value}
    >
      {value ?? "—"}
    </span>
  );
}

function StatusText({ value }: { value: string | undefined }) {
  const normalized = value?.toLowerCase() ?? "";
  const tone =
    normalized === "ready" ||
    normalized === "running" ||
    normalized === "listening" ||
    normalized === "active" ||
    normalized === "supported"
      ? "text-success"
      : normalized === "error" ||
          normalized === "failed" ||
          normalized === "unsupported"
        ? "text-danger"
        : normalized === "partial" || normalized === "degraded"
          ? "text-warning"
          : "text-muted";
  return (
    <span className={cn("font-mono uppercase tracking-wide", tone)}>
      {value ?? "—"}
    </span>
  );
}
