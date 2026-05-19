import type { ReactNode } from "react";

import type { LoadingValue } from "../shell/loading-value";

export function LoadingCell<T>({
  value,
  children,
  testid,
}: {
  value: LoadingValue<T>;
  children: (ok: T) => ReactNode;
  testid?: string;
}) {
  switch (value.kind) {
    case "ok":
      return <>{children(value.value)}</>;
    case "loading":
      return (
        <span
          aria-hidden
          className="tabular text-muted"
          data-testid={testid ? `${testid}-loading` : undefined}
          title="Loading…"
        >
          ·
        </span>
      );
    case "offline":
      return (
        <span
          className="font-mono text-[10px] uppercase tracking-wide text-muted"
          data-testid={testid ? `${testid}-offline` : undefined}
          title="Disconnected — value will refresh on reconnect"
        >
          offline
        </span>
      );
    case "error":
      return (
        <span
          className="font-mono text-xs text-danger"
          data-testid={testid ? `${testid}-error` : undefined}
          title={value.message}
        >
          {value.message}
        </span>
      );
  }
}
