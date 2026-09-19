'use strict';

// Nimbus regression probe: handle close callbacks follow the libuv and
// MakeCallback contract. Official Node v20, v22, v24 and v26 pass this check.
//
// 1. Node runs each close callback through MakeCallback, which drains the
//    nextTick queue and then the microtask queue when the callback returns.
// 2. uv_close() frees a UDP fd synchronously, so the port is free as soon as
//    close() returns.
// 3. libuv keeps the loop alive while a handle closes, so a close that starts
//    from a 'close' listener completes.

const common = require('../common');
const assert = require('assert');
const dgram = require('dgram');
const net = require('net');

function closeListenerDrainsTicksBeforeMicrotasks(next) {
  const order = [];
  const server = net.createServer((conn) => conn.destroy());
  server.listen(0, common.mustCall(() => {
    const client = net.connect(server.address().port, '127.0.0.1',
                               common.mustCall(() => client.destroy()));
    client.on('close', common.mustCall(() => {
      Promise.resolve().then(() => order.push('microtask'));
      process.nextTick(() => order.push('tick'));
      setTimeout(common.mustCall(() => {
        assert.deepStrictEqual(order, ['tick', 'microtask']);
        server.close(common.mustCall(next));
      }), 0);
    }));
  }));
}

function udpCloseReleasesThePort(next) {
  const first = dgram.createSocket('udp4');
  first.bind(0, '127.0.0.1', common.mustCall(() => {
    const { port } = first.address();
    first.close();
    const second = dgram.createSocket('udp4');
    second.bind(port, '127.0.0.1', common.mustCall(() => {
      second.close(common.mustCall(next));
    }));
  }));
}

function closeFromCloseListenerCompletes() {
  const first = dgram.createSocket('udp4');
  first.close(common.mustCall(() => {
    const second = dgram.createSocket('udp4');
    second.close(common.mustCall());
  }));
}

closeListenerDrainsTicksBeforeMicrotasks(common.mustCall(() => {
  udpCloseReleasesThePort(common.mustCall(closeFromCloseListenerCompletes));
}));
