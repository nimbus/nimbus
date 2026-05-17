import { Popover } from "@base-ui/react/popover";
import { useCallback, useState } from "react";

import type { VersionInfo } from "../api/system";
import { cn } from "../lib/cn";

type UpgradePopoverProps = {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  info: VersionInfo;
  isLocal: boolean;
  hasDesktopBridge: boolean;
  onUpdate: () => Promise<void> | void;
  onCopyCommand: () => Promise<void> | void;
  trigger: React.ReactNode;
};

export function UpgradePopover({
  open,
  onOpenChange,
  info,
  isLocal,
  hasDesktopBridge,
  onUpdate,
  onCopyCommand,
  trigger,
}: UpgradePopoverProps) {
  const remote = !isLocal;
  const canRunHere = isLocal && hasDesktopBridge && !!info.upgrade.command;
  const hasCommand = !!info.upgrade.command;
  const heading = remote
    ? `Copy command to run on ${info.host}?`
    : `Update Nimbus to ${info.latest}?`;

  return (
    <Popover.Root open={open} onOpenChange={onOpenChange}>
      <Popover.Trigger
        render={
          <button
            type="button"
            data-testid="status-version-trigger"
            aria-haspopup="dialog"
            className="inline-flex items-center gap-1.5 rounded px-1 font-mono text-xs hover:bg-surface-2 focus-visible:bg-surface-2"
          >
            {trigger}
          </button>
        }
      />
      <Popover.Portal>
        <Popover.Positioner sideOffset={8} side="top" align="start">
          <Popover.Popup
            data-testid="upgrade-popover"
            className={cn(
              "z-50 w-[360px] rounded-md border border-app bg-surface p-3 shadow-lg",
              "font-mono text-xs text-default outline-none",
            )}
          >
            <h2 className="text-sm text-default">{heading}</h2>
            {hasCommand ? (
              <CommandRow command={info.upgrade.command as string} />
            ) : (
              <FallbackRow url={info.upgrade.fallbackUrl} />
            )}
            <div className="mt-3 flex justify-end gap-2">
              <Popover.Close
                render={
                  <button
                    type="button"
                    data-testid="upgrade-popover-cancel"
                    className="rounded px-2 py-1 text-xs text-muted hover:bg-surface-2 hover:text-default"
                  >
                    Cancel
                  </button>
                }
              />
              {hasCommand ? (
                canRunHere ? (
                  <button
                    type="button"
                    data-testid="upgrade-popover-update"
                    onClick={() => {
                      void onUpdate();
                    }}
                    className="rounded px-2 py-1 text-xs text-default"
                    style={{
                      background: "var(--color-accent)",
                      color: "var(--color-on-accent, white)",
                    }}
                  >
                    Update
                  </button>
                ) : (
                  <button
                    type="button"
                    data-testid="upgrade-popover-copy"
                    onClick={() => {
                      void onCopyCommand();
                    }}
                    className="rounded px-2 py-1 text-xs text-default"
                    style={{
                      background: "var(--color-accent)",
                      color: "var(--color-on-accent, white)",
                    }}
                  >
                    Copy command
                  </button>
                )
              ) : null}
            </div>
          </Popover.Popup>
        </Popover.Positioner>
      </Popover.Portal>
    </Popover.Root>
  );
}

function CommandRow({ command }: { command: string }) {
  const [copied, setCopied] = useState(false);
  const onCopy = useCallback(async () => {
    try {
      await navigator.clipboard.writeText(command);
      setCopied(true);
      setTimeout(() => setCopied(false), 1200);
    } catch {
      // ignore
    }
  }, [command]);
  return (
    <div className="mt-3 flex items-center gap-2 rounded border border-app bg-surface-2 px-2 py-1.5">
      <code className="flex-1 truncate font-mono text-xs text-default">
        {command}
      </code>
      <button
        type="button"
        data-testid="upgrade-popover-inline-copy"
        onClick={() => void onCopy()}
        aria-label="Copy command"
        className="rounded px-1 py-px text-xs text-muted hover:bg-surface hover:text-default"
      >
        {copied ? "Copied" : "Copy"}
      </button>
    </div>
  );
}

function FallbackRow({ url }: { url: string }) {
  return (
    <p className="mt-3 text-xs text-muted">
      See the{" "}
      <a
        href={url}
        target="_blank"
        rel="noreferrer"
        className="underline hover:text-default"
        data-testid="upgrade-popover-fallback-link"
      >
        install docs
      </a>{" "}
      to upgrade.
    </p>
  );
}
