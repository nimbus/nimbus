import { create } from "zustand";

export type ThemeMode = "light" | "dark" | "system";
export type Theme = "light" | "dark";

type UiState = {
  paletteOpen: boolean;
  lensOpen: boolean;
  actionMenuOpen: boolean;
  themeMode: ThemeMode;
  theme: Theme;
  paletteOpener: HTMLElement | null;
  lensOpener: HTMLElement | null;
  setPaletteOpen: (open: boolean, opener?: HTMLElement | null) => void;
  setLensOpen: (open: boolean, opener?: HTMLElement | null) => void;
  setActionMenuOpen: (open: boolean) => void;
  setThemeMode: (mode: ThemeMode) => void;
  cycleThemeMode: () => void;
};

const THEME_STORAGE_KEY = "nimbus-ui:theme";
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

function resolveTheme(mode: ThemeMode): Theme {
  return mode === "system" ? readSystemTheme() : mode;
}

const initialMode = readStoredMode();

export const useUiStore = create<UiState>((set, get) => ({
  paletteOpen: false,
  lensOpen: false,
  actionMenuOpen: false,
  themeMode: initialMode,
  theme: resolveTheme(initialMode),
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

if (typeof window !== "undefined" && window.matchMedia) {
  const mql = window.matchMedia(SYSTEM_DARK_QUERY);
  const listener = () => {
    if (useUiStore.getState().themeMode === "system") {
      useUiStore.setState({ theme: readSystemTheme() });
    }
  };
  mql.addEventListener?.("change", listener);
}
