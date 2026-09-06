import {
  type QueryEntry,
  type QueryReference,
  queryEntry,
} from "@nimbus/nimbus/browser";
import { describe, expectTypeOf, it } from "vitest";

import { api } from "../../convex/_generated/api";

// Producer-side wrapper contract — verifies that `queryEntry(ref, args)`
// preserves both TArgs and TReturn at the type level so call-site
// mismatches are caught at compile time (no `as unknown as` escape).

describe("queryEntry wrapper preserves producer-side types", () => {
  it("infers TArgs from the QueryReference and constrains args at the call site", () => {
    const entry = queryEntry(api.machines.list, {
      state: null,
      provider: null,
      limit: 200,
    });

    // ref keeps the original QueryReference type (TArgs + TReturn both preserved).
    expectTypeOf(entry.ref).toEqualTypeOf<typeof api.machines.list>();

    // args is typed to the exact shape declared on api.machines.list.
    expectTypeOf(entry.args).toMatchTypeOf<{
      state: string | null;
      provider: string | null;
      limit: number | null;
    }>();

    // Returned shape is the named QueryEntry generic.
    expectTypeOf(entry).toMatchTypeOf<
      QueryEntry<
        {
          state: string | null;
          provider: string | null;
          limit: number | null;
        },
        readonly unknown[]
      >
    >();
  });

  it("preserves TReturn through the wrapper", () => {
    const entry = queryEntry(api.tables.list, {
      tenantId: null,
      limit: 200,
    });

    // The Doc<"tables">[] return type from codegen survives the wrapper —
    // toMatchTypeOf allows readonly arrays and structural compat.
    type Ref = typeof entry.ref;
    expectTypeOf<Ref>().toMatchTypeOf<
      QueryReference<{ tenantId: string | null; limit: number | null }, unknown>
    >();
  });

  it("rejects an arg shape that does not match the QueryReference", () => {
    // @ts-expect-error — `routes:list` takes { adapter, limit }, not { wrongKey }
    queryEntry(api.routes.list, { wrongKey: "nope" });
  });

  it("rejects a missing required arg field", () => {
    // @ts-expect-error — `machines:list` requires all three fields
    queryEntry(api.machines.list, { state: null });
  });
});
