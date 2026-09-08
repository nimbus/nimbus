import { X } from "lucide-react";
import type { ReactNode } from "react";

import { Button } from "@/components/ui/button";
import { Sheet, SheetContent } from "@/components/ui/sheet";

// PanelHeader is the title row of a side panel: the name at the label size
// and one close control named after the panel, so "Close Schema" and "Close
// Indexes" are different buttons to a screen reader.
export function PanelHeader({
  title,
  onClose,
}: {
  title: string;
  onClose: () => void;
}) {
  return (
    <div className="flex items-center justify-between border-b border-border-2 px-3 py-2">
      <h2 className="text-xs font-medium text-text-3">{title}</h2>
      <Button
        type="button"
        variant="ghost"
        size="icon-xs"
        aria-label={`Close ${title}`}
        onClick={onClose}
      >
        <X aria-hidden className="size-3.5" />
      </Button>
    </div>
  );
}

// Slideover is a modal panel that enters from the right edge for a form
// that edits one document. Base UI owns the focus trap, Escape, the outside
// press, and the focus restore to the control that opened it.
export function Slideover({
  title,
  onClose,
  testid,
  children,
}: {
  title: string;
  onClose: () => void;
  testid?: string;
  children: ReactNode;
}) {
  return (
    <Sheet
      open
      onOpenChange={(open) => {
        if (!open) onClose();
      }}
    >
      <SheetContent
        side="right"
        showCloseButton={false}
        aria-label={title}
        data-testid={testid}
        className="gap-2 border-border-2 bg-bg-panel p-4 text-text-1 data-[side=right]:w-[480px] data-[side=right]:max-w-full data-[side=right]:sm:max-w-full"
      >
        <PanelHeader title={title} onClose={onClose} />
        {children}
      </SheetContent>
    </Sheet>
  );
}
