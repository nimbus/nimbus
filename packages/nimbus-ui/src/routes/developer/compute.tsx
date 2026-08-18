import { useQuery } from "@nimbus/nimbus/react";
import { createFileRoute, useNavigate } from "@tanstack/react-router";
import { Search } from "lucide-react";
import { type ReactNode, useCallback, useMemo, useState } from "react";

import { api } from "../../../convex/_generated/api";
import { EmptyState } from "../../components/empty-state";
import { LoadingState } from "../../components/loading-state";
import { PageHeader } from "../../components/page-header";
import { cn } from "../../lib/cn";
import type { FunctionDoc } from "../../lib/types/function";
import { buildFunctionTree } from "../../shell/function-tree";
import { FunctionTreeView } from "../../shell/function-tree-view";
import {
  type SubDrawerSpec,
  useContributeSubDrawer,
} from "../../shell/sub-drawer";
import {
  COMPUTE_VIEWS,
  type ComputeView,
  parseComputeView,
} from "./compute-views";
import { GraphView } from "./graph-view";

type ComputeSearch = { view?: ComputeView };

export const Route = createFileRoute("/developer/compute")({
  validateSearch: (search: Record<string, unknown>): ComputeSearch => ({
    view: parseComputeView(search.view),
  }),
  component: ComputePage,
});

type BundleDoc = {
  _id: string;
  sha256?: string;
  status?: string;
  sourceRef?: string;
  _creationTime?: number;
};

function ComputePage() {
  const view = Route.useSearch().view ?? "functions";
  const navigate = useNavigate({ from: "/developer/compute" });

  const functions = useQuery(api.functions.list, {
    bundleId: null,
    kind: null,
    limit: 200,
  }) as FunctionDoc[] | undefined;
  const bundles = useQuery(api.bundles.list, {
    status: null,
    limit: 50,
  }) as BundleDoc[] | undefined;

  const setView = useCallback(
    (next: ComputeView) => {
      void navigate({
        search: (prev) => ({ ...prev, view: next }),
        replace: true,
      });
    },
    [navigate],
  );

  // The sub-drawer is purely the compute-type selector (Functions / Sandboxes).
  // Search and filters live in the main section's toolbar, not here.
  const spec = useMemo<SubDrawerSpec>(
    () => ({
      kind: "dynamic",
      title: "Compute",
      railItems: COMPUTE_VIEWS.map((v) => ({
        id: v.value,
        label: v.label,
        icon: v.icon,
        active: v.value === view,
        onSelect: () => setView(v.value),
      })),
      children: <ComputeDrawer view={view} onViewChange={setView} />,
    }),
    [view, setView],
  );
  useContributeSubDrawer(spec);

  return (
    <section
      className="flex h-full flex-col gap-4 overflow-hidden px-6 py-5"
      data-testid="page-compute"
    >
      <PageHeader
        title="Compute"
        subtitle={
          view === "graph"
            ? "The deployment's function call graph — api.* / internal.* calls between functions. Click a node to open its source."
            : view === "sandboxes"
              ? "Sandboxes are isolated execution environments for this tenant, read live from the sandbox runtime."
              : "Functions registered to this tenant, grouped by bundle and module. Open one to view its source, logs, and runs."
        }
      />

      {view === "graph" ? (
        <GraphView />
      ) : view === "sandboxes" ? (
        <SandboxesView />
      ) : (
        <FunctionsView functions={functions} bundles={bundles} />
      )}
    </section>
  );
}

// ---------------------------------------------------------------------------
// Sub-drawer: the compute-type selector (Functions / Sandboxes)
// ---------------------------------------------------------------------------

function ComputeDrawer({
  view,
  onViewChange,
}: {
  view: ComputeView;
  onViewChange: (view: ComputeView) => void;
}) {
  return (
    <nav aria-label="Compute type" className="flex flex-col gap-px px-2 py-2">
      {COMPUTE_VIEWS.map((opt) => {
        const Icon = opt.icon;
        const active = view === opt.value;
        return (
          <button
            key={opt.value}
            type="button"
            onClick={() => onViewChange(opt.value)}
            aria-current={active ? "page" : undefined}
            data-testid={`compute-drawer-view-${opt.value}`}
            data-active={active ? "true" : "false"}
            className={cn(
              "flex h-9 items-center gap-2 rounded-md border-l-2 border-transparent px-2 text-sm",
              active
                ? "bg-surface-2 text-default"
                : "text-muted hover:bg-surface-2 hover:text-default",
            )}
            style={
              active ? { borderLeftColor: "var(--nimbus-brand)" } : undefined
            }
          >
            <Icon size={14} aria-hidden className="shrink-0" />
            <span className="flex-1 text-left">{opt.label}</span>
          </button>
        );
      })}
    </nav>
  );
}

// ---------------------------------------------------------------------------
// Functions view — toolbar (search + kind filter) over a bundle/module tree
// ---------------------------------------------------------------------------

