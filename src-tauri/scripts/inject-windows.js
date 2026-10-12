// Windows-specific injection: DirectComposition Video Overlay & NVIDIA RTX VSR / TrueHDR Fixes
(function() {
    function setupWindowsVideoStyles() {
        const style = document.createElement('style');
        style.id = 'anivox-windows-styles';
        style.textContent = `
            /* DirectComposition Video Overlay & NVIDIA RTX VSR Fixes */
            .player-container, .player, [class*="player"], .video-element, video, iframe {
                border-radius: 0 !important;
                -webkit-mask: none !important;
                mask: none !important;
                filter: none !important;
                backdrop-filter: none !important;
                transform: none !important;
            }
            video.video-element, video {
                background-color: transparent !important;
            }
        `;
        (document.head || document.documentElement).appendChild(style);
    }

    if (document.readyState === 'loading') {
        document.addEventListener('DOMContentLoaded', setupWindowsVideoStyles);
    } else {
        setupWindowsVideoStyles();
    }
})();
