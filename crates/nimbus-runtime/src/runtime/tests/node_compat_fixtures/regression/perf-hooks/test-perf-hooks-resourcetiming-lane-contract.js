'use strict';

// Nimbus regression probe: the PerformanceResourceTiming contract that the
// official test-perf-hooks-resourcetiming.js fixture leaves unasserted, as
// observed on official Node v20.20.2, v22.23.1, v24.20.0 and v26.8.1.
// Node 22 added deliveryType, responseStatus, and the two trailing
// markResourceTiming parameters, and moved accessor receiver checks from
// ERR_INVALID_ARG_TYPE to ERR_INVALID_THIS.

require('../common');
const assert = require('assert');
const { performance, PerformanceResourceTiming } = require('perf_hooks');

const nodeMajor = Number(process.versions.node.split('.')[0]);
const hasDeliveryFields = nodeMajor >= 22;
const proto = PerformanceResourceTiming.prototype;

function timingInfo(overrides = {}) {
  return {
    startTime: 0,
    redirectStartTime: 0,
    redirectEndTime: 0,
    postRedirectStartTime: 0,
    finalServiceWorkerStartTime: 0,
    finalNetworkRequestStartTime: 0,
    finalNetworkResponseStartTime: 0,
    endTime: 0,
    encodedBodySize: 0,
    decodedBodySize: 0,
    finalConnectionTimingInfo: null,
    ...overrides,
  };
}

const enumerableMembers = [
  'initiatorType', 'workerStart', 'redirectStart', 'redirectEnd', 'fetchStart',
  'domainLookupStart', 'domainLookupEnd', 'connectStart', 'connectEnd',
  'secureConnectionStart', 'nextHopProtocol', 'requestStart', 'responseStart',
  'responseEnd', 'encodedBodySize', 'decodedBodySize', 'transferSize',
  ...(hasDeliveryFields ? ['deliveryType', 'responseStatus'] : []),
  'toJSON',
];
assert.deepStrictEqual(Object.keys(proto), enumerableMembers);
assert.strictEqual('deliveryType' in proto, hasDeliveryFields);
assert.strictEqual('responseStatus' in proto, hasDeliveryFields);
assert.strictEqual(performance.markResourceTiming.length, hasDeliveryFields ? 7 : 5);
assert.strictEqual(
  Object.getOwnPropertyDescriptor(performance, 'markResourceTiming')?.enumerable ??
    Object.getOwnPropertyDescriptor(Object.getPrototypeOf(performance), 'markResourceTiming')
      .enumerable,
  false,
);

assert.throws(() => new PerformanceResourceTiming(), { code: 'ERR_ILLEGAL_CONSTRUCTOR' });

for (const member of enumerableMembers) {
  const descriptor = Object.getOwnPropertyDescriptor(proto, member);
  const accessor = descriptor.get ?? descriptor.value;
  assert.throws(
    () => accessor.call({}),
    hasDeliveryFields ?
      {
        code: 'ERR_INVALID_THIS',
        name: 'TypeError',
        message: 'Value of "this" must be of type PerformanceResourceTiming',
      } :
      {
        code: 'ERR_INVALID_ARG_TYPE',
        name: 'TypeError',
        message: 'The "this" argument must be an instance of ' +
          'PerformanceResourceTiming. Received an instance of Object',
      },
    member,
  );
}

// Without a connection timing record, the connection members are undefined
// and transferSize adds the 300-byte header estimate to an empty body.
const bare = performance.markResourceTiming(
  timingInfo(), 'http://bare/', 'fetch', globalThis, '');
assert.strictEqual(bare.nextHopProtocol, undefined);
assert.strictEqual(bare.domainLookupStart, undefined);
assert.strictEqual(bare.connectEnd, undefined);
assert.strictEqual(bare.transferSize, 300);
assert.strictEqual(bare.responseStatus, undefined);
assert.strictEqual(bare.deliveryType, hasDeliveryFields ? '' : undefined);
const bareJson = {
  name: 'http://bare/',
  entryType: 'resource',
  startTime: 0,
  duration: 0,
  initiatorType: 'fetch',
  nextHopProtocol: undefined,
  workerStart: 0,
  redirectStart: 0,
  redirectEnd: 0,
  fetchStart: 0,
  domainLookupStart: undefined,
  domainLookupEnd: undefined,
  connectStart: undefined,
  connectEnd: undefined,
  secureConnectionStart: undefined,
  requestStart: 0,
  responseStart: 0,
  responseEnd: 0,
  transferSize: 300,
  encodedBodySize: 0,
  decodedBodySize: 0,
  ...(hasDeliveryFields ? { deliveryType: '', responseStatus: undefined } : {}),
};
assert.deepStrictEqual(bare.toJSON(), bareJson);

// The trailing arguments are recorded only where Node accepts them.
const extra = performance.markResourceTiming(
  timingInfo(), 'http://extra/', 'fetch', globalThis, '', {}, 204, 'cache');
assert.strictEqual(extra.responseStatus, hasDeliveryFields ? 204 : undefined);
assert.strictEqual(extra.deliveryType, hasDeliveryFields ? 'cache' : undefined);
assert.strictEqual('responseStatus' in extra.toJSON(), hasDeliveryFields);

// A local cache hit transfers nothing; duration is not clamped; the name and
// initiatorType are stored as given.
const local = performance.markResourceTiming(
  timingInfo({ startTime: 5, endTime: 3,
               finalConnectionTimingInfo: { ALPNNegotiatedProtocol: 'h2' } }),
  7, 3, globalThis, 'local');
assert.strictEqual(local.transferSize, 0);
assert.strictEqual(local.duration, -2);
assert.strictEqual(local.nextHopProtocol, 'h2');
assert.strictEqual(local.name, 7);
assert.strictEqual(local.initiatorType, 3);

for (const cacheMode of ['validated', undefined]) {
  assert.throws(
    () => performance.markResourceTiming(
      timingInfo(), 'http://bad/', 'fetch', globalThis, cacheMode),
    { code: 'ERR_INTERNAL_ASSERTION' });
}
assert.strictEqual(performance.getEntriesByName('http://bad/').length, 0);
