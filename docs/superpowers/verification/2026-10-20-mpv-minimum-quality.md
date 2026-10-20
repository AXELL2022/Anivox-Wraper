# MPV: minimum embedded quality

Date: 2026-10-20.

The current Anivox assets (`vendor-Ctwgv0S0.js`, Vue 3.5.13;
`CustomPlayer-CIn7BX6I.js`) were inspected from the live site. The production
Vue bundle has no `__vueParentComponent` DOM hooks. It retains the render tree
on the mount container's `_vnode`. The old injected lookup therefore could not
reach the site's `GetQualities` or `changeQuality`, despite the video element
remaining available for muted playback.

The lookup now resolves the element's owning component through the live VNode
tree, including component subtrees, fragments and active Suspense branches.
Quality selection also retries when the site's tracks arrive after MPV is ready
or a quality change fails. Original quality/preferences remain available for
restoration when the MPV session ends.

Verification:

- Production-shaped VNode handoff regression failed before the lookup fix and
  passed after it: episode identity, lowest-quality source, mute and playback.
- Late-track and failed-switch regressions failed before retry handling and
  passed afterward; original quality is restored on exit.
- All 87 JavaScript tests passed, including a failed HLS attachment after the
  site's reactive quality label has already changed and shared root elements
  on wrapper component VNodes.
- `cargo check --manifest-path src-tauri/Cargo.toml --offline` passed.
- Final `cargo build --release --manifest-path src-tauri/Cargo.toml --offline`
  passed; the local release EXE includes the updated injected scripts.
- Live playback/Discord/XP counters were not exercised; no runtime app was open
  during inspection. Regression fixtures do not constitute live playback QA.
