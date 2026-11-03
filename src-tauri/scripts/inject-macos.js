// Portable macOS site enhancements. No stream interception, MPV or VPN hooks.
(function() {
    if (window.top !== window || location.origin !== 'https://anivox.fun') return;
    let episodePending = false;
    const staleVideoSources = new WeakMap();
    const staleMediaUrls = new Set();
    function resolveUrl(u, base = document.baseURI || location.href) {
        if (!u || typeof u !== 'string') return null;
        const value = u.trim();
        if (!value || value.toLowerCase() === 'premium') return null;
        try {
            const url = new URL(value, base);
            return url.protocol === 'https:' || url.protocol === 'http:' ? url.href : null;
        } catch (_) {
            return null;
        }
    }

    function playerContext(el) {
        if (!el) return null;
        const comp = el && (el.__vueParentComponent || el._vnode?.component || el.__vue_app__);
        if (comp) return comp.proxy || comp.ctx || comp.setupState;
        // Production Vue omits __vueParentComponent on DOM nodes. Its live
        // VNode tree still records the component that rendered each element.
        const root = document.querySelector('#app');
        const pending = [{node: root?._vnode, owner: null}];
        const seen = new Set();
        while (pending.length) {
            const {node, owner} = pending.pop();
            if (!node || typeof node !== 'object' || seen.has(node)) continue;
            seen.add(node);
            const component = node.component || owner;
            // Component VNodes share their child's root element. Descend to
            // the rendered DOM VNode to avoid returning a wrapper's proxy.
            if (!node.component && !node.suspense && node.el === el && component) {
                return component.proxy || component.ctx || component.setupState;
            }
            if (node.component?.subTree) pending.push({node: node.component.subTree, owner: node.component});
            if (node.suspense?.activeBranch) pending.push({node: node.suspense.activeBranch, owner: component});
            if (Array.isArray(node.children)) {
                for (const child of node.children) pending.push({node: child, owner: component});
            }
        }
        return null;
    }

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

    function setupCommonStyles() {
        const style = document.createElement('style');
        style.id = 'anivox-common-styles';
        style.textContent = `
            /* Native CSS smooth scrolling */
            html {
                scroll-behavior: smooth !important;
            }
            /* Disable hover repaints during scrolling to keep frametimes flat */
            body.is-scrolling * {
                pointer-events: none !important;
            }
            .touch-zone {
                background: transparent !important;
            }
            .libassjs-canvas {
                pointer-events: none !important;
            }
            /* Eliminate expensive GPU readback backdrop blur over video to maintain steady 60 FPS */
            .player-container *,
            .player-container [class*="controls"] *,
            .progress-bar,
            .player-toast,
            .modal,
            .search_window,
            .arcs-modal-overlay {
                backdrop-filter: none !important;
                -webkit-backdrop-filter: none !important;
            }
            /* Clean dark styling for controls without blur overhead */
            .player-container .controls .controls-bar .audio,
            .player-container .controls .controls-bar .subtitle,
            .player-container .controls .controls-bar .volume,
            .player-container .controls .controls-bar .play,
            .player-container .controls .controls-bar .play-button,
            .player-container .controls .controls-bar .fullscreen,
            .player-container .controls .controls-bar .quality,
            .player-container .controls .controls-bar .chrome-cast,
            .player-container .controls .controls-bar .skip img,
            .player-container .controls .controls-bar .volume-percentage,
            .player-container .preview-container .play-button,
            .ad-mute-btn,
            .ad-skip-btn {
                background-color: rgba(22, 22, 22, 0.78) !important;
            }
            .progress-bar {
                background-color: rgba(255, 255, 255, 0.25) !important;
            }
        `;
        style.textContent += `
            #anivox-macos-controls { position: fixed; right: 12px; bottom: 12px; z-index: 2147483000;
                display: flex; gap: 4px; padding: 6px; border-radius: 12px; background: #161616ed;
                border: 1px solid #ffffff24; box-shadow: 0 4px 20px #0006; }
            #anivox-macos-controls button { color: #eee; background: #ffffff10; border: 0;
                border-radius: 7px; padding: 8px 12px; cursor: pointer; font: 16px system-ui; }
            #anivox-macos-controls button:hover { background: #ffffff28; }
            #anivox-macos-controls button:focus-visible { outline: 2px solid #ec8ec5; }
            :fullscreen #anivox-macos-controls { display: none; }
        `;
        (document.head || document.documentElement).appendChild(style);
        if (!document.getElementById('anivox-macos-controls')) {
            const controls = document.createElement('nav');
            controls.id = 'anivox-macos-controls';
            controls.setAttribute('aria-label', 'Управление Anivox');
            for (const [text, label, action] of [
                ['←', 'Назад', () => window.history.back()],
                ['→', 'Вперёд', () => window.history.forward()],
                ['↻', 'Обновить страницу', () => location.reload()],
                ['⛶', 'Полный экран плеера', () => togglePlayerFullscreen(document.querySelector('video'))],
            ]) {
                const button = document.createElement('button');
                button.type = 'button';
                button.textContent = text;
                button.title = label;
                button.setAttribute('aria-label', label);
                button.addEventListener('click', action);
                controls.appendChild(button);
            }
            document.body.appendChild(controls);
        }

        // Disable hover calculations while user is actively scrolling
        let scrollDebounceTimer = null;
        window.addEventListener('scroll', function() {
            if (document.body && !document.body.classList.contains('is-scrolling')) {
                document.body.classList.add('is-scrolling');
            }
            clearTimeout(scrollDebounceTimer);
            scrollDebounceTimer = setTimeout(function() {
                if (document.body) {
                    document.body.classList.remove('is-scrolling');
                }
            }, 100);
        }, { passive: true });
    }

    if (document.readyState === 'loading') {
        document.addEventListener('DOMContentLoaded', setupCommonStyles);
    } else {
        setupCommonStyles();
    }


    let lastEpisodeIdentity = null;
    function syncEpisodeIdentity() {
        const page = new URL(location.href);
        const player = playerContext(document.querySelector('.player-container'));
        const selected = document.querySelector('.anime__info-episodes-episodes-item.active');
        const identity = page.pathname + page.search + ':' + (player?.episode || selected?.textContent || '');
        if (lastEpisodeIdentity !== null && identity !== lastEpisodeIdentity) {
            const video = document.querySelector('video.video-element') || document.querySelector('video');
            if (video) staleVideoSources.set(video, video.currentSrc || video.src || '');
        }
        lastEpisodeIdentity = identity;
    }
    setInterval(() => updateDiscord(), 2000);
    let discordBufferingVideo = null;
    // Discord uses the current anime component and native video state, never media URLs.
    function discordPresence() {
        const page = new URL(location.href);
        const routeId = /^\/anime\/(\d+)(?:-|\/|$)/.exec(page.pathname)?.[1];
        const animeCtx = playerContext(document.querySelector('.anime'));
        const anime = animeCtx?.anime;
        const stale = routeId && anime?.id != null && String(anime.id) !== routeId;
        const isAnime = !!routeId && !stale;
        const text = value => typeof value === 'string' ? value.trim() : '';
        const heading = document.querySelector('.anime__info-title-name');
        // The heading also contains a tooltip with translated/alternative titles.
        const headingText = text(Array.from(heading?.childNodes || [])
            .filter(node => node.nodeType === 3).map(node => node.textContent).join(' '));
        const title = isAnime
            ? text(anime?.title) || headingText || text(heading?.innerText) || text(heading?.textContent)
            : '';
        const fallback = stale ? 'Выбирает аниме' : text(document.title).replace(/\s*[-—|]\s*Anivox.*$/i, '');
        const details = Array.from(title || fallback || 'Anivox').slice(0, 128).join('');
        const player = playerContext(document.querySelector('.player-container'));
        const selectedEpisode = document.querySelector(
            '.anime__info-episodes-episodes-item.active .anime__info-episodes-episodes-item-info-title');
        const episode = Number(player?.episode || animeCtx?.episode
            || /(\d+)\s*серия/i.exec(selectedEpisode?.textContent || '')?.[1]);
        const selectedSeason = document.querySelector('.anime__info-episodes-seasons-item.active');
        const season = text(String(anime?.season_label || '')).replace(/\s*сезон\s*$/i, '')
            || /(\d+)\s*сезон/i.exec(selectedSeason?.textContent || '')?.[1];
        const video = document.querySelector('video.video-element') || document.querySelector('video');
        const currentVideo = isAnime && !episodePending && video
            && staleVideoSources.get(video) !== (video.currentSrc || video.src || '')
            && !staleMediaUrls.has(resolveUrl(video.currentSrc || video.src));
        const parts = [];
        if (isAnime && Number.isInteger(episode) && episode > 0) parts.push(`Серия ${episode}`);
        if (isAnime && season) parts.push(`Сезон ${season}`);
        let endTimestamp = null;
        if (currentVideo) {
            if (video.ended) parts.push('Просмотр завершён');
            else if (video.readyState < 2 || discordBufferingVideo === video) parts.push('Загрузка');
            else if (video.paused) parts.push('На паузе');
            else if (Number.isFinite(video.duration) && Number.isFinite(video.currentTime)
                && video.duration > video.currentTime && video.playbackRate > 0) {
                const remaining = (video.duration - video.currentTime) / video.playbackRate;
                if (Number.isFinite(remaining) && remaining < 7 * 86400) {
                    endTimestamp = Math.floor(Date.now() / 1000 + remaining);
                }
            }
        } else {
            parts.push(isAnime ? 'Выбирает серию' : 'Просматривает сайт');
        }
        const posterUrl = isAnime ? resolveUrl(anime?.poster
            || document.querySelector('.anime__info-poster')?.src) : null;
        const shikimoriId = Number(anime?.shikimori_id);
        const shikimoriUrl = !isAnime || !title ? null
            : Number.isInteger(shikimoriId) && shikimoriId > 0
                ? `https://shikimori.io/animes/${shikimoriId}`
                : `https://shikimori.io/animes?search=${encodeURIComponent(title)}`;
        return {
            details,
            stateText: Array.from(parts.join(' · ')).slice(0, 128).join(''),
            posterUrl,
            animeUrl: isAnime ? `${page.origin}${page.pathname}` : null,
            shikimoriUrl,
            endTimestamp,
        };
    }

    // Acknowledged snapshots are deduplicated; heartbeat still probes the connection.
    let lastRpcKey = '';
    let rpcDebounceTimer = null;
    let rpcInFlight = false;

    function updateDiscord(force = false) {
        syncEpisodeIdentity();
        const key = JSON.stringify(discordPresence());
        if (rpcDebounceTimer) clearTimeout(rpcDebounceTimer);
        if (!force && key === lastRpcKey) return;
        rpcDebounceTimer = setTimeout(async () => {
            if (rpcInFlight || !window.__TAURI__?.core) return;
            rpcInFlight = true;
            const presence = discordPresence();
            const sentKey = JSON.stringify(presence);
            try {
                await window.__TAURI__.core.invoke('set_discord_rpc', { presence });
                lastRpcKey = sentKey;
            } catch (_) {
                // Leave the title unacknowledged so a later update can retry it.
            } finally {
                rpcInFlight = false;
                if (JSON.stringify(discordPresence()) !== sentKey) updateDiscord();
            }
        }, 1000);
    }
    const o = new MutationObserver(() => updateDiscord());
    const t = document.querySelector('title');
    if (t) o.observe(t, { subtree: true, characterData: true, childList: true });
    for (const event of ['play', 'playing', 'canplay', 'waiting', 'stalled', 'pause', 'seeking',
        'seeked', 'loadedmetadata', 'durationchange', 'ratechange', 'ended', 'emptied']) {
        document.addEventListener(event, e => {
            if (e.target === document.querySelector('video.video-element')
                || e.target === document.querySelector('video')) {
                if (event === 'waiting') discordBufferingVideo = e.target;
                else if (event === 'stalled') {
                    if (e.target.readyState < 3) discordBufferingVideo = e.target;
                } else if (['playing', 'canplay', 'pause', 'emptied'].includes(event)) {
                    discordBufferingVideo = null;
                }
                updateDiscord();
            }
        }, true);
    }
    // Probe even an unchanged status to recover after Discord starts or restarts.
    setInterval(() => updateDiscord(true), 30000);
    updateDiscord();
})();
