import { act, renderHook, waitFor } from "@testing-library/react";
import { HttpResponse, http } from "msw";
import { setupServer } from "msw/node";
import { afterAll, afterEach, beforeAll, describe, expect, it } from "vitest";

import { useTableDocuments } from "./use-table-documents";

const server = setupServer();

beforeAll(() => server.listen({ onUnhandledRequest: "error" }));
afterEach(() => server.resetHandlers());
afterAll(() => server.close());

// A two-page fixture keyed on the `after` cursor sent in the request body.
function paginate(counter?: { n: number }) {
  return http.post(
    "*/api/tenants/:t/query/paginated",
    async ({ request }) => {
      if (counter) counter.n += 1;
      const body = (await request.json()) as { after: string | null };
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

describe("useTableDocuments", () => {
  it("loads the first page and seeds the cursor stack", async () => {
    server.use(paginate());
    const { result } = renderHook(() => useTableDocuments("demo", "users"));

    await waitFor(() => expect(result.current.page).not.toBeNull());
    expect(result.current.page?.data).toEqual([{ _id: "a" }]);
    expect(result.current.page?.has_more).toBe(true);
    expect(result.current.cursorStack).toEqual([null]);
    expect(result.current.pageError).toBeNull();
  });

  it("pushes a cursor on next and pops it on prev", async () => {
    server.use(paginate());
    const { result } = renderHook(() => useTableDocuments("demo", "users"));

    await waitFor(() => expect(result.current.page?.data).toEqual([{ _id: "a" }]));

    act(() => result.current.onNext());
    await waitFor(() =>
      expect(result.current.cursorStack).toEqual([null, "c1"]),
    );
    await waitFor(() => expect(result.current.page?.data).toEqual([{ _id: "b" }]));
    expect(result.current.page?.has_more).toBe(false);

    act(() => result.current.onPrev());
    await waitFor(() => expect(result.current.cursorStack).toEqual([null]));
    await waitFor(() => expect(result.current.page?.data).toEqual([{ _id: "a" }]));
  });

  it("does not advance past the last page", async () => {
    server.use(paginate());
    const { result } = renderHook(() => useTableDocuments("demo", "users"));

    await waitFor(() => expect(result.current.page?.data).toEqual([{ _id: "a" }]));
    act(() => result.current.onNext());
    await waitFor(() => expect(result.current.page?.data).toEqual([{ _id: "b" }]));

    // Last page has next_cursor === null, so a further next is a no-op.
    act(() => result.current.onNext());
    expect(result.current.cursorStack).toEqual([null, "c1"]);
  });

  it("refresh refetches the current page", async () => {
    const counter = { n: 0 };
    server.use(paginate(counter));
    const { result } = renderHook(() => useTableDocuments("demo", "users"));

    await waitFor(() => expect(result.current.page).not.toBeNull());
    expect(counter.n).toBe(1);

    act(() => result.current.refresh());
    await waitFor(() => expect(counter.n).toBe(2));
    expect(result.current.cursorStack).toEqual([null]);
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
    const { result } = renderHook(() => useTableDocuments("demo", "users"));

    await waitFor(() =>
      expect(result.current.pageError).toBe("query failed"),
    );
    expect(result.current.page).toBeNull();
    expect(result.current.loading).toBe(false);
  });
});
