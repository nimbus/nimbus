import { RotateCw } from "lucide-react";

import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";

// LoadFailed is what a panel shows when its read failed and the page around
// it still stands. It names what is missing, quotes the failure, and offers
// the one action that can change the answer, so a failed read never looks
// like an empty result.
export function LoadFailed({
  what,
  error,
  onRetry,
  testid,
  className,
}: {
  what: string;
  error?: unknown;
  onRetry?: () => void;
  testid?: string;
  className?: string;
}) {
  const message =
    error instanceof Error
      ? error.message
      : typeof error === "string"
        ? error
        : undefined;
  return (
    <div
      role="alert"
      data-testid={testid}
      className={cn(
        "flex flex-col gap-2 rounded-md border border-border-1 bg-bg-panel p-5",
        className,
      )}
    >
      <p className="text-sm font-medium text-text-1">Could not load {what}</p>
      {message && (
        <p className="break-words font-mono text-xs text-text-3">{message}</p>
      )}
      {onRetry && (
        <div>
          <Button type="button" variant="outline" size="sm" onClick={onRetry}>
            <RotateCw aria-hidden className="size-3.5" />
            Try again
          </Button>
        </div>
      )}
    </div>
  );
}
