import { createFileRoute, Link } from "@tanstack/react-router";
import { useQuery } from "nimbus/react";
import { useMemo } from "react";

import { api } from "../../../convex/_generated/api";
import { CopyChip } from "../../components/copy-chip";
import { StateChip } from "../../components/state-chip";
import { RelativeTime } from "../../components/time";
import { cn } from "../../lib/cn";
import { shortId } from "../../lib/format";
import { buildFunctionTree } from "../../shell/function-tree";
import { FunctionTreeView } from "../../shell/function-tree-view";
import {
  type SubDrawerSpec,
  useContributeSubDrawer,
  useSubDrawerSearch,
} from "../../shell/sub-drawer";

export const Route = createFileRoute("/app/compute")({
  component: ComputePage,
});

type FunctionDoc = {
  _id: string;
  _updateTime?: number;
  path?: string;
  kind?: string;
  adapter?: string;
  bundleId?: string;
  source?: string;
  argsSchema?: unknown;
  returnsSchema?: unknown;
  lastStatus?: string;
  lastRunAt?: number;
};

type BundleDoc = {
  _id: string;
  sha256?: string;
  status?: string;
  sourceRef?: string;
  _creationTime?: number;
};

function ComputePage() {
  const functions = useQuery(api.functions.list, {
    bundleId: null,
    kind: null,
    limit: 200,
  }) as FunctionDoc[] | undefined;
  const bundles = useQuery(api.bundles.list, {
    status: null,
    limit: 50,
  }) as BundleDoc[] | undefined;

  const spec = useMemo<SubDrawerSpec>(
    () => ({
      kind: "dynamic",
      title: "Functions",
      search: { placeholder: "Filter functions" },
      children: <ComputeSubDrawer functions={functions} />,
    }),
    [functions],
  );
  useContributeSubDrawer(spec);

  return (
    <section
      className="flex h-full flex-col gap-4 overflow-hidden px-6 py-5"
      data-testid="page-compute"
    >
      <header className="flex items-baseline justify-between">
        <div>
          <h1
            className="text-xl text-default"
            style={{ fontSize: "var(--text-xl)" }}
          >
            Compute
          </h1>
          <p className="text-sm text-muted">
            Functions registered to this tenant. Click a function in the drawer
            to view its source, logs, and runs, or invoke it from the docked
            runner.
          </p>
        </div>
        <BundleHint bundles={bundles} />
      </header>

      <div className="min-h-0 flex-1 overflow-hidden rounded-md border border-app bg-surface">
        <FunctionsTable functions={functions} bundles={bundles} />
      </div>
    </section>
  );
}

function ComputeSubDrawer({
  functions,
}: {
  functions: FunctionDoc[] | undefined;
}) {
  const search = useSubDrawerSearch();
  const tree = useMemo(() => buildFunctionTree(functions ?? []), [functions]);
  if (functions === undefined) {
    return (
      <div className="px-3 py-3 text-xs text-muted">
        <span aria-hidden>·</span>
        <span className="sr-only">loading</span>
      </div>
    );
  }
  return (
    <FunctionTreeView tree={tree} filter={search} testidPrefix="sub-drawer" />
  );
}

function BundleHint({ bundles }: { bundles: BundleDoc[] | undefined }) {
  if (bundles === undefined) {
    return (
      <span
        className="font-mono text-[11px] text-muted"
        data-testid="compute-bundles-loading"
      >
        bundles: loading…
      </span>
    );
  }
  const active = bundles.filter((b) => b.status === "active").length;
  return (
    <span
      className="font-mono text-[11px] text-muted"
      data-testid="compute-bundles"
    >
      {bundles.length} bundle{bundles.length === 1 ? "" : "s"}
      {active > 0 ? ` · ${active} active` : ""}
    </span>
  );
}

