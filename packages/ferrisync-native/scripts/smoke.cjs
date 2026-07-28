#!/usr/bin/env node
/**
 * Smoke / debug harness for ferrisync-native (.node).
 * Run: npm run build && node scripts/smoke.cjs
 */
const fs = require('fs');
const os = require('os');
const path = require('path');

function assert(cond, msg) {
  if (!cond) {
    console.error('FAIL:', msg);
    process.exit(1);
  }
  console.log('PASS:', msg);
}

function mkTree() {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'ferrisync-smoke-'));
  const src = path.join(root, 'src');
  const dst = path.join(root, 'dst');
  fs.mkdirSync(path.join(src, 'nested'), { recursive: true });
  fs.mkdirSync(dst, { recursive: true });
  fs.writeFileSync(path.join(src, 'nested', 'a.txt'), 'hello');
  fs.writeFileSync(path.join(src, 'b with spaces.txt'), 'world');
  return { root, src, dst };
}

function main() {
  let native;
  try {
    native = require('..');
  } catch (e) {
    console.error('FAIL: N1 load native module:', e.message);
    console.error('Hint: cd packages/ferrisync-native && npm run build');
    process.exit(1);
  }
  assert(typeof native.compare === 'function', 'N1 exports compare');
  assert(typeof native.sync === 'function', 'N1 exports sync');
  assert(typeof native.appDataPath === 'function', 'N4 appDataPath exists');

  const appData = native.appDataPath();
  assert(typeof appData === 'string' && appData.length > 0, `N4 appDataPath=${appData}`);

  let threw = false;
  try {
    native.compare(path.join(os.tmpdir(), 'ferrisync-no-such-src'), os.tmpdir());
  } catch (e) {
    threw = true;
    console.log('PASS: N2 compare rejects missing source →', String(e.message || e).slice(0, 120));
  }
  assert(threw, 'N2 compare should throw on missing source');

  const { root, src, dst } = mkTree();
  try {
    const before = native.compare(src, dst);
    assert(before.scanned === 2, `N3 pre-sync scanned=2 got ${before.scanned}`);
    assert(before.toCopy === 2, `N3 pre-sync toCopy=2 got ${before.toCopy}`);

    const stats = native.sync(src, dst);
    assert(stats.copied === 2, `N3 sync copied=2 got ${stats.copied}`);
    assert(stats.errors === 0, `N3 sync errors=0 got ${stats.errors}`);

    const after = native.compare(src, dst);
    assert(
      after.toCopy === 0,
      `N3 post-sync toCopy=0 got ${after.toCopy} rows=${JSON.stringify(after.rows)}`,
    );
    assert(
      fs.existsSync(path.join(dst, 'nested', 'a.txt')),
      'N3 dest nested/a.txt exists',
    );
  } finally {
    fs.rmSync(root, { recursive: true, force: true });
  }

  console.log('All native smoke checks passed.');
}

main();
