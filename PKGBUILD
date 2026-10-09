# Maintainer: axell
pkgname=anivox
pkgver=0.1.2
pkgrel=1
pkgdesc="Anivox Desktop - Tauri Web Wrapper for anivox.fun with WireGuard VPN"
arch=('x86_64')
url="https://anivox.fun/"
license=('unknown')
depends=('webkit2gtk-4.1' 'gtk3')
optdepends=(
    'mpv: external player support'
    'wireguard-tools: WireGuard VPN tunnel support'
)
options=('!strip' '!debug')

package() {
    local root_dir="${startdir}"
    if [ ! -f "${root_dir}/src-tauri/target/release/anivox" ] && [ -f "${srcdir}/../src-tauri/target/release/anivox" ]; then
        root_dir="${srcdir}/.."
    fi

    install -Dm755 "${root_dir}/src-tauri/target/release/anivox" "${pkgdir}/usr/bin/anivox"

    install -Dm644 "${root_dir}/anivox.desktop" "${pkgdir}/usr/share/applications/anivox.desktop"

    install -Dm644 "${root_dir}/src-tauri/icons/32x32.png" "${pkgdir}/usr/share/icons/hicolor/32x32/apps/anivox.png"
    install -Dm644 "${root_dir}/src-tauri/icons/128x128.png" "${pkgdir}/usr/share/icons/hicolor/128x128/apps/anivox.png"
    install -Dm644 "${root_dir}/src-tauri/icons/128x128@2x.png" "${pkgdir}/usr/share/icons/hicolor/256x256/apps/anivox.png"
    install -Dm644 "${root_dir}/src-tauri/icons/icon.png" "${pkgdir}/usr/share/icons/hicolor/512x512/apps/anivox.png"
}
