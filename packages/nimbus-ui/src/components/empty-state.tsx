import { Link } from "@tanstack/react-router";
import type { ReactNode } from "react";

import { cn } from "../lib/cn";

export type EmptyStateCta =
  | { label: string; to: string }
  | { label: string; onClick: () => void };

export function EmptyState({
  title,
  body,
  cta,
  testid,
  className,
}: {
  title: string;
  body?: ReactNode;
  cta?: EmptyStateCta;
  testid?: string;
  className?: string;
}) {
  return (
    <div
      className={cn(
        "flex h-full flex-col items-center justify-center gap-2 px-6 text-center",
        className,
      )}
      data-testid={testid}
    >
      <h2
        className="text-default"
        style={{ fontSize: "var(--text-lg)" }}
        data-testid={testid ? `${testid}-title` : undefined}
      >
        {title}
      </h2>
      {body !== undefined ? (
        <p
          className="max-w-md text-sm text-muted"
          data-testid={testid ? `${testid}-body` : undefined}
        >
          {body}
        </p>
      ) : null}
      {cta !== undefined ? (
        <EmptyStateCtaButton cta={cta} testid={testid} />
      ) : null}
    </div>
  );
}

function EmptyStateCtaButton({
  cta,
  testid,
}: {
  cta: EmptyStateCta;
  testid?: string;
}) {
  const className =
    "mt-2 rounded border border-app px-3 py-1 font-mono text-[11px] uppercase tracking-wide text-muted hover:bg-surface hover:text-default";
  const ctaTestid = testid ? `${testid}-cta` : undefined;
  if ("to" in cta) {
    return (
      <Link to={cta.to} className={className} data-testid={ctaTestid}>
        {cta.label}
      </Link>
    );
  }
  return (
    <button
      type="button"
      onClick={cta.onClick}
      className={className}
      data-testid={ctaTestid}
    >
      {cta.label}
    </button>
  );
}
