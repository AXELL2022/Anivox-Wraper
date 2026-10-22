import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import vm from 'node:vm';

const source = readFileSync(new URL('../src-tauri/scripts/inject-macos.js', import.meta.url), 'utf8');

test('portable UI installs accessible navigation buttons without privileged calls', () => {
  const nodes = new Map();
  const calls = [];
  const create = tag => ({ tagName: tag, children: [], attrs: {}, textContent: '',
    setAttribute(k, v) { this.attrs[k] = v; },
    appendChild(el) { this.children.push(el); if (el.id) nodes.set(el.id, el); },
    addEventListener(name, handler) { this[name] = handler; },
  });
  const head = create('head'), body = create('body');
  const document = { head, body, documentElement: head, readyState: 'complete',
    createElement: create, getElementById: id => nodes.get(id), querySelector: () => null,
    addEventListener() {}, title: 'Anivox' };
  const window = { history: { back: () => calls.push('back'), forward: () => calls.push('forward') }, addEventListener() {} };
  window.top = window;
  vm.runInNewContext(source, { window, document, location: { origin: 'https://anivox.fun', href: 'https://anivox.fun/', reload: () => calls.push('reload') },
    URL, MutationObserver: class { observe() {} }, setInterval() {}, setTimeout() {}, clearTimeout() {} });
  const controls = nodes.get('anivox-macos-controls');
  assert.equal(controls.children.length, 4);
  assert.deepEqual(controls.children.map(button => button.attrs['aria-label']), ['Назад', 'Вперёд', 'Обновить страницу', 'Полный экран плеера']);
  controls.children.slice(0, 3).forEach(button => button.click());
  assert.deepEqual(calls, ['back', 'forward', 'reload']);
  assert.match(nodes.get('anivox-common-styles').textContent, /background-color: rgba\(22, 22, 22/);
});

test('injection does nothing in frames or other origins', () => {
  for (const frame of [true, false]) {
    const window = { top: null };
    if (!frame) window.top = window;
    vm.runInNewContext(source, { window, location: { origin: 'https://other.example' } });
  }
});
