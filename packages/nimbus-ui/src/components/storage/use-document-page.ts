import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import { documents } from "../../lib/api-mutations";
import type { PageResponse } from "../../lib/types/table";
import type { DocumentFilter, DocumentOrder } from "./table-query";

const PAGE_SIZE = 200;

export type DocumentQuery = {
  filters: DocumentFilter[];
  order: DocumentOrder | null;
};

/**
 * The page position, owned by the URL.
 *
 * `cursors` holds one cursor per page *after* the first, oldest first, so page
 * 1 is the empty array and page N replays `cursors[N - 2]`. The whole stack has
 * to travel, not just the current cursor: prev walks back down it, and a bare
 * `?cursor=` would deep-link a page whose PREV button is dead on reload.
 */
export type DocumentPager = {
  cursors: readonly string[];
  setCursors: (next: string[]) => void;
};

export type DocumentPage = {
  page: PageResponse | null;
  loading: boolean;
  pageError: string | null;
  /** 1-based, derived from the cursor stack in the URL. */
  pageNumber: number;
  refresh: () => void;
  onNext: () => void;
  onPrev: () => void;
  reset: () => void;
};

/**
 * Owns the document browser's pagination engine: the current page, the
 * load/error flags, and the cursor stack that drives prev/next. Selection and
 * drawers stay with the page component — this hook is purely the pager.
 *
 * Cursor invalidation is the hook's responsibility, not the caller's. A cursor
 * encodes the sort values it was produced under and is decoded against the
 * query it is replayed with (`evaluator/pagination.rs`), so replaying page 3 of
 * an unsorted scan against a newly sorted query is not merely wrong, it is
 * undecodable. Whenever the query's *structure* changes, paging starts again
 * from the first page.
 *
 * A sort or filter change is itself a navigation, so the route drops the
 * cursors in the same URL write and the two can never disagree. A tenant
 * change is not: the tenant selector mutates the store without leaving the
 * route, which is the case that would otherwise replay tenant A's cursor
 * against tenant B. The guard below covers it — the fetch reads `null` from
 * the render the scope changes on, and the URL is normalised behind it.
 */
export function useDocumentPage(
  tenant: string,
  table: string,
  query: DocumentQuery,
  pager: DocumentPager,
): DocumentPage {
  const [page, setPage] = useState<PageResponse | null>(null);
  const [loading, setLoading] = useState(false);
  const [pageError, setPageError] = useState<string | null>(null);
  const [refreshTick, setRefreshTick] = useState(0);
  const [staleCursors, setStaleCursors] = useState(false);

  const { cursors, setCursors } = pager;

  // The query arrives as a fresh object on every render (it is derived from URL
  // search params), so its structural serialization — not its identity — is
  // what the fetch and the cursor stack key off.
  const queryKey = JSON.stringify({
    table,
    filters: query.filters,
    order: query.order,
  });
  const stableQuery = useMemo(
    () => JSON.parse(queryKey) as { table: string } & DocumentQuery,
    [queryKey],
  );

  // Marked during render rather than in an effect: an effect would let one
  // fetch go out with the new scope and the *old* cursor before it landed.
  const scopeRef = useRef(`${tenant} ${queryKey}`);
  const scope = `${tenant} ${queryKey}`;
  if (scopeRef.current !== scope) {
    scopeRef.current = scope;
    setStaleCursors(cursors.length > 0);
  }

  // A different tenant or table is a different data set, not a re-query of the
  // same one: drop the rows too, so one table's documents can never be read
  // under another table's header or feed its discovered-field list.
  const dataScopeRef = useRef(`${tenant} ${table}`);
  const dataScope = `${tenant} ${table}`;
  if (dataScopeRef.current !== dataScope) {
    dataScopeRef.current = dataScope;
    setPage(null);
  }

  // While the URL still carries the previous scope's stack, this is page 1 and
  // the cursor is nothing. Normalising the URL is a navigation, and a
  // navigation cannot be awaited before the fetch effect runs.
  const currentCursor = staleCursors
    ? null
    : (cursors[cursors.length - 1] ?? null);
  const pageNumber = staleCursors ? 1 : cursors.length + 1;

  useEffect(() => {
    if (!staleCursors) return;
    if (cursors.length === 0) {
      setStaleCursors(false);
      return;
    }
    setCursors([]);
  }, [staleCursors, cursors.length, setCursors]);

  const loadPage = useCallback(
    async (cursor: string | null) => {
      // No tenant is selected: a request to `/api/tenants//query/paginated`
      // would return an opaque 404 where the page owes the operator a "select
      // a tenant" state.
      if (!tenant) {
        setPage(null);
        setPageError(null);
        setLoading(false);
        return;
      }
      setLoading(true);
      setPageError(null);
      const result = await documents.queryPaginated(
        tenant,
        {
          table: stableQuery.table,
          filters: stableQuery.filters,
          order: stableQuery.order,
          limit: null,
        },
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
    [tenant, stableQuery],
  );

  // biome-ignore lint/correctness/useExhaustiveDependencies: refreshTick is the manual refetch trigger
  useEffect(() => {
    void loadPage(currentCursor);
  }, [loadPage, currentCursor, refreshTick]);

  const reset = useCallback(() => {
    setCursors([]);
    setRefreshTick((t) => t + 1);
  }, [setCursors]);

  const refresh = useCallback(() => {
    setRefreshTick((t) => t + 1);
  }, []);

  const onNext = useCallback(() => {
    const next = page?.next_cursor;
    if (!next) return;
    setCursors([...cursors, next]);
  }, [page, cursors, setCursors]);

  const onPrev = useCallback(() => {
    if (cursors.length === 0) return;
    setCursors(cursors.slice(0, -1));
  }, [cursors, setCursors]);

  return {
    page,
    loading,
    pageError,
    pageNumber,
    refresh,
    onNext,
    onPrev,
    reset,
  };
}
