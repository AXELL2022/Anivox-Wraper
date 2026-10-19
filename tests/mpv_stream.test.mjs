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

function player({ video = null, context = null, launchMpv = async () => {}, withSession = false, getMpvState = async () => null, storage = new Map() } = {}) {
  let page = new URL('https://anivox.fun/anime/show/');
  const responses = new Map();
  const calls = [];
  const events = new Map();
  const intervals = [];
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
    __TAURI__: { core: { invoke: async (command, args) => { calls.push({ command, args }); if (command === 'open_in_mpv') return await launchMpv(args); if (command === 'get_mpv_state') return await getMpvState(); } } },
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
  const script = withSession ? readFileSync(new URL('../src-tauri/scripts/mpv-session.js', import.meta.url), 'utf8') + '\n' + source : source;
  runInNewContext(script, {
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
    localStorage: { getItem: key => storage.get(key) ?? null, setItem: (key,value) => storage.set(key,value), removeItem: key => storage.delete(key) },
    console: { log() {}, warn() {}, error() {} },
    MutationObserver: class { observe() {} },
    setTimeout() {}, clearTimeout() {}, setInterval(fn) { intervals.push(fn); },
  });
  function response(data, ok = true) { return { ok, clone: () => ({ json: async () => data }) }; }
  return {
    calls, window, async tick() { for (const fn of intervals) await fn(); },
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

test('MPV pauses embedded playback only after successful launch', async () => {
  const pending = deferred();
  let pauses = 0;
  const video = { currentTime: 42, paused: false, pause() { pauses++; } };
  const app = player({ video, launchMpv: () => pending.promise });
  await app.fetch('/api/episodes/1', { links: { 1080: '/current.m3u8' } });
  const launch = app.launch();
  await flush();
  assert.equal(pauses, 0);
  pending.resolve();
  const args = await launch;
  assert.equal(pauses, 1);
  assert.equal(args.startTime, 42);
});

test('Failed MPV launch leaves embedded playback running', async () => {
  let pauses = 0;
  const app = player({ video: { currentTime: 5, paused: false, pause() { pauses++; } }, launchMpv: async () => { throw new Error('not installed'); } });
  await app.fetch('/api/episodes/1', { links: { 1080: '/current.m3u8' } });
  await app.launch();
  assert.equal(pauses, 0);
  const errorStatus = app.calls.filter(call => call.command === 'report_player_status').at(-1)?.args;
  assert.match(errorStatus.detail, /not installed/);
});


test('Full handoff freezes high quality and identity before muting and lowering the site player', async () => {
  const storage = new Map([['quality__3_1','1080']]);
  const video = {currentTime:42,paused:false,muted:false,volume:.8,playbackRate:1,pause(){this.paused=true},async play(){this.paused=false}};
  const changes=[];
  const context={anime_id:1,episode:2,translations_id:3,quality:'1080',qualityPreset:'1080',GetQualities:[{id:'1080'},{id:'360'}],async changeQuality(choice){changes.push(choice.id);this.quality=choice.id;this.qualityPreset=choice.id;storage.set('quality__3_1',choice.id)}};
  let sample={sessionId:'full',mediaKey:'1:2:3',ready:true,mirrorEmbedded:true,position:42,duration:1200,speed:1,paused:false};
  const app=player({video,context,storage,withSession:true,launchMpv:async()=>({sessionId:'full',mirrorEmbedded:true}),getMpvState:async()=>sample});
  await app.fetch('/api/episodes/2',{links:{1080:'/best.m3u8',360:'/minimum.m3u8'}});
  const args=await app.launch();
  assert.equal(args.url,'https://anivox.fun/best.m3u8');assert.equal(args.media.key,'1:2:3');
  assert.deepEqual(changes,['360']);assert.equal(video.muted,true);assert.equal(video.paused,false);
  assert.equal(storage.get('quality__3_1'),'1080');assert.equal(context.qualityPreset,'1080');
  sample={...sample,position:120,paused:true};await app.tick();assert.equal(video.currentTime,120);assert.equal(video.paused,true);
  sample=null;await app.tick();assert.equal(context.quality,'1080');assert.equal(video.muted,false);
});
