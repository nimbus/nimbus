import { Upload } from "lucide-react";
import { type DragEvent, type ReactNode, useCallback, useState } from "react";

import { cn } from "@/lib/utils";

// DropZone makes a region accept dropped files. It wraps the region it
// serves, so a drop anywhere over the table lands, and it shows one overlay
// while a file is over it. Dragging text or a link over it does nothing: the
// overlay rises only for a transfer that carries files.
function carriesFiles(event: DragEvent): boolean {
  return Array.from(event.dataTransfer?.types ?? []).includes("Files");
}

export function DropZone({
  onFiles,
  disabled = false,
  label = "Drop files to upload",
  testid = "drop-zone",
  className,
  children,
}: {
  onFiles: (files: File[]) => void;
  disabled?: boolean;
  label?: string;
  testid?: string;
  className?: string;
  children: ReactNode;
}) {
  // A drag fires enter and leave for every child the pointer crosses; the
  // depth counter keeps the overlay up until the pointer leaves the zone.
  const [depth, setDepth] = useState(0);
  const active = depth > 0 && !disabled;

  const onDragEnter = useCallback(
    (event: DragEvent<HTMLDivElement>) => {
      if (disabled || !carriesFiles(event)) return;
      event.preventDefault();
      setDepth((d) => d + 1);
    },
    [disabled],
  );
  const onDragOver = useCallback(
    (event: DragEvent<HTMLDivElement>) => {
      if (disabled || !carriesFiles(event)) return;
      event.preventDefault();
      event.dataTransfer.dropEffect = "copy";
    },
    [disabled],
  );
  const onDragLeave = useCallback(
    (event: DragEvent<HTMLDivElement>) => {
      if (disabled || !carriesFiles(event)) return;
      setDepth((d) => Math.max(0, d - 1));
    },
    [disabled],
  );
  const onDrop = useCallback(
    (event: DragEvent<HTMLDivElement>) => {
      if (disabled || !carriesFiles(event)) return;
      event.preventDefault();
      setDepth(0);
      const files = Array.from(event.dataTransfer.files);
      if (files.length > 0) onFiles(files);
    },
    [disabled, onFiles],
  );

  return (
    // biome-ignore lint/a11y/noStaticElementInteractions: a drop target listens for drags only; the Upload button beside it is the keyboard path
    <div
      className={cn("relative", className)}
      data-testid={testid}
      data-active={active || undefined}
      onDragEnter={onDragEnter}
      onDragOver={onDragOver}
      onDragLeave={onDragLeave}
      onDrop={onDrop}
    >
      {children}
      {active ? (
        <div
          className="pointer-events-none absolute inset-0 z-10 flex items-center justify-center rounded-md border-2 border-dashed border-accent-edge bg-bg-panel/80"
          data-testid={`${testid}-overlay`}
        >
          <div className="flex items-center gap-2 rounded-md bg-bg-panel px-3 py-2 text-sm text-text-1 shadow-md">
            <Upload className="size-4 text-accent-edge" aria-hidden />
            {label}
          </div>
        </div>
      ) : null}
    </div>
  );
}
