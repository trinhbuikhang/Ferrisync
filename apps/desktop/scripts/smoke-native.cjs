#!/usr/bin/env node
/**
 * Debug: ensure desktop native-binding matches packages/ferrisync-native and loads.
 * Run from apps/desktop: node scripts/smoke-native.cjs
 */
const fs = require('fs');
const path = require('path');
const { execFileSync } = require('child_process');

function assert(cond, msg) {
  if (!cond) {
    console.error('FAIL:', msg);
    process.exit(1);
  }
  console.log('PASS:', msg);
}

const desktopRoot = path.resolve(__dirname, '..');
const pkgNative = path.resolve(desktopRoot, '..', '..', 'packages', 'ferrisync-native');
const localNative = path.join(desktopRoot, 'native-binding');

const nodeName = 'ferrisync-native.win32-x64-msvc.node';
const pkgNode = path.join(pkgNative, nodeName);
const localNode = path.join(localNative, nodeName);

assert(fs.existsSync(pkgNode), `N5 package .node exists at ${pkgNode}`);

// Refresh copy used by Electron
execFileSync(process.execPath, [path.join(__dirname, 'sync-native.cjs')], {
  stdio: 'inherit',
  cwd: desktopRoot,
});

assert(fs.existsSync(localNode), `N5 desktop native-binding .node exists`);

const pkgStat = fs.statSync(pkgNode);
const localStat = fs.statSync(localNode);
assert(
  localStat.size === pkgStat.size,
  `N5 .node size match package=${pkgStat.size} local=${localStat.size}`,
);

let native;
try {
  native = require(localNative);
} catch (e) {
  console.error('FAIL: N1 require native-binding:', e.message);
  process.exit(1);
}

assert(typeof native.compare === 'function', 'N1 desktop binding exports compare');
assert(typeof native.sync === 'function', 'N1 desktop binding exports sync');
console.log('appDataPath=', native.appDataPath());
console.log('Desktop native smoke OK.');
