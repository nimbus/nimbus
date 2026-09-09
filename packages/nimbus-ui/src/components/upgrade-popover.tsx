import { Popover } from "@base-ui/react/popover";
import { cn } from "@/lib/utils";
import type { VersionInfo } from "../api/system";

type UpgradePopoverProps = {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  info: VersionInfo;
  isLocal: boolean;
  hasDesktopBridge: boolean;
  onUpdate: () => Promise<void> | void;
  onCopyCommand: () => Promise<void> | void;
  trigger: React.ReactNode;
  // The trigger button is the caller's row: the sidebar footer and the
  // Settings page each hand in the classes that make it look like its
  // neighbours, and a label that names the action when the row shows only
  // a dot.
  triggerClassName?: string;
  triggerLabel?: string;
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
  triggerClassName = "inline-flex items-center gap-1.5 rounded-xs px-1 font-mono text-xs hover:bg-bg-raised focus-visible:bg-bg-raised",
  triggerLabel,
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
            data-testid="upgrade-popover-trigger"
            aria-haspopup="dialog"
            aria-label={triggerLabel}
            className={triggerClassName}
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
              "z-50 w-[360px] rounded-md border border-border-2 bg-bg-panel p-3 shadow-lg",
              "font-mono text-xs text-text-1 outline-none",
            )}
          >
            <h2 className="text-sm text-text-1">{heading}</h2>
            {hasCommand ? (
              <CommandRow
                command={info.upgrade.command as string}
                onCopy={onCopyCommand}
              />
            ) : (
              <FallbackRow url={info.upgrade.fallbackUrl} />
            )}
            <div className="mt-3 flex justify-end gap-2">
              <Popover.Close
                render={
                  <button
                    type="button"
                    data-testid="upgrade-popover-cancel"
                    className="rounded-xs px-2 py-1 text-xs text-text-3 hover:bg-bg-raised hover:text-text-1"
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
                    className="rounded-xs bg-accent px-2 py-1 text-xs text-accent-ink"
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
                    className="rounded-xs bg-accent px-2 py-1 text-xs text-accent-ink"
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

// The inline Copy runs the popover's own copy action -- the same one behind
// the primary "Copy command" button. It used to own a second
// `navigator.clipboard.writeText` whose catch was empty, so on the case this
// popover is written for (a remote host reached over plain http, where
// `navigator.clipboard` does not exist) the click did nothing at all and said
// nothing: no label change, no toast. The shared action reports the failure
// and, on success, arms the poll that watches for the upgrade to land.
function CommandRow({
  command,
  onCopy,
}: {
  command: string;
  onCopy: () => Promise<void> | void;
}) {
  return (
    <div className="mt-3 flex items-center gap-2 rounded-xs border border-border-2 bg-bg-raised px-2 py-1.5">
      {/*
        The wrapper already carries the border, fill and padding, so this
        opts out of the global bare-`code` chip rather than nesting a second
        box inside the first.
      */}
      <code className="flex-1 truncate border-0 bg-transparent p-0 font-mono text-xs text-text-1">
        {command}
      </code>
      <button
        type="button"
        data-testid="upgrade-popover-inline-copy"
        onClick={() => void onCopy()}
        aria-label="Copy command"
        className="rounded-xs px-1 py-px text-xs text-text-3 hover:bg-bg-panel hover:text-text-1"
      >
        Copy
      </button>
    </div>
  );
}

function FallbackRow({ url }: { url: string }) {
  return (
    <p className="mt-3 text-xs text-text-3">
      See the{" "}
      <a
        href={url}
        target="_blank"
        rel="noreferrer"
        className="link-inline"
        data-testid="upgrade-popover-fallback-link"
      >
        install docs
      </a>{" "}
      to upgrade.
    </p>
  );
}
