import { act, renderHook, waitFor } from "@testing-library/react";
import { HttpResponse, http } from "msw";
import { setupServer } from "msw/node";
import { useMemo, useState } from "react";
import { afterAll, afterEach, beforeAll, describe, expect, it } from "vitest";

import type { DocumentFilter, DocumentOrder } from "./table-query";
import { type DocumentQuery, useDocumentPage } from "./use-document-page";

const server = setupServer();

beforeAll(() => server.listen({ onUnhandledRequest: "error" }));
afterEach(() => server.resetHandlers());
afterAll(() => server.close());

type Body = {
  tenant: string;
  after: string | null;
  query: { table: string; filters: unknown[]; order: unknown };
};

// A two-page fixture keyed on the `after` cursor sent in the request body,
// recording every request so the tests can assert what the wire actually saw.
function paginate(seen?: Body[]) {
  return http.post(
    "*/api/tenants/:t/query/paginated",
    async ({ request, params }) => {
      const body = (await request.json()) as Omit<Body, "tenant">;
      seen?.push({ ...body, tenant: String(params.t) });
      if (body.after === "c1") {
        return HttpResponse.json({
          data: [{ _id: "b" }],
          next_cursor: null,
          has_more: false,
        });
      }
      return HttpResponse.json({
        data: [{ _id: "a" }],
        next_cursor: "c1",
        has_more: true,
      });
    },
  );
}

const NO_QUERY: DocumentQuery = { filters: [], order: null };

type Props = { tenant: string; table: string; query: DocumentQuery };

/**
 * Stands in for the route. The cursor stack lives outside the hook — in the URL
 * for real, in state here — and a `setCursors` call re-renders the hook with
 * the new stack exactly as a navigation would.
 */
function useHarness({ tenant, table, query }: Props, seed: string[]) {
  const [cursors, setCursors] = useState<string[]>(seed);
  const pager = useMemo(() => ({ cursors, setCursors }), [cursors]);
  return { ...useDocumentPage(tenant, table, query, pager), cursors };
}

function renderPager(initialProps: Partial<Props> = {}, seed: string[] = []) {
  return renderHook((props: Props) => useHarness(props, seed), {
    initialProps: {
      tenant: "demo",
      table: "users",
      query: NO_QUERY,
      ...initialProps,
    },
  });
}

