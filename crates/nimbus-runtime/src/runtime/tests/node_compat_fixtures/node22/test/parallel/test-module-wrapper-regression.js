'use strict';
const assert = require('assert');
const Module = require('module');
const path = require('path');

global.__nimbusModuleWrapperRegressionCounter = 0;

const originalWrapper = Module.wrapper;
const patchedWrapper = { ...Module.wrapper };
patchedWrapper[0] +=
  'global.__nimbusModuleWrapperRegressionCounter = ' +
  '(global.__nimbusModuleWrapperRegressionCounter || 0) + 1;';

Module.wrapper = patchedWrapper;

const probe = new Module(
  path.join(process.cwd(), 'nimbus-wrapper-regression-probe.js'),
  module,
);
probe.filename = probe.id;
probe.paths = Module._nodeModulePaths(process.cwd());
probe._compile(
  'module.exports = global.__nimbusModuleWrapperRegressionCounter;',
  probe.filename,
);

assert.strictEqual(probe.exports, 1);

Module.wrapper = originalWrapper;
delete global.__nimbusModuleWrapperRegressionCounter;
