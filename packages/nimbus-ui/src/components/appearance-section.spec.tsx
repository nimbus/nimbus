import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const THEME_STORAGE_KEY = "nimbus-ui:theme";
const PALETTE_STORAGE_KEY = "nimbus-ui:palette";

beforeEach(() => {
  vi.resetModules();
  window.localStorage.clear();
  document.documentElement.removeAttribute("data-theme");
  document.documentElement.removeAttribute("data-palette");
});

afterEach(() => {
  window.localStorage.clear();
  document.documentElement.removeAttribute("data-theme");
  document.documentElement.removeAttribute("data-palette");
});

async function mountAppearancePropagation() {
  const { ThemeController } = await import("../shell/theme-controller");
  const { AppearanceSection } = await import("./appearance-section");
  return render(
    <>
      <ThemeController />
      <AppearanceSection />
    </>,
  );
}

describe("appearance propagation", () => {
  it("clicking each mode button updates the <html data-theme> attribute and persists", async () => {
    const user = userEvent.setup();
    await mountAppearancePropagation();

    await user.click(screen.getByTestId("appearance-mode-light"));
    expect(document.documentElement.dataset.theme).toBe("light");
    expect(window.localStorage.getItem(THEME_STORAGE_KEY)).toBe("light");

    await user.click(screen.getByTestId("appearance-mode-dark"));
    expect(document.documentElement.dataset.theme).toBe("dark");
    expect(window.localStorage.getItem(THEME_STORAGE_KEY)).toBe("dark");

    await user.click(screen.getByTestId("appearance-mode-system"));
    expect(["light", "dark"]).toContain(document.documentElement.dataset.theme);
    expect(window.localStorage.getItem(THEME_STORAGE_KEY)).toBe("system");
  });

  it("clicking each palette button updates the <html data-palette> attribute and persists", async () => {
    const user = userEvent.setup();
    await mountAppearancePropagation();

    await user.click(screen.getByTestId("appearance-palette-mono"));
    expect(document.documentElement.dataset.palette).toBe("mono");
    expect(window.localStorage.getItem(PALETTE_STORAGE_KEY)).toBe("mono");

    await user.click(screen.getByTestId("appearance-palette-warm"));
    expect(document.documentElement.dataset.palette).toBe("warm");
    expect(window.localStorage.getItem(PALETTE_STORAGE_KEY)).toBe("warm");

    await user.click(screen.getByTestId("appearance-palette-blue"));
    expect(document.documentElement.dataset.palette).toBe("blue");
    expect(window.localStorage.getItem(PALETTE_STORAGE_KEY)).toBe("blue");
  });

  it("mode and palette propagate independently — switching mode does not reset palette", async () => {
    const user = userEvent.setup();
    await mountAppearancePropagation();

    await user.click(screen.getByTestId("appearance-palette-warm"));
    await user.click(screen.getByTestId("appearance-mode-dark"));

    expect(document.documentElement.dataset.palette).toBe("warm");
    expect(document.documentElement.dataset.theme).toBe("dark");

    await user.click(screen.getByTestId("appearance-mode-light"));
    expect(document.documentElement.dataset.palette).toBe("warm");
    expect(document.documentElement.dataset.theme).toBe("light");
  });

  it("hydrates DOM data attributes from persisted localStorage on mount", async () => {
    window.localStorage.setItem(THEME_STORAGE_KEY, "dark");
    window.localStorage.setItem(PALETTE_STORAGE_KEY, "mono");
    await mountAppearancePropagation();

    expect(document.documentElement.dataset.theme).toBe("dark");
    expect(document.documentElement.dataset.palette).toBe("mono");
  });

  it("marks the active mode and palette buttons via aria-checked + data-active", async () => {
    const user = userEvent.setup();
    await mountAppearancePropagation();

    await user.click(screen.getByTestId("appearance-mode-light"));
    await user.click(screen.getByTestId("appearance-palette-warm"));

    const lightMode = screen.getByTestId("appearance-mode-light");
    const darkMode = screen.getByTestId("appearance-mode-dark");
    expect(lightMode.getAttribute("aria-checked")).toBe("true");
    expect(lightMode.dataset.active).toBe("true");
    expect(darkMode.getAttribute("aria-checked")).toBe("false");
    expect(darkMode.dataset.active).toBe("false");

    const warmPalette = screen.getByTestId("appearance-palette-warm");
    const monoPalette = screen.getByTestId("appearance-palette-mono");
    expect(warmPalette.getAttribute("aria-checked")).toBe("true");
    expect(warmPalette.dataset.active).toBe("true");
    expect(monoPalette.getAttribute("aria-checked")).toBe("false");
    expect(monoPalette.dataset.active).toBe("false");
  });
});

