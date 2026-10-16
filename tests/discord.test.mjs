import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { runInNewContext } from 'node:vm';

const source = readFileSync(new URL('../src-tauri/scripts/inject-common.js', import.meta.url), 'utf8');
const flush = () => new Promise(resolve => setImmediate(resolve));

function discord(invoke, { anime = null, video = null, episode = null, page = 'https://anivox.fun/', nodes = {} } = {}) {
  const timers = new Map();
  const intervals = [];
  let observer;
  let id = 0;
  const events = new Map();
  const document = {
    title: 'First - Anivox', readyState: 'loading', baseURI: page,
    querySelector(selector) {
      if (selector === 'title') return {};
      if (selector === '.anime' && anime) return { __vueParentComponent: { proxy: { anime, episode } } };
      if (selector === '.player-container' && video) return { __vueParentComponent: { proxy: { episode } } };
      if (selector === 'video.video-element' || selector === 'video') return video;
      return nodes[selector] || null;
    },
    addEventListener(name, callback) { events.set(name, callback); },
  };
  const window = {
    __TAURI__: { core: { invoke } },
    history: { pushState() {}, replaceState() {} }, addEventListener() {},
    fetch: async () => ({ ok: true, clone: () => ({ json: async () => ({ links: {} }) }) }),
  };
  window.top = window;
  runInNewContext(source, {
    window, document, URL, location: { href: page, origin: 'https://anivox.fun' },
    XMLHttpRequest: class { open() {} send() {} },
    MutationObserver: class { constructor(callback) { observer = callback; } observe() {} },
    setTimeout: callback => { timers.set(++id, callback); return id; },
    clearTimeout: key => timers.delete(key), setInterval: callback => intervals.push(callback),
    console: { log() {}, warn() {}, error() {} },
  });
  return {
    async send() {
      const pending = [...timers.values()];
      timers.clear();
      for (const callback of pending) callback();
      await flush();
    },
    heartbeat() { for (const callback of intervals) callback(); },
    title(value) { document.title = value; observer([]); },
    event(name) { events.get(name)?.({ target: video }); },
    episode(value) { episode = value; },
    async requestEpisode() { await window.fetch(`/api/episodes/123/${episode}`); await flush(); },
  };
}

test('Discord retries an unchanged title after IPC failure', async () => {
  let attempts = 0;
  const app = discord(async () => { if (++attempts === 1) throw new Error('Discord offline'); });
  await app.send();
  app.heartbeat();
  await app.send();
  assert.equal(attempts, 2);
});

test('Discord heartbeat resends status so a restarted client can recover', async () => {
  let attempts = 0;
  const app = discord(async () => { attempts++; });
  await app.send();
  app.title('First - Anivox');
  await app.send();
  assert.equal(attempts, 1);
  app.heartbeat();
  await app.send();
  assert.equal(attempts, 2);
});

test('Discord debounces rapid title changes to the latest title', async () => {
  const calls = [];
  const app = discord(async (command, args) => calls.push(args.presence?.details));
  app.title('Second - Anivox');
  app.title('Third - Anivox');
  await app.send();
  assert.deepEqual(calls, ['Third']);
});

test('Returning to the acknowledged title cancels a pending outdated status', async () => {
  const calls = [];
  const app = discord(async (command, args) => calls.push(args.presence?.details));
  await app.send();
  app.title('Second - Anivox');
  app.title('First - Anivox');
  await app.send();
  assert.deepEqual(calls, ['First']);
});

function watching() {
  return {
    anime: { id: 123, title: 'Название аниме', poster: '//cdn.example/cover.jpg', season_label: '2' },
    episode: 5, page: 'https://anivox.fun/anime/123-name?token=private#player',
    video: { currentSrc: 'https://cdn.example/video.m3u8', currentTime: 60, duration: 1500,
      playbackRate: 2, paused: false, ended: false, readyState: 4 },
  };
}

test('Discord includes anime metadata, public links and a speed-aware remaining timer', async () => {
  let presence;
  const before = Math.floor(Date.now() / 1000);
  const app = discord(async (_, args) => { presence = args.presence; }, watching());
  await app.send();
  assert.equal(presence?.details, 'Название аниме');
  assert.equal(presence.stateText, 'Серия 5 · Сезон 2');
  assert.equal(presence.posterUrl, 'https://cdn.example/cover.jpg');
  assert.equal(presence.animeUrl, 'https://anivox.fun/anime/123-name');
  assert.equal(presence.shikimoriUrl, 'https://shikimori.io/animes?search=' + encodeURIComponent('Название аниме'));
  assert.ok(presence.endTimestamp >= before + 720 && presence.endTimestamp <= before + 722);
});

test('Pause and resume update an unchanged title and remove or restore the timer', async () => {
  const options = watching();
  const calls = [];
  const app = discord(async (_, args) => calls.push(args.presence), options);
  await app.send();
  options.video.paused = true;
  app.event('pause');
  await app.send();
  assert.match(calls.at(-1)?.stateText || '', /На паузе/);
  assert.equal(calls.at(-1).endTimestamp, null);
  options.video.paused = false;
  app.event('play');
  await app.send();
  assert.doesNotMatch(calls.at(-1).stateText, /На паузе/);
  assert.ok(calls.at(-1).endTimestamp > Date.now() / 1000);
});

