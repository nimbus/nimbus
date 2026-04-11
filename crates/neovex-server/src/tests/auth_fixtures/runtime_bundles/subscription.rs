pub(crate) fn runtime_auth_subscription_bundle_source() -> &'static str {
    runtime_bundle_source!(
        r#"  ["auth:watchIdentity", {
    name: "auth:watchIdentity",
    kind: "query",
    visibility: "public",
    plan: null,
    runtime_handler: "async (ctx) => ({ identity: await ctx.auth.getUserIdentity(), messages: await ctx.db.query(\"messages\").take(1) })",
  }],"#
    )
}
