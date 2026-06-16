'use strict';

require('../common');
const assert = require('assert');
const vm = require('vm');

const first = vm.createContext({ label: 'first' });
const second = vm.createContext({ label: 'second' });

vm.runInContext('value = label + "-context"', first);
vm.runInContext('value = label + "-context"', second);

assert.strictEqual(first.value, 'first-context');
assert.strictEqual(second.value, 'second-context');

const target = {};
const proxyContext = new Proxy(target, {});
assert.strictEqual(typeof vm.runInNewContext('String', proxyContext), 'function');
vm.runInNewContext('value = "proxy-context"', proxyContext);
assert.strictEqual(target.value, 'proxy-context');

