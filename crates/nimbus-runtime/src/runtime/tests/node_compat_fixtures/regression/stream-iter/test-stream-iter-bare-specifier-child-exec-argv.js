'use strict';
const assert = require('assert');

// A child process reads its own options, not the parent's. This parent runs
// without --experimental-stream-iter, so only a child that gets the flag can
// resolve the bare specifier.

// The child needs nothing from ../common, so it does not load it.
if (process.argv[2] === 'child') {
  const enabled = process.argv[3] === 'on';
  const { isBuiltin } = require('module');
  assert.strictEqual(isBuiltin('stream/iter'), enabled);
  if (enabled) {
    assert.strictEqual(require('stream/iter'), require('node:stream/iter'));
  } else {
    assert.throws(() => require('stream/iter'), { code: 'MODULE_NOT_FOUND' });
  }
  import('stream/iter').then(
    () => assert(enabled, 'bare import resolved without the flag'),
    (error) => {
      assert(!enabled, error);
      assert.strictEqual(error.code, 'ERR_MODULE_NOT_FOUND');
    },
  );
  return;
}

require('../common');
const { spawnSync } = require('child_process');

assert.strictEqual(require('module').isBuiltin('stream/iter'), false);
for (const [execArgv, state] of [
  [['--experimental-stream-iter'], 'on'],
  [[], 'off'],
]) {
  const child = spawnSync(
    process.execPath,
    [...execArgv, __filename, 'child', state],
    { encoding: 'utf8' },
  );
  assert.strictEqual(child.status, 0, `${state}: ${child.stderr}`);
}
