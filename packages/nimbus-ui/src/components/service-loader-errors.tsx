import { useRouter } from "@tanstack/react-router";
import { type ReactNode, useCallback } from "react";

import { EmptyState } from "./empty-state";

/* The router hands a boundary `unknown`; a loader rejects with whatever the
   client threw, which is an Error in practice but is not typed as one. */
function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

type LoaderErrorStateProps = {
  error: unknown;
  onRetry: () => void;
  pageTestId: string;
  title: string;
  subject: ReactNode;
};

function LoaderErrorState({
  error,
  onRetry,
  pageTestId,
  title,
  subject,
}: LoaderErrorStateProps) {
  return (
    <section
      className="flex h-full flex-col gap-4 overflow-hidden px-6 py-5"
      data-testid={pageTestId}
    >
      <div className="min-h-0 flex-1 overflow-hidden rounded-md border border-border-2 bg-bg-panel">
        <EmptyState
          title={title}
          body={
            <>
              {subject}:{" "}
              <span
                className="font-mono text-text-1"
                data-testid="storage-server-error"
              >
                {errorMessage(error)}
              </span>
              . Retry once the backend is reachable.
            </>
          }
          cta={{ label: "Retry", onClick: onRetry }}
          testid="storage-server-error-envelope"
        />
      </div>
    </section>
  );
}

export function ServicesLoaderError({
  error,
  reset,
}: {
  error: unknown;
  reset: () => void;
}) {
  return (
    <LoaderErrorState
      error={error}
      onRetry={reset}
      pageTestId="page-services"
      title="Services endpoint unavailable"
      subject="The services query failed"
    />
  );
}

export function ServiceDetailLoaderError({
  error,
  reset,
}: {
  error: unknown;
  reset: () => void;
}) {
  return (
    <LoaderErrorState
      error={error}
      onRetry={reset}
      pageTestId="page-service-detail"
      title="Service detail unavailable"
      subject="The service-detail query failed"
    />
  );
}

export function AdminServicesLoaderError({ error }: { error: unknown }) {
  const router = useRouter();
  const reload = useCallback(() => {
    void router.invalidate();
  }, [router]);
  return (
    <LoaderErrorState
      error={error}
      onRetry={reload}
      pageTestId="page-admin-services"
      title="Services endpoint unavailable"
      subject="The operator services query failed"
    />
  );
}

export function AdminServiceDetailLoaderError({ error }: { error: unknown }) {
  const router = useRouter();
  const reload = useCallback(() => {
    void router.invalidate();
  }, [router]);
  return (
    <LoaderErrorState
      error={error}
      onRetry={reload}
      pageTestId="page-admin-service-detail"
      title="Service detail unavailable"
      subject="The operator service-detail query failed"
    />
  );
}
