import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { runInNewContext } from 'node:vm';

const source = readFileSync(new URL('../src-tauri/scripts/inject-common.js', import.meta.url), 'utf8');
const flush = () => new Promise(resolve => setImmediate(resolve));
function deferred() {
  let resolve;
  const promise = new Promise(done => { resolve = done; });
  return { promise, resolve };
}

function player({ video = null, context = null } = {}) {
  let page = new URL('https://anivox.fun/anime/show/');
  const responses = new Map();
  const calls = [];
  const events = new Map();
  class Xhr {
    handlers = new Map();
    open(method, url) { this.url = String(url); }
    send() {}
    addEventListener(name, callback) {
      if (!this.handlers.has(name)) this.handlers.set(name, []);
      this.handlers.get(name).push(callback);
    }
    respond(data = null, status = 200) {
      this.status = status;
      this.responseText = JSON.stringify(data);
      for (const name of ['load', 'loadend']) {
        for (const callback of this.handlers.get(name) || []) callback.call(this);
      }
      this.handlers.clear();
    }
  }
  const history = {
    pushState(state, title, url) { if (url != null) page = new URL(url, page); },
    replaceState(state, title, url) { if (url != null) page = new URL(url, page); },
  };
  const window = {
    history,
    __TAURI__: { core: { invoke: async (command, args) => { calls.push({ command, args }); } } },
    async fetch(input) {
      const url = new URL(typeof input === 'string' ? input : input.url || input.href, page).href;
      return await responses.get(url) || { ok: true, clone: () => ({ json: async () => null }) };
    },
    addEventListener(name, callback) {
      if (!events.has(name)) events.set(name, []);
      events.get(name).push(callback);
    },
  };
  window.top = window;
  const location = {
    get href() { return page.href; }, get origin() { return page.origin; },
    get pathname() { return page.pathname; }, get search() { return page.search; },
    get hash() { return page.hash; },
  };
  runInNewContext(source, {
    window, location, history, URL, XMLHttpRequest: Xhr,
    document: {
      title: 'Episode - Anivox', readyState: 'loading',
      get baseURI() { return page.href; },
      querySelector(selector) {
        if (selector === 'video' || selector === 'video.video-element') return video;
        if (selector === '.player-container' && context) return { __vueParentComponent: { ctx: context } };
        return null;
      },
      addEventListener() {},
    },
    localStorage: { getItem: () => null },
    console: { log() {}, warn() {}, error() {} },
    MutationObserver: class { observe() {} },
    setTimeout() {}, clearTimeout() {}, setInterval() {},
  });
  function response(data, ok = true) { return { ok, clone: () => ({ json: async () => data }) }; }
  return {
    calls, window,
    async launch() {
      const start = calls.length;
      await window.__anivoxLaunchMpv();
      return calls.slice(start).find(call => call.command === 'open_in_mpv')?.args;
    },
    async fetch(url, data = null, ok = true) {
      responses.set(new URL(url, page).href, response(data, ok));
      await window.fetch(url);
      await flush();
    },
    pendingFetch(url) {
      const pending = deferred();
      responses.set(new URL(url, page).href, pending.promise);
      const request = window.fetch(url);
      return async data => { pending.resolve(response(data)); await request; await flush(); };
    },
    async pendingJson(url) {
      const pending = deferred();
      responses.set(new URL(url, page).href, { ok: true, clone: () => ({ json: () => pending.promise }) });
      await window.fetch(url);
      return async data => { pending.resolve(data); await flush(); };
    },
    async failFetch(url, kind) {
      const response = kind === 'network' ? Promise.reject(new Error('Network failed')) : {
        ok: true, clone: () => ({ json: async () => { throw new Error('Invalid JSON'); } }),
      };
      responses.set(new URL(url, page).href, response);
      if (kind === 'network') await assert.rejects(window.fetch(url), /Network failed/);
      else await window.fetch(url);
      await flush();
    },
    xhr(url) { const request = new Xhr(); request.open('GET', url); request.send(); return request; },
    navigate(url, method = 'pushState') { window.history[method](null, '', url); },
    popstate(url) { page = new URL(url, page); for (const callback of events.get('popstate') || []) callback(); },
    anchor(hash) { page.hash = hash; for (const callback of events.get('hashchange') || []) callback(); },
  };
}

