import { useEffect } from "react";
import { useUiStore } from "../store/ui-store";

export function ThemeController() {
  const theme = useUiStore((state) => state.theme);
  const palette = useUiStore((state) => state.palette);
  useEffect(() => {
    const root = document.documentElement;
    root.dataset.theme = theme;
    root.dataset.palette = palette;
    // The favicon follows the resolved console theme, not the OS scheme —
    // an SVG prefers-color-scheme query cannot see data-theme. The static
    // /ui/favicon.svg link stays as the no-JS/initial fallback.
    const icon = document.querySelector(
      'link[rel="icon"][type="image/svg+xml"]',
    );
    if (icon) {
      icon.setAttribute(
        "href",
        theme === "dark" ? "/ui/favicon-night.svg" : "/ui/favicon-warm.svg",
      );
    }
  }, [theme, palette]);
  return null;
}
