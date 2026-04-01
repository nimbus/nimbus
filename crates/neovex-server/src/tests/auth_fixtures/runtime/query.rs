pub(crate) fn runtime_auth_bundle_source() -> &'static str {
    runtime_bundle_source!(
        r#"  ["auth:whoami", {
    name: "auth:whoami",
    kind: "query",
    visibility: "public",
    plan: null,
    runtime_handler: "async (ctx) => await ctx.auth.getUserIdentity()",
  }],"#
    )
}

pub(crate) fn runtime_verified_auth_bundle_source() -> &'static str {
    runtime_bundle_source!(
        r#"  ["auth:whoami", {
    name: "auth:whoami",
    kind: "query",
    visibility: "public",
    plan: null,
    runtime_handler: "async (ctx) => ({ user: await ctx.auth.getUserIdentity(), verified: await ctx.auth.getVerifiedIdentity() })",
  }],"#
    )
}
