// OAuth providers can load in this webview, but only Shikimori has IPC access.
(() => {
    if (location.origin !== 'https://shikimori.io' || window !== window.top) return;
    document.addEventListener('keydown', event => {
        if (event.key === 'Escape' && !event.defaultPrevented) {
            event.preventDefault();
            window.__TAURI__.core.invoke('close_shikimori').catch(console.error);
        }
    });
})();
