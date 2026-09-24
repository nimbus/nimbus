import '../common/index.mjs';
import assert from 'node:assert';
import { builtinModules, createRequire, isBuiltin } from 'node:module';

// Without --experimental-stream-iter, a bare specifier does not name the
// hidden builtins: it resolves as a package. The `node:` specifier names
// them, but loading them fails.
const require = createRequire(import.meta.url);

for (const id of ['stream/iter', 'zlib/iter']) {
  const packageName = id.split('/')[0];
  const packageNotFound = {
    code: 'ERR_MODULE_NOT_FOUND',
    message: new RegExp(`^Cannot find package '${packageName}' imported from `),
  };
  await assert.rejects(import(id), packageNotFound);
  assert.throws(() => import.meta.resolve(id), packageNotFound);
  assert.throws(() => require(id), {
    code: 'MODULE_NOT_FOUND',
    message: new RegExp(`^Cannot find module '${id}'`),
  });

  const unknownBuiltin = {
    code: 'ERR_UNKNOWN_BUILTIN_MODULE',
    message: `No such built-in module: node:${id}`,
  };
  assert.strictEqual(import.meta.resolve(`node:${id}`), `node:${id}`);
  await assert.rejects(import(`node:${id}`), unknownBuiltin);
  assert.throws(() => require(`node:${id}`), unknownBuiltin);

  assert.strictEqual(isBuiltin(id), false);
  assert.strictEqual(isBuiltin(`node:${id}`), false);
  assert(!builtinModules.includes(id), id);
}
