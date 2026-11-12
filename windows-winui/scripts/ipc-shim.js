// Exposes the subset of `window.__TAURI__.core.invoke` that the shared site
// scripts use, backed by WebView2 `chrome.webview` messages. The shell answers
// with {id, ok, value | error}. `__ANIVOX_ORIGINS__` is replaced by the shell
// with the trusted site origins; the shell checks the sender origin again.
(function () {
    if (window.top !== window || !__ANIVOX_ORIGINS__.includes(location.origin)) return;
    const webview = window.chrome && window.chrome.webview;
    if (!webview) return;
    const post = webview.postMessage.bind(webview);
    const pending = new Map();
    let nextId = 1;
    webview.addEventListener('message', event => {
        const reply = event.data;
        if (!reply || typeof reply !== 'object' || !pending.has(reply.id)) return;
        const { resolve, reject } = pending.get(reply.id);
        pending.delete(reply.id);
        if (reply.ok) resolve(reply.value === null ? undefined : reply.value);
        else reject(String(reply.error || 'Ошибка'));
    });
    const invoke = (cmd, args) => new Promise((resolve, reject) => {
        const id = nextId++;
        pending.set(id, { resolve, reject });
        post({ id, cmd: String(cmd), args: args === undefined ? {} : args });
    });
    Object.defineProperty(window, '__TAURI__', {
        value: Object.freeze({ core: Object.freeze({ invoke }) }),
        configurable: false,
    });
})();
