// Flags: --experimental-stream-iter
import '../common/index.mjs';
import assert from 'node:assert';
import { builtinModules, createRequire, isBuiltin } from 'node:module';
import * as bareStreamIter from 'stream/iter';
import * as schemeStreamIter from 'node:stream/iter';
import * as bareZlibIter from 'zlib/iter';
import * as schemeZlibIter from 'node:zlib/iter';

// With --experimental-stream-iter, the bare specifiers name the same
// builtins as the `node:` specifiers, in ESM and in CommonJS.
const require = createRequire(import.meta.url);

assert.strictEqual(bareStreamIter, schemeStreamIter);
assert.strictEqual(bareZlibIter, schemeZlibIter);
assert.strictEqual(typeof schemeStreamIter.from, 'function');
assert.strictEqual(await import('stream/iter'), schemeStreamIter);
assert.strictEqual(await import('zlib/iter'), schemeZlibIter);

for (const id of ['stream/iter', 'zlib/iter']) {
  assert.strictEqual(import.meta.resolve(id), `node:${id}`);
  assert.strictEqual(import.meta.resolve(`node:${id}`), `node:${id}`);
  assert.strictEqual(require(id), require(`node:${id}`));
  assert.strictEqual(isBuiltin(id), true);
  assert.strictEqual(isBuiltin(`node:${id}`), true);
  assert(builtinModules.includes(id), id);
}
assert.strictEqual(schemeStreamIter.default, require('stream/iter'));
