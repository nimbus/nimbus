'use strict';

const { spawnSync } = require('child_process');
const fs = require('fs');
const path = require('path');
const { pathToFileURL } = require('url');
const { isMainThread } = require('worker_threads');

const isUnixLike = process.platform !== 'win32';
let escapePOSIXShell;

function safeEnv(name) {
  try {
    return process.env?.[name];
  } catch (error) {
    if (String(error?.message ?? '').includes('runtime env capability denied')) {
      return undefined;
    }
    throw error;
  }
}

function rmSync(pathname, useSpawn) {
  if (useSpawn) {
    if (isUnixLike) {
      escapePOSIXShell ??= require('./index.js').escapePOSIXShell;
      for (let i = 0; i < 3; i += 1) {
        const { status } = spawnSync(...escapePOSIXShell`rm -rf "${pathname}"`);
        if (status === 0) {
          break;
        }
      }
    } else {
      spawnSync(process.execPath, [
        '-e',
        `fs.rmSync(${JSON.stringify(pathname)}, { maxRetries: 3, recursive: true, force: true });`,
      ]);
    }
  } else {
    fs.rmSync(pathname, { maxRetries: 3, recursive: true, force: true });
  }
}

const nodeTestDir = safeEnv('NODE_TEST_DIR');
const testRoot = nodeTestDir ?
  fs.realpathSync(nodeTestDir) :
  path.resolve(__dirname, '..');

const tmpdirName = `.tmp.${safeEnv('TEST_SERIAL_ID') || safeEnv('TEST_THREAD_ID') || '0'}`;
const tmpPath = path.join(testRoot, tmpdirName);

let firstRefresh = true;

function refresh(useSpawn = false) {
  rmSync(tmpPath, useSpawn);
  fs.mkdirSync(tmpPath);

  if (firstRefresh) {
    firstRefresh = false;
    process.on('exit', () => onexit(useSpawn));
  }
}

function onexit(useSpawn) {
  if (isMainThread) {
    process.chdir(testRoot);
  }

  try {
    rmSync(tmpPath, useSpawn);
  } catch (error) {
    console.error("Can't clean tmpdir:", tmpPath);

    const files = fs.readdirSync(tmpPath);
    console.error('Files blocking:', files);

    if (files.some((file) => file.startsWith('.nfs'))) {
      console.error('Note: ".nfs*" might be files that were open and unlinked but not closed.');
      console.error('See http://nfs.sourceforge.net/#faq_d2 for details.');
    }

    console.error();
    throw error;
  }
}

function resolve(...paths) {
  return path.resolve(tmpPath, ...paths);
}

function hasEnoughSpace(size) {
  const { bavail, bsize } = fs.statfsSync(tmpPath);
  return bavail >= Math.ceil(size / bsize);
}

function fileURL(...paths) {
  const fullPath = path.resolve(tmpPath + path.sep, ...paths);
  return pathToFileURL(fullPath);
}

module.exports = {
  fileURL,
  hasEnoughSpace,
  path: tmpPath,
  refresh,
  resolve,
};
