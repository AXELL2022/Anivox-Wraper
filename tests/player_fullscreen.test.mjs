import { test } from 'node:test';
import assert from 'node:assert/strict';

// Helper extracting the embedded player script logic from lib.rs or running a simulated DOM environment
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

  // Setup the logic under test
  function isPlayerControl(el) {
    if (!el || !el.closest) return false;
    const controlSelector = [
      'button',
      'input',
      'select',
      'textarea',
      'a',
      '[role="button"]',
      '[role="slider"]',
      '[role="menu"]',
      '[role="dialog"]',
      '.controls-bar',
      '.progress-bar',
      '.volume',
      '.volume-icon',
      '.setting-button',
      '.chrome-cast',
      '.buttons',
      '.custom-dropdown__items__mobile',
      '.modal__body',
      '.modal__content',
      '.verify',
      '.ad-skip-btn',
      '.ad-mute-btn',
      '.skip_button',
      '.player-toasts'
    ].join(',');
    return !!el.closest(controlSelector);
  }

  function togglePlayerFullscreen(target) {
    const playerEls = [
      target && target.closest ? target.closest('.player-container') : null,
      document.querySelector('.player-container'),
      document.querySelector('video.video-element'),
      document.querySelector('video')
    ];
    for (const el of playerEls) {
      if (!el) continue;
      const comp = el.__vueParentComponent || el._vnode?.component || el.__vue_app__;
      if (comp) {
        const ctx = comp.proxy || comp.ctx || comp.setupState;
        if (ctx && typeof ctx.toggleFullscreen === 'function') {
          ctx.toggleFullscreen();
          return;
        }
      }
    }

    if (document.fullscreenElement) {
      if (document.exitFullscreen) {
        document.exitFullscreen();
      }
    } else {
      const container = (target && target.closest ? (target.closest('.player-container') || target.closest('.player')) : null)
        || document.querySelector('.player-container')
        || document.querySelector('video');
      if (container && container.requestFullscreen) {
        container.requestFullscreen();
      }
    }
  }

  let initialPlaying = null;
  let lastInteractionTime = 0;

  window.addEventListener('mousedown', (e) => {
    const rawTarget = (e.composedPath && e.composedPath()[0]) || e.target;
    if (!rawTarget || !rawTarget.closest) return;
    if (!rawTarget.closest('.player-container, video, .player, [class*="player"]')) return;
    if (isPlayerControl(rawTarget)) return;

    const now = Date.now();
    if (now - lastInteractionTime > 400) {
      const v = document.querySelector('video.video-element') || document.querySelector('video');
      initialPlaying = v ? !v.paused : null;
    }
    lastInteractionTime = now;
  });

  window.addEventListener('dblclick', (e) => {
    const rawTarget = (e.composedPath && e.composedPath()[0]) || e.target;
    if (!rawTarget || !rawTarget.closest) return;
    const player = rawTarget.closest('.player-container, video, .player, [class*="player"]');
    if (!player) return;
    if (isPlayerControl(rawTarget)) return;

    e.defaultPrevented = true;
    e.propagationStopped = true;

    togglePlayerFullscreen(rawTarget);

    if (initialPlaying !== null) {
      const expected = initialPlaying;
      setTimeout(() => {
        const v = document.querySelector('video.video-element') || document.querySelector('video');
        if (v) {
          if (expected && v.paused) {
            v.play();
          } else if (!expected && !v.paused) {
            v.pause();
          }
        }
      }, 60);
    }
  });

  return {
    window,
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

  const evt1 = { target: touchZone };
  env.window.dispatchEvent('mousedown', evt1);

  const dblEvt = { target: touchZone };
  env.window.dispatchEvent('dblclick', dblEvt);

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

  const evt1 = { target: touchZone };
  env.window.dispatchEvent('mousedown', evt1);

  const dblEvt = { target: touchZone };
  env.window.dispatchEvent('dblclick', dblEvt);

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

  const evt1 = { target: button };
  env.window.dispatchEvent('mousedown', evt1);

  const dblEvt = { target: button, defaultPrevented: false };
  env.window.dispatchEvent('dblclick', dblEvt);

  assert.equal(env.vueToggleFullscreenCalled, 0);
  assert.equal(dblEvt.defaultPrevented, false);
});

test('Fallback to native requestFullscreen and exitFullscreen when Vue context is absent', async () => {
  const env = createMockEnvironment({ isFullscreen: false, isPlaying: true });
  delete env.playerContainer.__vueParentComponent;

  const target = env.playerContainer;
  env.window.dispatchEvent('mousedown', { target });
  env.window.dispatchEvent('dblclick', { target });

  assert.equal(env.nativeRequestFullscreenCalled, 1);
  assert.equal(env.document.fullscreenElement, env.playerContainer);

  // Now exit fullscreen
  env.window.dispatchEvent('mousedown', { target });
  env.window.dispatchEvent('dblclick', { target });

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
  env.window.dispatchEvent('mousedown', { target: touchZone });
  env.video.pause();

  // Double click fires, video was left paused
  env.window.dispatchEvent('dblclick', { target: touchZone });

  // Wait for recovery timeout
  await new Promise(r => setTimeout(r, 80));

  // Should have been restored to play
  assert.equal(env.video.paused, false);
  assert.equal(env.video.playCalled, 1);
});
