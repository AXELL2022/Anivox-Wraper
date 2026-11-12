//! Lifts WebKitGTK's 60 fps cap on drivers without DRM vblank events.
//!
//! WebKitGTK paces page rendering (`requestAnimationFrame`, compositing) in
//! the UI process with `DisplayVBlankMonitorDRM`, which calls libdrm's
//! `drmWaitVBlank` on the monitor's CRTC. The NVIDIA DRM driver refuses that
//! ioctl for clients that are not the DRM master (`EOPNOTSUPP`; WebKit logs it
//! as "Operation not permitted"), so WebKit falls back to a timer hard-coded at
//! 60 fps, whatever the monitor's refresh rate is.
//!
//! The executable exports its own `drmWaitVBlank` (see `build.rs`), so the
//! dynamic linker binds WebKit's call here. Real vblank waits still go to
//! libdrm; only when the driver refuses a relative wait do we sleep until the
//! next tick of the CRTC's own refresh period. WebKit then keeps its DRM
//! monitor and runs at the monitor's real rate (165/240 Hz...).
//!
//! `ANIVOX_NATIVE_VBLANK=1` disables the emulation.

use libc::{c_char, c_int, c_long, c_uint, c_ulong, c_void};
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

const DRM_VBLANK_RELATIVE: c_uint = 0x1;
const DRM_VBLANK_SECONDARY: c_uint = 0x2000_0000;
const DRM_VBLANK_HIGH_CRTC_MASK: c_uint = 0x0000_003e;
const DRM_VBLANK_HIGH_CRTC_SHIFT: c_uint = 1;
/// Used when the CRTC mode cannot be read.
const FALLBACK_PERIOD_NS: u64 = 16_666_667;
/// Re-read the CRTC mode this often so refresh rate changes are picked up.
const PERIOD_CACHE_TTL: Duration = Duration::from_secs(2);

#[repr(C)]
#[derive(Clone, Copy)]
pub struct DrmVBlankRequest {
    kind: c_uint,
    sequence: c_uint,
    signal: c_ulong,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct DrmVBlankReply {
    kind: c_uint,
    sequence: c_uint,
    tval_sec: c_long,
    tval_usec: c_long,
}

/// `drmVBlank` from `xf86drm.h`.
#[repr(C)]
pub union DrmVBlank {
    request: DrmVBlankRequest,
    reply: DrmVBlankReply,
}

/// `drmModeRes` from `xf86drmMode.h`.
#[repr(C)]
struct DrmModeRes {
    count_fbs: c_int,
    fbs: *mut u32,
    count_crtcs: c_int,
    crtcs: *mut u32,
    count_connectors: c_int,
    connectors: *mut u32,
    count_encoders: c_int,
    encoders: *mut u32,
    min_width: u32,
    max_width: u32,
    min_height: u32,
    max_height: u32,
}

/// `drmModeModeInfo` from `xf86drmMode.h`.
#[repr(C)]
struct DrmModeModeInfo {
    clock: u32,
    hdisplay: u16,
    hsync_start: u16,
    hsync_end: u16,
    htotal: u16,
    hskew: u16,
    vdisplay: u16,
    vsync_start: u16,
    vsync_end: u16,
    vtotal: u16,
    vscan: u16,
    vrefresh: u32,
    flags: u32,
    kind: u32,
    name: [c_char; 32],
}

/// `drmModeCrtc` from `xf86drmMode.h`.
#[repr(C)]
struct DrmModeCrtc {
    crtc_id: u32,
    buffer_id: u32,
    x: u32,
    y: u32,
    width: u32,
    height: u32,
    mode_valid: c_int,
    mode: DrmModeModeInfo,
    gamma_size: c_int,
}

type WaitVBlankFn = unsafe extern "C" fn(c_int, *mut DrmVBlank) -> c_int;
type GetResourcesFn = unsafe extern "C" fn(c_int) -> *mut DrmModeRes;
type FreeResourcesFn = unsafe extern "C" fn(*mut DrmModeRes);
type GetCrtcFn = unsafe extern "C" fn(c_int, u32) -> *mut DrmModeCrtc;
type FreeCrtcFn = unsafe extern "C" fn(*mut DrmModeCrtc);

struct Libdrm {
    wait_vblank: WaitVBlankFn,
    get_resources: Option<GetResourcesFn>,
    free_resources: Option<FreeResourcesFn>,
    get_crtc: Option<GetCrtcFn>,
    free_crtc: Option<FreeCrtcFn>,
}

fn next_symbol(name: &[u8]) -> Option<*mut c_void> {
    let symbol = unsafe { libc::dlsym(libc::RTLD_NEXT, name.as_ptr().cast()) };
    (!symbol.is_null()).then_some(symbol)
}

/// libdrm is already loaded by the time WebKit calls `drmWaitVBlank`.
fn libdrm() -> Option<&'static Libdrm> {
    static LIBDRM: OnceLock<Option<Libdrm>> = OnceLock::new();
    LIBDRM
        .get_or_init(|| unsafe {
            Some(Libdrm {
                wait_vblank: std::mem::transmute::<*mut c_void, WaitVBlankFn>(next_symbol(
                    b"drmWaitVBlank\0",
                )?),
                get_resources: next_symbol(b"drmModeGetResources\0")
                    .map(|s| std::mem::transmute::<*mut c_void, GetResourcesFn>(s)),
                free_resources: next_symbol(b"drmModeFreeResources\0")
                    .map(|s| std::mem::transmute::<*mut c_void, FreeResourcesFn>(s)),
                get_crtc: next_symbol(b"drmModeGetCrtc\0")
                    .map(|s| std::mem::transmute::<*mut c_void, GetCrtcFn>(s)),
                free_crtc: next_symbol(b"drmModeFreeCrtc\0")
                    .map(|s| std::mem::transmute::<*mut c_void, FreeCrtcFn>(s)),
            })
        })
        .as_ref()
}