function FunctionsView({
  functions,
  bundles,
}: {
  functions: FunctionDoc[] | undefined;
  bundles: BundleDoc[] | undefined;
}) {
  const [search, setSearch] = useState("");
  const [kind, setKind] = useState<string | null>(null);
  const kinds = useMemo(
    () => uniqueSorted(functions, (fn) => fn.kind),
    [functions],
  );
  const treeFns = useMemo(() => {
    const list = functions ?? [];
    return kind ? list.filter((fn) => fn.kind === kind) : list;
  }, [functions, kind]);
  const tree = useMemo(() => buildFunctionTree(treeFns), [treeFns]);

  return (
    <div className="flex min-h-0 flex-1 flex-col gap-3 overflow-hidden">
      <Toolbar
        search={search}
        onSearch={setSearch}
        placeholder="Search functions"
        testid="compute-functions-toolbar"
      >
        <FilterChips
          ariaLabel="Filter by kind"
          allLabel="all kinds"
          options={kinds}
          active={kind}
          onChange={setKind}
          testidPrefix="compute-function-kind"
        />
        <span className="ml-auto">
          <BundleHint bundles={bundles} />
        </span>
      </Toolbar>
      <div className="min-h-0 flex-1 overflow-auto rounded-md border border-app bg-surface">
        {functions === undefined ? (
          <LoadingState label="Loading functions…" />
        ) : (
          <FunctionTreeView
            tree={tree}
            filter={search}
            testidPrefix="compute-functions"
          />
        )}
      </div>
    </div>
  );
}

function BundleHint({ bundles }: { bundles: BundleDoc[] | undefined }) {
  if (bundles === undefined) {
    return (
      <span
        className="font-mono text-xs text-muted"
        data-testid="compute-bundles-loading"
      >
        bundles: loading…
      </span>
    );
  }
  const active = bundles.filter((b) => b.status === "active").length;
  return (
    <span
      className="font-mono text-xs text-muted"
      data-testid="compute-bundles"
    >
      {bundles.length} bundle{bundles.length === 1 ? "" : "s"}
      {active > 0 ? ` · ${active} active` : ""}
    </span>
  );
}

// ---------------------------------------------------------------------------
// Sandboxes view — sandboxes are live runtime state, not deployment records,
// so this reads from the sandbox runtime (not a persisted table). Live wiring
// is a tracked follow-on; until then this shows an honest empty state rather
// than any placeholder data.
// ---------------------------------------------------------------------------

function SandboxesView() {
  return (
    <div
      className="flex min-h-0 flex-1 flex-col overflow-hidden rounded-md border border-app bg-surface"
      data-testid="compute-sandboxes"
    >
      <EmptyState
        title="No live sandboxes"
        body="Sandboxes are live runtime state, not deployment records. Live runtime wiring is in progress; running sandboxes for this tenant will appear here once connected. No placeholder data is shown."
        testid="compute-sandboxes-empty"
      />
    </div>
  );
}

// ---------------------------------------------------------------------------
// Shared bits
// ---------------------------------------------------------------------------

// Main-section toolbar: search input (left) + filters (children), following the
// shadcn data-table-toolbar convention of keeping search and filters together.
function Toolbar({
  search,
  onSearch,
  placeholder,
  children,
  testid,
}: {
  search: string;
  onSearch: (value: string) => void;
  placeholder: string;
  children?: ReactNode;
  testid?: string;
}) {
  return (
    <div
      className="flex flex-wrap items-center gap-2 rounded-md border border-app bg-surface-2 px-3 py-2"
      data-testid={testid}
    >
      <div className="relative">
        <Search
          size={13}
          aria-hidden
          className="pointer-events-none absolute left-2 top-1/2 -translate-y-1/2 text-muted"
        />
        <input
          type="search"
          value={search}
          onChange={(e) => onSearch(e.target.value)}
          placeholder={placeholder}
          data-testid={testid ? `${testid}-search` : undefined}
          className="h-7 w-56 rounded border border-app bg-surface pl-7 pr-2 font-mono text-xs text-default placeholder:text-muted focus:outline-none focus:ring-1 focus:ring-[color:var(--nimbus-brand)]"
        />
      </div>
      {children}
    </div>
  );
}

// Toggle buttons, not tabs. The set is data-derived and each chip filters the
// list in place rather than swapping a tabpanel, so a labelled group of
// `aria-pressed` buttons is the honest contract — and native Tab/Space/Enter is
// its complete keyboard behavior, with no roving tabindex or arrow keys
// promised. SegmentedControl (DESIGN.md's canonical exclusive-choice control)
// does not apply: it is scoped to fixed sets of ≤4 options.
function FilterChips({
  ariaLabel,
  allLabel,
  options,
  active,
  onChange,
  testidPrefix,
}: {
  ariaLabel: string;
  allLabel: string;
  options: string[];
  active: string | null;
  onChange: (value: string | null) => void;
  testidPrefix: string;
}) {
  return (
    <fieldset className="flex min-w-0 flex-wrap items-center gap-1">
      <legend className="sr-only">{ariaLabel}</legend>
      <Chip
        label={allLabel}
        active={active === null}
        onClick={() => onChange(null)}
        testid={`${testidPrefix}-all`}
      />
      {options.map((opt) => (
        <Chip
          key={opt}
          label={opt}
          active={active === opt}
          onClick={() => onChange(opt)}
          testid={`${testidPrefix}-${opt}`}
        />
      ))}
    </fieldset>
  );
}

function Chip({
  label,
  active,
  onClick,
  testid,
}: {
  label: string;
  active: boolean;
  onClick: () => void;
  testid: string;
}) {
  return (
    <button
      type="button"
      aria-pressed={active}
      onClick={onClick}
      data-testid={testid}
      className={cn(
        "rounded border px-2 py-0.5 font-mono text-xs uppercase tracking-wide",
        active
          ? "border-strong bg-surface text-default"
          : "border-app text-muted hover:bg-surface hover:text-default",
      )}
    >
      {label}
    </button>
  );
}

function uniqueSorted<T>(
  items: T[] | undefined,
  pick: (item: T) => string | undefined,
): string[] {
  const set = new Set<string>();
  for (const item of items ?? []) {
    const value = pick(item);
    if (value) set.add(value);
  }
  return Array.from(set).sort();
}
