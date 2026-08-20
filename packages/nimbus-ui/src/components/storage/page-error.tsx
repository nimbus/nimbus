// Full-panel error state for a failed document-page load, with a retry action
// that re-runs the current query.
export function PageError({
  message,
  onRetry,
}: {
  message: string;
  onRetry: () => void;
}) {
  return (
    <div className="flex h-full flex-col items-center justify-center gap-3 px-6 py-10 text-center">
      <p
        className="font-mono text-sm text-danger"
        data-testid="documents-error"
      >
        {message}
      </p>
      <button
        type="button"
        onClick={onRetry}
        className="rounded border border-app px-2 py-1 font-mono text-xs uppercase tracking-wide text-default hover:bg-surface"
        data-testid="documents-retry"
      >
        retry
      </button>
    </div>
  );
}
