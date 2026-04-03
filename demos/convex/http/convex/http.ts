import { httpRouter } from "convex/server";

import { api, internal } from "./_generated/api";
import { httpAction } from "./_generated/server";

const http = httpRouter();

http.route({
  path: "/messages",
  method: "POST",
  handler: httpAction(async (ctx, request) => {
    const { author, body } = await request.json();
    const id = await ctx.runMutation(internal.messages.sendInternal, { author, body });
    return Response.json({ id }, { status: 201 });
  }),
});

http.route({
  path: "/messages/by-author",
  method: "GET",
  handler: httpAction(async (ctx, request) => {
    const author = new URL(request.url).searchParams.get("author");
    return Response.json(await ctx.runQuery(api.messages.byAuthor, { author }));
  }),
});

export default http;