fn emulation_enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| std::env::var_os("ANIVOX_NATIVE_VBLANK").is_none_or(|v| v == "0"))
}

/// CRTC index encoded in a `drmVBlank` request type, as libdrm builds it.
fn crtc_index(kind: c_uint) -> usize {
    if kind & DRM_VBLANK_SECONDARY != 0 {
        1
    } else {
        ((kind & DRM_VBLANK_HIGH_CRTC_MASK) >> DRM_VBLANK_HIGH_CRTC_SHIFT) as usize
    }
}

/// Frame period of a display mode in nanoseconds (exact, not the rounded `vrefresh`).
fn mode_period_ns(clock_khz: u32, htotal: u16, vtotal: u16) -> Option<u64> {
    if clock_khz == 0 || htotal == 0 || vtotal == 0 {
        return None;
    }
    Some(u64::from(htotal) * u64::from(vtotal) * 1_000_000 / u64::from(clock_khz))
}

unsafe fn read_period_ns(drm: &Libdrm, fd: c_int, index: usize) -> Option<u64> {
    let (get_res, free_res, get_crtc, free_crtc) = (
        drm.get_resources?,
        drm.free_resources?,
        drm.get_crtc?,
        drm.free_crtc?,
    );
    let res = get_res(fd);
    if res.is_null() {
        return None;
    }
    let mut period = None;
    let count = usize::try_from((*res).count_crtcs).unwrap_or(0);
    if index < count && !(*res).crtcs.is_null() {
        let crtc = get_crtc(fd, *(*res).crtcs.add(index));
        if !crtc.is_null() {
            let mode = &(*crtc).mode;
            if (*crtc).mode_valid != 0 {
                period = mode_period_ns(mode.clock, mode.htotal, mode.vtotal);
            }
            free_crtc(crtc);
        }
    }
    free_res(res);
    period
}

type PeriodCache = HashMap<(c_int, usize), (u64, Instant)>;

fn period_ns(drm: &Libdrm, fd: c_int, index: usize) -> u64 {
    static CACHE: Mutex<Option<PeriodCache>> = Mutex::new(None);
    let now = Instant::now();
    if let Ok(cache) = CACHE.lock() {
        if let Some(&(period, at)) = cache.as_ref().and_then(|c| c.get(&(fd, index))) {
            if now.duration_since(at) < PERIOD_CACHE_TTL {
                return period;
            }
        }
    }
    let period = unsafe { read_period_ns(drm, fd, index) }.unwrap_or(FALLBACK_PERIOD_NS);
    if let Ok(mut cache) = CACHE.lock() {
        cache
            .get_or_insert_with(HashMap::new)
            .insert((fd, index), (period, now));
    }
    period
}

