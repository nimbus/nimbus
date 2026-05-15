import { createFileRoute, Link } from "@tanstack/react-router";
import { useQuery } from "nimbus/react";

import { api } from "../../convex/_generated/api";
import { Breadcrumb } from "../components/breadcrumb";
import { CopyChip } from "../components/copy-chip";
import { RelativeTime } from "../components/time";
import { cn } from "../lib/cn";

export const Route = createFileRoute("/storage_/$tenant")({
  component: TenantTablesPage,
});

type TableDoc = {
  _id: string;
  tenantId?: string;
  name?: string;
  schema?: unknown;
  rowCount?: number;
  lastWriteAt?: number;
};

function TenantTablesPage() {
  const { tenant } = Route.useParams();
  const tables = useQuery(api.tables.list, {
    tenantId: tenant,
    limit: 200,
  }) as TableDoc[] | undefined;

  const sortedTables = (tables ?? [])
    .slice()
    .sort((a, b) => (a.name ?? "").localeCompare(b.name ?? ""));

  return (
    <section
      className="flex h-full flex-col gap-4 overflow-hidden px-6 py-5"
      data-testid="page-tenant-tables"
    >
      <header className="flex flex-col gap-2">
        <Breadcrumb
          segments={[
            { label: "storage", href: "/storage" },
            {
              label: tenant,
              copyValue: tenant,
              copyLabel: "tenant id",
              active: true,
            },
          ]}
          testid="tenant-breadcrumb"
        />
        <div className="flex items-baseline justify-between">
          <div>
            <h1 className="text-default" style={{ fontSize: "var(--text-xl)" }}>
              Tables in <span className="font-mono">{tenant}</span>
            </h1>
            <p className="text-sm text-muted">
              Tables are reactive — they appear here as soon as documents are
              written. A table without a schema accepts any document shape.
            </p>
          </div>
        </div>
      </header>

      <div className="min-h-0 flex-1 overflow-hidden rounded-md border border-app bg-surface">
        {tables === undefined ? (
          <Loading label="Loading tables…" />
        ) : sortedTables.length === 0 ? (
          <Empty
            title="No tables"
            detail={`Insert a document via POST /api/tenants/${tenant}/documents or call ctx.db.insert("<table>", ...) from a registered function. Tables appear here as soon as they receive their first write.`}
          />
        ) : (
          <div className="overflow-auto">
            <table
              className="w-full border-collapse text-sm"
              data-testid="tenant-tables-table"
            >
              <thead className="sticky top-0 bg-surface-2 text-[10px] uppercase tracking-[0.14em] text-muted">
                <tr>
                  <Th>Table</Th>
                  <Th>Schema</Th>
                  <Th align="right">Rows</Th>
                  <Th>Last write</Th>
                </tr>
              </thead>
              <tbody>
                {sortedTables.map((table) => {
                  const name = table.name ?? table._id;
                  return (
                    <tr
                      key={table._id}
                      className="border-t border-app hover:bg-surface-2"
                      data-testid={`tenant-table-row-${name}`}
                    >
                      <Td>
                        <Link
                          to="/storage/$tenant/$table"
                          params={{ tenant, table: name }}
                          className="font-mono text-default hover:underline"
                          data-testid={`tenant-table-link-${name}`}
                        >
                          {name}
                        </Link>
                        <span className="ml-2 align-middle">
                          <CopyChip
                            label="table name"
                            value={name}
                            hideUntilHover
                            testid={`tenant-table-copy-${name}`}
                          >
                            copy
                          </CopyChip>
                        </span>
                      </Td>
                      <Td mono>{table.schema ? "defined" : "any"}</Td>
                      <Td align="right" mono>
                        {table.rowCount ?? 0}
                      </Td>
                      <Td>
                        {table.lastWriteAt ? (
                          <RelativeTime epochMs={table.lastWriteAt} />
                        ) : (
                          <span className="text-muted">never</span>
                        )}
                      </Td>
                    </tr>
                  );
                })}
              </tbody>
            </table>
          </div>
        )}
      </div>
    </section>
  );
}

function Th({
  children,
  align = "left",
}: {
  children: React.ReactNode;
  align?: "left" | "right";
}) {
  return (
    <th
      className={cn(
        "px-3 py-2 font-semibold",
        align === "right" ? "text-right" : "text-left",
      )}
    >
      {children}
    </th>
  );
}

function Td({
  children,
  align = "left",
  mono,
}: {
  children: React.ReactNode;
  align?: "left" | "right";
  mono?: boolean;
}) {
  return (
    <td
      className={cn(
        "px-3 py-2 text-default",
        align === "right" ? "text-right" : "text-left",
        mono && "font-mono tabular",
      )}
    >
      {children}
    </td>
  );
}

function Loading({ label }: { label: string }) {
  return (
    <div className="flex h-full items-center justify-center font-mono text-xs text-muted">
      {label}
    </div>
  );
}

function Empty({ title, detail }: { title: string; detail: string }) {
  return (
    <div
      className="flex h-full flex-col items-center justify-center gap-2 px-6 py-10 text-center"
      data-testid="tenant-tables-empty"
    >
      <p className="font-mono text-sm text-default">{title}</p>
      <p className="max-w-md text-xs text-muted">{detail}</p>
    </div>
  );
}
