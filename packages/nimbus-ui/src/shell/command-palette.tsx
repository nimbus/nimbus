import { useQuery } from "@nimbus/nimbus/react";
import { useNavigate, useRouterState } from "@tanstack/react-router";
import { Command } from "cmdk";
import type { LucideIcon } from "lucide-react";
import {
  Boxes,
  Building2,
  Command as CommandIcon,
  Cpu,
  Database,
  Filter,
  MonitorCog,
  Network,
  PlayCircle,
  RotateCw,
  Search,
} from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";
import { api } from "../../convex/_generated/api";
import { Kbd } from "../components/kbd";
import { useTenantList } from "../hooks/use-tenant-list";
import { cn } from "../lib/cn";
import { metaGlyph } from "../lib/platform";
import { useUiStore } from "../store/ui-store";
import {
  DEVELOPER_NAV_ENTRIES,
  type NavEntry,
  OPERATOR_NAV_ENTRIES,
  viewFromPathname,
} from "./nav-entries";

type Mode = "navigate" | "run" | "filter";

const RECENT_KEY = "nimbus-ui:palette:recent";
const RECENT_LIMIT = 5;

// Resource kinds the palette can jump to, plus "section" for the 15 drawer
// entries. The kind is persisted with each recent so a stored resource can be
// re-rendered (icon + label + target) without re-querying the server.
type TargetKind =
  | "section"
  | "table"
  | "function"
  | "tenant"
  | "machine"
  | "service"
  | "route";

const TARGET_ICONS: Record<TargetKind, LucideIcon> = {
  section: CommandIcon,
  table: Database,
  function: Cpu,
  tenant: Building2,
  machine: MonitorCog,
  service: Boxes,
  route: Network,
};

// One selectable jump target. `href` is a resolved path (never a route id with
// params) so a target read back out of localStorage stays navigable.
type PaletteTarget = {
  kind: TargetKind;
  key: string;
  label: string;
  detail?: string;
  href: string;
};

function sectionTarget(entry: NavEntry): PaletteTarget {
  return {
    kind: "section",
    key: `${entry.view}:${entry.id}`,
    label: entry.label,
    detail: entry.view,
    href: entry.to,
  };
}

type RunAction = {
  id: string;
  label: string;
  hint?: string;
  perform: () => void;
};

