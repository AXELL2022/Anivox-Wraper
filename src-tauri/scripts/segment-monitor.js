// Live monitor of video segment downloads (.ts/.m4s/...) for the local "connection" panel.
// Passive: observes progress of requests, never alters them. Unfinished requests count
// immediately (a stalled download is visible while it is still stalled), and the speed is
// measured over a short sliding window so it recovers quickly after a problem ends.
(() => {
    if (window !== window.top || location.origin !== 'https://anivox.fun') return;
    if (window.__anivoxSegmentMonitor) return;
    window.__anivoxSegmentMonitor = true;
    const MEDIA = /\.(ts|m4s|mp4|aac|m4a|cmfv|cmfa)(?:$|[?#])/i;
    const WINDOW_MS = 6000;   // sliding window for speed
    const RECENT_MS = 15000;  // how long finished requests influence counters
    const SLOW_MS = 8000;     // a single segment taking longer than this is "slow"
    const inflight = new Map();
    let chunks = [];          // {t, bytes}
    let done = [];            // {host, start, end, bytes, ok}
    let nextId = 0;
    let lastActivity = 0;
    let lastHost = '';
    let timer = null;

    const hostOf = url => { try { return new URL(url, location.href).host; } catch (_) { return ''; } };

    function summary(now) {
        chunks = chunks.filter(c => c.t >= now - RECENT_MS);
        done = done.filter(d => d.end >= now - RECENT_MS);
        const winStart = now - WINDOW_MS;
        const intervals = [];
        for (const d of done) if (d.end > winStart) intervals.push([Math.max(d.start, winStart), d.end]);
        for (const r of inflight.values()) intervals.push([Math.max(r.start, winStart), now]);
        intervals.sort((a, b) => a[0] - b[0]);
        let busy = 0, curStart = null, curEnd = null;
        for (const [s, e] of intervals) {
            if (curEnd === null || s > curEnd) {
                if (curEnd !== null) busy += curEnd - curStart;
                curStart = s; curEnd = e;
            } else if (e > curEnd) curEnd = e;
        }
        if (curEnd !== null) busy += curEnd - curStart;
        const bytes = chunks.filter(c => c.t >= winStart).reduce((sum, c) => sum + c.bytes, 0);
        const hasSpeed = busy >= 1000;
        let stalledMs = 0, slow = 0, maxMs = 0;
        for (const r of inflight.values()) {
            const elapsed = now - r.start;
            stalledMs = Math.max(stalledMs, now - Math.max(r.lastProgress, r.start));
            maxMs = Math.max(maxMs, elapsed);
            if (elapsed > SLOW_MS) slow++;
        }
        for (const d of done) {
            const took = d.end - d.start;
            maxMs = Math.max(maxMs, took);
            if (d.ok && took > SLOW_MS) slow++;
        }
        return {
            host: [...inflight.values()].pop()?.host || lastHost,
            active: inflight.size > 0 || now - lastActivity < WINDOW_MS,
            inflight: inflight.size,
            stalledMs: Math.round(stalledMs),
            hasSpeed,
            mbps: hasSpeed ? (bytes * 8) / (busy / 1000) / 1e6 : 0,
            slow,
            failed: done.filter(d => !d.ok).length,
            samples: done.length,
            maxMs: Math.round(maxMs),
            updatedAt: Date.now(),
        };
    }

    function send() {
        const stats = summary(performance.now());
        try { window.__TAURI__?.core.invoke('report_segment_stats', { stats }).catch(() => {}); } catch (_) {}
        if (!stats.active && timer) { clearInterval(timer); timer = null; }
    }

    function touch() {
        lastActivity = performance.now();
        if (!timer) timer = setInterval(send, 1000);
    }

    function begin(url) {
        const id = ++nextId;
        const now = performance.now();
        const host = hostOf(url);
        lastHost = host || lastHost;
        inflight.set(id, { host, start: now, loaded: 0, lastProgress: now });
        touch();
        send();
        return id;
    }

    function progress(id, loaded) {
        const r = inflight.get(id);
        if (!r || !(loaded > r.loaded)) return;
        const now = performance.now();
        chunks.push({ t: now, bytes: loaded - r.loaded });
        r.loaded = loaded;
        r.lastProgress = now;
        lastActivity = now;
    }

    function finish(id, ok, loaded) {
        const r = inflight.get(id);
        if (!r) return;
        if (loaded) progress(id, loaded);
        inflight.delete(id);
        const now = performance.now();
        done.push({ host: r.host, start: r.start, end: now, bytes: r.loaded, ok });
        lastActivity = now;
        touch();
        send();
    }

    // Aborted by the player (quality switch, seek): neither a success nor a failure.
    function discard(id) {
        if (!inflight.delete(id)) return;
        lastActivity = performance.now();
        send();
    }

    const open = XMLHttpRequest.prototype.open;
    XMLHttpRequest.prototype.open = function (method, url, ...rest) {
        try { this.__anivoxSeg = MEDIA.test(String(url)) ? String(url) : null; } catch (_) {}
        return open.call(this, method, url, ...rest);
    };
    const xhrSend = XMLHttpRequest.prototype.send;
    XMLHttpRequest.prototype.send = function (...args) {
        const url = this.__anivoxSeg;
        if (url) {
            const id = begin(url);
            this.addEventListener('progress', e => progress(id, e.loaded));
            this.addEventListener('load', e => finish(id, this.status >= 200 && this.status < 400, e.loaded));
            this.addEventListener('error', () => finish(id, false));
            this.addEventListener('timeout', () => finish(id, false));
            this.addEventListener('abort', () => discard(id));
        }
        return xhrSend.apply(this, args);
    };

    const nativeFetch = window.fetch;
    if (typeof nativeFetch === 'function') {
        window.fetch = function (input, init) {
            const url = typeof input === 'string' ? input : input?.url;
            const promise = nativeFetch.call(this, input, init);
            if (url && MEDIA.test(String(url))) {
                const id = begin(String(url));
                promise.then(async response => {
                    try {
                        const reader = response.clone().body.getReader();
                        let total = 0;
                        for (;;) {
                            const { done: end, value } = await reader.read();
                            if (end) break;
                            total += value.byteLength;
                            progress(id, total);
                        }
                        finish(id, response.ok, total);
                    } catch (error) {
                        if (error?.name === 'AbortError') discard(id); else finish(id, false);
                    }
                }, error => { if (error?.name === 'AbortError') discard(id); else finish(id, false); });
            }
            return promise;
        };
    }
})();
