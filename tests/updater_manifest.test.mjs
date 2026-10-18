import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import os from 'node:os';
import { generateManifest, extractReleaseNotes } from '../scripts/generate-updater-manifest.mjs';

test('generateManifest correctly creates latest.json with signatures and asset urls', () => {
  const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), 'manifest-test-'));
  const inputDir = path.join(tmpDir, 'input');
  const outputDir = path.join(tmpDir, 'output');
  const notesPath = path.join(tmpDir, 'release-notes.md');
  const notes = '## [0.2.4] — Изменения\n* Исправлена работа MPV.\n';
  fs.writeFileSync(notesPath, notes);

  const winSubDir = path.join(inputDir, 'anivox-windows');
  const linuxSubDir = path.join(inputDir, 'anivox-linux');
  fs.mkdirSync(winSubDir, { recursive: true });
  fs.mkdirSync(linuxSubDir, { recursive: true });

  fs.writeFileSync(path.join(winSubDir, 'Anivox_0.2.4_x64-setup.exe'), 'mock binary');
  fs.writeFileSync(path.join(winSubDir, 'Anivox_0.2.4_x64-setup.exe.sig'), 'mock-windows-sig\n');

  fs.writeFileSync(path.join(linuxSubDir, 'Anivox_0.2.4_amd64.AppImage.tar.gz'), 'mock tar');
  fs.writeFileSync(path.join(linuxSubDir, 'Anivox_0.2.4_amd64.AppImage.tar.gz.sig'), 'mock-linux-sig\n');

  const manifest = generateManifest({
    inputDir,
    outputDir,
    tag: 'v0.2.4',
    repo: 'AXELL2022/Anivox-Wraper',
    notesPath,
  });

  assert.equal(manifest.version, '0.2.4');
  assert.equal(manifest.notes, notes);
  assert.equal(manifest.platforms['windows-x86_64'].signature, 'mock-windows-sig');
  assert.equal(
    manifest.platforms['windows-x86_64'].url,
    'https://github.com/AXELL2022/Anivox-Wraper/releases/download/v0.2.4/Anivox_0.2.4_x64-setup.exe'
  );

  assert.equal(manifest.platforms['linux-x86_64'].signature, 'mock-linux-sig');
  assert.equal(
    manifest.platforms['linux-x86_64'].url,
    'https://github.com/AXELL2022/Anivox-Wraper/releases/download/v0.2.4/Anivox_0.2.4_amd64.AppImage.tar.gz'
  );

  const saved = JSON.parse(fs.readFileSync(path.join(outputDir, 'latest.json'), 'utf8'));
  assert.deepEqual(saved, manifest);

  fs.rmSync(tmpDir, { recursive: true, force: true });
});

test('Release notes select exact version and normalize CRLF without adjacent releases', () => {
  const changelog = '# History\r\n## [0.2.40] — Other\r\nWrong\r\n## [0.2.4] — Changes\r\n* Реальные изменения\r\n\r\n---\r\n\r\n## [0.2.3]\r\nOld\r\n';
  assert.equal(extractReleaseNotes(changelog, 'v0.2.4'), '## [0.2.4] — Changes\n* Реальные изменения');
  assert.throws(() => extractReleaseNotes(changelog, 'v9.0.0'), /Missing/);
  assert.throws(() => extractReleaseNotes('## [0.2.4]\n\n---\n', 'v0.2.4'), /Empty/);
});

test('Manifest rejects blank or missing release notes before creating artifacts', () => {
  const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'manifest-notes-'));
  try {
    const notesPath = path.join(tmp, 'notes.md');
    const options = { notesPath, inputDir: path.join(tmp, 'input'), outputDir: path.join(tmp, 'output') };
    assert.throws(() => generateManifest(options), /ENOENT/);
    fs.writeFileSync(notesPath, ' \n');
    assert.throws(() => generateManifest(options), /Empty/);
    assert.equal(fs.existsSync(options.outputDir), false);
  } finally { fs.rmSync(tmp, { recursive: true, force: true }); }
});
