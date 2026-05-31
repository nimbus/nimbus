'use strict';

require('../common');
const assert = require('assert');
const vm = require('vm');

const context = vm.createContext({});
let thrown;

try {
  vm.runInContext('throw new Error("shared")', context, 'shared-context.js');
} catch (err) {
  thrown = err;
}

assert.strictEqual(thrown.message, 'shared');
assert.match(thrown.stack, /shared-context\.js/);

const target = {};
const proxyContext = new Proxy(target, {});
assert.strictEqual(typeof vm.runInNewContext('String', proxyContext), 'function');

