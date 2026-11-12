fn main() {
    // Export our `drmWaitVBlank` from the executable so WebKitGTK binds to it
    // instead of libdrm's (see src/vblank.rs).
    println!("cargo:rustc-link-arg-bins=-Wl,--export-dynamic-symbol=drmWaitVBlank");
}
