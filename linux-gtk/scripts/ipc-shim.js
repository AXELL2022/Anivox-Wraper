// Exposes the subset of `window.__TAURI__.core.invoke` that the shared site
// scripts use, backed by the native WebKit message handler `anivox`.
// `__ANIVOX_ORIGINS__` is replaced by the shell with the trusted site origins.
(function () {
    if (window.top !== window || !__ANIVOX_ORIGINS__.includes(location.origin)) return;
    const handler = window.webkit && window.webkit.messageHandlers && window.webkit.messageHandlers.anivox;
    if (!handler) return;
    const invoke = (cmd, args) =>
        handler.postMessage(JSON.stringify({ cmd: String(cmd), args: args === undefined ? {} : args }))
            .then(raw => (raw === undefined || raw === null || raw === '' ? undefined : JSON.parse(raw)));
    Object.defineProperty(window, '__TAURI__', {
        value: Object.freeze({ core: Object.freeze({ invoke }) }),
        configurable: false,
    });
})();
