// Common injection: MPV interception, fullscreen controls, UI styles & Discord RPC
(function() {
    // Multi-layer video stream & subtitle interceptor for MPV
    let mpvSession = null;
    let mpvBinding = null;
    let cancelResume = null;
    let apiLinks = null;
    let currentHlsUrl = null;
    let lastStreamUrl = null;
    let lastSubUrl = null;
    let mediaGeneration = 0;
    let episodeUrl = null;
    let episodePending = false;
    let staleMediaUrls = new Set();
    let stalePlayerContexts = new WeakMap();
    let staleVideoSources = new WeakMap();
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

    function linksKey(links) {
        if (typeof links === 'string') return links;
        if (!links || typeof links !== 'object') return null;
        return JSON.stringify(Object.entries(links).filter(([, value]) => typeof value === 'string').sort());
    }

    function resetMediaCache() {
        cancelResume?.();
        mpvBinding = null;
        mpvSession?.stop().catch(console.error);
        const hadCachedMedia = !!(apiLinks || currentHlsUrl || lastStreamUrl || lastSubUrl);
        if (hadCachedMedia || !staleMediaUrls.size) staleBase = mediaBase;
        const old = [currentHlsUrl, lastStreamUrl, lastSubUrl];
        const rememberLinks = links => {
            if (typeof links === 'string') old.push(links);
            else if (links && typeof links === 'object') old.push(...Object.values(links));
        };
        rememberLinks(apiLinks);
        stalePlayerContexts = new WeakMap();
        staleVideoSources = new WeakMap();
        for (const el of playerElements()) {
            if (!el) continue;
            staleVideoSources.set(el, el.currentSrc || el.src || '');
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

    function setBtnStatus(text, color, resetAfterMs, detail = null) {
        window.__TAURI__.core.invoke('report_player_status', { text, color, resetAfterMs, detail }).catch(console.error);
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

    function playbackMedia() {
        const ctx = playerContext(document.querySelector('.player-container'));
        const anime = playerContext(document.querySelector('.anime'));
        const route = /^\/anime\/(\d+)(?:-|\/|$)/.exec(location.pathname)?.[1];
        const animeId = Number(ctx?.anime_id || anime?.anime?.id || route);
        const episode = Number(ctx?.episode || anime?.episode);
        const translation = Number(ctx?.translations_id || anime?.translations_id);
        const key = [animeId, episode, translation].every(n => Number.isSafeInteger(n) && n > 0)
            ? `${animeId}:${episode}:${translation}` : '';
        const presence = discordPresence();
        presence.stateText = presence.stateText.split(' · ').filter(part => /^Серия |^Сезон /.test(part)).join(' · ');
        presence.endTimestamp = null;
        return {key, presence};
    }

    function chooseResume(point) {
        return new Promise(resolve => {
            const overlay = document.createElement('div');
            overlay.setAttribute('role', 'dialog');
            overlay.setAttribute('aria-modal', 'true');
            overlay.setAttribute('aria-label', 'Продолжить просмотр');
            overlay.style.cssText = 'position:fixed;inset:0;z-index:2147483647;background:#000b;display:flex;align-items:center;justify-content:center';
            const box = document.createElement('div');
            box.style.cssText = 'background:#20212a;color:white;padding:24px;border-radius:12px;font:16px sans-serif;max-width:380px';
            const label = document.createElement('p');
            const seconds = Math.floor(point.position);
            label.textContent = `Продолжить просмотр с ${Math.floor(seconds / 60)}:${String(seconds % 60).padStart(2, '0')}?`;
            box.append(label);
            const finish = value => { overlay.remove(); document.removeEventListener('keydown', escape, true); cancelResume = null; resolve(value); };
            const escape = event => { if (event.key === 'Escape') { event.preventDefault(); event.stopPropagation(); finish(null); } };
            for (const [title, value] of [['Продолжить', point.position], ['С начала', 0], ['Отмена', null]]) {
                const button = document.createElement('button');
                button.textContent = title;
                button.style.cssText = 'margin:4px;padding:10px;cursor:pointer';
                button.addEventListener('click', () => finish(value));
                box.append(button);
            }
            overlay.append(box); document.body.append(overlay);
            document.addEventListener('keydown', escape, true);
            cancelResume = () => finish(null);
            box.querySelector('button').focus();
        });
    }

    mpvSession = window.__anivoxCreateMpvSession?.({
        getVideo: () => document.querySelector('video.video-element') || document.querySelector('video'),
        getContext: () => playerContext(document.querySelector('.player-container')),
        getIdentity: () => playbackMedia().key,
        getGeneration: () => mediaGeneration,
        changeQuality: async (context, choice) => {
            const keys = [`quality__${context.translations_id}_${context.anime_id}`, `quality_${context.translations_id}_${context.anime_id}`];
            const saved = keys.map(key => localStorage.getItem(key));
            const preset = context.qualityPreset;
            const generation = mediaGeneration;
            const identity = playbackMedia().key;
            try { await context.changeQuality(choice); }
            finally {
                if (generation === mediaGeneration && identity === playbackMedia().key) {
                    keys.forEach((key, index) => {
                        if (saved[index] == null) localStorage.removeItem(key);
                        else localStorage.setItem(key, saved[index]);
                    });
                    context.qualityPreset = preset;
                }
            }
        },
    });
    let mpvSyncPending = false;
    async function syncMpv() {
        if (!mpvBinding || mpvSyncPending || !mpvSession) return;
        mpvSyncPending = true;
        const binding = mpvBinding;
        try {
            const sample = await window.__TAURI__.core.invoke('get_mpv_state');
            if (mpvBinding !== binding) return;
            await mpvSession.update(sample);
            if (!sample || sample.sessionId !== binding.sessionId) mpvBinding = null;
        } catch (_) {
            if (mpvBinding === binding) { mpvBinding = null; await mpvSession.stop(); }
        } finally { mpvSyncPending = false; }
    }
    setInterval(syncMpv, 1000);
    document.addEventListener('ended', event => {
        if (mpvBinding?.mirrorEmbedded && event.target === (document.querySelector('video.video-element') || document.querySelector('video'))) {
            event.stopImmediatePropagation();
        }
    }, true);
    window.addEventListener('pagehide', () => { mpvBinding = null; mpvSession?.stop().catch(console.error); });

    async function launchInMpv() {
        setBtnStatus('⌛ Поиск...', '#ffeb3b', 0);
        const video = document.querySelector('video.video-element') || document.querySelector('video');
        const info = findActiveStream();

        if (!info || !info.url) {
            console.warn('[Anivox-MPV] Stream URL not found yet');
            setBtnStatus('⚠ Включите серию!', '#ff5252', 2500);
            return;
        }

        let startTime = (video && video.currentTime) ? video.currentTime : (info.time || 0);
        const generation = mediaGeneration;
        const media = playbackMedia();

        const qLabel = info.quality && info.quality !== 'auto' ? ` (${info.quality})` : ' (MAX)';
        setBtnStatus(`▶ Запуск MPV${qLabel}...`, '#69f0ae', 0);

        let title = document.title || 'Anivox';
        title = title.replace(/ — Anivox| - Anivox| \| Anivox/gi, '').trim();
        const token = localStorage.getItem('token') || null;

        try {
            if (window.__TAURI__ && window.__TAURI__.core) {
                if (media.key) {
                    const saved = await window.__TAURI__.core.invoke('get_playback_resume', {key: media.key});
                    if (saved && Math.abs(saved.position - startTime) > 8) {
                        const chosen = await chooseResume(saved);
                        if (chosen == null) return;
                        startTime = chosen;
                    }
                }
                if (generation !== mediaGeneration) return;
                const launched = await window.__TAURI__.core.invoke('open_in_mpv', {
                    url: info.url,
                    title: title || null,
                    startTime: startTime > 0 ? startTime : null,
                    subUrl: lastSubUrl || null,
                    token: token,
                    media,
                });
                if (mpvSession && launched?.sessionId && generation === mediaGeneration) {
                    await mpvSession.begin(launched, media.key);
                    mpvBinding = launched;
                    await syncMpv();
                } else if (!mpvSession && video && !video.paused) video.pause();
                setBtnStatus(`✔ Запущен${qLabel}!`, '#69f0ae', 2500);
            } else {
                throw new Error('Tauri API недоступен');
            }
        } catch (err) {
            console.error('[Anivox-MPV] Launch error:', err);
            setBtnStatus('⚠ Ошибка!', '#ff5252', 3000, `MPV: ${String(err)}`);
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
