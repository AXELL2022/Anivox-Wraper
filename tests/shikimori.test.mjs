import { test } from 'node:test';
import assert from 'node:assert/strict';
import { existsSync, readFileSync } from 'node:fs';
import { runInNewContext } from 'node:vm';

function controls(path) {
  const handlers = new Map();
  const calls = [];
  const status = { textContent: '' };
  const buttons = ['back', 'forward', 'reload', 'close'].map(action => ({
    dataset: { action }, disabled: false,
    addEventListener: (event, handler) => handlers.set(action, handler),
  }));
  const file = new URL(path, import.meta.url);
  runInNewContext(existsSync(file) ? readFileSync(file, 'utf8') : '', {
    window: { top: null, __TAURI__: { core: { invoke: async (command, args) => {
      calls.push([command, args?.action]); return false;
    } } } },
    location: { origin: 'https://shikimori.io' },
    document: {
      querySelectorAll: () => buttons,
      querySelector: () => status,
      addEventListener: (event, handler) => handlers.set(event, handler),
    },
  });
  return { handlers, calls, buttons };
}

test('Overlay navigation and close affect only Shikimori', async () => {
  const { handlers, calls, buttons } = controls('../ui/shikimori.js');
  for (const action of ['back', 'forward', 'reload', 'close']) {
    assert.equal(typeof handlers.get(action), 'function');
    await handlers.get(action)();
  }
  assert.deepEqual(calls, [
    ['shikimori_action', 'back'], ['shikimori_action', 'forward'],
    ['shikimori_action', 'reload'], ['shikimori_action', 'close'],
  ]);
  assert.ok(buttons.every(button => !button.disabled));
});

test('Escape closes the overlay from its local controls', async () => {
  const { handlers, calls } = controls('../ui/shikimori.js');
  assert.equal(typeof handlers.get('keydown'), 'function');
  await handlers.get('keydown')({ key: 'Escape', preventDefault() {} });
  assert.deepEqual(calls, [['shikimori_action', 'close']]);
});

test('Social login pages have no Shikimori Escape handler or IPC calls', () => {
  const source = readFileSync(new URL('../src-tauri/scripts/shikimori.js', import.meta.url), 'utf8');
  const handlers = new Map();
  const window = { __TAURI__: { core: { invoke() { throw new Error('No IPC on provider pages'); } } } };
  window.top = window;
  runInNewContext(source, {
    window, location: { origin: 'https://passport.yandex.ru' },
    document: { addEventListener: (event, handler) => handlers.set(event, handler) },
  });
  assert.equal(handlers.has('keydown'), false);
});

test('Escape still closes Shikimori after returning from a social login page', async () => {
  const source = readFileSync(new URL('../src-tauri/scripts/shikimori.js', import.meta.url), 'utf8');
  const handlers = new Map();
  const calls = [];
  const window = { __TAURI__: { core: { invoke: async command => calls.push(command) } } };
  window.top = window;
  runInNewContext(source, {
    window, location: { origin: 'https://shikimori.io' },
    document: { addEventListener: (event, handler) => handlers.set(event, handler) },
  });
  await handlers.get('keydown')({ key: 'Escape', defaultPrevented: false, preventDefault() {} });
  assert.deepEqual(calls, ['close_shikimori']);
});
