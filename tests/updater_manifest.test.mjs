import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import os from 'node:os';
import { generateManifest } from '../scripts/generate-updater-manifest.mjs';

test('generateManifest correctly creates latest.json with signatures and asset urls', () => {
  const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), 'manifest-test-'));
  const inputDir = path.join(tmpDir, 'input');
  const outputDir = path.join(tmpDir, 'output');

  const winSubDir = path.join(inputDir, 'anivox-windows');
  const linuxSubDir = path.join(inputDir, 'anivox-linux');
  fs.mkdirSync(winSubDir, { recursive: true });
  fs.mkdirSync(linuxSubDir, { recursive: true });

  fs.writeFileSync(path.join(winSubDir, 'Anivox_0.1.3_x64-setup.exe'), 'mock binary');
  fs.writeFileSync(path.join(winSubDir, 'Anivox_0.1.3_x64-setup.exe.sig'), 'mock-windows-sig\n');

  fs.writeFileSync(path.join(linuxSubDir, 'Anivox_0.1.3_amd64.AppImage.tar.gz'), 'mock tar');
  fs.writeFileSync(path.join(linuxSubDir, 'Anivox_0.1.3_amd64.AppImage.tar.gz.sig'), 'mock-linux-sig\n');

  const manifest = generateManifest({
    inputDir,
    outputDir,
    tag: 'v0.1.3',
    repo: 'AXELL2022/Anivox-Wraper',
  });

  assert.equal(manifest.version, '0.1.3');
  assert.equal(manifest.platforms['windows-x86_64'].signature, 'mock-windows-sig');
  assert.equal(
    manifest.platforms['windows-x86_64'].url,
    'https://github.com/AXELL2022/Anivox-Wraper/releases/download/v0.1.3/Anivox_0.1.3_x64-setup.exe'
  );

  assert.equal(manifest.platforms['linux-x86_64'].signature, 'mock-linux-sig');
  assert.equal(
    manifest.platforms['linux-x86_64'].url,
    'https://github.com/AXELL2022/Anivox-Wraper/releases/download/v0.1.3/Anivox_0.1.3_amd64.AppImage.tar.gz'
  );

  const saved = JSON.parse(fs.readFileSync(path.join(outputDir, 'latest.json'), 'utf8'));
  assert.deepEqual(saved, manifest);

  fs.rmSync(tmpDir, { recursive: true, force: true });
});
