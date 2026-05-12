'use strict';

const common = require('../common');
const async_hooks = require('async_hooks');
const assert = require('assert');
const fs = require('fs');

let outerInitCount = 0;
let nestedInitCount = 0;

function maybeStopHooks() {
  if (outerInitCount === 2 && nestedInitCount === 1) {
    nestedHook.disable();
    outerHook.disable();
  }
}

const nestedHook = async_hooks.createHook({
  init() {
    nestedInitCount++;
    maybeStopHooks();
  }
});

const outerHook = async_hooks.createHook({
  init() {
    outerInitCount++;
    nestedHook.enable();
    maybeStopHooks();
  }
}).enable();

fs.access(__filename, common.mustCall(() => {
  fs.access(__filename, common.mustCall(() => {
    assert.strictEqual(outerInitCount, 2);
    assert.strictEqual(nestedInitCount, 1);
  }));
}));
