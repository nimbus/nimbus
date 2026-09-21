'use strict';

// Nimbus regression probe: the runtime must not leave an async resource
// pending when user code starts. A hook that the main module enables must see
// `init` for every resource whose `before` or `after` it sees. Official Node
// v20, v22, v24 and v26 pass this check.
//
// Nimbus once opened and closed a UDP socket during module bootstrap. The
// native close completed at a later event-loop turn and queued a
// `socketCloseNT` tick, so init-hooks based fixtures intermittently failed
// with "Found a handle whose before hook was invoked but not its init hook".

require('../common');
const assert = require('assert');
const async_hooks = require('async_hooks');

const initialized = new Set();
const orphans = [];

const hook = async_hooks.createHook({
  init(asyncId) {
    initialized.add(asyncId);
  },
  before(asyncId) {
    if (!initialized.has(asyncId)) orphans.push(`before ${asyncId}`);
  },
  after(asyncId) {
    if (!initialized.has(asyncId)) orphans.push(`after ${asyncId}`);
  },
}).enable();

// Give every completion that was pending at startup time to settle.
setTimeout(() => {
  setTimeout(() => {
    hook.disable();
    assert.deepStrictEqual(orphans, []);
  }, 50);
}, 50);
