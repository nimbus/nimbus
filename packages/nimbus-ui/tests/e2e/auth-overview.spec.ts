import { expect, test } from "@playwright/test";
import { readFileSync } from "node:fs";
import { homedir } from "node:os";
import { join } from "node:path";

const TOKEN_PATH =
  process.env.NIMBUS_E2E_TOKEN_PATH ??
  join(homedir(), "Library/Application Support/nimbus/auth/token");

function readToken(): string {
  const raw = readFileSync(TOKEN_PATH, "utf8").trim();
  if (raw.startsWith("{")) {
    const parsed = JSON.parse(raw) as { token?: string };
    if (!parsed.token) {
      throw new Error(`token file ${TOKEN_PATH} has no .token field`);
    }
    return parsed.token;
  }
  return raw;
}

test.describe("auth -> overview", () => {
  test("auth page renders the local admin token form", async ({ page }) => {
    await page.goto("/ui/auth");
    await expect(page.getByRole("heading", { name: "Nimbus" })).toBeVisible();
    await expect(page.getByLabel(/admin token/i)).toBeVisible();
  });

  test("POST /ui/auth/session with a valid token returns 200 ok:true", async ({
    request,
    baseURL,
  }) => {
    const token = readToken();
    const res = await request.post(`${baseURL}/ui/auth/session`, {
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
    baseURL,
  }) => {
    const res = await request.get(`${baseURL}/ui/`);
    expect(res.status()).toBe(200);
    const csp = res.headers()["content-security-policy"] ?? "";
    expect(csp).toContain("script-src 'self'");
  });

  test("unauthenticated /ui/ falls back to the sign-in form, not the SPA shell", async ({
    request,
    baseURL,
  }) => {
    const res = await request.get(`${baseURL}/ui/`);
    expect(res.status()).toBe(200);
    const body = await res.text();
    expect(body).toContain("Nimbus Sign In");
    expect(body).not.toContain("<script");
  });
});
