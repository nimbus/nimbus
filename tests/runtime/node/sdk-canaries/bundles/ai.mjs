import { jsonSchema, stepCountIs, tool } from "ai";
import { z } from "zod";

globalThis.__nimbusInvoke = async function () {
  const schema = jsonSchema({
    type: "object",
    properties: {
      ok: { type: "boolean" },
    },
    required: ["ok"],
    additionalProperties: false,
  });
  const canaryTool = tool({
    description: "Nimbus AI SDK canary tool",
    inputSchema: z.object({ value: z.string() }),
    execute: async ({ value }) => `ai:${value}`,
  });
  const stopWhen = stepCountIs(1);
  const toolResult = await canaryTool.execute({ value: "ok" }, {});

  return {
    schemaType: schema.jsonSchema.type,
    toolResult,
    hasStopPredicate: typeof stopWhen === "function",
  };
};

export {};
