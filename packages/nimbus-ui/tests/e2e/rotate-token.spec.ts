import { expect, test } from "./fixtures/nimbus-server";

test.describe("POST /api/system/token/rotate", () => {
  test("rotation increments generation, invalidates old token, and accepts the new one", async ({
    request,
    nimbusServer,
  }) => {
    const initial = nimbusServer.readTokenRecord();
    expect(initial.token).toMatch(/^nimbus_at_/);

    const initialAuth = await request.post(
      `${nimbusServer.baseURL}/ui/auth/session`,
      {
        data: { token: initial.token },
        headers: {
          "Content-Type": "application/json",
          Accept: "application/json",
        },
      },
    );
    expect(initialAuth.status()).toBe(200);
    expect(await initialAuth.json()).toEqual({ ok: true });

    const rotateRes = await request.post(
      `${nimbusServer.baseURL}/api/system/token/rotate`,
      {
        headers: {
          Authorization: `Bearer ${initial.token}`,
          Accept: "application/json",
        },
      },
    );
    expect(rotateRes.status()).toBe(200);
    const rotateBody = await rotateRes.json();
    expect(rotateBody).toEqual({ generation: initial.generation + 1 });
    expect(Object.keys(rotateBody).sort()).toEqual(["generation"]);
    expect(rotateBody).not.toHaveProperty("token");

    const rotated = nimbusServer.readTokenRecord();
    expect(rotated.generation).toBe(initial.generation + 1);
    expect(rotated.token).not.toBe(initial.token);
    expect(rotated.token).toMatch(/^nimbus_at_/);

    const oldAuth = await request.post(
      `${nimbusServer.baseURL}/ui/auth/session`,
      {
        data: { token: initial.token },
        headers: {
          "Content-Type": "application/json",
          Accept: "application/json",
        },
      },
    );
    expect(oldAuth.status()).toBe(401);

    const newAuth = await request.post(
      `${nimbusServer.baseURL}/ui/auth/session`,
      {
        data: { token: rotated.token },
        headers: {
          "Content-Type": "application/json",
          Accept: "application/json",
        },
      },
    );
    expect(newAuth.status()).toBe(200);
    expect(await newAuth.json()).toEqual({ ok: true });
  });

  test("rotation rejects callers that omit or forge the bearer", async ({
    request,
    nimbusServer,
  }) => {
    const noBearer = await request.post(
      `${nimbusServer.baseURL}/api/system/token/rotate`,
      {
        headers: { Accept: "application/json" },
      },
    );
    expect(noBearer.status()).toBe(401);

    const forged = await request.post(
      `${nimbusServer.baseURL}/api/system/token/rotate`,
      {
        headers: {
          Authorization: "Bearer nimbus_at_not_a_real_token",
          Accept: "application/json",
        },
      },
    );
    expect(forged.status()).toBe(401);

    const stillValid = nimbusServer.readTokenRecord();
    expect(stillValid.generation).toBe(1);
  });
});
