/**
 * Ensure Electron's binary exists.
 * Needed when npm blocks postinstall scripts (allowScripts).
 */
const { downloadArtifact } = require('@electron/get');
const extract = require('extract-zip');
const fs = require('fs');
const path = require('path');

const electronRoot = path.dirname(require.resolve('electron/package.json'));
const { version } = require(path.join(electronRoot, 'package.json'));
const distDir = path.join(electronRoot, 'dist');
const exeName = process.platform === 'win32' ? 'electron.exe' : 'electron';
const exePath = path.join(distDir, exeName);
const pathTxt = path.join(electronRoot, 'path.txt');
const versionFile = path.join(distDir, 'version');

async function main() {
  if (fs.existsSync(exePath)) {
    fs.writeFileSync(pathTxt, exeName);
    if (!fs.existsSync(versionFile)) {
      fs.writeFileSync(versionFile, version);
    }
    console.log(`[ensure-electron] ok → ${exePath}`);
    return;
  }

  console.log(`[ensure-electron] downloading Electron v${version}…`);
  const zipPath = await downloadArtifact({
    version,
    artifactName: 'electron',
    platform: process.platform,
    arch: process.arch,
  });
  console.log(`[ensure-electron] zip → ${zipPath}`);

  fs.rmSync(distDir, { recursive: true, force: true });
  fs.mkdirSync(distDir, { recursive: true });
  await extract(zipPath, { dir: distDir });

  if (!fs.existsSync(exePath)) {
    throw new Error(`Electron extract finished but missing: ${exePath}`);
  }

  fs.writeFileSync(pathTxt, exeName);
  fs.writeFileSync(versionFile, version);
  console.log(`[ensure-electron] installed → ${exePath}`);
}

main().catch((err) => {
  console.error('[ensure-electron] failed:', err);
  process.exit(1);
});
