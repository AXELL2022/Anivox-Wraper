# Maintainer: axell
pkgname=anivox
pkgver=0.1.0
pkgrel=1
pkgdesc="Anivox Desktop - Tauri Web Wrapper for anivox.fun with WireGuard VPN"
arch=('x86_64')
url="https://anivox.fun/"
license=('unknown')
depends=('webkit2gtk-4.1' 'gtk3')
options=('!strip')

package() {
    install -Dm755 "${srcdir}/../src-tauri/target/release/anivox" "${pkgdir}/usr/bin/anivox"
    
    if [ -f "${srcdir}/../src-tauri/target/release/bundle/deb/Anivox_0.1.0_amd64/data/usr/share/applications/Anivox.desktop" ]; then
        install -Dm644 "${srcdir}/../src-tauri/target/release/bundle/deb/Anivox_0.1.0_amd64/data/usr/share/applications/Anivox.desktop" "${pkgdir}/usr/share/applications/anivox.desktop"
    fi

    for res in 32x32 128x128 256x256@2; do
        local icondir="${srcdir}/../src-tauri/target/release/bundle/deb/Anivox_0.1.0_amd64/data/usr/share/icons/hicolor/${res}/apps"
        if [ -d "$icondir" ]; then
            install -d "${pkgdir}/usr/share/icons/hicolor/${res}/apps"
            install -m644 "$icondir/"* "${pkgdir}/usr/share/icons/hicolor/${res}/apps/"
        fi
    done
}
