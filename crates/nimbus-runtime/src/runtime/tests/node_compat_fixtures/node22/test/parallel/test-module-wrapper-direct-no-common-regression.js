'use strict';
const assert = require('assert');
const Module = require('module');

global.__nimbusDirectWrapperCounter = 0;

const originalWrapper = Module.wrapper;
const patchedWrapper = { ...Module.wrapper };
patchedWrapper[0] +=
  'global.__nimbusDirectWrapperCounter = ' +
  '(global.__nimbusDirectWrapperCounter || 0) + 1;';

Module.wrapper = patchedWrapper;
require('../fixtures/not-main-module.js');

assert.strictEqual(global.__nimbusDirectWrapperCounter, 1);

Module.wrapper = originalWrapper;
delete require.cache[require.resolve('../fixtures/not-main-module.js')];
delete global.__nimbusDirectWrapperCounter;
