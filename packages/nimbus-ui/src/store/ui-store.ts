import { create } from "zustand";

export type ThemeMode = "light" | "dark" | "system";
export type Theme = "light" | "dark";
export type Palette = "blue" | "mono" | "warm";
export type NavView = "developer" | "operator";

export const PALETTES: ReadonlyArray<{
  id: Palette;
  label: string;
  description: string;
}> = [
  {
    id: "blue",
    label: "Blue",
    description: "Cool Blue · Night Blue — product default",
  },
  {
    id: "mono",
    label: "Mono",
    description: "Monochrome · Reverse Mono — minimal, enterprise",
  },
  {
    id: "warm",
    label: "Warm",
    description: "Warm · Golden Hour — friendly, marketing",
  },
];

type UiState = {
  paletteOpen: boolean;
  lensOpen: boolean;
  actionMenuOpen: boolean;
  themeMode: ThemeMode;
  theme: Theme;
  palette: Palette;
  lastView: NavView;
  primaryDrawerCollapsed: boolean;
  subDrawerOpen: boolean;
  activeTenant: string | null;
  paletteOpener: HTMLElement | null;
  lensOpener: HTMLElement | null;
  setPaletteOpen: (open: boolean, opener?: HTMLElement | null) => void;
  setLensOpen: (open: boolean, opener?: HTMLElement | null) => void;
  setActionMenuOpen: (open: boolean) => void;
  setThemeMode: (mode: ThemeMode) => void;
  setPalette: (palette: Palette) => void;
  setLastView: (view: NavView) => void;
  setPrimaryDrawerCollapsed: (collapsed: boolean) => void;
  togglePrimaryDrawer: () => void;
  setSubDrawerOpen: (open: boolean) => void;
  toggleSubDrawer: () => void;
  setActiveTenant: (tenant: string | null) => void;
  cycleThemeMode: () => void;
};

const THEME_STORAGE_KEY = "nimbus-ui:theme";
const PALETTE_STORAGE_KEY = "nimbus-ui:palette";
const LAST_VIEW_STORAGE_KEY = "nimbus-ui:last-view";
const LAST_ROUTE_STORAGE_PREFIX = "nimbus-ui:last-route:";
const PRIMARY_DRAWER_COLLAPSED_KEY = "nimbus-ui:primary-drawer-collapsed";
const SUB_DRAWER_OPEN_KEY = "nimbus-ui:sub-drawer-open";
const ACTIVE_TENANT_KEY = "nimbus-ui:active-tenant";
const SYSTEM_DARK_QUERY = "(prefers-color-scheme: dark)";

function readSystemTheme(): Theme {
  if (typeof window === "undefined") return "dark";
  return window.matchMedia?.(SYSTEM_DARK_QUERY).matches ? "dark" : "light";
}

function readStoredMode(): ThemeMode {
  if (typeof window === "undefined") return "system";
  const stored = window.localStorage.getItem(THEME_STORAGE_KEY);
  if (stored === "light" || stored === "dark" || stored === "system") {
    return stored;
  }
  return "system";
}

function readStoredPalette(): Palette {
  if (typeof window === "undefined") return "blue";
  const stored = window.localStorage.getItem(PALETTE_STORAGE_KEY);
  if (stored === "blue" || stored === "mono" || stored === "warm") {
    return stored;
  }
  return "blue";
}

export function readLastView(): NavView {
  if (typeof window === "undefined") return "developer";
  const stored = window.localStorage.getItem(LAST_VIEW_STORAGE_KEY);
  return stored === "operator" ? "operator" : "developer";
}

export function readLastRouteForView(view: NavView): string | null {
  if (typeof window === "undefined") return null;
  const stored = window.localStorage.getItem(
    `${LAST_ROUTE_STORAGE_PREFIX}${view}`,
  );
  return stored?.startsWith(view === "operator" ? "/admin" : "/app")
    ? stored
    : null;
}

export function persistLastRouteForView(view: NavView, pathname: string) {
  if (typeof window === "undefined") return;
  const prefix = view === "operator" ? "/admin" : "/app";
  if (!pathname.startsWith(prefix)) return;
  window.localStorage.setItem(`${LAST_ROUTE_STORAGE_PREFIX}${view}`, pathname);
}

function resolveTheme(mode: ThemeMode): Theme {
  return mode === "system" ? readSystemTheme() : mode;
}

export function readPrimaryDrawerCollapsed(): boolean {
  if (typeof window === "undefined") return false;
  return window.localStorage.getItem(PRIMARY_DRAWER_COLLAPSED_KEY) === "true";
}

export function readSubDrawerOpen(): boolean {
  if (typeof window === "undefined") return true;
  const stored = window.localStorage.getItem(SUB_DRAWER_OPEN_KEY);
  return stored === null ? true : stored === "true";
}

