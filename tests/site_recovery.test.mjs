import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { runInNewContext } from 'node:vm';
import { localWindow } from './local_window_harness.mjs';

const file = '../ui/site-recovery.js';
const state = (phase, detail = null, generation = 1) => ({ phase, detail, generation });
const tick = () => new Promise(resolve => setImmediate(resolve));
const deferred = () => {
  let resolve, reject;
  const promise = new Promise((done, fail) => { resolve = done; reject = fail; });
  return { promise, resolve, reject };
};
const emit = (app, payload) => app.listeners.get('site-load-state')({ payload });

for (const [phase, expected] of [
  ['loading', 'Загрузка Anivox…'],
  ['timeout', 'Сайт не ответил вовремя'],
  ['error', 'Не удалось загрузить Anivox'],
  ['ready', 'Anivox готов'],
]) {
  test(`Initial ${phase} snapshot renders Russian copy`, async () => {
    const app = await localWindow(file, async command => {
      assert.equal(command, 'get_site_load_state');
      return state(phase);
    });
    assert.equal(app.element('#title').textContent, expected);
    assert.equal(app.element('#spinner').hidden, phase !== 'loading');
    assert.equal(app.element('#detail').hidden, true);
    if (phase === 'timeout') {
      assert.match(app.element('#description').textContent, /медленно/);
      assert.doesNotMatch(app.element('#description').textContent, /нет интернета|нет подключения/i);
    }
  });
}

test('Subscribes before requesting the initial snapshot', async () => {
  const subscription = deferred();
  const calls = [];
  runInNewContext(readFileSync(new URL(file, import.meta.url), 'utf8'), {
    document: { querySelector: () => ({ addEventListener() {} }) },
    window: { __TAURI__: {
      core: { invoke: async command => { calls.push(command); return state('loading'); } },
      event: { listen: (name, callback) => { calls.push(name); assert.equal(typeof callback, 'function'); return subscription.promise; } },
    } },
  });
  assert.deepEqual(calls, ['site-load-state']);
  subscription.resolve(() => {});
  await tick();
  assert.deepEqual(calls, ['site-load-state', 'get_site_load_state']);
});

test('A newer event wins over an in-flight query, including the same generation', async () => {
  const snapshot = deferred();
  const app = await localWindow(file, () => snapshot.promise);
  emit(app, state('timeout', null, 5));
  snapshot.resolve(state('loading', null, 5));
  await tick();
  assert.equal(app.element('#title').textContent, 'Сайт не ответил вовремя');
  emit(app, state('loading', null, 6));
  emit(app, state('error', 'old error', 5));
  assert.equal(app.element('#title').textContent, 'Загрузка Anivox…');
});

test('A stale query failure cannot overwrite a newer event', async () => {
  const snapshot = deferred();
  const app = await localWindow(file, () => snapshot.promise);
  emit(app, state('ready'));
  snapshot.reject(new Error('outdated failure'));
  await tick();
  assert.equal(app.element('#title').textContent, 'Anivox готов');
  assert.equal(app.element('#action-error').textContent, '');
});

test('Backend error details are text only and cleared by the next state', async () => {
  const unsafe = '<img src=x onerror=alert(1)>\nОшибка';
  const app = await localWindow(file, async () => state('error', unsafe));
  assert.equal(app.element('#detail').textContent, unsafe);
  assert.equal(app.element('#detail').hidden, false);
  assert.equal(app.element('#detail').innerHTML, undefined);
  emit(app, state('loading', null, 2));
  assert.equal(app.element('#detail').textContent, '');
  assert.equal(app.element('#detail').hidden, true);
});

for (const action of ['retry', 'home', 'wait']) {
  test(`${action} uses only the local recovery command and blocks overlapping actions`, async () => {
    const request = deferred();
    const calls = [];
    const app = await localWindow(file, (command, args) => {
      if (command === 'get_site_load_state') return Promise.resolve(state('timeout'));
      calls.push([command, args.action]);
      return request.promise;
    });
    const pending = app.click(`#${action}`);
    for (const button of ['retry', 'home', 'wait']) {
      assert.equal(app.element(`#${button}`).disabled, true);
      await app.click(`#${button}`);
    }
    assert.deepEqual(calls, [['site_recovery_action', action]]);
    request.reject(new Error('<b>IPC failed</b>'));
    await pending;
    for (const button of ['retry', 'home', 'wait']) assert.equal(app.element(`#${button}`).disabled, false);
    assert.match(app.element('#action-error').textContent, /<b>IPC failed<\/b>/);
    assert.equal(app.element('#action-error').hidden, false);
    await app.click(`#${action}`);
    assert.equal(calls.length, 2);
  });
}

test('Successful wait delegates hiding to the backend and clears earlier action errors', async () => {
  let attempts = 0;
  const app = await localWindow(file, async (command, args) => {
    if (command === 'get_site_load_state') return state('timeout');
    assert.equal(command, 'site_recovery_action');
    assert.equal(args.action, 'wait');
    if (++attempts === 1) throw new Error('try again');
  });
  await app.click('#wait');
  assert.equal(app.element('#action-error').hidden, false);
  await app.click('#wait');
  assert.equal(app.element('#action-error').hidden, true);
  assert.equal(app.element('#wait').disabled, false);
});

test('Initial query failure remains recoverable with enabled actions', async () => {
  const app = await localWindow(file, async () => { throw new Error('query failed'); });
  assert.match(app.element('#action-error').textContent, /query failed/);
  for (const button of ['retry', 'home', 'wait']) assert.equal(app.element(`#${button}`).disabled, false);
});

test('Static page has status semantics, local assets, and reduced-motion spinner', () => {
  const html = readFileSync(new URL('../ui/site-recovery.html', import.meta.url), 'utf8');
  const css = readFileSync(new URL('../ui/site-recovery.css', import.meta.url), 'utf8');
  assert.match(html, /<html lang="ru">/);
  assert.match(html, /role="status" aria-live="polite" aria-atomic="true"/);
  assert.match(html, /role="alert"/);
  assert.match(html, /Продолжить ожидание/);
  assert.doesNotMatch(html, /https?:\/\//);
  assert.match(css, /prefers-reduced-motion: reduce/);
  assert.match(css, /animation: none/);
  assert.match(css, /:focus-visible/);
});
