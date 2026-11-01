import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';

const read = file => readFileSync(new URL(`../${file}`, import.meta.url), 'utf8');
const first = (file, pattern) => {
  const match = pattern.exec(read(file));
  assert.ok(match, `${file}: version not found`);
  return match[1];
};

// The release process (CLAUDE.md, "Release") requires every file below to carry one version.
const versions = {
  'package.json': JSON.parse(read('package.json')).version,
  'PKGBUILD': first('PKGBUILD', /^pkgver=(\S+)$/m),
  'src-tauri/Cargo.toml': first('src-tauri/Cargo.toml', /^version = "([^"]+)"$/m),
  'src-tauri/Cargo.lock': first('src-tauri/Cargo.lock', /name = "anivox"\r?\nversion = "([^"]+)"/),
  'src-tauri/tauri.conf.json': JSON.parse(read('src-tauri/tauri.conf.json')).version,
  'ui/index.html': first('ui/index.html', /class="brand">v([^<]+)</),
  'ui/toolbar-fragment.html': first('ui/toolbar-fragment.html', /class="brand">v([^<]+)</),
};

test('application version is identical in every release file', () => {
  const expected = versions['package.json'];
  assert.match(expected, /^\d+\.\d+\.\d+$/);
  assert.deepEqual(versions, Object.fromEntries(Object.keys(versions).map(file => [file, expected])));
});
