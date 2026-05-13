
import Module from "node:module";

const internalReadlineUtils = Module._load("internal/readline/utils", null, false);
if (!internalReadlineUtils || typeof internalReadlineUtils !== "object") {
  throw new Error(
    "Nimbus Node22 bootstrap expected node:module to expose internal/readline/utils",
  );
}

const {
  CSI,
  charLengthAt,
  charLengthLeft,
  commonPrefix,
  emitKeys,
  kSubstringSearch,
} = internalReadlineUtils;

export {
  CSI,
  charLengthAt,
  charLengthLeft,
  commonPrefix,
  emitKeys,
  kSubstringSearch,
};
export default internalReadlineUtils;
