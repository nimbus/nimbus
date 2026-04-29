const bootstrap = globalThis.__bootstrap ?? Object.create(null);
const denoGlobals = bootstrap.ext_node_denoGlobals ?? Object.create(null);
const nodeGlobals = bootstrap.ext_node_nodeGlobals ?? Object.create(null);
const publicDenoPrototype = globalThis.Deno ?? null;

export { denoGlobals, nodeGlobals, publicDenoPrototype };
