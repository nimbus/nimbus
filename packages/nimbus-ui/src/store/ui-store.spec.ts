import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const THEME_STORAGE_KEY = "nimbus-ui:theme";

beforeEach(() => {
  vi.resetModules();
  window.localStorage.clear();
});

afterEach(() => {
  window.localStorage.clear();
});

describe("ui-store", () => {
  it("defaults to system mode when nothing is persisted", async () => {
    const { useUiStore } = await import("./ui-store");
    expect(useUiStore.getState().themeMode).toBe("system");
    expect(["light", "dark"]).toContain(useUiStore.getState().theme);
  });

  it("hydrates from the persisted plain-string key", async () => {
    window.localStorage.setItem(THEME_STORAGE_KEY, "dark");
    const { useUiStore } = await import("./ui-store");
    expect(useUiStore.getState().themeMode).toBe("dark");
    expect(useUiStore.getState().theme).toBe("dark");
  });

  it("falls back to system when the persisted value is malformed", async () => {
    window.localStorage.setItem(THEME_STORAGE_KEY, "neon");
    const { useUiStore } = await import("./ui-store");
    expect(useUiStore.getState().themeMode).toBe("system");
  });

  it("setThemeMode writes through and updates the resolved theme", async () => {
    const { useUiStore } = await import("./ui-store");
    useUiStore.getState().setThemeMode("light");
    expect(useUiStore.getState().themeMode).toBe("light");
    expect(useUiStore.getState().theme).toBe("light");
    expect(window.localStorage.getItem(THEME_STORAGE_KEY)).toBe("light");
  });

  it("cycleThemeMode rotates Light -> Dark -> System", async () => {
    const { useUiStore } = await import("./ui-store");
    useUiStore.getState().setThemeMode("light");
    useUiStore.getState().cycleThemeMode();
    expect(useUiStore.getState().themeMode).toBe("dark");
    useUiStore.getState().cycleThemeMode();
    expect(useUiStore.getState().themeMode).toBe("system");
    useUiStore.getState().cycleThemeMode();
    expect(useUiStore.getState().themeMode).toBe("light");
  });

  it("setPaletteOpen captures the opener and restores focus on close", async () => {
    const { useUiStore } = await import("./ui-store");
    const button = document.createElement("button");
    document.body.appendChild(button);
    const focusSpy = vi.spyOn(button, "focus");
    useUiStore.getState().setPaletteOpen(true, button);
    expect(useUiStore.getState().paletteOpen).toBe(true);
    expect(useUiStore.getState().paletteOpener).toBe(button);
    useUiStore.getState().setPaletteOpen(false);
    expect(useUiStore.getState().paletteOpen).toBe(false);
    await new Promise((r) => queueMicrotask(() => r(undefined)));
    expect(focusSpy).toHaveBeenCalled();
    button.remove();
  });

  it("setLensOpen mirrors palette behavior on its own opener slot", async () => {
    const { useUiStore } = await import("./ui-store");
    const button = document.createElement("button");
    document.body.appendChild(button);
    useUiStore.getState().setLensOpen(true, button);
    expect(useUiStore.getState().lensOpen).toBe(true);
    expect(useUiStore.getState().lensOpener).toBe(button);
    useUiStore.getState().setLensOpen(false);
    expect(useUiStore.getState().lensOpen).toBe(false);
    expect(useUiStore.getState().lensOpener).toBeNull();
    button.remove();
  });
});
