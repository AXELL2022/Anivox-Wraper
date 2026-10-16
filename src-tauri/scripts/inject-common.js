// Common injection: MPV interception, fullscreen controls, UI styles & Discord RPC
(function() {
    // Multi-layer video stream & subtitle interceptor for MPV
    let apiLinks = null;
    let currentHlsUrl = null;
    let lastStreamUrl = null;
    let lastSubUrl = null;
    let mediaGeneration = 0;
    let episodeUrl = null;
    let episodePending = false;
    let staleMediaUrls = new Set();
    let stalePlayerContexts = new WeakMap();
    let mediaPage = pageKey();
    let mediaBase = document.baseURI || location.href;
    let staleBase = mediaBase;

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

    function playerElements() {
        return [document.querySelector('.player-container'),
            document.querySelector('video.video-element'), document.querySelector('video')];
    }

    function playerContext(el) {
        const comp = el && (el.__vueParentComponent || el._vnode?.component || el.__vue_app__);
        return comp && (comp.proxy || comp.ctx || comp.setupState);
    }

    function linksKey(links) {
        if (typeof links === 'string') return links;
        if (!links || typeof links !== 'object') return null;
        return JSON.stringify(Object.entries(links).filter(([, value]) => typeof value === 'string').sort());
    }

    function resetMediaCache() {
        const hadCachedMedia = !!(apiLinks || currentHlsUrl || lastStreamUrl || lastSubUrl);
        if (hadCachedMedia || !staleMediaUrls.size) staleBase = mediaBase;
        const old = [currentHlsUrl, lastStreamUrl, lastSubUrl];
        const rememberLinks = links => {
            if (typeof links === 'string') old.push(links);
            else if (links && typeof links === 'object') old.push(...Object.values(links));
        };
        rememberLinks(apiLinks);
        stalePlayerContexts = new WeakMap();
        for (const el of playerElements()) {
            if (!el) continue;
            old.push(el.currentSrc, el.src);
            const ctx = playerContext(el);
            if (ctx) {
                stalePlayerContexts.set(ctx, {
                    links: linksKey(ctx.links), videoSrc: ctx.videoSrc, hls: ctx.hls?.url,
                });
                rememberLinks(ctx.links);
                old.push(ctx.videoSrc, ctx.hls?.url);
            }
        }
        old.push(document.querySelector('iframe')?.src);
        staleMediaUrls = new Set([
            ...(hadCachedMedia ? [] : staleMediaUrls),
            ...old.map(value => resolveUrl(value, staleBase)).filter(Boolean),
        ]);
        apiLinks = currentHlsUrl = lastStreamUrl = lastSubUrl = null;
        episodePending = false;
        mediaGeneration++;
    }

    function pageKey() {
        const url = new URL(location.href);
        // Ordinary anchors (e.g. #comments) do not change the episode.
        return url.pathname + url.search + (url.hash.startsWith('#/') ? url.hash : '');
    }

    function syncMediaPage() {
        const next = pageKey();
        if (next !== mediaPage) {
            resetMediaCache();
            mediaPage = next;
            mediaBase = document.baseURI || location.href;
            episodeUrl = null;
        }
    }

    for (const method of ['pushState', 'replaceState']) {
        const original = window.history[method];
        window.history[method] = function() {
            const result = original.apply(this, arguments);
            syncMediaPage();
            return result;
        };
    }
    window.addEventListener('popstate', syncMediaPage);
    window.addEventListener('hashchange', syncMediaPage);

    function beginMediaRequest(value) {
        syncMediaPage();
        const url = resolveUrl(value);
        const episode = !!url && /\/episodes(?:\/|$)/.test(new URL(url).pathname);
        if (episode && url !== episodeUrl) {
            resetMediaCache();
            episodeUrl = url;
            episodePending = true;
        }
        return { url, episode, generation: mediaGeneration, base: document.baseURI || location.href };
    }

    function isCurrentRequest(request) {
        syncMediaPage();
        return request.url && request.generation === mediaGeneration;
    }

    function captureEpisode(request, data) {
        if (!isCurrentRequest(request)) return;
        const links = data && data.links;
        if (typeof links === 'string') apiLinks = resolveUrl(links, request.base);
        else if (links && typeof links === 'object') {
            apiLinks = Object.fromEntries(Object.entries(links).map(([quality, link]) =>
                [quality, resolveUrl(link, request.base)]));
        } else apiLinks = null;
        episodePending = false;
    }

    function finishEpisode(request) {
        if (request.episode && isCurrentRequest(request)) episodePending = false;
    }

    function captureMedia(request) {
        if (!isCurrentRequest(request) || request.episode || staleMediaUrls.has(request.url)) return;
        const path = new URL(request.url).pathname.toLowerCase();
        if (path.endsWith('.ass')) {
            lastSubUrl = request.url;
        } else if (!/\.(ts|m4s|aac|mp3|vtt|srt)$/.test(path)
            && !path.includes('segment') && !path.includes('/audio/') && !path.includes('/subs/')) {
            if (path.includes('/api/hls/')) {
                currentHlsUrl = lastStreamUrl = request.url;
            } else if (path.includes('.m3u8') || path.includes('.mp4') || path.includes('/stream')) {
                lastStreamUrl = request.url;
            }
        }
    }

    // 1. Hook XMLHttpRequest for episodes API and stream manifests
    try {
        const origXhrOpen = XMLHttpRequest.prototype.open;
        const origXhrSend = XMLHttpRequest.prototype.send;
        XMLHttpRequest.prototype.open = function(method, url) {
            this._anivoxUrl = typeof url === 'string' ? url : (url && url.href ? url.href : String(url));
            return origXhrOpen.apply(this, arguments);
        };
        XMLHttpRequest.prototype.send = function() {
            const request = beginMediaRequest(this._anivoxUrl);
            this.addEventListener('loadend', function() {
                try {
                    if (!isCurrentRequest(request) || this.status < 200 || this.status >= 300) return;
                    if (request.episode) {
                        const data = this.responseType === 'json' ? this.response : JSON.parse(this.responseText);
                        captureEpisode(request, data);
                    } else {
                        captureMedia(request);
                    }
                } catch (e) {} finally {
                    finishEpisode(request);
                }
            }, { once: true });
            return origXhrSend.apply(this, arguments);
        };

        // 2. Hook Fetch API
        const origFetch = window.fetch;
        window.fetch = async function(input, init) {
            const request = beginMediaRequest(typeof input === 'string' ? input : (input?.url || input?.href));
            let response;
            try {
                response = await origFetch.apply(this, arguments);
            } catch (e) {
                finishEpisode(request);
                throw e;
            }
            try {
                if (response.ok && isCurrentRequest(request)) {
                    if (request.episode) {
                        response.clone().json().then(data => captureEpisode(request, data))
                            .catch(() => finishEpisode(request));
                    } else {
                        captureMedia(request);
                    }
                } else {
                    finishEpisode(request);
                }
            } catch (e) {
                finishEpisode(request);
            }
            return response;
        };
    } catch (e) {
        console.error('[Anivox-MPV] Failed to hook network:', e);
    }

    // The local toolbar is separate; only player hooks belong in the site.
    if (window !== window.top || location.origin !== 'https://anivox.fun') return;

    function setBtnStatus(text, color, resetAfterMs) {
        window.__TAURI__.core.invoke('report_player_status', { text, color, resetAfterMs }).catch(console.error);
    }

    function parseQualityHeight(q) {
        if (!q) return 0;
        const s = String(q).toUpperCase().replace('~', '').replace('_UPSCALE', '').trim();
        if (s === '4K' || s.includes('2160')) return 2160;
        if (s === '2K' || s.includes('1440')) return 1440;
        return parseInt(s) || 0;
    }

    function selectMaxStreamUrl(links) {
        if (!links) return null;
        if (typeof links === 'string') {
            const url = resolveUrl(links);
            return url ? { url, quality: 'MAX' } : null;
        }
        if (typeof links !== 'object') return null;

        // Sort descending by resolution (4K / 2160 > 2K / 1440 > 1080 > 720 > 480 > 360)
        const sortedKeys = Object.keys(links).sort((a, b) => parseQualityHeight(b) - parseQualityHeight(a));
        for (const k of sortedKeys) {
            const url = resolveUrl(links[k]);
            if (url) return { url, quality: k };
        }
        return null;
    }

    function findActiveStream() {
        syncMediaPage();
        if (episodePending) return null;
        const candidate = (value, time = 0, quality = 'MAX') => {
            const url = resolveUrl(value);
            return url && !staleMediaUrls.has(url) ? { url, time, quality } : null;
        };
        // 1. Captured API links from episodes endpoint - FORCED MAX QUALITY!
        if (apiLinks) {
            const res = selectMaxStreamUrl(apiLinks);
            if (res && res.url) {
                console.log('[Anivox-MPV] Selected MAX quality stream from apiLinks:', res.quality, ':', res.url);
                return { url: res.url, time: 0, quality: res.quality };
            }
        }

        // 2. Direct inspection of Vue 3 component state (.player-container)
        const playerEls = playerElements();
        for (const el of playerEls) {
            const ctx = playerContext(el);
            if (!ctx) continue;
            const stale = stalePlayerContexts.get(ctx);
            const selected = !stale || stale.links !== linksKey(ctx.links)
                ? selectMaxStreamUrl(ctx.links) : null;
            const best = candidate(selected?.url, ctx.currentTime || 0, selected?.quality);
            if (best) return best;
            for (const value of [
                !stale || stale.videoSrc !== ctx.videoSrc ? ctx.videoSrc : null,
                !stale || stale.hls !== ctx.hls?.url ? ctx.hls?.url : null,
            ]) {
                const stream = candidate(value, ctx.currentTime || 0);
                if (stream) return stream;
            }
        }

        // 3-4. Active HLS or other network-intercepted stream.
        for (const value of [currentHlsUrl, lastStreamUrl]) {
            const stream = candidate(value);
            if (stream) return stream;
        }

        // 5. HTML5 video tag currentSrc / src
        const video = document.querySelector('video');
        if (video) {
            for (const value of [video.currentSrc, video.src]) {
                const stream = candidate(value, video.currentTime || 0);
                if (stream) return stream;
            }
        }

        // 6. Fallback iframe player (Kodik / Sibnet etc.)
        const iframe = document.querySelector('iframe');
        return candidate(iframe?.src, 0, 'iframe');
    }

    async function launchInMpv() {
        setBtnStatus('⌛ Поиск...', '#ffeb3b', 0);
        const video = document.querySelector('video.video-element') || document.querySelector('video');
        const info = findActiveStream();

        if (!info || !info.url) {
            console.warn('[Anivox-MPV] Stream URL not found yet');
            setBtnStatus('⚠ Включите серию!', '#ff5252', 2500);
            return;
        }

        const startTime = (video && video.currentTime) ? video.currentTime : (info.time || 0);
        if (video && !video.paused) {
            video.pause();
        }

        const qLabel = info.quality && info.quality !== 'auto' ? ` (${info.quality})` : ' (MAX)';
        setBtnStatus(`▶ Запуск MPV${qLabel}...`, '#69f0ae', 0);

        let title = document.title || 'Anivox';
        title = title.replace(/ — Anivox| - Anivox| \| Anivox/gi, '').trim();
        const token = localStorage.getItem('token') || null;

        try {
            if (window.__TAURI__ && window.__TAURI__.core) {
                await window.__TAURI__.core.invoke('open_in_mpv', {
                    url: info.url,
                    title: title || null,
                    startTime: startTime > 0 ? startTime : null,
                    subUrl: lastSubUrl || null,
                    token: token
                });
                setBtnStatus(`✔ Запущен${qLabel}!`, '#69f0ae', 2500);
            } else {
                throw new Error('Tauri API недоступен');
            }
        } catch (err) {
            console.error('[Anivox-MPV] Launch error:', err);
            setBtnStatus('⚠ Ошибка!', '#ff5252', 3000);
        }
    }

    window.__anivoxLaunchMpv = launchInMpv;
    document.addEventListener('fullscreenchange', () => {
        window.__TAURI__.core.invoke('content_fullscreen', {
            fullscreen: !!document.fullscreenElement
        }).catch(console.error);
    });

    window.addEventListener('keydown', (e) => {
        const tag = document.activeElement ? document.activeElement.tagName : '';
        if (tag === 'INPUT' || tag === 'TEXTAREA' || (document.activeElement && document.activeElement.isContentEditable)) {
            return;
        }
        if (e.key === 'm' || e.key === 'M' || e.key === 'ь' || e.key === 'Ь') {
            e.preventDefault();
            launchInMpv();
        }
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
    }, true);

    window.addEventListener('dblclick', (e) => {
        const rawTarget = (e.composedPath && e.composedPath()[0]) || e.target;
        if (!rawTarget || !rawTarget.closest) return;
        const player = rawTarget.closest('.player-container, video, .player, [class*="player"]');
        if (!player) return;
        if (isPlayerControl(rawTarget)) return;

        e.preventDefault();
        e.stopPropagation();

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
        (document.head || document.documentElement).appendChild(style);

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

    // Discord RPC logic with deduplication and debouncing
    let lastRpcTitle = '';
    let rpcDebounceTimer = null;

    function updateDiscord() {
        const t = document.title || '';
        let d = t.endsWith(' - Anivox') ? t.replace(' - Anivox', '') : t;
        if (!d || d === lastRpcTitle) return;
        lastRpcTitle = d;

        if (rpcDebounceTimer) clearTimeout(rpcDebounceTimer);
        rpcDebounceTimer = setTimeout(() => {
            if (window.__TAURI__ && window.__TAURI__.core) {
                window.__TAURI__.core.invoke('set_discord_rpc', { details: 'Смотрит: ' + d, stateText: 'Anivox' }).catch(() => {});
            }
        }, 1000);
    }
    const o = new MutationObserver(updateDiscord);
    const t = document.querySelector('title');
    if (t) o.observe(t, { subtree: true, characterData: true, childList: true });
    setInterval(updateDiscord, 30000);
    updateDiscord();
})();
