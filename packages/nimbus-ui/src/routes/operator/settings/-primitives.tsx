import { useEffect, useRef } from "react";

export function SectionCard({
  title,
  description,
  testid,
  tone,
  children,
}: {
  title: string;
  description?: string;
  testid: string;
  tone?: "default" | "danger";
  children: React.ReactNode;
}) {
  const borderClass = tone === "danger" ? "border-danger/40" : "border-app";
  return (
    <section
      data-testid={testid}
      className={`rounded-md border ${borderClass} bg-surface p-4`}
    >
      <header className="mb-3">
        <h2
          className="text-sm text-default"
          style={{ fontSize: "var(--text-base)" }}
        >
          {title}
        </h2>
        {description ? (
          <p className="text-xs text-muted">{description}</p>
        ) : null}
      </header>
      {children}
    </section>
  );
}

export function DefinitionList({
  children,
  compact,
}: {
  children: React.ReactNode;
  compact?: boolean;
}) {
  return (
    <dl
      className={`grid grid-cols-1 gap-x-4 gap-y-2 sm:grid-cols-2 ${compact ? "" : "lg:grid-cols-3"}`}
    >
      {children}
    </dl>
  );
}

export function Definition({
  label,
  children,
}: {
  label: string;
  children: React.ReactNode;
}) {
  return (
    <div className="flex flex-col gap-0.5">
      <dt className="text-[10px] uppercase tracking-[0.14em] text-muted">
        {label}
      </dt>
      <dd className="text-sm text-default">{children}</dd>
    </div>
  );
}

export function Cell({
  label,
  children,
}: {
  label: string;
  children: React.ReactNode;
}) {
  return (
    <div className="flex flex-col gap-1 bg-surface px-3 py-2">
      <span className="text-[10px] uppercase tracking-[0.14em] text-muted">
        {label}
      </span>
      <span className="text-sm">{children}</span>
    </div>
  );
}

export function DialogShell({
  title,
  onClose,
  testid,
  children,
}: {
  title: string;
  onClose: () => void;
  testid: string;
  children: React.ReactNode;
}) {
  const previouslyFocusedRef = useRef<HTMLElement | null>(null);

  useEffect(() => {
    previouslyFocusedRef.current =
      (document.activeElement as HTMLElement | null) ?? null;
    return () => {
      previouslyFocusedRef.current?.focus?.();
    };
  }, []);

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/40 px-4"
      data-testid={`${testid}-backdrop`}
    >
      <button
        type="button"
        aria-label="Close dialog"
        onClick={onClose}
        className="absolute inset-0 cursor-default"
      />
      <div
        role="dialog"
        aria-modal="true"
        aria-label={title}
        data-testid={testid}
        className="relative z-10 w-full max-w-md rounded-md border border-app bg-surface p-4 shadow-lg"
      >
        <header className="mb-3 flex items-baseline justify-between">
          <h2 className="text-sm text-default">{title}</h2>
          <button
            type="button"
            onClick={onClose}
            aria-label="Dismiss"
            className="font-mono text-xs text-muted hover:text-default"
          >
            ✕
          </button>
        </header>
        {children}
      </div>
    </div>
  );
}
