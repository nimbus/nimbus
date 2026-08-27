import { useRouter } from "@tanstack/react-router";
import { type ReactNode, useCallback } from "react";

import { EmptyState } from "./empty-state";

type LoaderErrorStateProps = {
  error: Error;
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
      <div className="min-h-0 flex-1 overflow-hidden rounded-md border border-app bg-surface">
        <EmptyState
          title={title}
          body={
            <>
              {subject}:{" "}
              <span
                className="font-mono text-default"
                data-testid="storage-server-error"
              >
                {error.message}
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
  error: Error;
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
  error: Error;
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

export function AdminServicesLoaderError({ error }: { error: Error }) {
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

export function AdminServiceDetailLoaderError({ error }: { error: Error }) {
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
