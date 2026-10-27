import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { runInNewContext } from 'node:vm';

const source = readFileSync(new URL('../ui/toolbar.js', import.meta.url), 'utf8');

async function toolbar(invoke) {
  const elements = new Map();
  const listeners = new Map();
  const timers = new Map();
  let timerId = 0;
  function element(selector) {
    if (!elements.has(selector)) elements.set(selector, {
      textContent: '', style: {}, disabled: false, attributes: {}, handlers: {},
      classList: { toggle() {} },
      setAttribute(name, value) { this.attributes[name] = value; },
      addEventListener(name, handler) { this.handlers[name] = handler; },
    });
    return elements.get(selector);
  }
  runInNewContext(source, {
    window: { __TAURI__: {
      core: { invoke },
      event: { listen: async (name, handler) => listeners.set(name, handler) },
    } },
    document: { querySelector: element, querySelectorAll: () => [] },
    setTimeout: (callback) => { timers.set(++timerId, callback); return timerId; },
    clearTimeout: id => timers.delete(id),
    setInterval() {},
  });
  await new Promise(resolve => setImmediate(resolve));
  return { element, listeners, timers };
}

test('Settings opens a local window and recovers after failure', async () => {
  let attempts = 0;
  const { element } = await toolbar(async command => {
    if (command === 'open_settings' && ++attempts === 1) throw new Error('Window failed');
    return false;
  });
  await element('#settings').handlers.click();
  assert.equal(element('#settings').disabled, false);
  assert.match(element('#status').textContent, /Window failed/);
  await element('#settings').handlers.click();
  assert.equal(attempts, 2);
});

test('MPV recovers when the site cannot respond and receives player feedback', async () => {
  const { element, listeners, timers } = await toolbar(async command => {
  });
  await element('#mpv').handlers.click();
  assert.equal(element('#mpv').disabled, true);
  [...timers.values()][0]();
  assert.equal(element('#mpv').disabled, false);
  assert.match(element('#status').textContent, /загрузите сайт/);
  listeners.get('player-status')({ payload: {
    text: '✔ Запущен!', color: '#69f0ae', resetAfterMs: 2500,
  } });
  assert.equal(element('#mpv').textContent, '✔ Запущен!');
  assert.equal(element('#mpv').disabled, false);
  listeners.get('player-status')({ payload: {
    text: '⚠ Ошибка!', color: '#ff5252', resetAfterMs: 3000,
    detail: 'Не удалось запустить MPV: executable missing. Выберите плеер в настройках.',
  } });
  assert.match(element('#status').textContent, /executable missing/);
});

test('Updater opens a review window without installing and retries opening errors', async () => {
  let opens = 0;
  let installs = 0;
  const { element, listeners } = await toolbar(async command => {
    if (command === 'install_update') installs++;
    if (command === 'open_update_review' && ++opens === 1) throw new Error('Window failed');
    return false;
  });
  listeners.get('update-available')({ payload: { version: 'v0.2.5', body: 'Changes' } });
  assert.equal(element('#updater').textContent, '✨ v0.2.5');
  await element('#updater').handlers.click();
  assert.match(element('#status').textContent, /Window failed/);
  assert.equal(element('#updater').disabled, false);
  await element('#updater').handlers.click();
  assert.equal(opens, 2);
  assert.equal(installs, 0);
});

test('Toolbar tracks confirmed download progress and restores latest badge on failure', async () => {
  const { element, listeners } = await toolbar(async () => false);
  listeners.get('update-available')({ payload: { version: '0.2.5' } });
  listeners.get('update-started')();
  listeners.get('update-progress')({ payload: { percent: 42 } });
  listeners.get('update-available')({ payload: { version: '0.2.6' } });
  assert.equal(element('#updater').textContent, 'Загрузка… 42%');
  listeners.get('update-finished')();
  assert.equal(element('#updater').textContent, 'Перезапуск…');
  listeners.get('update-idle')();
  assert.equal(element('#updater').textContent, '✨ v0.2.6');
  assert.equal(element('#updater').disabled, false);
  listeners.get('update-progress')({ payload: { percent: 99 } });
  assert.equal(element('#updater').textContent, '✨ v0.2.6');
});
test('Brand element displays application version from get_app_version', async () => {
  const { element } = await toolbar(async command => {
    if (command === 'get_app_version') return 'v0.2.4';
  });
  assert.equal(element('.brand').textContent, 'v0.2.4');
});

test('Shikimori toggles its overlay without navigating the main site', async () => {
  let visible = false;
  const actions = [];
  const { element } = await toolbar(async (command, args) => {
    if (command === 'shikimori_action') {
      if (args.action === 'toggle') { actions.push(args.action); visible = !visible; }
      return visible;
    }
    if (command === 'browser_action') throw new Error('Main site must stay loaded');
  });
  const button = element('#shikimori');
  assert.equal(typeof button.handlers.click, 'function');
  await button.handlers.click();
  assert.equal(button.attributes['aria-pressed'], 'true');
  await button.handlers.click();
  assert.equal(button.attributes['aria-pressed'], 'false');
  assert.equal(button.disabled, false);
  assert.deepEqual(actions, ['toggle', 'toggle']);
});

test('Shikimori can be retried after an opening failure', async () => {
  let attempts = 0;
  const { element } = await toolbar(async (command, args) => {
    if (command === 'shikimori_action' && args.action === 'toggle') {
      if (++attempts === 1) throw new Error('Webview failed');
      return true;
    }
    return false;
  });
  const button = element('#shikimori');
  assert.equal(typeof button.handlers.click, 'function');
  await button.handlers.click();
  assert.equal(button.disabled, false);
  assert.equal(button.attributes['aria-pressed'], 'false');
  assert.match(element('#status').textContent, /Webview failed/);
  await button.handlers.click();
  assert.equal(button.attributes['aria-pressed'], 'true');
});

test('Closing Shikimori from the overlay updates the toolbar button', async () => {
  const { element, listeners } = await toolbar(async command => command === 'shikimori_action');
  assert.equal(typeof listeners.get('shikimori-status'), 'function');
  listeners.get('shikimori-status')({ payload: { visible: false } });
  assert.equal(element('#shikimori').attributes['aria-pressed'], 'false');
});

