import { expect, test } from "./fixtures/nimbus-server";

test.describe("auth -> overview", () => {
  test("auth page renders the local admin token form", async ({ page }) => {
    await page.goto("/ui/auth");
    await expect(page.getByRole("heading", { name: "Nimbus" })).toBeVisible();
    await expect(page.getByLabel(/admin token/i)).toBeVisible();
  });

  test("POST /ui/auth/session with a valid token returns 200 ok:true", async ({
    request,
    nimbusServer,
  }) => {
    const token = nimbusServer.readToken();
    const res = await request.post(`${nimbusServer.baseURL}/ui/auth/session`, {
      data: { token },
      headers: {
        "Content-Type": "application/json",
        Accept: "application/json",
      },
    });
    expect(res.status()).toBe(200);
    expect(await res.json()).toEqual({ ok: true });
  });

  test("static UI shell serves a CSP-compliant /ui/ index", async ({
    request,
    nimbusServer,
  }) => {
    const res = await request.get(`${nimbusServer.baseURL}/ui/`);
    expect(res.status()).toBe(200);
    const csp = res.headers()["content-security-policy"] ?? "";
    expect(csp).toContain("script-src 'self'");
  });

  test("unauthenticated /ui/ falls back to the sign-in form, not the SPA shell", async ({
    request,
    nimbusServer,
  }) => {
    const res = await request.get(`${nimbusServer.baseURL}/ui/`);
    expect(res.status()).toBe(200);
    const body = await res.text();
    expect(body).toContain("Nimbus Sign In");
    expect(body).not.toContain("<script");
  });
});
