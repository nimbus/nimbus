// Portable base64 codec used wherever the firebase package crosses the
// Firestore REST/JSON boundary (bytes values, transaction tokens). It prefers
// Node's Buffer when present and falls back to the browser btoa/atob globals so
// the same SDK build works in both runtimes.

export function encodeBase64(bytes: Uint8Array): string {
  const bufferCtor = (globalThis as {
    Buffer?: {
      from(bytes: Uint8Array): {
        toString(encoding: "base64"): string;
      };
    };
  }).Buffer;
  if (bufferCtor) {
    return bufferCtor.from(bytes).toString("base64");
  }
  const encode = globalThis.btoa;
  if (typeof encode !== "function") {
    throw new Error("No base64 encoder is available in this runtime.");
  }
  let binary = "";
  for (const byte of bytes) {
    binary += String.fromCharCode(byte);
  }
  return encode(binary);
}

export function decodeBase64(value: string): Uint8Array {
  const bufferCtor = (globalThis as {
    Buffer?: {
      from(value: string, encoding: "base64"): Uint8Array;
    };
  }).Buffer;
  if (bufferCtor) {
    return new Uint8Array(bufferCtor.from(value, "base64"));
  }
  const decode = globalThis.atob;
  if (typeof decode !== "function") {
    throw new Error("No base64 decoder is available in this runtime.");
  }
  const binary = decode(value);
  return Uint8Array.from(binary, (character) => character.charCodeAt(0));
}
