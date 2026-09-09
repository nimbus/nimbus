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

import { SchemaTab } from "./schema-tab";

vi.mock("sonner", () => ({ toast: { success: vi.fn(), error: vi.fn() } }));

const server = setupServer();

beforeAll(() => server.listen({ onUnhandledRequest: "error" }));
afterEach(() => server.resetHandlers());
afterAll(() => server.close());

const USERS_SCHEMA = {
  table: "users",
  fields: [{ name: "email", field_type: "string", required: true }],
  indexes: [{ name: "by_email", fields: ["email"] }],
};

function serveApply(
  respond: (body: unknown, dryRun: boolean) => Record<string, unknown>,
) {
  const calls: { body: unknown; dryRun: boolean }[] = [];
  server.use(
    http.post(
      "*/api/tenants/:t/schema/:table/apply",
      async ({ request, params }) => {
        expect(params.t).toBe("demo");
        expect(params.table).toBe("users");
        const dryRun =
          new URL(request.url).searchParams.get("dry_run") === "true";
        const body = await request.json();
        calls.push({ body, dryRun });
        return HttpResponse.json(respond(body, dryRun));
      },
    ),
  );
  return calls;
}

const applied = (scanned: number) => ({
  applied: true,
  dry_run: false,
  scanned,
  violation_count: 0,
  violations: [],
});

describe("SchemaTab", () => {
  it("POSTs the draft to the apply route and refetches when it lands", async () => {
    const calls = serveApply(() => applied(3));
    const onSaved = vi.fn();
    render(
      <SchemaTab
        tenant="demo"
        table="users"
        schema={USERS_SCHEMA}
        onSaved={onSaved}
      />,
    );

    fireEvent.click(screen.getByTestId("documents-schema-apply"));
    await waitFor(() => expect(onSaved).toHaveBeenCalled());
    expect(calls).toHaveLength(1);
    expect(calls[0]?.dryRun).toBe(false);
    // The draft is the committed schema without the server-owned fields.
    expect(calls[0]?.body).toEqual(USERS_SCHEMA);
    expect(screen.getByTestId("documents-schema-report")).toHaveAttribute(
      "data-outcome",
      "applied",
    );
    expect(screen.getByTestId("documents-schema-report")).toHaveTextContent(
      "3 documents",
    );
  });

  // The whole point of the apply route: a schema the table already violates
  // is refused, and the operator sees which documents to fix.
  it("lists the violating documents and does not refetch when apply is refused", async () => {
    serveApply(() => ({
      applied: false,
      dry_run: false,
      scanned: 120,
      violation_count: 52,
      violations: Array.from({ length: 50 }, (_, i) => ({
        id: `doc_${i}`,
        message: "missing required field: email",
      })),
    }));
    const onSaved = vi.fn();
    render(
      <SchemaTab
        tenant="demo"
        table="users"
        schema={USERS_SCHEMA}
        onSaved={onSaved}
      />,
    );

    fireEvent.click(screen.getByTestId("documents-schema-apply"));
    const report = await screen.findByTestId("documents-schema-report");
    expect(report).toHaveAttribute("data-outcome", "refused");
    expect(report).toHaveTextContent("52 documents of 120 scanned");
    expect(
      screen.getByTestId("documents-schema-violation-doc_0"),
    ).toHaveTextContent("missing required field: email");
    expect(screen.getByTestId("documents-schema-violations")).toHaveTextContent(
      "and 2 more",
    );
    expect(onSaved).not.toHaveBeenCalled();
  });

  it("checks with dry_run=true and reports without applying", async () => {
    const calls = serveApply((_body, dryRun) => ({
      applied: false,
      dry_run: dryRun,
      scanned: 7,
      violation_count: 0,
      violations: [],
    }));
    const onSaved = vi.fn();
    render(
      <SchemaTab
        tenant="demo"
        table="users"
        schema={USERS_SCHEMA}
        onSaved={onSaved}
      />,
    );

    fireEvent.click(screen.getByTestId("documents-schema-check"));
    const report = await screen.findByTestId("documents-schema-report");
    expect(calls[0]?.dryRun).toBe(true);
    expect(report).toHaveAttribute("data-outcome", "checked");
    expect(report).toHaveTextContent("Check passed");
    expect(onSaved).not.toHaveBeenCalled();
  });

  it("names the field at fault and never calls the server for a bad draft", async () => {
    const calls = serveApply(() => applied(0));
    render(
      <SchemaTab tenant="demo" table="users" schema={null} onSaved={vi.fn()} />,
    );

    fireEvent.change(screen.getByTestId("documents-schema-textarea"), {
      target: { value: "{ not json" },
    });
    fireEvent.click(screen.getByTestId("documents-schema-apply"));
    expect(
      await screen.findByTestId("documents-schema-error"),
    ).toHaveTextContent("Invalid JSON");

    fireEvent.change(screen.getByTestId("documents-schema-textarea"), {
      target: {
        value: JSON.stringify({
          table: "users",
          fields: [{ name: "age", field_type: "integer" }],
        }),
      },
    });
    fireEvent.click(screen.getByTestId("documents-schema-check"));
    expect(
      await screen.findByTestId("documents-schema-error"),
    ).toHaveTextContent("age");
    expect(calls).toHaveLength(0);
  });

  it("drops the schema only after the confirmation is accepted", async () => {
    let dropped = false;
    server.use(
      http.delete("*/api/tenants/:t/schema/:table", () => {
        dropped = true;
        return new HttpResponse(null, { status: 204 });
      }),
    );
    const onSaved = vi.fn();
    render(
      <SchemaTab
        tenant="demo"
        table="users"
        schema={{ table: "users", fields: [] }}
        onSaved={onSaved}
      />,
    );

    // Opening the dialog does not delete anything yet.
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
      <SchemaTab tenant="demo" table="users" schema={null} onSaved={vi.fn()} />,
    );
    expect(screen.getByTestId("documents-schema-drop")).toBeDisabled();
  });

  it("starts from the current schema without server-owned index fields", () => {
    render(
      <SchemaTab
        tenant="demo"
        table="users"
        schema={{
          table: "users",
          fields: [{ name: "email" }],
          indexes: [
            {
              id: "idx_1",
              name: "by_email",
              fields: ["email"],
              state: "enabled",
            },
          ],
        }}
        onSaved={vi.fn()}
      />,
    );
    const textarea = screen.getByTestId(
      "documents-schema-textarea",
    ) as HTMLTextAreaElement;
    expect(textarea.value).toContain('"email"');
    expect(textarea.value).toContain('"by_email"');
    expect(textarea.value).not.toContain("idx_1");
    expect(textarea.value).not.toContain("enabled");
  });
});
