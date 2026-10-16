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

test('VPN can connect and retry navigation without any loaded remote document', async () => {
  let active = false;
  const actions = [];
  const { element } = await toolbar(async (command, args) => {
    if (command === 'get_vpn_status') return active;
    if (command === 'toggle_vpn') { active = args.enable; return active; }
    if (command === 'browser_action') actions.push(args.action);
  });
  await element('#vpn').handlers.click();
  assert.equal(element('#vpn').attributes['aria-pressed'], 'true');
  assert.equal(element('#vpn').disabled, false);
  assert.deepEqual(actions, ['reload']);
});

test('VPN failure restores the actual state and leaves the control retryable', async () => {
  let attempts = 0;
  const { element } = await toolbar(async command => {
    if (command === 'get_vpn_status') return false;
    if (command === 'toggle_vpn') { attempts++; throw new Error('access denied'); }
    throw new Error('Site must not reload after failed VPN connection');
  });
  await element('#vpn').handlers.click();
  assert.equal(element('#vpn').disabled, false);
  assert.equal(element('#vpn').attributes['aria-pressed'], 'false');
  assert.match(element('#status').textContent, /access denied/);
  await element('#vpn').handlers.click();
  assert.equal(attempts, 2);
});

test('MPV recovers when the site cannot respond and receives player feedback', async () => {
  const { element, listeners, timers } = await toolbar(async command => {
    if (command === 'get_vpn_status') return false;
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
});

test('Updater shows badge on available update and displays progress on click', async () => {
  let installCalled = false;
  const { element, listeners } = await toolbar(async command => {
    if (command === 'get_vpn_status') return false;
    if (command === 'check_update') return { available: false };
    if (command === 'install_update') { installCalled = true; return new Promise(() => {}); }
  });

  assert.equal(element('#updater').style.display, undefined);
  listeners.get('update-available')({ payload: { version: '0.2.2', body: 'New features' } });
  assert.equal(element('#updater').style.display, 'inline-flex');
  assert.equal(element('#updater').textContent, '✨ v0.2.2');

  listeners.get('update-available')({ payload: { version: 'v0.2.3', body: 'Bug fixes' } });
  assert.equal(element('#updater').textContent, '✨ v0.2.3');

  element('#updater').handlers.click();
  assert.equal(installCalled, true);
  assert.equal(element('#updater').disabled, true);

  listeners.get('update-progress')({ payload: { percent: 42 } });
  assert.equal(element('#updater').textContent, 'Загрузка… 42%');

  listeners.get('update-finished')();
  assert.equal(element('#updater').textContent, 'Перезапуск…');
});

for (const result of ['release_page', 'no_update', 'error']) {
  test(`Updater restores its button when installation returns ${result}`, async () => {
    let attempts = 0;
    const { element, listeners } = await toolbar(async command => {
      if (command === 'get_vpn_status') return false;
      if (command === 'install_update') {
        attempts++;
        if (result === 'error') throw new Error('Download failed');
        return result;
      }
    });
    listeners.get('update-available')({ payload: { version: '0.2.3', body: 'Fixes' } });
    await element('#updater').handlers.click();
    assert.equal(element('#updater').disabled, false);
    assert.equal(element('#updater').textContent, '✨ v0.2.3');
    assert.doesNotMatch(element('#status').textContent, /Загрузка обновления…/);
    await element('#updater').handlers.click();
    assert.equal(attempts, 2);
  });
}

test('Updater keeps progress during a repeated availability event and restores the latest badge', async () => {
  let finish;
  let attempts = 0;
  const { element, listeners } = await toolbar(async command => {
    if (command === 'get_vpn_status') return false;
    if (command === 'install_update') {
      attempts++;
      return await new Promise(resolve => { finish = resolve; });
    }
  });
  listeners.get('update-available')({ payload: { version: '0.2.3' } });
  const click = element('#updater').handlers.click();
  await element('#updater').handlers.click();
  assert.equal(attempts, 1);
  listeners.get('update-progress')({ payload: { percent: 42 } });
  listeners.get('update-available')({ payload: { version: '0.2.4' } });
  assert.equal(element('#updater').textContent, 'Загрузка… 42%');
  finish('no_update');
  await click;
  assert.equal(element('#updater').disabled, false);
  assert.equal(element('#updater').textContent, '✨ v0.2.4');
  listeners.get('update-progress')({ payload: { percent: 99 } });
  listeners.get('update-finished')();
  assert.equal(element('#updater').textContent, '✨ v0.2.4');
});

test('Brand element displays application version from get_app_version', async () => {
  const { element } = await toolbar(async command => {
    if (command === 'get_vpn_status') return false;
    if (command === 'get_app_version') return 'v0.2.2';
  });
  assert.equal(element('.brand').textContent, 'v0.2.2');
});

test('Shikimori toggles its overlay without navigating the main site', async () => {
  let visible = false;
  const actions = [];
  const { element } = await toolbar(async (command, args) => {
    if (command === 'get_vpn_status') return false;
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
    if (command === 'get_vpn_status') return false;
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