export function readActiveTenant(): string | null {
  if (typeof window === "undefined") return null;
  const stored = window.localStorage.getItem(ACTIVE_TENANT_KEY);
  return stored && stored.length > 0 ? stored : null;
}

export function persistActiveTenant(tenant: string | null) {
  if (typeof window === "undefined") return;
  if (tenant === null) {
    window.localStorage.removeItem(ACTIVE_TENANT_KEY);
  } else {
    window.localStorage.setItem(ACTIVE_TENANT_KEY, tenant);
  }
}

const initialMode = readStoredMode();
const initialPalette = readStoredPalette();
const initialLastView = readLastView();
const initialPrimaryDrawerCollapsed = readPrimaryDrawerCollapsed();
const initialSubDrawerOpen = readSubDrawerOpen();
const initialActiveTenant = readActiveTenant();

export const useUiStore = create<UiState>((set, get) => ({
  paletteOpen: false,
  lensOpen: false,
  actionMenuOpen: false,
  themeMode: initialMode,
  theme: resolveTheme(initialMode),
  palette: initialPalette,
  lastView: initialLastView,
  primaryDrawerCollapsed: initialPrimaryDrawerCollapsed,
  subDrawerOpen: initialSubDrawerOpen,
  activeTenant: initialActiveTenant,
  paletteOpener: null,
  lensOpener: null,
  setPaletteOpen: (open, opener) =>
    set((state) => {
      if (open) {
        return {
          paletteOpen: true,
          paletteOpener:
            opener ?? (document.activeElement as HTMLElement | null) ?? null,
        };
      }
      const restore = state.paletteOpener;
      queueMicrotask(() => restore?.focus?.());
      return { paletteOpen: false, paletteOpener: null };
    }),
  setLensOpen: (open, opener) =>
    set((state) => {
      if (open) {
        return {
          lensOpen: true,
          lensOpener:
            opener ?? (document.activeElement as HTMLElement | null) ?? null,
        };
      }
      const restore = state.lensOpener;
      queueMicrotask(() => restore?.focus?.());
      return { lensOpen: false, lensOpener: null };
    }),
  setActionMenuOpen: (open) => set({ actionMenuOpen: open }),
  setThemeMode: (mode) => {
    persistMode(mode);
    set({ themeMode: mode, theme: resolveTheme(mode) });
  },
  setPalette: (palette) => {
    persistPalette(palette);
    set({ palette });
  },
  setLastView: (view) => {
    persistLastView(view);
    set({ lastView: view });
  },
  setPrimaryDrawerCollapsed: (collapsed) => {
    persistPrimaryDrawerCollapsed(collapsed);
    set({ primaryDrawerCollapsed: collapsed });
  },
  togglePrimaryDrawer: () => {
    const next = !get().primaryDrawerCollapsed;
    persistPrimaryDrawerCollapsed(next);
    set({ primaryDrawerCollapsed: next });
  },
  setSubDrawerOpen: (open) => {
    persistSubDrawerOpen(open);
    set({ subDrawerOpen: open });
  },
  toggleSubDrawer: () => {
    const next = !get().subDrawerOpen;
    persistSubDrawerOpen(next);
    set({ subDrawerOpen: next });
  },
  setActiveTenant: (tenant) => {
    persistActiveTenant(tenant);
    set({ activeTenant: tenant });
  },
  cycleThemeMode: () => {
    const order: ThemeMode[] = ["light", "dark", "system"];
    const current = get().themeMode;
    const next = order[(order.indexOf(current) + 1) % order.length];
    persistMode(next);
    set({ themeMode: next, theme: resolveTheme(next) });
  },
}));

function persistMode(mode: ThemeMode) {
  if (typeof window === "undefined") return;
  window.localStorage.setItem(THEME_STORAGE_KEY, mode);
}

function persistPalette(palette: Palette) {
  if (typeof window === "undefined") return;
  window.localStorage.setItem(PALETTE_STORAGE_KEY, palette);
}

function persistLastView(view: NavView) {
  if (typeof window === "undefined") return;
  window.localStorage.setItem(LAST_VIEW_STORAGE_KEY, view);
}

function persistPrimaryDrawerCollapsed(collapsed: boolean) {
  if (typeof window === "undefined") return;
  window.localStorage.setItem(
    PRIMARY_DRAWER_COLLAPSED_KEY,
    collapsed ? "true" : "false",
  );
}

export function persistSubDrawerOpen(open: boolean) {
  if (typeof window === "undefined") return;
  window.localStorage.setItem(SUB_DRAWER_OPEN_KEY, open ? "true" : "false");
}

if (typeof window !== "undefined" && window.matchMedia) {
  const mql = window.matchMedia(SYSTEM_DARK_QUERY);
  const listener = () => {
    if (useUiStore.getState().themeMode === "system") {
      useUiStore.setState({ theme: readSystemTheme() });
    }
  };
  mql.addEventListener?.("change", listener);
}
