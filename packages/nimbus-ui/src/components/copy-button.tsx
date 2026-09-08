import { Check, Copy } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { toast } from "sonner";

import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import { cn } from "@/lib/utils";

// CopyButton copies text, says "Copied" for two seconds where a screen
// reader hears it, and reports a copy the browser refused. It is the icon
// control next to a snippet or a header value; CopyChip is the inline value
// that copies itself.
export function CopyButton({
  text,
  label,
  testid,
  className,
}: {
  text: string | (() => string);
  // label names what is copied ("command", "run id") in the tooltip and the
  // accessible name, and shows beside the icon while nothing is copied.
  label?: string;
  testid?: string;
  className?: string;
}) {
  const [copied, setCopied] = useState(false);
  const timer = useRef<ReturnType<typeof setTimeout>>(undefined);
  useEffect(() => () => clearTimeout(timer.current), []);

  const copy = async () => {
    const value = typeof text === "function" ? text() : text;
    try {
      await navigator.clipboard.writeText(value);
      setCopied(true);
      clearTimeout(timer.current);
      timer.current = setTimeout(() => setCopied(false), 2000);
    } catch {
      toast.error(`Failed to copy ${label ?? "text"}`, {
        description: "Select the text and copy it by hand.",
      });
    }
  };

  const Icon = copied ? Check : Copy;
  const name = label ? `Copy ${label}` : "Copy";
  return (
    <Tooltip>
      <TooltipTrigger
        render={
          <button
            type="button"
            onClick={copy}
            aria-label={name}
            data-testid={testid}
            data-copied={copied ? "true" : undefined}
            className={cn(
              "inline-flex h-7 shrink-0 items-center gap-1.5 rounded-xs px-1.5 text-xs transition-colors duration-150 ease-standard hover:bg-bg-hover",
              copied ? "text-success" : "text-text-3 hover:text-text-2",
              className,
            )}
          />
        }
      >
        <Icon aria-hidden className="size-3.5" />
        <span role="status">{copied ? "Copied" : label}</span>
      </TooltipTrigger>
      <TooltipContent>{name}</TooltipContent>
    </Tooltip>
  );
}
