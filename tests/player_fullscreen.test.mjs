import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import vm from 'node:vm';

const source = readFileSync(new URL('../src-tauri/scripts/inject-common.js', import.meta.url), 'utf8');
const fullscreenScript = source.slice(source.indexOf('    function isPlayerControl('), source.indexOf('    function setupCommonStyles('));

// Run the production fullscreen handlers against a simulated DOM and clock.
function createMockEnvironment({ isFullscreen = false, isPlaying = true } = {}) {
  let fullscreenElement = isFullscreen ? {} : null;
  const events = new Map();

  const video = {
    tagName: 'VIDEO',
    className: 'video-element',
    paused: !isPlaying,
    playCalled: 0,
    pauseCalled: 0,
    async play() { this.playCalled++; this.paused = false; },
    pause() { this.pauseCalled++; this.paused = true; },
    closest(selector) {
      const parts = selector.split(',').map(s => s.trim());
      if (parts.some(p => p === 'video' || p === '.video-element' || p === '.player-container')) return this;
      return null;
    },
  };

  let vueToggleFullscreenCalled = 0;
  let nativeRequestFullscreenCalled = 0;
  let nativeExitFullscreenCalled = 0;

  const playerContainer = {
    tagName: 'DIV',
    className: 'player-container',
    __vueParentComponent: {
      proxy: {
        toggleFullscreen() {
          vueToggleFullscreenCalled++;
          fullscreenElement = fullscreenElement ? null : playerContainer;
        },
      },
    },
    async requestFullscreen() {
      nativeRequestFullscreenCalled++;
      fullscreenElement = playerContainer;
    },
    closest(selector) {
      const parts = selector.split(',').map(s => s.trim());
      if (parts.some(p => p === '.player-container' || p === '.player' || p === '[class*="player"]')) return this;
      return null;
    },
  };

  const document = {
    get fullscreenElement() { return fullscreenElement; },
    querySelector(sel) {
      if (sel.includes('video')) return video;
      if (sel.includes('.player-container')) return playerContainer;
      return null;
    },
    async exitFullscreen() {
      nativeExitFullscreenCalled++;
      fullscreenElement = null;
    },
  };

  const window = {
    addEventListener(name, handler) {
      if (!events.has(name)) events.set(name, []);
      events.get(name).push(handler);
    },
    dispatchEvent(name, event) {
      for (const h of events.get(name) || []) {
        h(event);
      }
    },
  };

  let now = 1000;
  vm.runInNewContext(fullscreenScript, { window, document, console, setTimeout, performance: { now: () => now } });

  return {
    window,
    advance(ms) { now += ms; },
    click(target, interval = 100) {
      window.dispatchEvent('mousedown', { target, button: 0 });
      now += interval;
      window.dispatchEvent('mousedown', { target, button: 0 });
      const event = { target, preventDefault() { this.defaultPrevented = true; }, stopPropagation() { this.propagationStopped = true; } };
      window.dispatchEvent('dblclick', event);
      return event;
    },
    document,
    video,
    playerContainer,
    get vueToggleFullscreenCalled() { return vueToggleFullscreenCalled; },
    get nativeRequestFullscreenCalled() { return nativeRequestFullscreenCalled; },
    get nativeExitFullscreenCalled() { return nativeExitFullscreenCalled; },
  };
}

test('Double-clicking on video player area toggles fullscreen into fullscreen', async () => {
  const env = createMockEnvironment({ isFullscreen: false, isPlaying: true });

  const touchZone = {
    tagName: 'DIV',
    className: 'touch-zone left',
    closest(selector) {
      if (selector.includes('.player-container')) return env.playerContainer;
      return null;
    },
  };

  const dblEvt = env.click(touchZone);

  assert.equal(env.vueToggleFullscreenCalled, 1);
  assert.equal(env.document.fullscreenElement, env.playerContainer);
  assert.equal(dblEvt.defaultPrevented, true);
});

test('Double-clicking while in fullscreen exits fullscreen', async () => {
  const env = createMockEnvironment({ isFullscreen: true, isPlaying: true });

  const touchZone = {
    tagName: 'DIV',
    className: 'touch-zone right',
    closest(selector) {
      if (selector.includes('.player-container')) return env.playerContainer;
      return null;
    },
  };

  const dblEvt = env.click(touchZone);

  assert.equal(env.vueToggleFullscreenCalled, 1);
  assert.equal(env.document.fullscreenElement, null);
});

test('Double-clicking on player controls is ignored', async () => {
  const env = createMockEnvironment({ isFullscreen: false, isPlaying: true });

  const button = {
    tagName: 'BUTTON',
    className: 'setting-button',
    closest(selector) {
      if (selector.includes('button') || selector.includes('.setting-button')) return button;
      if (selector.includes('.player-container')) return env.playerContainer;
      return null;
    },
  };

  const dblEvt = env.click(button);

  assert.equal(env.vueToggleFullscreenCalled, 0);
  assert.notEqual(dblEvt.defaultPrevented, true);
});

test('Fallback to native requestFullscreen and exitFullscreen when Vue context is absent', async () => {
  const env = createMockEnvironment({ isFullscreen: false, isPlaying: true });
  delete env.playerContainer.__vueParentComponent;

  const target = env.playerContainer;
  env.click(target);

  assert.equal(env.nativeRequestFullscreenCalled, 1);
  assert.equal(env.document.fullscreenElement, env.playerContainer);

  // Now exit fullscreen
  env.click(target);

  assert.equal(env.nativeExitFullscreenCalled, 1);
  assert.equal(env.document.fullscreenElement, null);
});

test('Playback state is restored if single clicks inverted play state during double click', async () => {
  const env = createMockEnvironment({ isFullscreen: false, isPlaying: true });

  const touchZone = {
    tagName: 'DIV',
    className: 'touch-zone left',
    closest(selector) {
      if (selector.includes('.player-container')) return env.playerContainer;
      return null;
    },
  };

  // First click pauses video
  env.window.dispatchEvent('mousedown', { target: touchZone, button: 0 });
  env.video.pause();

  // Double click fires, video was left paused
  env.advance(100);
  env.window.dispatchEvent('mousedown', { target: touchZone, button: 0 });
  env.window.dispatchEvent('dblclick', { target: touchZone, preventDefault() {}, stopPropagation() {} });

  // Wait for recovery timeout
  await new Promise(r => setTimeout(r, 80));

  // Should have been restored to play
  assert.equal(env.video.paused, false);
  assert.equal(env.video.playCalled, 1);
});

test('Double-click threshold accepts 250 ms but suppresses slower system double-clicks', () => {
  for (const interval of [250, 251, 400]) {
    const env = createMockEnvironment();
    const event = env.click(env.playerContainer, interval);
    assert.equal(env.vueToggleFullscreenCalled, interval <= 250 ? 1 : 0);
    assert.equal(event.defaultPrevented, true);
    assert.equal(event.propagationStopped, true);
  }
});

test('Single clicks and right clicks do not toggle fullscreen', () => {
  const env = createMockEnvironment();
  const target = env.playerContainer;
  env.window.dispatchEvent('mousedown', { target, button: 0 });
  assert.equal(env.vueToggleFullscreenCalled, 0);
  env.advance(100);
  env.window.dispatchEvent('mousedown', { target, button: 2 });
  assert.equal(env.vueToggleFullscreenCalled, 0);
});
