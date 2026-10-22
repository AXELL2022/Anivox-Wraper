import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';

const read = (path) => readFileSync(new URL(`../${path}`, import.meta.url), 'utf8');

test('macOS config exposes only origin-scoped Discord IPC', () => {
  const config = JSON.parse(read('src-tauri/tauri.macos.conf.json'));
  assert.equal(config.app.withGlobalTauri, true);
  assert.equal(config.app.security.capabilities.length, 1);
  const capability = JSON.parse(read('src-tauri/capabilities/macos-discord.json'));
  assert.deepEqual(capability.permissions, ['allow-set-discord-rpc']);
  assert.deepEqual(capability.remote.urls, ['https://anivox.fun/*']);
  assert.deepEqual(capability.webviews, ['macos-main']);
  assert.deepEqual(config.bundle.targets, ['app', 'dmg']);
});

test('macOS uses a separate entry point and excludes legacy library', () => {
  assert.match(read('src-tauri/src/main.rs'), /mod macos/);
  assert.match(read('src-tauri/src/main.rs'), /macos::run\(\)/);
  assert.match(read('src-tauri/src/lib.rs'), /#!\[cfg\(not\(target_os = "macos"\)\)\]/);
  const cargo = read('src-tauri/Cargo.toml');
  const integrations = cargo.indexOf(`[target.'cfg(not(target_os = "macos"))'.dependencies]`);
  assert.ok(integrations > 0);
  assert.ok(cargo.indexOf('discord-rich-presence') < integrations);
  assert.ok(cargo.indexOf('tauri-plugin-opener') > integrations);
});

test('macOS backend excludes MPV and VPN and preserves reviewed updater', () => {
  const source = read('src-tauri/src/macos.rs');
  assert.doesNotMatch(source, /disconnect_vpn|open_in_mpv|inject-common\.js|inject-windows\.js|inject-linux\.js/);
  const injection = read('src-tauri/scripts/inject-macos.js');
  assert.doesNotMatch(injection, /open_in_mpv|toggle_vpn|XMLHttpRequest|window\.fetch\s*=/);
  assert.match(injection, /anivox-macos-controls/);
  assert.match(source, /tauri_plugin_updater/);
  assert.match(source, /macos-main/);
  assert.match(source, /if !confirm_install\(app, prompt\)\.await\?/);
  assert.match(source, /update\.version != reviewed\.version/);
  assert.match(source, /update\.download_url != reviewed\.download_url/);
  assert.match(source, /update\.signature != reviewed\.signature/);
  assert.ok(source.indexOf('if !confirm_install') < source.indexOf('.download('));
});

test('experimental CI only uploads artifacts with read-only repository access', () => {
  const workflow = read('.github/workflows/macos-minimal.yml');
  assert.match(workflow, /contents: read/);
  assert.match(workflow, /aarch64-apple-darwin/);
  assert.match(workflow, /x86_64-apple-darwin/);
  assert.match(workflow, /actions\/upload-artifact@/);
  assert.doesNotMatch(workflow, /action-gh-release|contents: write|git tag|gh release/);
});