for (const [link, expected] of [
  ['/api/hls/episode.m3u8', 'https://anivox.fun/api/hls/episode.m3u8'],
  ['../episode.m3u8', 'https://anivox.fun/anime/episode.m3u8'],
  ['episode.m3u8', 'https://anivox.fun/anime/show/episode.m3u8'],
  ['//cdn.example/episode.m3u8', 'https://cdn.example/episode.m3u8'],
  ['http://cdn.example/episode.m3u8', 'http://cdn.example/episode.m3u8'],
]) {
  test(`MPV resolves stream ${link} against the page URL`, async () => {
    const app = player();
    await app.fetch('/api/episodes/1', { links: { 1080: link } });
    assert.equal((await app.launch())?.url, expected);
  });
}

test('MPV selects the highest playable quality and skips invalid URLs', async () => {
  const app = player();
  await app.fetch('/api/episodes/1', { links: {
    '4K': 'premium', '2K': 'blob:invalid', 1080: '/best.m3u8', 720: '/low.m3u8',
  } });
  assert.equal((await app.launch())?.url, 'https://anivox.fun/best.m3u8');
});

test('Premium-only and empty quality lists never launch MPV', async () => {
  const app = player();
  for (const links of [{ 1080: 'premium' }, { 1080: ' PREMIUM ' }, {}, { 1080: null }]) {
    await app.fetch('/api/episodes/1', { links });
    assert.equal(await app.launch(), undefined);
  }
});

test('A single relative API link can launch MPV', async () => {
  const app = player();
  await app.fetch('/api/episodes/1', { links: '/single.m3u8' });
  assert.equal((await app.launch())?.url, 'https://anivox.fun/single.m3u8');
});

test('A new episode clears its old stream and subtitles before the response arrives', async () => {
  const app = player();
  await app.fetch('/api/episodes/1', { links: { 1080: '/old.m3u8' } });
  await app.fetch('/subs/old.ass');
  assert.equal((await app.launch())?.subUrl, 'https://anivox.fun/subs/old.ass');
  const finish = app.pendingFetch('/api/episodes/2');
  assert.equal(await app.launch(), undefined);
  await finish({ links: { 1080: '/new.m3u8' } });
  const launched = await app.launch();
  assert.equal(launched?.url, 'https://anivox.fun/new.m3u8');
  assert.equal(launched?.subUrl, null);
});

test('Late fetch responses from the previous episode cannot replace its new links', async () => {
  const app = player();
  const finishOld = app.pendingFetch('/api/episodes/1');
  await app.fetch('/api/episodes/2', { links: { 1080: '/new.m3u8' } });
  await finishOld({ links: { 1080: '/old.m3u8' } });
  assert.equal((await app.launch())?.url, 'https://anivox.fun/new.m3u8');
});

test('Late JSON decoding and subtitle responses from the old episode are ignored', async () => {
  const app = player();
  const finishOldJson = await app.pendingJson('/api/episodes/1');
  const finishOldSub = app.pendingFetch('/subs/old.ass');
  await app.fetch('/api/episodes/2', { links: { 1080: '/new.m3u8' } });
  await finishOldJson({ links: { 1080: '/old.m3u8' } });
  await finishOldSub(null);
  const launched = await app.launch();
  assert.equal(launched?.url, 'https://anivox.fun/new.m3u8');
  assert.equal(launched?.subUrl, null);
});

test('Late XHR responses cannot replace the new episode or restore old subtitles', async () => {
  const app = player();
  const old = app.xhr('/api/episodes/1');
  const sub = app.xhr('/subs/old.ass');
  const fresh = app.xhr('/api/episodes/2');
  fresh.respond({ links: { 1080: '/new.m3u8' } });
  old.respond({ links: { 1080: '/old.m3u8' } });
  sub.respond();
  const launched = await app.launch();
  assert.equal(launched?.url, 'https://anivox.fun/new.m3u8');
  assert.equal(launched?.subUrl, null);
});

for (const method of ['pushState', 'replaceState', 'popstate']) {
  test(`${method} clears cached streams and prevents reuse of the old player`, async () => {
    const video = { currentSrc: 'https://cdn.example/old.m3u8', paused: true, currentTime: 10 };
    const app = player({ video, context: { links: { 1080: video.currentSrc } } });
    await app.fetch('/api/episodes/1', { links: { 1080: video.currentSrc } });
    await app.fetch('/subs/old.ass');
    if (method === 'popstate') app.popstate('/anime/next/');
    else app.navigate('/anime/next/', method);
    assert.equal(await app.launch(), undefined);
    video.currentSrc = 'https://cdn.example/new.m3u8';
    const launched = await app.launch();
    assert.equal(launched?.url, video.currentSrc);
    assert.equal(launched?.subUrl, null);
  });
}

