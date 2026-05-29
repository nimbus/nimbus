import { SignJWT, jwtVerify } from "jose";

globalThis.__nimbusInvoke = async function () {
  const secret = new TextEncoder().encode("nimbus-secret");
  const token = await new SignJWT({ role: "admin" })
    .setProtectedHeader({ alg: "HS256" })
    .setSubject("user_123")
    .sign(secret);
  const verified = await jwtVerify(token, secret);
  return {
    subject: verified.payload.sub,
    role: verified.payload.role,
    headerAlg: verified.protectedHeader.alg,
    tokenParts: token.split(".").length,
  };
};

export {};
