// Flags: --expose-gc
'use strict';
const common = require('../common');
const { onGC } = require('../common/gc');
const assert = require('assert');
const zlib = require('zlib');

const ongc = common.mustCall();

{
  const input = Buffer.from('foobar');
  const strm = zlib.createInflate();
  strm.end(input);
  strm.once('error', common.mustCall(function(err) {
    assert(err);
    setImmediate(() => {
      globalThis.gc();
      // Keep the event loop alive for seeing the async_hooks destroy hook
      // we use for GC tracking...
      // TODO(addaleax): This should maybe not be necessary?
      setImmediate(() => {});
    });
  }));
  onGC(strm, { ongc });
}
