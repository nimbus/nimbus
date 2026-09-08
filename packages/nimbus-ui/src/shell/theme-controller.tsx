import { useEffect } from "react";
import { useUiStore } from "../store/ui-store";

// ThemeController stamps the resolved theme on <html>. The favicon is the
// solid mascot in fixed colours, so it needs no swap when the theme changes.
export function ThemeController() {
  const theme = useUiStore((state) => state.theme);
  useEffect(() => {
    document.documentElement.dataset.theme = theme;
  }, [theme]);
  return null;
}
