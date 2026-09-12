import { Dialog } from "@base-ui/react/dialog";
import { Link, useRouterState } from "@tanstack/react-router";
import { ChevronsLeft, ChevronsRight, type LucideIcon } from "lucide-react";
import {
  createContext,
  type ReactNode,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import {
  Group,
  type Layout,
  Panel,
  Separator,
  usePanelRef,
} from "react-resizable-panels";
import { cn } from "@/lib/utils";
import { RailTooltip } from "./sidebar/rail";
import { useViewportTier, type ViewportTier } from "./use-viewport-tier";

// The sub-panel is the second navigation column: a list of the things inside
// the current section (tables, functions, tenants, services) beside the page
// that shows one of them. Routes contribute a spec through
// `useContributeSubPanel`; the shell decides how to show it by viewport tier.
//
// Desktop: a resizable in-flow column on react-resizable-panels, between
// 180px and 400px, collapsible to a 32px rail. The width and the collapsed
// state persist per section, so Storage can sit at 320px for long table names
// while Tenants stays at the 240px default.
//
// Below desktop: the rail stays in flow and the expanded panel is a modal
// sheet anchored to it, so the content area keeps its width.

export const SUB_PANEL_MIN_WIDTH = 180;
export const SUB_PANEL_DEFAULT_WIDTH = 240;
export const SUB_PANEL_MAX_WIDTH = 400;
export const SUB_PANEL_RAIL_WIDTH = 32;
// A list this short scans faster than it filters; the search field only
// appears once the list is long enough that typing beats scrolling.
export const SUB_PANEL_SEARCH_THRESHOLD = 20;
// The library stamps each panel's id on it as `data-testid`, so these must
// not collide with the shell's own `sub-panel` testids.
const PANEL_ID = "sub-panel-column";
const MAIN_ID = "main-column";
const SEPARATOR_ID = "sub-panel-separator";

export type SubPanelItem<TId extends string = string> = {
  readonly id: TId;
  readonly label: string;
  readonly to: string;
  readonly search?: Record<string, unknown>;
  readonly description?: string;
  readonly count?: number | null;
};

// Items shown in the collapsed rail: each sub-view as an icon with the active
// one indicated. Clicking one switches sub-view without expanding the panel.
export type SubPanelRailItem = {
  readonly id: string;
  readonly label: string;
  readonly icon: LucideIcon;
  readonly active: boolean;
  readonly onSelect: () => void;
};

export type StaticSubPanelSpec<TId extends string = string> = {
  readonly kind: "static";
  readonly title: string;
  readonly items: ReadonlyArray<SubPanelItem<TId>>;
  readonly railItems?: ReadonlyArray<SubPanelRailItem>;
};

export type DynamicSubPanelSpec = {
  readonly kind: "dynamic";
  readonly title: string;
  /**
   * The list's filter field. `rows` is the unfiltered row count; the field
   * renders only when it exceeds `SUB_PANEL_SEARCH_THRESHOLD`.
   */
  readonly search?: { placeholder: string; rows: number };
  readonly children: ReactNode;
  readonly railItems?: ReadonlyArray<SubPanelRailItem>;
};

export type SubPanelSpec<TId extends string = string> =
  | StaticSubPanelSpec<TId>
  | DynamicSubPanelSpec;

type SubPanelContextValue = {
  spec: SubPanelSpec | null;
  setSpec: (spec: SubPanelSpec | null) => void;
  search: string;
  setSearch: (next: string) => void;
};

const SubPanelContext = createContext<SubPanelContextValue | null>(null);

export function SubPanelProvider({ children }: { children: ReactNode }) {
  const [spec, setSpec] = useState<SubPanelSpec | null>(null);
  const [search, setSearch] = useState<string>("");
  const value = useMemo(
    () => ({ spec, setSpec, search, setSearch }),
    [spec, search],
  );
  return (
    <SubPanelContext.Provider value={value}>
      {children}
    </SubPanelContext.Provider>
  );
}

export function useSubPanelSearch(): string {
  const ctx = useContext(SubPanelContext);
  if (!ctx) {
    throw new Error("useSubPanelSearch must be used within SubPanelProvider");
  }
  return ctx.search;
}

export function useContributeSubPanel(spec: SubPanelSpec | null) {
  const ctx = useContext(SubPanelContext);
  if (!ctx) {
    throw new Error(
      "useContributeSubPanel must be used within a SubPanelProvider",
    );
  }
  const { setSpec, setSearch } = ctx;
  useEffect(() => {
    setSpec(spec);
    setSearch("");
    return () => {
      setSpec(null);
      setSearch("");
    };
  }, [spec, setSpec, setSearch]);
}

export function showsSubPanelSearch(spec: SubPanelSpec): boolean {
  return (
    spec.kind === "dynamic" &&
    spec.search !== undefined &&
    spec.search.rows > SUB_PANEL_SEARCH_THRESHOLD
  );
}

// ---------------------------------------------------------------------------
// Per-section preferences
//
// One key per section, so the width an operator settles on for long table
// names does not follow them into Tenants. The section is the second path
// segment: `/developer/storage/users` and `/developer/storage` share
// `storage`; the developer and operator Settings pages share `settings`.

export type SubPanelPrefs = {
  readonly width: number;
  readonly collapsed: boolean;
};

export const SUB_PANEL_STORAGE_PREFIX = "nimbus-ui:panel:";

export function subPanelSection(pathname: string): string {
  const segments = pathname.split("/").filter((s) => s.length > 0);
  return segments[1] ?? segments[0] ?? "root";
}

export function subPanelStorageKey(section: string): string {
  return `${SUB_PANEL_STORAGE_PREFIX}${section}`;
}

function clampWidth(width: number): number {
  return Math.min(SUB_PANEL_MAX_WIDTH, Math.max(SUB_PANEL_MIN_WIDTH, width));
}

const DEFAULT_PREFS: SubPanelPrefs = {
  width: SUB_PANEL_DEFAULT_WIDTH,
  collapsed: false,
};

export function readSubPanelPrefs(section: string): SubPanelPrefs {
  if (typeof window === "undefined") return DEFAULT_PREFS;
  const raw = window.localStorage.getItem(subPanelStorageKey(section));
  if (!raw) return DEFAULT_PREFS;
  try {
    const parsed = JSON.parse(raw) as Partial<SubPanelPrefs> | null;
    const width =
      typeof parsed?.width === "number" && Number.isFinite(parsed.width)
        ? clampWidth(Math.round(parsed.width))
        : SUB_PANEL_DEFAULT_WIDTH;
    return { width, collapsed: parsed?.collapsed === true };
  } catch {
    return DEFAULT_PREFS;
  }
}

export function persistSubPanelPrefs(section: string, prefs: SubPanelPrefs) {
  if (typeof window === "undefined") return;
  window.localStorage.setItem(
    subPanelStorageKey(section),
    JSON.stringify(prefs),
  );
}

// ---------------------------------------------------------------------------
// Layout host

/**
 * Wraps the page column. Without a contributed spec it is a plain pass-through;
 * with one it puts the sub-panel beside the page. The page always sits in the
 * same place in the tree (inside the main panel of one Group keyed by section),
 * so a spec arriving after the page mounts, or the viewport crossing a tier,
 * does not remount the page.
 */
export function SubPanelLayout({ children }: { children: ReactNode }) {
  const ctx = useContext(SubPanelContext);
  const spec = ctx?.spec ?? null;
  const search = ctx?.search ?? "";
  const setSearch = ctx?.setSearch ?? noop;
  const tier = useViewportTier();
  const section = useRouterState({
    select: (s) => subPanelSection(s.location.pathname),
  });
  const sheet = tier !== "desktop";
  return (
    <>
      {sheet && spec ? (
        <SheetSubPanel
          spec={spec}
          search={search}
          setSearch={setSearch}
          tier={tier}
        />
      ) : null}
      <ResizableGroup
        key={section}
        section={section}
        spec={sheet ? null : spec}
        search={search}
        setSearch={setSearch}
      >
        {children}
      </ResizableGroup>
    </>
  );
}

function noop() {}

function ResizableGroup({
  section,
  spec,
  search,
  setSearch,
  children,
}: {
  section: string;
  spec: SubPanelSpec | null;
  search: string;
  setSearch: (next: string) => void;
  children: ReactNode;
}) {
  const [prefs, setPrefs] = useState<SubPanelPrefs>(() =>
    readSubPanelPrefs(section),
  );
  const prefsRef = useRef(prefs);
  prefsRef.current = prefs;
  const panelRef = usePanelRef();
  const groupRef = useRef<HTMLDivElement | null>(null);

  // `onLayoutChange` fires while a drag is in flight, so the rail swaps in
  // the moment the panel snaps closed instead of on pointer release. Both
  // callbacks ignore a layout without the sub-panel: the group registers
  // with the main column alone before a spec arrives, and reading the
  // collapsed state then would report "expanded" and overwrite the stored
  // preference before the panel has mounted.
  const onLayoutChange = useCallback(
    (layout: Layout) => {
      const panel = panelRef.current;
      if (!panel || layout[PANEL_ID] === undefined) return;
      const collapsed = panel.isCollapsed();
      setPrefs((prev) =>
        prev.collapsed === collapsed ? prev : { ...prev, collapsed },
      );
    },
    [panelRef],
  );

  // `onLayoutChanged` runs before React commits the new widths, so the pixel
  // size comes from the layout percentage and the group's width, not from the
  // panel element. A collapsed panel keeps the width it had, so expanding it
  // restores the operator's size rather than the minimum.
  const onLayoutChanged = useCallback(
    (layout: Layout) => {
      const group = groupRef.current;
      const panel = panelRef.current;
      const percent = layout[PANEL_ID];
      if (!group || !panel || percent === undefined) return;
      const collapsed = panel.isCollapsed();
      const width = collapsed
        ? prefsRef.current.width
        : clampWidth(Math.round((percent / 100) * group.offsetWidth));
      const next = { width, collapsed };
      persistSubPanelPrefs(section, next);
      setPrefs(next);
    },
    [section, panelRef],
  );

  const collapse = () => panelRef.current?.collapse();
  // Not `expand()`: after a collapsed mount the library has no expanded size
  // to return to and would open at the minimum. The stored width is the one
  // the operator chose.
  const expand = () => panelRef.current?.resize(prefsRef.current.width);

  return (
    <Group
      orientation="horizontal"
      elementRef={groupRef}
      onLayoutChange={onLayoutChange}
      onLayoutChanged={onLayoutChanged}
      className="min-h-0"
      style={{ width: "auto", flex: "1 1 0%", minWidth: 0 }}
    >
      {spec ? (
        <Panel
          id={PANEL_ID}
          panelRef={panelRef}
          defaultSize={prefs.collapsed ? SUB_PANEL_RAIL_WIDTH : prefs.width}
          minSize={SUB_PANEL_MIN_WIDTH}
          maxSize={SUB_PANEL_MAX_WIDTH}
          collapsible
          collapsedSize={SUB_PANEL_RAIL_WIDTH}
          groupResizeBehavior="preserve-pixel-size"
          style={PANEL_STYLE}
        >
          {prefs.collapsed ? (
            <SubPanelRail
              spec={spec}
              onExpand={expand}
              className="w-full"
              testid="sub-panel"
            />
          ) : (
            <aside
              aria-label={spec.title}
              data-testid="sub-panel"
              data-kind={spec.kind}
              data-collapsed="false"
              className="flex h-full min-h-0 w-full flex-col bg-bg-panel"
            >
              <SubPanelBody
                spec={spec}
                search={search}
                setSearch={setSearch}
                onCollapse={collapse}
              />
            </aside>
          )}
        </Panel>
      ) : null}
      {spec ? (
        <Separator
          id={SEPARATOR_ID}
          aria-label="Resize sub-panel"
          // The visible line is the panel's border; the pointer target around
          // it is the library's `resizeTargetMinimumSize`. Focus paints the
          // line with the accent, and the global :focus-visible ring is
          // switched off here because a ring around a 1px line reads as a
          // stray mark.
          className="w-px shrink-0 bg-border-2 outline-none transition-colors duration-150 ease-standard data-[separator=active]:bg-accent-edge data-[separator=focus]:bg-accent-edge data-[separator=hover]:bg-accent-edge"
        />
      ) : null}
      <Panel id={MAIN_ID} style={PANEL_STYLE}>
        {children}
      </Panel>
    </Group>
  );
}

// The library's inner panel div scrolls by default. Both panels here are
// columns whose children own their scrolling, so the div is a flex column
// that clips.
const PANEL_STYLE = {
  display: "flex",
  flexDirection: "column",
  overflow: "hidden",
} as const;

// ---------------------------------------------------------------------------
// Sheet mode (tablet and mobile)

// The sheet is modal, so entering the tier must not open it: a remembered
// desktop width would otherwise drop a scrim over the content on arrival.
// Below desktop the panel starts closed and only an explicit expand opens it.
// That choice is tagged with the tier it was made in, so returning to desktop
// shows the stored preference untouched.
function SheetSubPanel({
  spec,
  search,
  setSearch,
  tier,
}: {
  spec: SubPanelSpec;
  search: string;
  setSearch: (next: string) => void;
  tier: ViewportTier;
}) {
  const [override, setOverride] = useState<{
    tier: ViewportTier;
    value: boolean;
  } | null>(null);
  const open = override?.tier === tier ? override.value : false;
  const setOpen = (value: boolean) => setOverride({ tier, value });
  const railRef = useRef<HTMLElement | null>(null);
  // The sheet hangs off the rail's right edge and top, wherever the rail is:
  // beside the sidebar column at tablet width, under the top bar on a phone.
  const [anchor, setAnchor] = useState<{ top: number; left: number }>({
    top: 0,
    left: 0,
  });
  const expand = () => {
    const rect = railRef.current?.getBoundingClientRect();
    if (rect) setAnchor({ top: rect.top, left: rect.right });
    setOpen(true);
  };
  return (
    <>
      <SubPanelRail
        spec={spec}
        onExpand={expand}
        elementRef={railRef}
        className="w-8 shrink-0 border-r border-border-2"
        testid="sub-panel"
      />
      {open ? (
        <Dialog.Root open onOpenChange={setOpen} modal>
          <Dialog.Portal>
            <Dialog.Backdrop
              data-testid="sub-panel-scrim"
              className="fixed inset-0 z-40 bg-black/50"
            />
            <Dialog.Popup
              aria-label={spec.title}
              data-testid="sub-panel-overlay"
              data-kind={spec.kind}
              style={{ top: anchor.top, left: anchor.left }}
              className="fixed bottom-0 z-50 flex w-64 flex-col border-r border-border-2 bg-bg-panel shadow-lg outline-none"
            >
              <SubPanelBody
                spec={spec}
                search={search}
                setSearch={setSearch}
                onCollapse={() => setOpen(false)}
              />
            </Dialog.Popup>
          </Dialog.Portal>
        </Dialog.Root>
      ) : null}
    </>
  );
}

// ---------------------------------------------------------------------------
// Pieces shared by both modes

// Collapsed: a thin rail with an expand toggle (mirrors the sidebar rail).
// The panel is never fully removed, so it is always reachable again.
function SubPanelRail({
  spec,
  onExpand,
  elementRef,
  className,
  testid,
}: {
  spec: SubPanelSpec;
  onExpand: () => void;
  elementRef?: React.Ref<HTMLElement>;
  className?: string;
  testid: string;
}) {
  return (
    <aside
      ref={elementRef}
      aria-label={spec.title}
      data-testid={testid}
      data-kind={spec.kind}
      data-collapsed="true"
      className={cn("flex h-full flex-col gap-1 bg-bg-panel py-2", className)}
    >
      <RailTooltip label="Expand sub-panel">
        <button
          type="button"
          onClick={onExpand}
          aria-label="Expand sub-panel"
          data-testid="sub-panel-toggle"
          // 32px square: DESIGN.md §Spacing And Shape, "Icon button: 32px
          // square, 36px on touch surfaces." This is the one control that
          // must not be fiddly; it is the only way back to the expanded panel.
          className="flex h-8 w-full items-center justify-center rounded-sm text-text-3 transition-colors hover:bg-bg-hover hover:text-text-1"
        >
          <ChevronsRight size={14} aria-hidden />
        </button>
      </RailTooltip>
      {spec.railItems?.map((item) => {
        const Icon = item.icon;
        return (
          <RailTooltip key={item.id} label={item.label}>
            <button
              type="button"
              onClick={item.onSelect}
              aria-label={item.label}
              aria-current={item.active ? "page" : undefined}
              data-testid={`sub-panel-rail-item-${item.id}`}
              data-active={item.active ? "true" : "false"}
              className={cn(
                "flex h-8 w-full items-center justify-center border-l-2 border-transparent text-text-3 transition-colors hover:bg-bg-hover hover:text-text-1",
                item.active && "bg-bg-hover text-text-1",
              )}
              style={
                item.active ? { borderLeftColor: "var(--accent)" } : undefined
              }
            >
              <Icon size={14} aria-hidden />
            </button>
          </RailTooltip>
        );
      })}
    </aside>
  );
}

function SubPanelBody({
  spec,
  search,
  setSearch,
  onCollapse,
}: {
  spec: SubPanelSpec;
  search: string;
  setSearch: (next: string) => void;
  onCollapse: () => void;
}) {
  return (
    <>
      <header className="flex h-10 shrink-0 items-center justify-between gap-2 border-b border-border-2 px-3">
        <span className="truncate text-xs font-medium text-text-3">
          {spec.title}
        </span>
        <button
          type="button"
          onClick={onCollapse}
          aria-label="Collapse sub-panel"
          title="Collapse sub-panel"
          data-testid="sub-panel-toggle"
          // Same 32px square as the rail toggle it swaps places with.
          className="flex h-8 w-8 shrink-0 items-center justify-center rounded-sm text-text-3 transition-colors hover:bg-bg-hover hover:text-text-1"
        >
          <ChevronsLeft size={14} aria-hidden />
        </button>
      </header>
      {spec.kind === "dynamic" && spec.search && showsSubPanelSearch(spec) ? (
        <div className="shrink-0 border-b border-border-2 px-3 py-2">
          <input
            type="search"
            value={search}
            onChange={(e) => setSearch(e.target.value)}
            placeholder={spec.search.placeholder}
            data-testid="sub-panel-search"
            // `panel` marks this as the fallback target for the `/` shortcut;
            // a page-level filter claims `primary` and wins. No focus styling
            // of its own: the global :focus-visible rule paints the accent
            // ring.
            data-inline-search="panel"
            className="h-7 w-full rounded-sm border border-border-2 bg-bg-canvas px-2 text-xs text-text-1 placeholder:text-text-3"
          />
        </div>
      ) : null}
      <div className="min-h-0 flex-1 overflow-auto">
        {spec.kind === "static" ? (
          <SubPanelStaticList items={spec.items} />
        ) : (
          spec.children
        )}
      </div>
    </>
  );
}

function isItemActive(
  location: { pathname: string; search?: Record<string, unknown> },
  item: SubPanelItem<string>,
): boolean {
  const pathMatches =
    location.pathname === item.to ||
    location.pathname.startsWith(`${item.to}/`);
  if (!pathMatches) return false;
  if (!item.search) return true;
  const current = location.search ?? {};
  for (const [key, value] of Object.entries(item.search)) {
    if (current[key] !== value) return false;
  }
  return true;
}

function SubPanelStaticList({
  items,
}: {
  items: ReadonlyArray<SubPanelItem<string>>;
}) {
  const location = useRouterState({
    select: (s) => ({
      pathname: s.location.pathname,
      search: s.location.search as Record<string, unknown> | undefined,
    }),
  });
  return (
    <ul className="flex flex-col gap-px px-2 py-2">
      {items.map((item) => {
        const active = isItemActive(location, item);
        const row = cn(
          "flex h-8 items-center gap-2 rounded-sm border-l-2 border-transparent px-2 text-sm",
          active
            ? "bg-bg-hover text-text-1"
            : "text-text-3 hover:bg-bg-hover hover:text-text-1",
        );
        const label = <span className="flex-1 truncate">{item.label}</span>;
        return (
          <li key={item.id}>
            <Link
              to={item.to}
              search={item.search ?? undefined}
              aria-current={active ? "page" : undefined}
              data-testid={`sub-panel-item-${item.id}`}
              data-active={active ? "true" : "false"}
              className={row}
              style={active ? { borderLeftColor: "var(--accent)" } : undefined}
            >
              {label}
              {typeof item.count === "number" ? (
                <span className="tabular font-mono text-xs text-text-3">
                  {item.count}
                </span>
              ) : null}
            </Link>
          </li>
        );
      })}
    </ul>
  );
}
