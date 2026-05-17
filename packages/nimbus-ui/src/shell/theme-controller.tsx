import { useEffect } from "react";
import { useUiStore } from "../store/ui-store";

export function ThemeController() {
  const theme = useUiStore((state) => state.theme);
  const palette = useUiStore((state) => state.palette);
  useEffect(() => {
    const root = document.documentElement;
    root.dataset.theme = theme;
    root.dataset.palette = palette;
  }, [theme, palette]);
  return null;
}
