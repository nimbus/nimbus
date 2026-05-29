import { customAlphabet } from "nanoid";

globalThis.__nimbusInvoke = function () {
  const id = customAlphabet("n", 8)();
  return {
    id,
    length: id.length,
  };
};

export {};