test('An episode change updates status without changing the document title', async () => {
  const options = watching();
  const calls = [];
  const app = discord(async (_, args) => calls.push(args.presence), options);
  await app.send();
  app.episode(6);
  app.event('loadedmetadata');
  app.heartbeat();
  await app.send();
  assert.match(calls.at(-1)?.stateText || '', /Серия 6/);
});

for (const overrides of [{ duration: Infinity }, { ended: true }, { readyState: 0 }, { playbackRate: 0 }]) {
  test(`Discord does not create a misleading timer for ${JSON.stringify(overrides)}`, async () => {
    const options = watching();
    Object.assign(options.video, overrides);
    let presence;
    const app = discord(async (_, args) => { presence = args.presence; }, options);
    await app.send();
    assert.equal(presence?.endTimestamp, null);
  });
}

test('Discord ignores old anime metadata when the route points to another anime', async () => {
  const options = watching();
  options.anime.id = 999;
  let presence;
  const app = discord(async (_, args) => { presence = args.presence; }, options);
  await app.send();
  assert.equal(presence?.posterUrl, null);
  assert.equal(presence.endTimestamp, null);
  assert.doesNotMatch(presence.details, /Название аниме/);
});

test('Public DOM provides title, poster, episode and season when Vue internals are unavailable', async () => {
  const options = watching();
  options.anime = null;
  options.episode = null;
  options.nodes = {
    '.anime__info-title-name': { textContent: 'Название аниме' },
    '.anime__info-poster': { src: 'https://cdn.example/cover.jpg' },
    '.anime__info-episodes-episodes-item.active .anime__info-episodes-episodes-item-info-title': { textContent: '5 Серия 1080p' },
    '.anime__info-episodes-seasons-item.active': { textContent: '2 Сезон' },
  };
  let presence;
  const app = discord(async (_, args) => { presence = args.presence; }, options);
  await app.send();
  assert.equal(presence?.details, 'Название аниме');
  assert.equal(presence.stateText, 'Серия 5 · Сезон 2');
  assert.equal(presence.posterUrl, 'https://cdn.example/cover.jpg');
});

test('Discord excludes alternative titles nested in the heading tooltip', async () => {
  const options = watching();
  options.anime = null;
  const primary = 'О моём перерождении в слизь 2';
  options.nodes = {
    '.anime__info-title-name': {
      textContent: primary + ' Tensei shitara Slime Datta Ken 2nd SeasonTensura 2That Time I Got Reincarnated as a Slime Season 2',
      childNodes: [
        { nodeType: 3, textContent: primary + ' ' },
        { nodeType: 1, textContent: 'Tensei shitara Slime Datta Ken 2nd SeasonTensura 2' },
      ],
    },
  };
  let presence;
  const app = discord(async (_, args) => { presence = args.presence; }, options);
  await app.send();
  assert.equal(presence?.details, primary);
  assert.equal(presence.shikimoriUrl, 'https://shikimori.io/animes?search=' + encodeURIComponent(primary));
});

test('Playing and waiting events update loading state and timer promptly', async () => {
  const options = watching();
  options.video.readyState = 1;
  const calls = [];
  const app = discord(async (_, args) => calls.push(args.presence), options);
  await app.send();
  assert.match(calls.at(-1).stateText, /Загрузка/);
  options.video.readyState = 4;
  app.event('playing');
  await app.send();
  assert.ok(calls.at(-1).endTimestamp);
  app.event('waiting');
  await app.send();
  assert.equal(calls.at(-1).endTimestamp, null);
  assert.match(calls.at(-1).stateText, /Загрузка/);
  app.event('playing');
  await app.send();
  assert.ok(calls.at(-1).endTimestamp);
});

test('An old blob video cannot provide the timer for a newly selected episode', async () => {
  const options = watching();
  options.video.currentSrc = 'blob:https://anivox.fun/old';
  const calls = [];
  const app = discord(async (_, args) => calls.push(args.presence), options);
  await app.send();
  assert.ok(calls.at(-1).endTimestamp);
  app.episode(6);
  await app.requestEpisode();
  app.heartbeat();
  await app.send();
  assert.equal(calls.at(-1).endTimestamp, null);
  assert.match(calls.at(-1).stateText, /Серия 6/);
  options.video.currentSrc = 'blob:https://anivox.fun/new';
  app.event('loadedmetadata');
  await app.send();
  assert.ok(calls.at(-1).endTimestamp);
});

test('Discord does not overlap IPC requests while a send is pending', async () => {
  let attempts = 0;
  let finish;
  const app = discord(() => {
    attempts++;
    return new Promise(resolve => { finish = resolve; });
  });
  await app.send();
  app.heartbeat();
  await app.send();
  assert.equal(attempts, 1);
  finish();
  await flush();
  app.heartbeat();
  await app.send();
  assert.equal(attempts, 2);
  finish();
  await flush();
});