export function CommandPalette() {
  const open = useUiStore((s) => s.paletteOpen);
  const setOpen = useUiStore((s) => s.setPaletteOpen);
  const setLensOpen = useUiStore((s) => s.setLensOpen);
  const navigate = useNavigate();
  const pathname = useRouterState({ select: (s) => s.location.pathname });
  const view = viewFromPathname(pathname);
  const [mode, setMode] = useState<Mode>("navigate");
  const [search, setSearch] = useState("");
  const [recent, setRecent] = useState<PaletteTarget[]>(loadRecent);

  useEffect(() => {
    if (open) {
      setSearch("");
      setMode("navigate");
    }
  }, [open]);

  const runActions: RunAction[] = [
    ...(view === "developer"
      ? [
          {
            id: "open-system-tenant-lens",
            label: "Open system tenant lens",
            hint: `${metaGlyph} \\`,
            perform: () => {
              setOpen(false);
              queueMicrotask(() => setLensOpen(true));
            },
          },
        ]
      : []),
    {
      id: "refresh-current-view",
      label: "Refresh current view",
      hint: `${metaGlyph} R`,
      perform: () => {
        setOpen(false);
        window.location.reload();
      },
    },
  ];

  // Jump to a target and push it onto the recents list. Recents hold the whole
  // target record (not a bare key), so a table or function selected here still
  // resolves after a reload — resolving keys against the nav list alone would
  // render every stored resource as `null` forever.
  const pickTarget = useCallback(
    (target: PaletteTarget) => {
      setRecent((current) => {
        const next = [
          target,
          ...current.filter((existing) => existing.key !== target.key),
        ].slice(0, RECENT_LIMIT);
        persistRecent(next);
        return next;
      });
      setOpen(false);
      navigate({ to: target.href });
    },
    [navigate, setOpen],
  );

  if (!open) return null;
  const query = search.trim();
  return (
    <div className="fixed inset-0 z-50 flex items-start justify-center bg-black/40 backdrop-blur-[1px] pt-[12vh] animate-in fade-in-0">
      <button
        type="button"
        aria-label="Close command palette"
        className="absolute inset-0 cursor-default"
        onClick={() => setOpen(false)}
      />
      <Command
        loop
        role="dialog"
        aria-label="Command palette"
        className="relative flex max-h-[80vh] w-[min(640px,90vw)] flex-col overflow-hidden rounded-lg border bg-surface shadow-2xl border-strong animate-in zoom-in-95 fade-in-0"
        data-testid="command-palette"
      >
        <div className="flex items-center gap-2 border-b border-app px-3 py-2">
          <Search size={14} className="text-muted" aria-hidden />
          <Command.Input
            value={search}
            onValueChange={setSearch}
            autoFocus
            placeholder={
              mode === "navigate"
                ? "Jump to a table, function, tenant, machine, service, route…"
                : mode === "run"
                  ? "Run an action…"
                  : "Filter the current view…"
            }
            className="h-7 flex-1 bg-transparent text-sm outline-none placeholder:text-muted text-default"
            data-testid="command-palette-input"
          />
          <ModeToggle current={mode} onChange={setMode} />
        </div>
        <Command.List
          className="min-h-0 flex-1 max-h-[60vh] overflow-y-auto px-1 py-1"
          data-testid="command-palette-list"
        >
          <Command.Empty className="px-3 py-6 text-center text-sm text-muted">
            No matches.
          </Command.Empty>

          {recent.length > 0 && query === "" ? (
            <Command.Group heading="Recent">
              {recent.map((target) => (
                <PaletteItem
                  key={`recent-${target.key}`}
                  target={target}
                  hint={`${metaGlyph} ⏎`}
                  onSelect={() => pickTarget(target)}
                />
              ))}
            </Command.Group>
          ) : null}

          {mode === "navigate" ? (
            <>
              {/* Resource groups mount only once the operator types: the palette
                  can hold several hundred rows, and rendering them all on open
                  would cost more than it buys when nothing has been searched
                  for yet. The queries themselves live in PaletteResults, which
                  is mounted only inside this open branch — the palette holds no
                  subscriptions while it is closed. */}
              {query === "" ? null : <PaletteResults onPick={pickTarget} />}
              <Command.Group heading="Developer console">
                {DEVELOPER_NAV_ENTRIES.map((entry) => {
                  const target = sectionTarget(entry);
                  return (
                    <PaletteItem
                      key={`nav-${target.key}`}
                      target={target}
                      icon={entry.icon}
                      hint="⏎ open"
                      onSelect={() => pickTarget(target)}
                    />
                  );
                })}
              </Command.Group>
              <Command.Group heading="Operator console">
                {OPERATOR_NAV_ENTRIES.map((entry) => {
                  const target = sectionTarget(entry);
                  return (
                    <PaletteItem
                      key={`nav-${target.key}`}
                      target={target}
                      icon={entry.icon}
                      hint="⏎ open"
                      onSelect={() => pickTarget(target)}
                    />
                  );
                })}
              </Command.Group>
            </>
          ) : null}

          {mode === "run" ? (
            <Command.Group heading="Run">
              {runActions.map((action) => (
                <ActionItem
                  key={action.id}
                  icon={
                    action.id === "refresh-current-view" ? RotateCw : PlayCircle
                  }
                  label={action.label}
                  hint={action.hint}
                  onSelect={action.perform}
                />
              ))}
            </Command.Group>
          ) : null}

          {mode === "filter" ? (
            <Command.Group heading="Filter">
              <ActionItem
                icon={Filter}
                label="Apply text filter to current view"
                hint="⏎"
                onSelect={() => {
                  setOpen(false);
                  window.dispatchEvent(
                    new CustomEvent("nimbus:filter", { detail: search }),
                  );
                }}
              />
            </Command.Group>
          ) : null}
        </Command.List>
        <div
          className="flex shrink-0 items-center gap-3 border-t border-app px-3 py-1.5 text-xs text-muted"
          data-testid="command-palette-footer"
        >
          <span className="inline-flex items-center gap-1">
            <Kbd>↑</Kbd>
            <Kbd>↓</Kbd>
            <span>move</span>
          </span>
          <span className="inline-flex items-center gap-1">
            <Kbd>⏎</Kbd>
            <span>run</span>
          </span>
          <span className="inline-flex items-center gap-1">
            <Kbd>⎋</Kbd>
            <span>close</span>
          </span>
          <span className="ml-auto inline-flex items-center gap-1">
            <CommandIcon size={12} aria-hidden />
            <span>{mode}</span>
          </span>
        </div>
      </Command>
    </div>
  );
}

type ResourceRow = {
  _id: string;
  name?: string;
  path?: string;
  method?: string;
  adapter?: string;
  kind?: string;
  state?: string;
  tenantId?: string;
};

