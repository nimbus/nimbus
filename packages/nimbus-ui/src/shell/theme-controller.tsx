import { useEffect } from "react";
import { useUiStore } from "../store/ui-store";

export function ThemeController() {
  const theme = useUiStore((state) => state.theme);
  useEffect(() => {
    const root = document.documentElement;
    root.dataset.theme = theme;
  }, [theme]);
  return null;
}
