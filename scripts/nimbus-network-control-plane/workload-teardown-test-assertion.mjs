// NNCV035 reuses the NNCV034 attributed-test assertion contract. Keep one
// lexical implementation for Rust test attribution and meaningful assertions.

export {
  createAttributedTestChecker as createTeardownAttributedTestChecker,
  remaskTestSources as remaskTeardownTestSources,
} from "./workload-restart-test-assertion.mjs";