// The resource half of the navigate list. Kept in its own component so the six
// resource reads mount only while the palette is open — hooks placed on
// CommandPalette itself would hold live subscriptions for the whole session,
// since `if (!open) return null` runs after the hooks.
function PaletteResults({ onPick }: { onPick: (t: PaletteTarget) => void }) {
  const tenant = useUiStore((s) => s.activeTenant);
  const tenantList = useTenantList();

  const tables = useQuery(
    api.tables.list,
    tenant ? { tenantId: tenant, limit: 200 } : "skip",
  ) as ResourceRow[] | undefined;
  const functions = useQuery(api.functions.list, {
    bundleId: null,
    kind: null,
    limit: 200,
  }) as ResourceRow[] | undefined;
  const services = useQuery(api.services.list, {
    tenantId: null,
    machineId: null,
    state: null,
    limit: 200,
  }) as ResourceRow[] | undefined;
  const machines = useQuery(api.machines.list, {
    state: null,
    provider: null,
    limit: 200,
  }) as ResourceRow[] | undefined;
  const routes = useQuery(api.routes.list, {
    adapter: null,
    limit: 200,
  }) as ResourceRow[] | undefined;

  const tableTargets = useMemo<PaletteTarget[]>(
    () =>
      (tables ?? [])
        .filter((row) => Boolean(row.name))
        .map((row) => ({
          kind: "table" as const,
          key: `table:${row._id}`,
          label: row.name ?? row._id,
          detail: tenant ?? undefined,
          href: `/developer/storage/${encodeURIComponent(row.name ?? "")}`,
        })),
    [tables, tenant],
  );

  const functionTargets = useMemo<PaletteTarget[]>(
    () =>
      (functions ?? [])
        .filter((row) => Boolean(row.path))
        .map((row) => ({
          kind: "function" as const,
          key: `function:${row._id}`,
          label: row.path ?? row._id,
          detail: row.kind,
          href: `/developer/compute/${encodeURIComponent(row.path ?? "")}`,
        })),
    [functions],
  );

  const serviceTargets = useMemo<PaletteTarget[]>(
    () =>
      (services ?? []).map((row) => ({
        kind: "service" as const,
        key: `service:${row._id}`,
        label: row.name ?? row._id,
        detail: row.state,
        href: `/developer/services/${encodeURIComponent(row._id)}`,
      })),
    [services],
  );

  // machine-detail.tsx has no createFileRoute, so a machine has no detail URL
  // to target; these items land on the machines list.
  const machineTargets = useMemo<PaletteTarget[]>(
    () =>
      (machines ?? []).map((row) => ({
        kind: "machine" as const,
        key: `machine:${row._id}`,
        label: row.name ?? row._id,
        detail: row.state,
        href: "/operator/machines",
      })),
    [machines],
  );

  const routeTargets = useMemo<PaletteTarget[]>(
    () =>
      (routes ?? [])
        .filter((row) => Boolean(row.path))
        .map((row) => ({
          kind: "route" as const,
          key: `route:${row._id}`,
          label: `${row.method ?? "ANY"} ${row.path ?? ""}`,
          detail: row.adapter,
          href: "/operator/network",
        })),
    [routes],
  );

  const tenantTargets = useMemo<PaletteTarget[]>(
    () =>
      tenantList.kind === "loaded"
        ? tenantList.tenants.map((entry) => ({
            kind: "tenant" as const,
            key: `tenant:${entry.id}`,
            label: entry.id,
            detail: entry.backend,
            href: "/operator/tenants",
          }))
        : [],
    [tenantList],
  );

  return (
    <>
      <ResourceGroup heading="Tables" targets={tableTargets} onPick={onPick} />
      <ResourceGroup
        heading="Functions"
        targets={functionTargets}
        onPick={onPick}
      />
      <ResourceGroup
        heading="Tenants"
        targets={tenantTargets}
        onPick={onPick}
        error={tenantList.kind === "error" ? tenantList.message : undefined}
      />
      <ResourceGroup
        heading="Services"
        targets={serviceTargets}
        onPick={onPick}
      />
      <ResourceGroup
        heading="Machines"
        targets={machineTargets}
        onPick={onPick}
      />
      <ResourceGroup heading="Routes" targets={routeTargets} onPick={onPick} />
    </>
  );
}

function ResourceGroup({
  heading,
  targets,
  onPick,
  error,
}: {
  heading: string;
  targets: PaletteTarget[];
  onPick: (t: PaletteTarget) => void;
  error?: string;
}) {
  if (error) {
    // forceMount on both parts: this row is a status message, not a match
    // candidate. Left to the filter it would score 0 against a tenant name and
    // vanish, so the one query most likely to need the warning is the one that
    // would never see it.
    return (
      <Command.Group heading={heading} forceMount>
        <Command.Item
          disabled
          forceMount
          value={`${heading} unavailable`}
          className="flex h-9 cursor-default items-center gap-2 rounded px-2 text-sm text-muted"
          data-testid={`palette-group-${heading.toLowerCase()}-error`}
        >
          {heading} unavailable — {error}
        </Command.Item>
      </Command.Group>
    );
  }
  if (targets.length === 0) return null;
  return (
    <Command.Group heading={heading}>
      {targets.map((target) => (
        <PaletteItem
          key={target.key}
          target={target}
          hint="⏎ open"
          onSelect={() => onPick(target)}
        />
      ))}
    </Command.Group>
  );
}