fn monotonic_ns() -> u64 {
    let mut ts = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut ts) };
    ts.tv_sec as u64 * 1_000_000_000 + ts.tv_nsec as u64
}

/// Absolute time of the `count`-th tick after `now` on a grid of `period`
/// (`count == 0` means "now", like a relative wait for 0 vblanks).
fn target_tick_ns(now: u64, period: u64, count: u32) -> u64 {
    if count == 0 {
        now
    } else {
        (now / period + u64::from(count)) * period
    }
}

fn sleep_until_ns(target: u64) {
    let ts = libc::timespec {
        tv_sec: (target / 1_000_000_000) as libc::time_t,
        tv_nsec: (target % 1_000_000_000) as c_long,
    };
    // Restart on EINTR; `clock_nanosleep` returns the error number directly.
    while unsafe {
        libc::clock_nanosleep(
            libc::CLOCK_MONOTONIC,
            libc::TIMER_ABSTIME,
            &ts,
            std::ptr::null_mut(),
        )
    } == libc::EINTR
    {}
}

/// Interposes libdrm's `drmWaitVBlank` for the whole process.
///
/// # Safety
/// Same contract as libdrm: `vbl` points to a valid `drmVBlank`.
#[no_mangle]
pub unsafe extern "C" fn drmWaitVBlank(fd: c_int, vbl: *mut DrmVBlank) -> c_int {
    let Some(drm) = libdrm() else {
        *libc::__errno_location() = libc::ENOSYS;
        return -1;
    };
    if vbl.is_null() {
        return (drm.wait_vblank)(fd, vbl);
    }
    let request = (*vbl).request;
    let ret = (drm.wait_vblank)(fd, vbl);
    if ret == 0 || !emulation_enabled() || request.kind & DRM_VBLANK_RELATIVE == 0 {
        return ret;
    }
    let errno = *libc::__errno_location();
    if errno != libc::EOPNOTSUPP && errno != libc::EINVAL {
        return ret;
    }

    let period = period_ns(drm, fd, crtc_index(request.kind)).max(1);
    let target = target_tick_ns(monotonic_ns(), period, request.sequence);
    sleep_until_ns(target);
    (*vbl).reply = DrmVBlankReply {
        kind: request.kind,
        sequence: (target / period) as c_uint,
        tval_sec: (target / 1_000_000_000) as c_long,
        tval_usec: ((target % 1_000_000_000) / 1_000) as c_long,
    };
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crtc_index_matches_libdrm_encoding() {
        assert_eq!(crtc_index(DRM_VBLANK_RELATIVE), 0);
        assert_eq!(crtc_index(DRM_VBLANK_RELATIVE | DRM_VBLANK_SECONDARY), 1);
        for index in 2..32u32 {
            let bits = (index << DRM_VBLANK_HIGH_CRTC_SHIFT) & DRM_VBLANK_HIGH_CRTC_MASK;
            assert_eq!(crtc_index(DRM_VBLANK_RELATIVE | bits), index as usize);
        }
    }

    #[test]
    fn mode_period_uses_exact_timings() {
        // 2560x1440 @ ~240 Hz: 1056.75 MHz pixel clock.
        let p240 = mode_period_ns(1_056_750, 2720, 1619).unwrap();
        assert!((4_160_000..4_170_000).contains(&p240), "{p240}");
        assert_eq!(mode_period_ns(0, 2720, 1619), None);
        assert_eq!(mode_period_ns(1_000, 0, 1), None);
    }

    #[test]
    fn target_tick_lands_on_the_grid() {
        assert_eq!(target_tick_ns(1_005, 100, 0), 1_005);
        assert_eq!(target_tick_ns(1_005, 100, 1), 1_100);
        assert_eq!(target_tick_ns(1_000, 100, 1), 1_100);
        assert_eq!(target_tick_ns(1_005, 100, 3), 1_300);
    }

    #[test]
    fn struct_layouts_match_libdrm() {
        assert_eq!(std::mem::size_of::<DrmVBlank>(), 24);
        assert_eq!(std::mem::size_of::<DrmModeModeInfo>(), 68);
        assert_eq!(std::mem::offset_of!(DrmModeCrtc, mode), 28);
    }
}
