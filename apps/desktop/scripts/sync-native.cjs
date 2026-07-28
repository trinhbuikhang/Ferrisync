const fs = require('fs');
const path = require('path');

const srcDir = path.resolve(__dirname, '..', '..', '..', 'packages', 'ferrisync-native');
const destDir = path.resolve(__dirname, '..', 'native-binding');

const files = [
  'index.js',
  'index.d.ts',
  'ferrisync-native.win32-x64-msvc.node',
  'ferrisync-native.node',
];

fs.mkdirSync(destDir, { recursive: true });

let copied = 0;
for (const name of files) {
  const src = path.join(srcDir, name);
  if (!fs.existsSync(src)) continue;
  fs.copyFileSync(src, path.join(destDir, name));
  copied += 1;
}

if (copied === 0) {
  console.warn(
    '[sync-native] No .node artifacts found. Run: cd packages/ferrisync-native && npm run build',
  );
  process.exitCode = 1;
} else {
  console.log(`[sync-native] copied ${copied} file(s) → native-binding/`);
}
