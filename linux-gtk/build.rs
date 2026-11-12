fn main() {
    // Export our `drmWaitVBlank` from the executable so WebKitGTK binds to it
    // instead of libdrm's (see src/vblank.rs).
    println!("cargo:rustc-link-arg-bins=-Wl,--export-dynamic-symbol=drmWaitVBlank");
    // Only this file affects the script; skip rescanning the crate directory.
    println!("cargo:rerun-if-changed=build.rs");
}
