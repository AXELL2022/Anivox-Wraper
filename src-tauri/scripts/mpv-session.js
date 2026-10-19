// Anivox-owned MPV playback session. This only mirrors real playback events.
(function () {
    window.__anivoxCreateMpvSession = function ({getVideo, getContext, getIdentity, getGeneration, changeQuality = (ctx, choice) => ctx.changeQuality(choice)}) {
        let binding = null;
        const same = b => getGeneration() === b.generation && getIdentity() === b.key && getVideo() === b.video;
        async function stop() {
            const b = binding;
            binding = null;
            if (!b) return;
            b.video?.removeEventListener?.('volumechange', b.mute);
            if (b.video) {
                // Restore sound before any await; later navigation owns the element.
                b.video.muted = b.muted;
                if (same(b)) {
                    b.video.pause();
                    if (b.changed && b.context?.changeQuality && b.quality != null) {
                        try { await changeQuality(b.context, {id: b.quality}); } catch (_) {}
                    }
                    if (!same(b)) return;
                    b.video.playbackRate = b.speed;
                    b.video.pause();
                }
            }
        }
        return {
            async begin(info, key) {
                await stop();
                const video = getVideo();
                binding = {id: info.sessionId, mirror: !!info.mirrorEmbedded, key,
                    generation: getGeneration(), video, context: getContext(),
                    muted: video?.muted ?? false, volume: video?.volume ?? 1,
                    speed: video?.playbackRate ?? 1, quality: getContext()?.quality,
                    started: false, changed: false};
                const b = binding;
                b.mute = () => { if (binding === b && b.mirror && b.started && same(b)) b.video.muted = true; };
            },
            async update(sample) {
                const b = binding;
                if (!b) return;
                if (!sample || sample.sessionId !== b.id || sample.mediaKey !== b.key || !same(b)) {
                    await stop(); return;
                }
                if (!sample.ready) return;
                if (!b.video) return;
                if (!b.mirror) { b.started = true; b.video.pause(); return; }
                if (!b.started) {
                    b.started = true;
                    b.video.addEventListener?.('volumechange', b.mute);
                    b.video.muted = true;
                    const choices = b.context?.GetQualities;
                    const qualities = (Array.isArray(choices) ? choices : []).filter(q =>
                        !q.disabled && !q.local && /^\d+p?$/.test(String(q.id)));
                    qualities.sort((a, c) => parseInt(a.id) - parseInt(c.id));
                    if (qualities[0] && String(qualities[0].id) !== String(b.quality) && b.context?.changeQuality) {
                        b.changed = true;
                        try { await changeQuality(b.context, qualities[0]); }
                        catch (_) { b.changed = false; }
                        if (binding !== b) return;
                        if (!same(b)) { await stop(); return; }
                    }
                }
                b.video.muted = true;
                if (Number.isFinite(sample.position) && sample.position >= 0 &&
                    Math.abs(b.video.currentTime - sample.position) > 2) {
                    try { b.video.currentTime = sample.position; } catch (_) {}
                }
                if (Number.isFinite(sample.speed) && sample.speed > 0 && sample.speed <= 16) b.video.playbackRate = sample.speed;
                if (sample.paused || sample.buffering || sample.ended) b.video.pause();
                else if (b.video.paused) { try { await b.video.play(); } catch (_) {} }
            },
            stop,
        };
    };
})();
