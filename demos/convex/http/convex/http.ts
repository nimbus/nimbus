import { httpRouter } from "convex/server";

import { api } from "./_generated/api";
import { httpAction } from "./_generated/server";
import { sendViaHttp } from "./messages";

const http = httpRouter();

http.route({
  path: "/messages",
  method: "POST",
  handler: sendViaHttp,
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
