// Only reports top-level document/SPA lifecycle, never media buffering failures.
(() => {
    if (window !== window.top || location.origin !== 'https://anivox.fun') return;
    const attempt = window.__TAURI__?.core.invoke('get_site_document_generation');
    const report = loaded => {
        const href = location.href;
        return attempt?.then(generation => window.__TAURI__.core.invoke('site_document_state', {
            href, loaded, generation,
        })).catch(error => console.warn('[Anivox] Lifecycle:', error));
    };
    let loaded = false;
    // A window load event only proves the HTML arrived, not that Vue mounted.
    // Keep native recovery/watchdog active for an empty SPA shell or script error.
    let observer;
    const ready = () => {
        const root = document.querySelector('#app');
        if (!root || !root.children.length || !root.getBoundingClientRect().height) return;
        if (loaded) return;
        loaded = true;
        observer?.disconnect();
        report(true);
    };
    const observe = () => {
        observer = new MutationObserver(ready);
        observer.observe(document.documentElement, {childList: true, subtree: true, attributes: true});
    };
    if (document.documentElement) observe();
    else document.addEventListener('DOMContentLoaded', observe, {once: true});
    if (document.readyState === 'complete') ready();
    else window.addEventListener('load', ready, {once: true});
    for (const method of ['pushState', 'replaceState']) {
        const original = history[method];
        history[method] = function (...args) {
            const value = original.apply(this, args);
            report(loaded);
            return value;
        };
    }
    window.addEventListener('popstate', () => report(loaded));
    window.addEventListener('hashchange', () => report(loaded));
})();
