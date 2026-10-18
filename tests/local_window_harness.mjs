import { readFileSync } from 'node:fs';
import { runInNewContext } from 'node:vm';

export async function localWindow(file, invoke) {
  const elements = new Map();
  const listeners = new Map();
  const keys = new Map();
  const element = selector => {
    if (!elements.has(selector)) elements.set(selector, {
      value: '', textContent: '', disabled: false, hidden: false, handlers: {},
      addEventListener(name, fn) { this.handlers[name] = fn; },
      focus() {},
    });
    return elements.get(selector);
  };
  runInNewContext(readFileSync(new URL(file, import.meta.url), 'utf8'), {
    document: { querySelector: element, addEventListener: (name, fn) => keys.set(name, fn) },
    window: { __TAURI__: { core: { invoke }, event: { listen: async (name, fn) => listeners.set(name, fn) } } },
    console,
  });
  await new Promise(resolve => setImmediate(resolve));
  return { element, listeners, keys, async click(selector) { await element(selector).handlers.click?.({ preventDefault() {} }); }, async submit() { await element('#settings-form').handlers.submit({ preventDefault() {} }); } };
}
