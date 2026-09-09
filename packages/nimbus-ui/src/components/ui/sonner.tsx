import {
  CircleCheck,
  Info,
  Loader2,
  OctagonX,
  TriangleAlert,
} from "lucide-react";
import type { ComponentProps, CSSProperties } from "react";
import { Toaster as Sonner } from "sonner";

import { useUiStore } from "@/store/ui-store";

// Toaster binds sonner to the console's own theme store, so a toast follows
// the html[data-theme] switch without a second theme provider.
function Toaster(props: ComponentProps<typeof Sonner>) {
  const theme = useUiStore((s) => s.theme);
  return (
    <Sonner
      theme={theme}
      className="toaster group"
      icons={{
        success: <CircleCheck className="size-4" />,
        info: <Info className="size-4" />,
        warning: <TriangleAlert className="size-4" />,
        error: <OctagonX className="size-4" />,
        loading: <Loader2 className="size-4 animate-spin" />,
      }}
      style={
        {
          "--normal-bg": "var(--bg-raised)",
          "--normal-text": "var(--text-1)",
          "--normal-border": "var(--border-2)",
          "--border-radius": "var(--radius-md)",
        } as CSSProperties
      }
      {...props}
    />
  );
}

export { Toaster };