/* The swatch used to be a table of hand-copied hexes, which drifted: it
   previewed an amber "Warm dark" theme that globals.css does not define. It is
   now a live probe of the cascade. These tests lock the probe mechanism; the
   colour values it produces are locked in styles/contrast.spec.ts, which reads
   the real stylesheet (vitest runs with `css: false`, so computed colours are
   not available here). */
describe("palette swatch preview", () => {
  const PALETTE_IDS = ["warm", "blue", "mono"] as const;
  const MODES = ["light", "dark"] as const;

  // The probe needs data-palette *and* data-theme. Under a dark <html>, a
  // [data-palette="blue"] / [data-palette="mono"] wrapper alone would win over
  // the inherited dark tokens and preview the *light* variant of that palette.
  it.each(MODES)("%s: swatch carries both probe attributes", async (mode) => {
    const user = userEvent.setup();
    await mountAppearancePropagation();
    await user.click(screen.getByTestId(`appearance-mode-${mode}`));

    for (const id of PALETTE_IDS) {
      const swatch = screen.getByTestId(`appearance-swatch-${id}`);
      expect(swatch.dataset.palette).toBe(id);
      expect(swatch.dataset.theme).toBe(mode);
      expect(document.documentElement.dataset.theme).toBe(mode);
    }
  });

  it("paints the swatch from --nimbus-* tokens, never a literal colour", async () => {
    await mountAppearancePropagation();

    for (const id of PALETTE_IDS) {
      const swatch = screen.getByTestId(`appearance-swatch-${id}`);
      // --color-* is not a real custom property: `@theme inline` inlines its
      // values into utilities and emits no variable, so it would resolve to
      // nothing here.
      expect(swatch.outerHTML).not.toMatch(/#[0-9a-fA-F]{3,8}\b/);
      expect(swatch.outerHTML).not.toMatch(/var\(--color-/);
      expect(swatch.style.background).toBe("var(--nimbus-surface)");
      for (const part of ["brand", "accent"] as const) {
        const cell = screen.getByTestId(`appearance-swatch-${id}-${part}`);
        expect(cell.style.background).toBe(`var(--nimbus-${part})`);
      }
    }
  });

  it("explains the shared Night Blue dark variant, in dark mode only", async () => {
    const user = userEvent.setup();
    await mountAppearancePropagation();

    await user.click(screen.getByTestId("appearance-mode-dark"));
    expect(screen.getByTestId("appearance-palette-dark-note")).toBeTruthy();

    await user.click(screen.getByTestId("appearance-mode-light"));
    expect(screen.queryByTestId("appearance-palette-dark-note")).toBeNull();
  });

  it("keeps every palette selectable in dark mode, including the two that share it", async () => {
    const user = userEvent.setup();
    await mountAppearancePropagation();
    await user.click(screen.getByTestId("appearance-mode-dark"));

    for (const id of PALETTE_IDS) {
      const button = screen.getByTestId(`appearance-palette-${id}`);
      expect(button.hasAttribute("disabled")).toBe(false);
      await user.click(button);
      expect(document.documentElement.dataset.palette).toBe(id);
    }
  });
});
