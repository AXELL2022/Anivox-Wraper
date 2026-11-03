// Performance-oriented CSS and scroll handling for the Anivox site
(function() {
    if (window !== window.top || !['https://anivox.fun', 'https://anivox.me'].includes(location.origin)) return;

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
})();
