import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { HttpResponse, http } from "msw";
import { setupServer } from "msw/node";
import {
  afterAll,
  afterEach,
  beforeAll,
  describe,
  expect,
  it,
  vi,
} from "vitest";

import { SchemaPanel } from "./schema-panel";

vi.mock("sonner", () => ({ toast: { success: vi.fn(), error: vi.fn() } }));

const server = setupServer();

beforeAll(() => server.listen({ onUnhandledRequest: "error" }));
afterEach(() => server.resetHandlers());
afterAll(() => server.close());

describe("SchemaPanel", () => {
  // jsdom does not lay out, so this locks the constraint, not the width it
  // resolves to. `w-[420px]` is the panel's preferred width; with `shrink-0`
  // it was also its floor, so in a window under ~564px the row's
  // `overflow-hidden` cut off the panel's right edge — the close button
  // included — and left no way to dismiss it.
  it("treats 420px as a preferred width, not a floor", () => {
    render(
      <SchemaPanel
        tenant="demo"
        table="users"
        schema={null}
        onClose={() => {}}
        onSaved={() => {}}
      />,
    );
    const panel = screen.getByTestId("documents-schema-panel");
    expect(panel.className).not.toContain("shrink-0");
    // min-w-0 is what lets a flex item shrink below its min-content width.
    expect(panel.className).toContain("min-w-0");
  });

  // The other half of the same fix. Shrinking kept the panel closeable but not
  // usable: once the documents table took its own `min-w-[20rem]` floor, the
  // panel became the side that yields, and a browser measurement of the real
  // flex row put it at 6px wide on a 390px viewport (342px of row content
  // width, minus the table's 320px floor, minus the 16px `gap-4`). Full-width
  // until the row can afford 320 + 16 + 420 = 756px puts it under the table
  // instead: the same measurement reads 342px at 390px and 420px once the row
  // clears 756px.
  //
  // The threshold is a container query against the `documents-row` container
  // on the page section, not a viewport breakpoint, because the shell's
  // drawers take 80px to 480px between the viewport and this row -- `lg` was
  // the closest single number and still left the panel at 160px on a 1024px
  // viewport with both drawers open.
  //
  // happy-dom performs no layout, so the utility class is the only observable
  // here -- these assertions lock the constraint, not the width it resolves
  // to. The bare `w-[420px]` must be *absent* rather than merely outranked:
  // two conflicting width utilities on one element are settled by stylesheet
  // order, not by which one the class list mentions last.
  it("spans the row until it can afford 420px beside the table", () => {
    render(
      <SchemaPanel
        tenant="demo"
        table="users"
        schema={null}
        onClose={() => {}}
        onSaved={() => {}}
      />,
    );
    const classes = screen
      .getByTestId("documents-schema-panel")
      .className.split(" ");
    expect(classes).toContain("w-full");
    expect(classes).toContain("@min-[756px]/documents-row:w-[420px]");
    expect(classes).not.toContain("w-[420px]");
    // The container name is a cross-file contract: this query is only ever
    // measured against the section in storage_.$table.tsx. A viewport variant
    // here would silently reintroduce the drawer blind spot.
    expect(classes).not.toContain("lg:w-[420px]");
  });

  it("PUTs the edited schema through the schema client on save", async () => {
    let put: unknown = null;
    server.use(
      http.put(
        "*/api/tenants/:t/schema/:table",
        async ({ request, params }) => {
          put = await request.json();
          expect(params.t).toBe("demo");
          expect(params.table).toBe("users");
          return HttpResponse.json({ ok: true });
        },
      ),
    );
    const onSaved = vi.fn();
    render(
      <SchemaPanel
        tenant="demo"
        table="users"
        schema={null}
        onClose={() => {}}
        onSaved={onSaved}
      />,
    );

    // The empty-schema initial draft "{\n  \n}" parses to {}.
    fireEvent.click(screen.getByTestId("documents-schema-save"));
    await waitFor(() => expect(onSaved).toHaveBeenCalled());
    expect(put).toEqual({});
  });

  it("shows a parse error and does not call the client for invalid JSON", async () => {
    let called = false;
    server.use(
      http.put("*/api/tenants/:t/schema/:table", () => {
        called = true;
        return HttpResponse.json({ ok: true });
      }),
    );
    render(
      <SchemaPanel
        tenant="demo"
        table="users"
        schema={null}
        onClose={() => {}}
        onSaved={vi.fn()}
      />,
    );

    fireEvent.change(screen.getByTestId("documents-schema-textarea"), {
      target: { value: "{ not json" },
    });
    fireEvent.click(screen.getByTestId("documents-schema-save"));
    await waitFor(() =>
      expect(screen.getByTestId("documents-schema-error")).toBeInTheDocument(),
    );
    expect(called).toBe(false);
  });

  it("drops the schema only after the confirmation is accepted", async () => {
    let dropped = false;
    server.use(
      http.delete("*/api/tenants/:t/schema/:table", () => {
        dropped = true;
        return HttpResponse.json({ ok: true });
      }),
    );
    const onSaved = vi.fn();
    render(
      <SchemaPanel
        tenant="demo"
        table="users"
        schema={{ table: "users", fields: [] }}
        onClose={() => {}}
        onSaved={onSaved}
      />,
    );

    // Opening the drawer does not delete anything yet.
    fireEvent.click(screen.getByTestId("documents-schema-drop"));
    expect(
      screen.getByTestId("documents-drop-schema-dialog"),
    ).toBeInTheDocument();
    expect(dropped).toBe(false);

    fireEvent.click(screen.getByTestId("documents-drop-schema-dialog-confirm"));
    await waitFor(() => expect(dropped).toBe(true));
    await waitFor(() => expect(onSaved).toHaveBeenCalled());
  });

  it("disables drop when the table has no schema", () => {
    render(
      <SchemaPanel
        tenant="demo"
        table="users"
        schema={null}
        onClose={() => {}}
        onSaved={vi.fn()}
      />,
    );
    expect(screen.getByTestId("documents-schema-drop")).toBeDisabled();
  });
});