test('Unavailable new episodes cannot fall back to the previous Vue/video source', async () => {
  const video = { currentSrc: 'https://cdn.example/old.m3u8', paused: true };
  const app = player({ video, context: { links: { 1080: video.currentSrc } } });
  await app.fetch('/api/episodes/1', { links: { 1080: video.currentSrc } });
  await app.fetch('/api/episodes/2', { links: { 1080: 'premium' } });
  assert.equal(await app.launch(), undefined);
});

test('Failed episode requests do not supply playable links', async () => {
  const app = player();
  await app.fetch('/api/episodes/1', { links: { 1080: '/error.m3u8' } }, false);
  assert.equal(await app.launch(), undefined);
});

test('A route change followed by an episode request still rejects the old Vue links', async () => {
  const video = { currentSrc: 'https://anivox.fun/anime/show/episode.m3u8', paused: true };
  const app = player({ video, context: { links: { 1080: 'episode.m3u8' } } });
  await app.fetch('/api/episodes/1', { links: { 1080: video.currentSrc } });
  app.navigate('/anime/next/');
  assert.equal(await app.launch(), undefined);
  await app.fetch('/api/episodes/2', { links: {} });
  assert.equal(await app.launch(), undefined);
  video.currentSrc = 'https://anivox.fun/anime/next/episode.m3u8';
  assert.equal((await app.launch())?.url, video.currentSrc);
});

test('Ordinary anchor changes retain the current stream', async () => {
  const app = player();
  await app.fetch('/api/episodes/1', { links: { 1080: '/current.m3u8' } });
  app.anchor('comments');
  assert.equal((await app.launch())?.url, 'https://anivox.fun/current.m3u8');
});

test('Media segments cannot replace the captured manifest', async () => {
  const app = player();
  await app.fetch('/api/hls/episode/index.m3u8');
  await app.fetch('/api/hls/episode/part.ts');
  assert.equal((await app.launch())?.url, 'https://anivox.fun/api/hls/episode/index.m3u8');
});

test('Fetch URL objects are intercepted without changing the original request', async () => {
  const app = player();
  await app.fetch(new URL('https://anivox.fun/api/episodes/1'), { links: { 1080: '/current.m3u8' } });
  assert.equal((await app.launch())?.url, 'https://anivox.fun/current.m3u8');
});

for (const failure of ['http', 'network', 'json']) {
  test(`A valid current manifest still works after an episode ${failure} failure`, async () => {
    const app = player();
    if (failure === 'http') await app.fetch('/api/episodes/1', null, false);
    else await app.failFetch('/api/episodes/1', failure);
    await app.fetch('/api/hls/current/index.m3u8');
    assert.equal((await app.launch())?.url, 'https://anivox.fun/api/hls/current/index.m3u8');
  });
}

test('XHR failure releases the current manifest fallback', async () => {
  const app = player();
  app.xhr('/api/episodes/1').respond(null, 500);
  await app.fetch('/api/hls/current/index.m3u8');
  assert.equal((await app.launch())?.url, 'https://anivox.fun/api/hls/current/index.m3u8');
});

test('An old failed XHR cannot release a pending new episode', async () => {
  const app = player();
  const old = app.xhr('/api/episodes/1');
  const current = app.xhr('/api/episodes/2');
  await app.fetch('/api/hls/current/index.m3u8');
  old.respond(null, 500);
  assert.equal(await app.launch(), undefined);
  current.respond({ links: { 1080: '/new.m3u8' } });
  assert.equal((await app.launch())?.url, 'https://anivox.fun/new.m3u8');
});

test('Vue links updated in place become playable after an episode change', async () => {
  const context = { links: { 1080: 'old.m3u8' } };
  const app = player({ context });
  await app.fetch('/api/episodes/1', { links: context.links });
  await app.fetch('/api/episodes/2', { links: {} });
  assert.equal(await app.launch(), undefined);
  context.links[1080] = 'new.m3u8';
  assert.equal((await app.launch())?.url, 'https://anivox.fun/anime/show/new.m3u8');
});
