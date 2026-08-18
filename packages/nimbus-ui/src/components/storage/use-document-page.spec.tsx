import { act, renderHook, waitFor } from "@testing-library/react";
import { HttpResponse, http } from "msw";
import { setupServer } from "msw/node";
import { afterAll, afterEach, beforeAll, describe, expect, it } from "vitest";

import type { DocumentFilter, DocumentOrder } from "./table-query";
import { useDocumentPage } from "./use-document-page";

const server = setupServer();

beforeAll(() => server.listen({ onUnhandledRequest: "error" }));
afterEach(() => server.resetHandlers());
afterAll(() => server.close());

type Body = {
  after: string | null;
  query: { table: string; filters: unknown[]; order: unknown };
};

// A two-page fixture keyed on the `after` cursor sent in the request body,
// recording every request so the tests can assert what the wire actually saw.
function paginate(seen?: Body[]) {
  return http.post("*/api/tenants/:t/query/paginated", async ({ request }) => {
    const body = (await request.json()) as Body;
    seen?.push(body);
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
  });
}

const NO_QUERY = { filters: [] as DocumentFilter[], order: null };

describe("useDocumentPage", () => {
  it("loads the first page and seeds the cursor stack", async () => {
    server.use(paginate());
    const { result } = renderHook(() =>
      useDocumentPage("demo", "users", NO_QUERY),
    );

    await waitFor(() => expect(result.current.page).not.toBeNull());
    expect(result.current.page?.data).toEqual([{ _id: "a" }]);
    expect(result.current.page?.has_more).toBe(true);
    expect(result.current.cursorStack).toEqual([null]);
    expect(result.current.pageError).toBeNull();
  });

  it("pushes a cursor on next and pops it on prev", async () => {
    server.use(paginate());
    const { result } = renderHook(() =>
      useDocumentPage("demo", "users", NO_QUERY),
    );

    await waitFor(() =>
      expect(result.current.page?.data).toEqual([{ _id: "a" }]),
    );

    act(() => result.current.onNext());
    await waitFor(() =>
      expect(result.current.cursorStack).toEqual([null, "c1"]),
    );
    await waitFor(() =>
      expect(result.current.page?.data).toEqual([{ _id: "b" }]),
    );
    expect(result.current.page?.has_more).toBe(false);

    act(() => result.current.onPrev());
    await waitFor(() => expect(result.current.cursorStack).toEqual([null]));
    await waitFor(() =>
      expect(result.current.page?.data).toEqual([{ _id: "a" }]),
    );
  });

  it("does not advance past the last page", async () => {
    server.use(paginate());
    const { result } = renderHook(() =>
      useDocumentPage("demo", "users", NO_QUERY),
    );

    await waitFor(() =>
      expect(result.current.page?.data).toEqual([{ _id: "a" }]),
    );
    act(() => result.current.onNext());
    await waitFor(() =>
      expect(result.current.page?.data).toEqual([{ _id: "b" }]),
    );

    // Last page has next_cursor === null, so a further next is a no-op.
    act(() => result.current.onNext());
    expect(result.current.cursorStack).toEqual([null, "c1"]);
  });

  it("refresh refetches the current page", async () => {
    const seen: Body[] = [];
    server.use(paginate(seen));
    const { result } = renderHook(() =>
      useDocumentPage("demo", "users", NO_QUERY),
    );

    await waitFor(() => expect(result.current.page).not.toBeNull());
    expect(seen).toHaveLength(1);

    act(() => result.current.refresh());
    await waitFor(() => expect(seen).toHaveLength(2));
    expect(result.current.cursorStack).toEqual([null]);
  });

  it("keeps fetching from the same identity when the query object is rebuilt", async () => {
    const seen: Body[] = [];
    server.use(paginate(seen));
    // The route derives the query from URL search params, so a fresh object
    // arrives on every render. Only a structural change may refetch.
    const { result, rerender } = renderHook(() =>
      useDocumentPage("demo", "users", { filters: [], order: null }),
    );

    await waitFor(() => expect(seen).toHaveLength(1));
    rerender();
    rerender();
    await waitFor(() =>
      expect(result.current.page?.data).toEqual([{ _id: "a" }]),
    );
    expect(seen).toHaveLength(1);
  });

  // A cursor encodes the sort values it was produced under and is decoded
  // against the query it is replayed with, so replaying page two of an
  // unsorted scan against a newly sorted query is undecodable, not merely
  // stale. Every structural query change has to restart paging.
  it("resets the cursor stack when the sort changes", async () => {
    const seen: Body[] = [];
    server.use(paginate(seen));
    const order: DocumentOrder = { field: "name", direction: "asc" };
    const { result, rerender } = renderHook(
      ({ o }: { o: DocumentOrder | null }) =>
        useDocumentPage("demo", "users", { filters: [], order: o }),
      { initialProps: { o: null as DocumentOrder | null } },
    );

    await waitFor(() =>
      expect(result.current.page?.data).toEqual([{ _id: "a" }]),
    );
    act(() => result.current.onNext());
    await waitFor(() =>
      expect(result.current.cursorStack).toEqual([null, "c1"]),
    );

    rerender({ o: order });
    await waitFor(() => expect(result.current.cursorStack).toEqual([null]));
    // No request may carry the old cursor under the new sort.
    const sorted = seen.filter((b) => b.query.order !== null);
    expect(sorted.length).toBeGreaterThan(0);
    for (const body of sorted) expect(body.after).toBeNull();
    expect(sorted[0]?.query.order).toEqual(order);
  });

  it("resets the cursor stack when a filter changes", async () => {
    const seen: Body[] = [];
    server.use(paginate(seen));
    const filter: DocumentFilter = { field: "name", op: "eq", value: "ada" };
    const { result, rerender } = renderHook(
      ({ f }: { f: DocumentFilter[] }) =>
        useDocumentPage("demo", "users", { filters: f, order: null }),
      { initialProps: { f: [] as DocumentFilter[] } },
    );

    await waitFor(() =>
      expect(result.current.page?.data).toEqual([{ _id: "a" }]),
    );
    act(() => result.current.onNext());
    await waitFor(() =>
      expect(result.current.cursorStack).toEqual([null, "c1"]),
    );

    rerender({ f: [filter] });
    await waitFor(() => expect(result.current.cursorStack).toEqual([null]));
    const filtered = seen.filter((b) => b.query.filters.length > 0);
    expect(filtered.length).toBeGreaterThan(0);
    for (const body of filtered) expect(body.after).toBeNull();
    expect(filtered[0]?.query.filters).toEqual([filter]);
  });

  // One table's rows must never be readable under another table's header, and
  // must never seed the other table's discovered-column list.
  it("drops the current rows when the table changes", async () => {
    server.use(paginate());
    const { result, rerender } = renderHook(
      ({ t }: { t: string }) => useDocumentPage("demo", t, NO_QUERY),
      { initialProps: { t: "users" } },
    );

    await waitFor(() =>
      expect(result.current.page?.data).toEqual([{ _id: "a" }]),
    );
    rerender({ t: "messages" });
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
    const { result } = renderHook(() =>
      useDocumentPage("demo", "users", NO_QUERY),
    );

    await waitFor(() => expect(result.current.pageError).toBe("query failed"));
    expect(result.current.page).toBeNull();
    expect(result.current.loading).toBe(false);
  });
});
