import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { runInNewContext } from 'node:vm';

const lifecycle = readFileSync(new URL('../src-tauri/scripts/site-lifecycle.js', import.meta.url), 'utf8');
const paused = readFileSync(new URL('../src-tauri/scripts/restore-paused.js', import.meta.url), 'utf8');
function app(origin = 'https://anivox.fun', frame = false) {
  const events = new Map(), calls = [];
  const location = {origin, href: `${origin}/anime/42`};
  const window = {location, addEventListener: (key, fn) => events.set(key, fn), __TAURI__: {core: {invoke: async (cmd, args) => { if(cmd === 'get_site_document_generation') return 7; calls.push({cmd,args});}}}};
  window.top = frame ? {} : window;
  const history = { pushState() {}, replaceState() {} };
  const root = {children: [{}], getBoundingClientRect: () => ({height: 600})};
  const document = {querySelector: () => root, documentElement: {}, readyState: 'loading', addEventListener: (key, fn) => events.set(`document:${key}`, fn), removeEventListener: key => events.delete(`document:${key}`)};
  class Media { play() { this.played = true; return Promise.resolve(); } pause() { this.paused = true; } }
  class MutationObserver { constructor(fn) { events.set('mutation',fn); } observe() {} disconnect() {} }
  const context = {window,location,history,document,MutationObserver,HTMLMediaElement: Media,DOMException,console};
  return {events,calls,location,context,Media,run: source => runInNewContext(source,context)};
}
test('Lifecycle waits for document load and reports SPA routes without reloading', async () => {
  const a = app(); a.run(lifecycle);
  assert.equal(a.calls.length,0);
  a.events.get('load')(); await Promise.resolve();
  assert.equal(a.calls[0].args.loaded,true);
  a.location.href += '?episode=2';
  a.context.history.pushState({},'',a.location.href); await Promise.resolve();
  assert.equal(a.calls[1].args.href,a.location.href);
  assert.equal(a.calls[1].cmd,'site_document_state');
  a.events.get('popstate')(); a.events.get('hashchange')(); await Promise.resolve();
  assert.equal(a.calls.length,4);
  assert.equal(a.events.has('offline'),false);
});
test('Empty SPA shell never dismisses recovery merely because load fired', async () => {
  const a = app();
  const root = {children: [], getBoundingClientRect: () => ({height: 0})};
  a.context.document.querySelector = () => root;
  a.run(lifecycle);
  a.events.get('load')(); await Promise.resolve();
  assert.equal(a.calls.length,0);
  root.children.push({});
  a.events.get('mutation')(); await Promise.resolve();
  assert.equal(a.calls.length,0);
  root.getBoundingClientRect = () => ({height:600});
  a.events.get('mutation')(); await Promise.resolve();
  assert.equal(a.calls.length,1);
  assert.equal(a.calls[0].args.loaded,true);
});

test('Lifecycle route before document load is not a successful load', async () => {
  const a = app(); a.run(lifecycle); a.context.history.replaceState(); await Promise.resolve();
  assert.equal(a.calls[0].args.loaded,false);
});
for (const [origin,frame] of [['https://evil.example',false],['https://anivox.fun',true]]) {
  test(`Lifecycle and autoplay guard ignore external/frame ${origin}/${frame}`, () => {
    const a = app(origin,frame); const original = a.Media.prototype.play;
    a.run(lifecycle); a.run(paused);
    assert.equal(a.calls.length,0); assert.equal(a.events.size,0); assert.equal(a.Media.prototype.play,original);
  });
}
test('Restored page stays paused until trusted input; synthetic input cannot unlock', async () => {
  const a = app(); const original = a.Media.prototype.play; a.run(paused);
  const video = new a.Media();
  await assert.rejects(video.play(),{name:'NotAllowedError'});
  a.events.get('document:play')({target:video}); assert.equal(video.paused,true);
  a.events.get('document:pointerdown')({isTrusted:false});
  await assert.rejects(video.play());
  a.events.get('document:keydown')({isTrusted:true});
  assert.equal(a.Media.prototype.play,original);
  await video.play(); assert.equal(video.played,true);
});
