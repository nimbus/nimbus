'use strict';
const common = require('../common');
const async_hooks = require('async_hooks');

Promise.resolve(1).then(common.mustCall(() => {
  const hook = async_hooks.createHook({
    init: common.mustCall(),
    before: common.mustCallAtLeast(),
    after: common.mustCallAtLeast(2)
  });
  hook.enable();

  process.nextTick(common.mustCall(() => {
    hook.disable();
  }, 1));
}));
