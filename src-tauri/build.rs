fn main() {
    let windows = tauri_build::WindowsAttributes::new().static_vc_runtime(true);
    let attrs = tauri_build::Attributes::new().windows_attributes(windows);
    tauri_build::try_build(attrs).expect("failed to run build script");
    // Tauri's app manifest is linked only into the main binary. The library
    // test executable also links native dialogs that require Common Controls 6.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        println!("cargo:rustc-link-arg=/MANIFEST:EMBED");
        println!("cargo:rustc-link-arg=/MANIFESTDEPENDENCY:type='win32' name='Microsoft.Windows.Common-Controls' version='6.0.0.0' processorArchitecture='*' publicKeyToken='6595b64144ccf1df' language='*'");
        // The main binary already embeds Tauri's manifest through resource.lib.
        println!("cargo:rustc-link-arg-bin=anivox=/MANIFEST:NO");
    }
}