function ModeToggle({
  current,
  onChange,
}: {
  current: Mode;
  onChange: (m: Mode) => void;
}) {
  const modes: Array<{ id: Mode; label: string }> = [
    { id: "navigate", label: "Navigate" },
    { id: "run", label: "Run" },
    { id: "filter", label: "Filter" },
  ];
  return (
    <fieldset className="inline-flex overflow-hidden rounded-md border text-xs border-app">
      <legend className="sr-only">Palette mode</legend>
      {modes.map((m) => (
        <button
          key={m.id}
          type="button"
          aria-pressed={current === m.id}
          onClick={() => onChange(m.id)}
          className={cn(
            "px-2 py-1 font-mono text-xs uppercase tracking-wide transition-colors",
            current === m.id
              ? "bg-surface-2 text-default"
              : "text-muted hover:bg-surface-2",
          )}
          data-testid={`palette-mode-${m.id}`}
        >
          {m.label}
        </button>
      ))}
    </fieldset>
  );
}

// The selected row is what Enter runs, so it carries an accent wash plus a 2px
// accent bar — not the ~5% surface step the list used to reach for, which was
// indistinguishable from the empty background.
const ITEM_CLASS =
  "flex h-9 cursor-default items-center gap-2 rounded px-2 text-sm text-muted " +
  "data-[selected=true]:bg-accent/15 " +
  "data-[selected=true]:shadow-[inset_2px_0_0_var(--nimbus-accent)]";

function PaletteItem({
  target,
  icon,
  hint,
  onSelect,
}: {
  target: PaletteTarget;
  icon?: LucideIcon;
  hint?: string;
  onSelect: () => void;
}) {
  const Icon = icon ?? TARGET_ICONS[target.kind];
  const shown = target.detail;
  return (
    <Command.Item
      // The key is part of the match value so two resources that share a name
      // stay distinct rows (cmdk keys selection by value), and so an operator
      // can match on the id as well as the name.
      value={[target.label, target.detail, target.key]
        .filter(Boolean)
        .join(" ")}
      onSelect={onSelect}
      className={ITEM_CLASS}
      data-testid={`palette-item-${target.key}`}
    >
      <Icon size={14} className="shrink-0" />
      <span className="flex-1 truncate text-default">{target.label}</span>
      {shown ? (
        <span className="shrink-0 font-mono text-xs uppercase tracking-wide text-muted">
          {shown}
        </span>
      ) : null}
      {hint ? (
        <span className="shrink-0 text-xs font-mono text-muted">{hint}</span>
      ) : null}
    </Command.Item>
  );
}

// Run/filter rows: an action, not a jump target, so it has no palette target
// record and never lands in recents.
function ActionItem({
  icon: Icon,
  label,
  hint,
  onSelect,
}: {
  icon: LucideIcon;
  label: string;
  hint?: string;
  onSelect: () => void;
}) {
  return (
    <Command.Item
      onSelect={onSelect}
      className={ITEM_CLASS}
      data-testid={`palette-action-${label}`}
    >
      <Icon size={14} className="shrink-0" />
      <span className="flex-1 text-default">{label}</span>
      {hint ? (
        <span className="text-xs font-mono text-muted">{hint}</span>
      ) : null}
    </Command.Item>
  );
}

function isPaletteTarget(value: unknown): value is PaletteTarget {
  if (!value || typeof value !== "object") return false;
  const candidate = value as Record<string, unknown>;
  return (
    typeof candidate.key === "string" &&
    typeof candidate.label === "string" &&
    typeof candidate.href === "string" &&
    typeof candidate.kind === "string" &&
    candidate.kind in TARGET_ICONS
  );
}

function loadRecent(): PaletteTarget[] {
  if (typeof window === "undefined") return [];
  try {
    const raw = window.localStorage.getItem(RECENT_KEY);
    if (!raw) return [];
    const parsed = JSON.parse(raw);
    return Array.isArray(parsed) ? parsed.filter(isPaletteTarget) : [];
  } catch {
    return [];
  }
}

function persistRecent(list: PaletteTarget[]) {
  if (typeof window === "undefined") return;
  try {
    window.localStorage.setItem(RECENT_KEY, JSON.stringify(list));
  } catch {
    /* ignore */
  }
}
