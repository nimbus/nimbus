'use strict';
const common = require('../common');
const path = require('path');

const kNodeShared = Boolean(process.config.variables.node_shared);
const kShlibSuffix = process.config.variables.shlib_suffix;
const kExecPath = path.dirname(process.execPath);

function addLibraryPath(env) {
  if (!kNodeShared) {
    return;
  }

  env ||= process.env;

  env.LD_LIBRARY_PATH =
    (env.LD_LIBRARY_PATH ? env.LD_LIBRARY_PATH + path.delimiter : '') +
    kExecPath;
  env.LIBPATH =
    (env.LIBPATH ? env.LIBPATH + path.delimiter : '') +
    kExecPath;
  env.DYLD_LIBRARY_PATH =
    (env.DYLD_LIBRARY_PATH ? env.DYLD_LIBRARY_PATH + path.delimiter : '') +
    kExecPath;
  env.PATH = (env.PATH ? env.PATH + path.delimiter : '') + kExecPath;
}

function getSharedLibPath() {
  if (common.isWindows) {
    return path.join(kExecPath, 'node.dll');
  }
  return path.join(kExecPath, `libnode.${kShlibSuffix}`);
}

function getBinaryPath() {
  return kNodeShared ? getSharedLibPath() : process.execPath;
}

module.exports = {
  addLibraryPath,
  getBinaryPath,
  getSharedLibPath,
};
