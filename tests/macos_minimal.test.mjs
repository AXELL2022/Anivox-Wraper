import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';

const read = (path) => readFileSync(new URL(`../${path}`, import.meta.url), 'utf8');

test('macOS config disables remote bridge and capabilities', () => {
  const config = JSON.parse(read('src-tauri/tauri.macos.conf.json'));
  assert.equal(config.app.withGlobalTauri, false);
  assert.equal(config.app.security.capabilities.length, 1);
  assert.deepEqual(config.app.security.capabilities[0].permissions, []);
  assert.deepEqual(config.bundle.targets, ['app', 'dmg']);
});

test('macOS uses a separate entry point and excludes legacy library', () => {
  assert.match(read('src-tauri/src/main.rs'), /mod macos/);
  assert.match(read('src-tauri/src/main.rs'), /macos::run\(\)/);
  assert.match(read('src-tauri/src/lib.rs'), /#!\[cfg\(not\(target_os = "macos"\)\)\]/);
  const cargo = read('src-tauri/Cargo.toml');
  const integrations = cargo.indexOf(`[target.'cfg(not(target_os = "macos"))'.dependencies]`);
  assert.ok(integrations > 0);
  assert.ok(cargo.indexOf('discord-rich-presence') > integrations);
  assert.ok(cargo.indexOf('tauri-plugin-opener') > integrations);
});

test('macOS backend has no legacy integration or remote command registration', () => {
  const source = read('src-tauri/src/macos.rs');
  assert.doesNotMatch(source, /generate_handler!|discord_rich_presence|disconnect_vpn|open_in_mpv|include_str!/);
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
