// Convex compatibility surface for value validators. The validators are
// identical to the Nimbus implementation, so re-export the canonical module
// rather than forking it. Add or change validators in `@nimbus/nimbus/values`.
export type { GenericId, Infer, Validator } from "@nimbus/nimbus/values";
export { v } from "@nimbus/nimbus/values";
