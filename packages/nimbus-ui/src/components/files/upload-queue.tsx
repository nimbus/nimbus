import { CircleAlert, CircleCheck, X } from "lucide-react";

import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";
import { formatBytes } from "../../lib/format";

export type UploadItem = {
  id: string;
  name: string;
  loaded: number;
  total: number;
  status: "uploading" | "done" | "error";
  error?: string;
};

export function uploadPercent(item: UploadItem): number {
  if (item.status === "done") return 100;
  if (item.total <= 0) return 0;
  return Math.min(100, Math.floor((item.loaded / item.total) * 100));
}

// UploadQueue is the receipt strip for the files an operator just dropped:
// one row per file with a progress bar while the bytes move, a check when
// the object landed, and the server's reason when it refused. A settled row
// stays until dismissed so a refusal is read, not lost under the next drop.
export function UploadQueue({
  items,
  onDismiss,
  testid = "upload-queue",
}: {
  items: readonly UploadItem[];
  onDismiss: (id: string) => void;
  testid?: string;
}) {
  if (items.length === 0) return null;
  return (
    <ul
      className="flex shrink-0 flex-col gap-1 rounded-md border border-border-2 bg-bg-raised p-2"
      aria-label="Uploads"
      data-testid={testid}
    >
      {items.map((item) => {
        const percent = uploadPercent(item);
        return (
          <li
            key={item.id}
            className="flex items-center gap-3 px-1 text-xs"
            data-testid={`${testid}-item-${item.id}`}
            data-status={item.status}
          >
            <span
              className="min-w-0 flex-1 truncate font-mono text-text-1"
              title={item.name}
            >
              {item.name}
            </span>
            {item.status === "uploading" ? (
              <>
                <progress
                  className="h-1.5 w-32 overflow-hidden rounded-full [&::-webkit-progress-bar]:bg-bg-panel [&::-webkit-progress-value]:bg-accent"
                  max={100}
                  value={percent}
                  aria-label={`Uploading ${item.name}`}
                  data-testid={`${testid}-progress-${item.id}`}
                />
                <span
                  className="w-10 text-right font-mono tabular text-text-3"
                  data-testid={`${testid}-percent-${item.id}`}
                >
                  {percent}%
                </span>
              </>
            ) : item.status === "done" ? (
              <span className="flex items-center gap-1 text-success">
                <CircleCheck className="size-3.5" aria-hidden />
                <span className="font-mono tabular text-text-3">
                  {formatBytes(item.total)}
                </span>
              </span>
            ) : (
              <span
                className="flex min-w-0 items-center gap-1 text-error"
                data-testid={`${testid}-error-${item.id}`}
              >
                <CircleAlert className="size-3.5 shrink-0" aria-hidden />
                <span className="truncate" title={item.error}>
                  {item.error ?? "Upload failed"}
                </span>
              </span>
            )}
            <Button
              type="button"
              variant="ghost"
              size="icon-xs"
              aria-label={`Dismiss ${item.name}`}
              className={cn(item.status === "uploading" && "invisible")}
              onClick={() => onDismiss(item.id)}
              data-testid={`${testid}-dismiss-${item.id}`}
            >
              <X />
            </Button>
          </li>
        );
      })}
    </ul>
  );
}
