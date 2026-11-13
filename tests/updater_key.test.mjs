import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';

const read = path => readFileSync(new URL(`../${path}`, import.meta.url), 'utf8');

test('native shells verify downloads with the release key the Tauri updater uses', () => {
  const tauri = JSON.parse(read('src-tauri/tauri.conf.json')).plugins.updater.pubkey;
  const core = read('src-tauri/anivox-core/src/updater.rs').match(/pub const UPDATE_PUBKEY: &str =\s*"([^"]+)"/);
  assert.ok(core, 'UPDATE_PUBKEY not found');
  assert.equal(core[1], tauri);
});
