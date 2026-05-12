'use strict';
const assert = require('assert');
const Module = require('module');

const patchedWrapper = { ...Module.wrapper };
Module.wrapper = patchedWrapper;

assert.strictEqual(Module.wrapper, patchedWrapper);
