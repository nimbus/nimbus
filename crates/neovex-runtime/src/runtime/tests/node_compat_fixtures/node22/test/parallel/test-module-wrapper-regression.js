'use strict';
require('../common');
const assert = require('assert');
const Module = require('module');

const patchedWrapper = { ...Module.wrapper };
patchedWrapper[0] += 'global.__neovexModuleWrapperRegression = 1;';
Module.wrapper = patchedWrapper;

const wrapped = Module.wrap('');
assert.match(wrapped, /__neovexModuleWrapperRegression = 1/);

