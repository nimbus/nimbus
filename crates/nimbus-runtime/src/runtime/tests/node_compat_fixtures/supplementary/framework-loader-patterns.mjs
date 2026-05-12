import assert from 'node:assert';
import fs from 'node:fs';
import { createRequire } from 'node:module';

const require = createRequire(import.meta.url);
const fixtureSpecifier = './fixtures/framework-loader-patterns/message.fixture';
const packageSpecifier = './fixtures/framework-loader-patterns/package-entry';
const previousFixtureLoader = require.extensions['.fixture'];

try {
  require.extensions['.fixture'] = (module, filename) => {
    module.exports = {
      filename,
      payload: fs.readFileSync(filename, 'utf8').trim(),
    };
  };

  const loaded = require(fixtureSpecifier);
  assert.strictEqual(loaded.payload, 'custom-loader-payload');
  assert.ok(loaded.filename.endsWith('message.fixture'));

  const resolved = require.resolve(fixtureSpecifier);
  assert.strictEqual(require.cache[resolved].exports.payload, 'custom-loader-payload');
} finally {
  if (previousFixtureLoader === undefined) {
    delete require.extensions['.fixture'];
  } else {
    require.extensions['.fixture'] = previousFixtureLoader;
  }
}

assert.deepStrictEqual(require(packageSpecifier), {
  mode: 'package-main-resolution',
  dirnameBasename: 'package-entry',
});
