import { z } from "zod";

globalThis.__nimbusInvoke = function () {
  const schema = z.object({
    id: z.string().uuid(),
    count: z.number().int().positive(),
    tags: z.array(z.string()).min(1),
  });
  const parsed = schema.parse({
    id: "6ba7b810-9dad-11d1-80b4-00c04fd430c8",
    count: 3,
    tags: ["nimbus"],
  });
  const failure = schema.safeParse({ id: "nope", count: 0, tags: [] });
  return {
    count: parsed.count,
    firstTag: parsed.tags[0],
    failure: failure.success,
    issueCount: failure.success ? 0 : failure.error.issues.length,
  };
};

export {};
