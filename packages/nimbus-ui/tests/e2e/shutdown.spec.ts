import { expect, test } from "./fixtures/nimbus-server";
import { existsSync } from "node:fs";

test.describe("POST /api/system/shutdown", () => {
  test("session-authenticated shutdown returns 200, child exits, discovery cleared", async ({
    request,
    nimbusServer,
  }) => {
    const token = nimbusServer.readToken();

    const authRes = await request.post(
      `${nimbusServer.baseURL}/ui/auth/session`,
      {
        data: { token },
        headers: {
          "Content-Type": "application/json",
          Accept: "application/json",
        },
      },
    );
    expect(authRes.status()).toBe(200);

    expect(existsSync(nimbusServer.discoveryPath)).toBe(true);

    const shutdownRes = await request.post(
      `${nimbusServer.baseURL}/api/system/shutdown`,
      {
        headers: { Accept: "application/json" },
      },
    );
    expect(shutdownRes.status()).toBe(200);
    expect(await shutdownRes.json()).toEqual({ accepted: true });

    const exited = await nimbusServer.waitForExit(5_000);
    expect(exited).toBe(true);

    expect(existsSync(nimbusServer.discoveryPath)).toBe(false);

    let connectionRefused = false;
    try {
      await fetch(`${nimbusServer.baseURL}/ui/`, {
        signal: AbortSignal.timeout(1_000),
      });
    } catch {
      connectionRefused = true;
    }
    expect(connectionRefused).toBe(true);
  });

  test("unauthenticated shutdown is rejected", async ({
    request,
    nimbusServer,
  }) => {
    const res = await request.post(
      `${nimbusServer.baseURL}/api/system/shutdown`,
      {
        headers: { Accept: "application/json" },
      },
    );
    expect([401, 403]).toContain(res.status());
    expect(nimbusServer.hasExited()).toBe(false);
  });
});
