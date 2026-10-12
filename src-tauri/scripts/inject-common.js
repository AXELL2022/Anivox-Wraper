// Common injection: MPV interception, fullscreen controls, UI styles & Discord RPC
(function() {
    // Multi-layer video stream & subtitle interceptor for MPV
    let apiLinks = null;
    let currentHlsUrl = null;
    let lastStreamUrl = null;
    let lastSubUrl = null;

    function resolveUrl(u) {
        if (!u || typeof u !== 'string') return null;
        if (u.startsWith('//')) return 'https:' + u;
        if (u.startsWith('http://')) return u.replace('http://', 'https://');
        if (!u.startsWith('http')) return 'https://' + u;
        return u;
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
            this.addEventListener('load', function() {
                try {
                    const u = this._anivoxUrl || '';
                    if (u.includes('episodes/')) {
                        const data = JSON.parse(this.responseText);
                        if (data && data.links) {
                            apiLinks = data.links;
                            console.log('[Anivox-MPV] Intercepted episode links from XHR:', apiLinks);
                        }
                    }
                    if (u.includes('/api/hls/')) {
                        currentHlsUrl = u;
                        lastStreamUrl = u;
                        console.log('[Anivox-MPV] Intercepted active HLS URL from XHR:', u);
                    } else if (u.includes('.m3u8') || u.includes('.mp4') || u.includes('/stream')) {
                        if (!u.includes('.ts') && !u.includes('segment') && !u.includes('/audio/') && !u.includes('/subs/')) {
                            lastStreamUrl = u;
                            console.log('[Anivox-MPV] Intercepted stream URL from XHR:', u);
                        }
                    }
                    if (u.includes('.ass')) {
                        lastSubUrl = u;
                    }
                } catch (e) {}
            });
            return origXhrSend.apply(this, arguments);
        };

        // 2. Hook Fetch API
        const origFetch = window.fetch;
        window.fetch = async function(input, init) {
            const u = typeof input === 'string' ? input : (input && input.url ? input.url : '');
            const response = await origFetch.apply(this, arguments);
            try {
                if (u.includes('episodes/')) {
                    const clone = response.clone();
                    clone.json().then(data => {
                        if (data && data.links) {
                            apiLinks = data.links;
                            console.log('[Anivox-MPV] Intercepted episode links from fetch:', apiLinks);
                        }
                    }).catch(() => {});
                }
                if (u.includes('/api/hls/')) {
                    currentHlsUrl = u;
                    lastStreamUrl = u;
                    console.log('[Anivox-MPV] Intercepted active HLS URL from fetch:', u);
                } else if (u.includes('.m3u8') || u.includes('.mp4') || u.includes('/stream')) {
                    if (!u.includes('.ts') && !u.includes('segment') && !u.includes('/audio/') && !u.includes('/subs/')) {
                        lastStreamUrl = u;
                        console.log('[Anivox-MPV] Intercepted stream URL from fetch:', u);
                    }
                }
                if (u.includes('.ass')) {
                    lastSubUrl = u;
                }
            } catch (e) {}
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
        if (typeof links === 'string') return { url: resolveUrl(links), quality: 'MAX' };

        // Sort descending by resolution (4K / 2160 > 2K / 1440 > 1080 > 720 > 480 > 360)
        const sortedKeys = Object.keys(links).sort((a, b) => parseQualityHeight(b) - parseQualityHeight(a));
        for (const k of sortedKeys) {
            if (links[k] && links[k] !== 'premium' && typeof links[k] === 'string') {
                console.log('[Anivox-MPV] Forced MAX quality selected:', k, '->', links[k]);
                return { url: resolveUrl(links[k]), quality: k };
            }
        }

        const firstK = Object.keys(links)[0];
        return { url: resolveUrl(links[firstK]), quality: firstK };
    }

    function findActiveStream() {
        // 1. Captured API links from episodes endpoint - FORCED MAX QUALITY!
        if (apiLinks && typeof apiLinks === 'object') {
            const res = selectMaxStreamUrl(apiLinks);
            if (res && res.url) {
                console.log('[Anivox-MPV] Selected MAX quality stream from apiLinks:', res.quality, ':', res.url);
                return { url: res.url, time: 0, quality: res.quality };
            }
        }

        // 2. Direct inspection of Vue 3 component state (.player-container)
        const playerEls = [
            document.querySelector('.player-container'),
            document.querySelector('video.video-element'),
            document.querySelector('video')
        ];
        for (const el of playerEls) {
            if (!el) continue;
            const comp = el.__vueParentComponent || el._vnode?.component || el.__vue_app__;
            if (comp) {
                const ctx = comp.ctx || comp.proxy || comp.setupState;
                if (ctx) {
                    if (ctx.links && typeof ctx.links === 'object') {
                        const res = selectMaxStreamUrl(ctx.links);
                        if (res && res.url) {
                            console.log('[Anivox-MPV] Selected MAX quality stream from Vue ctx.links:', res.quality, ':', res.url);
                            return { url: res.url, time: ctx.currentTime || 0, quality: res.quality };
                        }
                    }
                    if (ctx.videoSrc && typeof ctx.videoSrc === 'string' && !ctx.videoSrc.startsWith('blob:')) {
                        console.log('[Anivox-MPV] Found stream in Vue ctx.videoSrc:', ctx.videoSrc);
                        return { url: resolveUrl(ctx.videoSrc), time: ctx.currentTime || 0, quality: 'MAX' };
                    }
                    if (ctx.hls && ctx.hls.url) {
                        console.log('[Anivox-MPV] Found stream in Vue ctx.hls.url:', ctx.hls.url);
                        return { url: resolveUrl(ctx.hls.url), time: ctx.currentTime || 0, quality: 'MAX' };
                    }
                }
            }
        }

        // 3. Active HLS stream from network
        if (currentHlsUrl) {
            console.log('[Anivox-MPV] Fallback to currentHlsUrl:', currentHlsUrl);
            return { url: resolveUrl(currentHlsUrl), time: 0, quality: 'MAX' };
        }

        // 4. Other network intercepted stream URL (XHR / Fetch)
        if (lastStreamUrl && typeof lastStreamUrl === 'string' && !lastStreamUrl.startsWith('blob:')) {
            console.log('[Anivox-MPV] Fallback to lastStreamUrl:', lastStreamUrl);
            return { url: resolveUrl(lastStreamUrl), time: 0, quality: 'MAX' };
        }

        // 5. HTML5 video tag currentSrc / src
        const video = document.querySelector('video');
        if (video) {
            if (video.currentSrc && !video.currentSrc.startsWith('blob:')) {
                return { url: resolveUrl(video.currentSrc), time: video.currentTime || 0, quality: 'MAX' };
            }
            if (video.src && !video.src.startsWith('blob:')) {
                return { url: resolveUrl(video.src), time: video.currentTime || 0, quality: 'MAX' };
            }
        }

        // 6. Fallback iframe player (Kodik / Sibnet etc.)
        const iframe = document.querySelector('iframe');
        if (iframe && iframe.src && iframe.src.startsWith('http')) {
            console.log('[Anivox-MPV] Found stream in fallback iframe:', iframe.src);
            return { url: iframe.src, time: 0, quality: 'iframe' };
        }

        return null;
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
