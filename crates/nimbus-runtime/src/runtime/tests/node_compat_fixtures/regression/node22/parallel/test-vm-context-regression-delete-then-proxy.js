'use strict';

require('../common');
const assert = require('assert');
const vm = require('vm');

let context = {};
Object.defineProperty(context, 'b', { configurable: false });
context = vm.createContext(context);
assert.strictEqual(vm.createScript('delete b').runInContext(context), false);

const target = {};
const proxyContext = new Proxy(target, {});
vm.runInNewContext('createdThroughProxy = true', proxyContext);
assert.strictEqual(target.createdThroughProxy, true);

