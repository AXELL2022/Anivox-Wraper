// Linux-specific injection: WebKitGTK HLS MSE fallbacks, buffer recovery & frame pacing
(function() {
    // Disable broken ManagedMediaSource so hls.js uses standard MediaSource
    try {
        delete window.ManagedMediaSource;
        delete self.ManagedMediaSource;
        Object.defineProperty(window, 'ManagedMediaSource', {
            get: () => undefined,
            set: () => {},
            configurable: true
        });
        Object.defineProperty(self, 'ManagedMediaSource', {
            get: () => undefined,
            set: () => {},
            configurable: true
        });
    } catch (e) {}

    // Override HLS player buffer configuration to allow smooth, deep caching
    function applyHlsBufferOptimization(ctx) {
        if (!ctx) return;
        if (ctx.hlsConfig) {
            ctx.hlsConfig.preferManagedMediaSource = false;
            ctx.hlsConfig.maxBufferLength = 120;
            ctx.hlsConfig.maxMaxBufferLength = 300;
            ctx.hlsConfig.maxBufferSize = 268435456; // 256MB
        }
        if (ctx.hls && ctx.hls.config) {
            ctx.hls.config.preferManagedMediaSource = false;
            ctx.hls.config.maxBufferLength = 120;
            ctx.hls.config.maxMaxBufferLength = 300;
            ctx.hls.config.maxBufferSize = 268435456;
            if (ctx.hls.buffering === false && typeof ctx.hls.resumeBuffering === 'function') {
                ctx.hls.resumeBuffering();
            }
        }
        if (typeof ctx.updateBuffered === 'function') {
            ctx.updateBuffered();
        }
    }

    // Dispatch progress events when SourceBuffer receives video/audio chunks
    try {
        if (typeof SourceBuffer !== 'undefined' && SourceBuffer.prototype) {
            const origAppend = SourceBuffer.prototype.appendBuffer;
            SourceBuffer.prototype.appendBuffer = function(data) {
                this.addEventListener('updateend', function onAppendEnd() {
                    const videos = document.querySelectorAll('video');
                    for (let i = 0; i < videos.length; i++) {
                        videos[i].dispatchEvent(new Event('progress'));
                    }
                }, { once: true });
                return origAppend.apply(this, arguments);
            };
        }
    } catch (e) {}

    // Periodic buffer synchronization and stall auto-kick for WebKitGTK
    setInterval(() => {
        const container = document.querySelector('.player-container');
        if (container) {
            const comp = container.__vueParentComponent || container._vnode?.component || container.__vue_app__;
            if (comp) {
                const ctx = comp.ctx || comp.proxy || comp.setupState;
                applyHlsBufferOptimization(ctx);
            }
        }
        const video = document.querySelector('video.video-element') || document.querySelector('video');
        if (video) {
            video.dispatchEvent(new Event('progress'));
            // If video is stalled/waiting but has buffered content at currentTime, resume playback
            if (video.readyState < 3 && !video.paused && video.buffered && video.buffered.length > 0) {
                for (let i = 0; i < video.buffered.length; i++) {
                    if (video.currentTime >= video.buffered.start(i) && video.currentTime < video.buffered.end(i) - 0.1) {
                        video.play().catch(() => {});
                        break;
                    }
                }
            }
        }
    }, 400);

    // Force HLS.js (MSE) over broken native WebKitGTK GStreamer HLS
    try {
        const origCanPlay = HTMLMediaElement.prototype.canPlayType;
        HTMLMediaElement.prototype.canPlayType = function(type) {
            if (typeof type === 'string' && (type.toLowerCase().includes('mpegurl') || type.toLowerCase().includes('m3u8'))) {
                return '';
            }
            return origCanPlay.apply(this, arguments);
        };
    } catch (e) {}

    // Constant 60 FPS frame pacer to keep WebKitGTK compositor active and prevent idle drops
    function setupLinuxFramePacer() {
        const pacer = document.createElement('div');
        pacer.id = 'anivox-frame-pacer';
        pacer.style.cssText = 'position:fixed;top:0;left:0;width:1px;height:1px;opacity:0.01;pointer-events:none;z-index:-1;';
        (document.head || document.documentElement).appendChild(pacer);
        let pacerToggle = false;
        function framePace() {
            pacer.style.transform = pacerToggle ? 'translate3d(0,0,0.05px)' : 'translate3d(0,0,0px)';
            pacerToggle = !pacerToggle;
            requestAnimationFrame(framePace);
        }
        requestAnimationFrame(framePace);
    }

    if (document.readyState === 'loading') {
        document.addEventListener('DOMContentLoaded', setupLinuxFramePacer);
    } else {
        setupLinuxFramePacer();
    }
})();
