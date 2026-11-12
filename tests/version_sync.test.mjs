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
  // Experimental native GTK shell: its interface shows CARGO_PKG_VERSION.
  'linux-gtk/Cargo.toml': first('linux-gtk/Cargo.toml', /^version = "([^"]+)"$/m),
  'linux-gtk/Cargo.lock': first('linux-gtk/Cargo.lock', /name = "anivox-gtk"\r?\nversion = "([^"]+)"/),
  'linux-gtk/arch/PKGBUILD': first('linux-gtk/arch/PKGBUILD', /^pkgver=(\S+)$/m),
  // Experimental WinUI 3 shell and its Rust core (anivox_ffi.dll).
  'windows-winui/Anivox.WinUI.csproj': first('windows-winui/Anivox.WinUI.csproj', /<Version>([^<]+)<\/Version>/),
  'windows-winui/native/Cargo.toml': first('windows-winui/native/Cargo.toml', /^version = "([^"]+)"$/m),
  'windows-winui/native/Cargo.lock': first('windows-winui/native/Cargo.lock', /name = "anivox-ffi"\r?\nversion = "([^"]+)"/),
};

test('application version is identical in every release file', () => {
  const expected = versions['package.json'];
  assert.match(expected, /^\d+\.\d+\.\d+$/);
  assert.deepEqual(versions, Object.fromEntries(Object.keys(versions).map(file => [file, expected])));
});