describe("useDocumentPage", () => {
  it("loads the first page with an empty cursor stack", async () => {
    const seen: Body[] = [];
    server.use(paginate(seen));
    const { result } = renderPager();

    await waitFor(() => expect(result.current.page).not.toBeNull());
    expect(result.current.page?.data).toEqual([{ _id: "a" }]);
    expect(result.current.page?.has_more).toBe(true);
    expect(result.current.cursors).toEqual([]);
    expect(result.current.pageNumber).toBe(1);
    expect(seen[0]?.after).toBeNull();
    expect(result.current.pageError).toBeNull();
  });

  // The stack is the pager's whole position: page N replays `cursors[N - 2]`,
  // so a reload of a shared URL lands on the page it names.
  it("restores a deep-linked page from the cursor stack it is given", async () => {
    const seen: Body[] = [];
    server.use(paginate(seen));
    const { result } = renderPager({}, ["c1"]);

    await waitFor(() =>
      expect(result.current.page?.data).toEqual([{ _id: "b" }]),
    );
    expect(result.current.pageNumber).toBe(2);
    expect(seen[0]?.after).toBe("c1");
  });

  it("pushes a cursor on next and pops it on prev", async () => {
    server.use(paginate());
    const { result } = renderPager();

    await waitFor(() =>
      expect(result.current.page?.data).toEqual([{ _id: "a" }]),
    );

    act(() => result.current.onNext());
    await waitFor(() => expect(result.current.cursors).toEqual(["c1"]));
    await waitFor(() =>
      expect(result.current.page?.data).toEqual([{ _id: "b" }]),
    );
    expect(result.current.pageNumber).toBe(2);
    expect(result.current.page?.has_more).toBe(false);

    act(() => result.current.onPrev());
    await waitFor(() => expect(result.current.cursors).toEqual([]));
    await waitFor(() =>
      expect(result.current.page?.data).toEqual([{ _id: "a" }]),
    );
    expect(result.current.pageNumber).toBe(1);
  });

  it("does not advance past the last page", async () => {
    server.use(paginate());
    const { result } = renderPager();

    await waitFor(() =>
      expect(result.current.page?.data).toEqual([{ _id: "a" }]),
    );
    act(() => result.current.onNext());
    await waitFor(() =>
      expect(result.current.page?.data).toEqual([{ _id: "b" }]),
    );

    // Last page has next_cursor === null, so a further next is a no-op.
    act(() => result.current.onNext());
    expect(result.current.cursors).toEqual(["c1"]);
  });

  it("refresh refetches the current page", async () => {
    const seen: Body[] = [];
    server.use(paginate(seen));
    const { result } = renderPager();

    await waitFor(() => expect(result.current.page).not.toBeNull());
    expect(seen).toHaveLength(1);

    act(() => result.current.refresh());
    await waitFor(() => expect(seen).toHaveLength(2));
    expect(result.current.cursors).toEqual([]);
  });

  it("keeps fetching from the same identity when the query object is rebuilt", async () => {
    const seen: Body[] = [];
    server.use(paginate(seen));
    // The route derives the query from URL search params, so a fresh object
    // arrives on every render. Only a structural change may refetch.
    const { result, rerender } = renderPager({
      query: { filters: [], order: null },
    });

    await waitFor(() => expect(seen).toHaveLength(1));
    rerender({
      tenant: "demo",
      table: "users",
      query: { filters: [], order: null },
    });
    rerender({
      tenant: "demo",
      table: "users",
      query: { filters: [], order: null },
    });
    await waitFor(() =>
      expect(result.current.page?.data).toEqual([{ _id: "a" }]),
    );
    expect(seen).toHaveLength(1);
  });

  // A cursor encodes the sort values it was produced under and is decoded
  // against the query it is replayed with, so replaying page two of an
  // unsorted scan against a newly sorted query is undecodable, not merely
  // stale. Every structural query change has to restart paging — including the
  // case where the caller hands the hook a new query while the old stack is
  // still in the URL.
  it("resets the cursor stack when the sort changes on page two", async () => {
    const seen: Body[] = [];
    server.use(paginate(seen));
    const order: DocumentOrder = { field: "name", direction: "asc" };
    const { result, rerender } = renderPager();

    await waitFor(() =>
      expect(result.current.page?.data).toEqual([{ _id: "a" }]),
    );
    act(() => result.current.onNext());
    await waitFor(() => expect(result.current.cursors).toEqual(["c1"]));
    expect(result.current.pageNumber).toBe(2);

    rerender({ tenant: "demo", table: "users", query: { filters: [], order } });
    await waitFor(() => expect(result.current.cursors).toEqual([]));
    expect(result.current.pageNumber).toBe(1);
    // No request may carry the old cursor under the new sort.
    const sorted = seen.filter((b) => b.query.order !== null);
    expect(sorted.length).toBeGreaterThan(0);
    for (const body of sorted) expect(body.after).toBeNull();
    expect(sorted[0]?.query.order).toEqual(order);
  });

  it("resets the cursor stack when a filter changes on page two", async () => {
    const seen: Body[] = [];
    server.use(paginate(seen));
    const filter: DocumentFilter = { field: "name", op: "eq", value: "ada" };
    const { result, rerender } = renderPager();

    await waitFor(() =>
      expect(result.current.page?.data).toEqual([{ _id: "a" }]),
    );
    act(() => result.current.onNext());
    await waitFor(() => expect(result.current.cursors).toEqual(["c1"]));

    rerender({
      tenant: "demo",
      table: "users",
      query: { filters: [filter], order: null },
    });
    await waitFor(() => expect(result.current.cursors).toEqual([]));
    const filtered = seen.filter((b) => b.query.filters.length > 0);
    expect(filtered.length).toBeGreaterThan(0);
    for (const body of filtered) expect(body.after).toBeNull();
    expect(filtered[0]?.query.filters).toEqual([filter]);
  });

  // The tenant selector mutates the store without leaving the route, so unlike
  // a sort change it cannot drop the stack in the same navigation. A cursor
  // minted inside one tenant must never be replayed inside another.
  it("never replays one tenant's cursor against another tenant", async () => {
    const seen: Body[] = [];
    server.use(paginate(seen));
    const { result, rerender } = renderPager();

    await waitFor(() =>
      expect(result.current.page?.data).toEqual([{ _id: "a" }]),
    );
    act(() => result.current.onNext());
    await waitFor(() => expect(result.current.cursors).toEqual(["c1"]));

    rerender({ tenant: "other", table: "users", query: NO_QUERY });
    await waitFor(() => expect(result.current.cursors).toEqual([]));

    const crossed = seen.filter((b) => b.tenant === "other");
    expect(crossed.length).toBeGreaterThan(0);
    for (const body of crossed) expect(body.after).toBeNull();
    expect(result.current.pageNumber).toBe(1);
  });

  // One table's rows must never be readable under another table's header, and
  // must never seed the other table's discovered-column list.
  it("drops the current rows when the table changes", async () => {
    server.use(paginate());
    const { result, rerender } = renderPager();

    await waitFor(() =>
      expect(result.current.page?.data).toEqual([{ _id: "a" }]),
    );
    rerender({ tenant: "demo", table: "messages", query: NO_QUERY });
    expect(result.current.page).toBeNull();
  });

  it("drops the current rows when the tenant changes", async () => {
    server.use(paginate());
    const { result, rerender } = renderPager();

    await waitFor(() =>
      expect(result.current.page?.data).toEqual([{ _id: "a" }]),
    );
    rerender({ tenant: "other", table: "users", query: NO_QUERY });
    expect(result.current.page).toBeNull();
  });

  it("surfaces the error message and clears the page on failure", async () => {
    server.use(
      http.post("*/api/tenants/:t/query/paginated", () =>
        HttpResponse.json(
          { error: { message: "query failed" } },
          { status: 500 },
        ),
      ),
    );
    const { result } = renderPager();

    await waitFor(() => expect(result.current.pageError).toBe("query failed"));
    expect(result.current.page).toBeNull();
    expect(result.current.loading).toBe(false);
  });
});
