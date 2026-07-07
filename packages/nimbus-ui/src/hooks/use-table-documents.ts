import { useCallback, useEffect, useState } from "react";

import { documents } from "../lib/api-mutations";
import type { PageResponse } from "../lib/types/table";

const PAGE_SIZE = 25;

export type TableDocuments = {
  page: PageResponse | null;
  loading: boolean;
  pageError: string | null;
  cursorStack: Array<string | null>;
  refresh: () => void;
  onNext: () => void;
  onPrev: () => void;
  reset: () => void;
};

// Owns the storage table's pagination engine: the current page, the load/error
// flags, and the cursor stack that drives prev/next. Reads flow through the
// typed `documents.queryPaginated` client. Selection and drawers stay with the
// page component — this hook is purely the pager.
export function useTableDocuments(
  tenant: string,
  table: string,
): TableDocuments {
  const [page, setPage] = useState<PageResponse | null>(null);
  const [loading, setLoading] = useState(false);
  const [pageError, setPageError] = useState<string | null>(null);
  const [cursorStack, setCursorStack] = useState<Array<string | null>>([null]);
  const [refreshTick, setRefreshTick] = useState(0);

  const currentCursor = cursorStack[cursorStack.length - 1] ?? null;

  const loadPage = useCallback(
    async (cursor: string | null) => {
      setLoading(true);
      setPageError(null);
      const result = await documents.queryPaginated(
        tenant,
        { table, filters: [], order: null, limit: null },
        PAGE_SIZE,
        cursor,
      );
      if (result.ok) {
        setPage({
          data: result.data.data,
          next_cursor: result.data.next_cursor,
          has_more: result.data.has_more,
        });
      } else {
        setPageError(result.error);
        setPage(null);
      }
      setLoading(false);
    },
    [tenant, table],
  );

  // biome-ignore lint/correctness/useExhaustiveDependencies: refreshTick is the manual refetch trigger
  useEffect(() => {
    void loadPage(currentCursor);
  }, [loadPage, currentCursor, refreshTick]);

  const reset = useCallback(() => {
    setCursorStack([null]);
    setRefreshTick((t) => t + 1);
  }, []);

  const refresh = useCallback(() => {
    setRefreshTick((t) => t + 1);
  }, []);

  const onNext = useCallback(() => {
    if (page?.next_cursor) {
      setCursorStack((stack) => [...stack, page.next_cursor]);
    }
  }, [page]);

  const onPrev = useCallback(() => {
    setCursorStack((stack) => (stack.length > 1 ? stack.slice(0, -1) : stack));
  }, []);

  return {
    page,
    loading,
    pageError,
    cursorStack,
    refresh,
    onNext,
    onPrev,
    reset,
  };
}
