// A restored page is opened for context, not permission to start playback.
(() => {
    if (window !== window.top || location.origin !== 'https://anivox.fun') return;
    let blocked = true;
    const original = HTMLMediaElement.prototype.play;
    const unlock = event => {
        if (!event.isTrusted) return;
        blocked = false;
        if (window.__anivoxRestoreToken) {
            try { sessionStorage.setItem('anivox-restore-unlocked', window.__anivoxRestoreToken); } catch (_) {}
        }
        document.removeEventListener('pointerdown', unlock, true);
        document.removeEventListener('keydown', unlock, true);
        if (HTMLMediaElement.prototype.play === guardedPlay) HTMLMediaElement.prototype.play = original;
    };
    function guardedPlay(...args) {
        if (blocked) return Promise.reject(new DOMException('Нажмите на страницу для начала просмотра', 'NotAllowedError'));
        return original.apply(this, args);
    }
    HTMLMediaElement.prototype.play = guardedPlay;
    document.addEventListener('play', event => {
        if (blocked && event.target instanceof HTMLMediaElement) event.target.pause();
    }, true);
    document.addEventListener('pointerdown', unlock, true);
    document.addEventListener('keydown', unlock, true);
})();