function FunctionsTable({
  functions,
  bundles,
}: {
  functions: FunctionDoc[] | undefined;
  bundles: BundleDoc[] | undefined;
}) {
  const bundleLookup = useMemo(() => {
    const map = new Map<string, BundleDoc>();
    bundles?.forEach((b) => {
      map.set(b._id, b);
    });
    return map;
  }, [bundles]);
  if (functions === undefined) return <Loading label="Loading functions…" />;
  if (functions.length === 0) {
    return (
      <Empty
        title="No functions registered"
        detail="Deploy a Convex, Nimbus, or Cloud Functions app to populate the function inventory."
      />
    );
  }
  return (
    <div className="overflow-auto">
      <table
        className="w-full border-collapse text-sm"
        data-testid="compute-functions-table"
      >
        <thead className="sticky top-0 bg-surface-2 text-[10px] uppercase tracking-[0.14em] text-muted">
          <tr>
            <Th>Path</Th>
            <Th>Kind</Th>
            <Th>Adapter</Th>
            <Th>Bundle</Th>
            <Th>Last status</Th>
            <Th>Last run</Th>
            <Th>Action</Th>
          </tr>
        </thead>
        <tbody>
          {functions.map((fn) => {
            const bundle = fn.bundleId
              ? bundleLookup.get(fn.bundleId)
              : undefined;
            const path = fn.path;
            return (
              <tr
                key={fn._id}
                className="border-t border-app hover:bg-surface-2"
                data-testid={`compute-function-${path ?? fn._id}`}
              >
                <Td>
                  <span className="font-mono text-default">{path ?? "—"}</span>
                </Td>
                <Td>
                  <span className="font-mono text-xs uppercase tracking-wide text-muted">
                    {fn.kind ?? "—"}
                  </span>
                </Td>
                <Td>
                  <span className="font-mono text-xs text-default">
                    {fn.adapter ?? "—"}
                  </span>
                </Td>
                <Td>
                  {bundle?.sha256 ? (
                    <CopyChip
                      label="bundle sha256"
                      value={bundle.sha256}
                      testid={`compute-function-bundle-${path ?? fn._id}`}
                    >
                      {shortId(bundle.sha256, 12)}
                    </CopyChip>
                  ) : (
                    <span className="tabular text-muted">—</span>
                  )}
                </Td>
                <Td>
                  <StateChip state={fn.lastStatus ?? "idle"} />
                </Td>
                <Td>
                  {typeof fn.lastRunAt === "number" ? (
                    <RelativeTime epochMs={fn.lastRunAt} />
                  ) : (
                    <span className="tabular text-muted">never</span>
                  )}
                </Td>
                <Td>
                  {path ? (
                    <Link
                      to="/app/compute/$function"
                      params={{ function: path }}
                      data-testid={`compute-function-open-${path}`}
                      className="rounded border border-app px-1.5 py-0.5 font-mono text-[11px] uppercase tracking-wide text-muted hover:bg-surface hover:text-default"
                    >
                      open
                    </Link>
                  ) : (
                    <span className="tabular text-muted">—</span>
                  )}
                </Td>
              </tr>
            );
          })}
        </tbody>
      </table>
    </div>
  );
}

function Th({
  children,
  className,
}: {
  children: React.ReactNode;
  className?: string;
}) {
  return (
    <th
      className={cn(
        "border-b border-app px-3 py-2 text-left font-normal",
        className,
      )}
    >
      {children}
    </th>
  );
}

function Td({
  children,
  className,
}: {
  children: React.ReactNode;
  className?: string;
}) {
  return (
    <td className={cn("px-3 py-2 align-middle", className)}>{children}</td>
  );
}

function Loading({ label }: { label: string }) {
  return (
    <div className="flex h-32 items-center justify-center text-xs text-muted">
      {label}
    </div>
  );
}

function Empty({ title, detail }: { title: string; detail: string }) {
  return (
    <div className="flex h-32 flex-col items-center justify-center gap-1 text-center">
      <span className="font-mono text-sm text-default">{title}</span>
      <span className="max-w-md text-xs text-muted">{detail}</span>
    </div>
  );
}
