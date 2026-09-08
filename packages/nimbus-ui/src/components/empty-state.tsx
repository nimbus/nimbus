import { Link } from "@tanstack/react-router";
import type { LucideIcon } from "lucide-react";
import type { ReactNode } from "react";

import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";
import { CopyButton } from "./copy-button";
import { Mascot, type MascotState } from "./mascot";

export type EmptyStateCta =
  | { label: string; to: string }
  | { label: string; onClick: () => void };

// EmptyState is what a list shows when it has nothing to list: an icon, one
// sentence that says why, one action that changes the answer, and, when the
// answer is a command, that command with a copy control. It never looks like
// a failed read; LoadFailed owns that.
//
// `mascot` puts the solid Nimbus mark in the icon's place. It is for the
// shell's own screens — a crash, a lost connection, a first run — where the
// console itself is the subject, not one list on it.
export function EmptyState({
  icon: Icon,
  mascot,
  title,
  body,
  cta,
  snippet,
  testid,
  className,
}: {
  icon?: LucideIcon;
  mascot?: MascotState;
  title: string;
  body?: ReactNode;
  cta?: EmptyStateCta;
  // snippet is the one command that creates the first item.
  snippet?: string;
  testid?: string;
  className?: string;
}) {
  return (
    <div
      className={cn(
        "flex h-full flex-col items-center justify-center gap-3 px-6 py-10 text-center",
        className,
      )}
      data-testid={testid}
    >
      {mascot ? (
        <Mascot size={56} state={mascot} variant="solid" decorative />
      ) : (
        Icon && (
          <span
            aria-hidden
            className="flex size-10 items-center justify-center rounded-full bg-bg-raised text-text-3"
          >
            <Icon className="size-5" />
          </span>
        )
      )}
      <div className="flex flex-col gap-1">
        <h2
          className="text-base font-medium text-text-1"
          data-testid={testid ? `${testid}-title` : undefined}
        >
          {title}
        </h2>
        {body && (
          <p
            className="max-w-md text-sm text-text-3"
            data-testid={testid ? `${testid}-body` : undefined}
          >
            {body}
          </p>
        )}
      </div>
      {cta &&
        ("to" in cta ? (
          <Button
            variant="outline"
            size="sm"
            render={<Link to={cta.to} />}
            data-testid={testid ? `${testid}-cta` : undefined}
          >
            {cta.label}
          </Button>
        ) : (
          <Button
            type="button"
            variant="outline"
            size="sm"
            onClick={cta.onClick}
            data-testid={testid ? `${testid}-cta` : undefined}
          >
            {cta.label}
          </Button>
        ))}
      {snippet && (
        <div
          className="flex max-w-full items-center gap-1 rounded-sm border border-border-1 bg-bg-panel py-0.5 pr-0.5 pl-2.5"
          data-testid={testid ? `${testid}-snippet` : undefined}
        >
          <code className="truncate font-mono text-xs text-text-2">
            {snippet}
          </code>
          <CopyButton text={snippet} label="command" />
        </div>
      )}
    </div>
  );
}
