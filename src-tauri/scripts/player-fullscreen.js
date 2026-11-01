// Fullscreen controls for the Anivox web player: sync with the native window and double-click toggle
(function() {
    if (window !== window.top || location.origin !== 'https://anivox.fun') return;

    document.addEventListener('fullscreenchange', () => {
        window.__TAURI__.core.invoke('content_fullscreen', {
            fullscreen: !!document.fullscreenElement
        }).catch(console.error);
    });

    // Double-click on web player to toggle fullscreen on and off
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
                    try {
                        ctx.toggleFullscreen();
                        return;
                    } catch (e) {
                        console.warn('[Anivox] Failed to toggle fullscreen via Vue ctx:', e);
                    }
                }
            }
        }

        if (document.fullscreenElement) {
            if (document.exitFullscreen) {
                document.exitFullscreen().catch(() => {});
            } else if (document.webkitExitFullscreen) {
                document.webkitExitFullscreen();
            }
        } else {
            const container = (target && target.closest ? (target.closest('.player-container') || target.closest('.player') || target.closest('[class*="player"]')) : null)
                || document.querySelector('.player-container')
                || document.querySelector('.player')
                || document.querySelector('video');
            if (container) {
                if (container.requestFullscreen) {
                    container.requestFullscreen({ navigationUI: 'hide' }).catch(() => {
                        const video = document.querySelector('video');
                        if (video && video.requestFullscreen) {
                            video.requestFullscreen().catch(() => {});
                        }
                    });
                } else if (container.webkitRequestFullscreen) {
                    container.webkitRequestFullscreen();
                }
            }
        }
    }

    const DOUBLE_CLICK_INTERVAL_MS = 250;
    let initialPlaying = null;
    let lastInteractionTime = -Infinity;
    let clickInterval = Infinity;

    window.addEventListener('mousedown', (e) => {
        const rawTarget = (e.composedPath && e.composedPath()[0]) || e.target;
        if (!rawTarget || !rawTarget.closest) return;
        if (!rawTarget.closest('.player-container, video, .player, [class*="player"]')) return;
        if (isPlayerControl(rawTarget) || e.button !== 0) return;

        const now = performance.now();
        clickInterval = now - lastInteractionTime;
        if (clickInterval > DOUBLE_CLICK_INTERVAL_MS) {
            const v = document.querySelector('video.video-element') || document.querySelector('video');
            initialPlaying = v ? !v.paused : null;
        }
        lastInteractionTime = now;
    }, true);

    window.addEventListener('dblclick', (e) => {
        const rawTarget = (e.composedPath && e.composedPath()[0]) || e.target;
        if (!rawTarget || !rawTarget.closest) return;
        const player = rawTarget.closest('.player-container, video, .player, [class*="player"]');
        if (!player) return;
        if (isPlayerControl(rawTarget)) return;

        e.preventDefault();
        e.stopPropagation();

        // Suppress the site's/system's longer double-click window as well.
        if (clickInterval > DOUBLE_CLICK_INTERVAL_MS) return;
        clickInterval = Infinity;
        lastInteractionTime = -Infinity;

        togglePlayerFullscreen(rawTarget);

        if (initialPlaying !== null) {
            const expected = initialPlaying;
            setTimeout(() => {
                const v = document.querySelector('video.video-element') || document.querySelector('video');
                if (v) {
                    if (expected && v.paused) {
                        v.play().catch(() => {});
                    } else if (!expected && !v.paused) {
                        v.pause();
                    }
                }
            }, 60);
        }
    }, true);
})();
