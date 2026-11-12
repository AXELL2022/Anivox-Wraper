//! Tray icon (StatusNotifierItem over D-Bus) and the close-to-tray preference.
//!
//! GTK4 has no tray API, so the icon is served by `ksni` on its own thread and
//! menu clicks reach the GTK main thread as [`TrayEvent`]s. The preference is
//! the shared `tray.json` from `anivox-core`. Without a working tray host the
//! window close button quits, so the application never becomes unreachable.

use gtk4::gdk_pixbuf::Pixbuf;
use gtk4::glib;
use ksni::blocking::TrayMethods;
use ksni::menu::{CheckmarkItem, StandardItem};
use ksni::{Icon, MenuItem, OfflineReason, ToolTip};

/// Requests from the tray thread for the GTK main thread.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayEvent {
    /// Left click: hide a visible window, show a hidden one.
    Toggle,
    Show,
    ToggleMinimize,
    Quit,
    Online(bool),
}

pub struct AnivoxTray {
    events: async_channel::Sender<TrayEvent>,
    pub minimize_to_tray: bool,
    icons: Vec<Icon>,
}

impl AnivoxTray {
    pub fn new(events: async_channel::Sender<TrayEvent>, minimize_to_tray: bool) -> Self {
        Self {
            events,
            minimize_to_tray,
            icons: load_icons(),
        }
    }

    fn send(&self, event: TrayEvent) {
        let _ = self.events.send_blocking(event);
    }
}

impl ksni::Tray for AnivoxTray {
    fn id(&self) -> String {
        "anivox-gtk".into()
    }

    fn title(&self) -> String {
        "Anivox".into()
    }

    fn icon_name(&self) -> String {
        // Installed by the package; the pixmap below covers `cargo run`.
        "anivox-gtk".into()
    }

    fn icon_pixmap(&self) -> Vec<Icon> {
        self.icons.clone()
    }

    fn tool_tip(&self) -> ToolTip {
        ToolTip {
            title: "Anivox".into(),
            ..Default::default()
        }
    }

    fn activate(&mut self, _x: i32, _y: i32) {
        self.send(TrayEvent::Toggle);
    }

    fn menu(&self) -> Vec<MenuItem<Self>> {
        vec![
            StandardItem {
                label: "Открыть".into(),
                activate: Box::new(|tray: &mut Self| tray.send(TrayEvent::Show)),
                ..Default::default()
            }
            .into(),
            CheckmarkItem {
                label: "Сворачивать в трей".into(),
                checked: self.minimize_to_tray,
                activate: Box::new(|tray: &mut Self| tray.send(TrayEvent::ToggleMinimize)),
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            StandardItem {
                label: "Выход".into(),
                activate: Box::new(|tray: &mut Self| tray.send(TrayEvent::Quit)),
                ..Default::default()
            }
            .into(),
        ]
    }

    fn watcher_online(&self) {
        self.send(TrayEvent::Online(true));
    }

    fn watcher_offline(&self, _reason: OfflineReason) -> bool {
        self.send(TrayEvent::Online(false));
        true
    }
}

pub type TrayHandle = ksni::blocking::Handle<AnivoxTray>;

/// Registers the icon; blocks on D-Bus, so call it off the GTK main thread.
pub fn spawn(tray: AnivoxTray) -> Result<TrayHandle, String> {
    tray.spawn().map_err(|err| err.to_string())
}

const ICONS: [&[u8]; 2] = [
    include_bytes!("../../src-tauri/icons/32x32.png"),
    include_bytes!("../../src-tauri/icons/128x128.png"),
];

fn load_icons() -> Vec<Icon> {
    ICONS
        .iter()
        .filter_map(|bytes| {
            let pixbuf = Pixbuf::from_read(std::io::Cursor::new(*bytes)).ok()?;
            pixbuf_to_argb(&pixbuf)
        })
        .collect()
}

/// SNI pixmaps are ARGB32 in network byte order; GdkPixbuf rows are RGB(A) with padding.
fn pixbuf_to_argb(pixbuf: &Pixbuf) -> Option<Icon> {
    let (width, height) = (pixbuf.width(), pixbuf.height());
    let channels = pixbuf.n_channels() as usize;
    if pixbuf.bits_per_sample() != 8 || !(3..=4).contains(&channels) {
        return None;
    }
    let stride = pixbuf.rowstride() as usize;
    let bytes = pixbuf.read_pixel_bytes();
    let mut data = Vec::with_capacity((width * height * 4) as usize);
    for y in 0..height as usize {
        for x in 0..width as usize {
            let pixel = bytes.get(y * stride + x * channels..)?;
            let alpha = if channels == 4 { pixel[3] } else { 255 };
            data.extend_from_slice(&[alpha, pixel[0], pixel[1], pixel[2]]);
        }
    }
    Some(Icon {
        width,
        height,
        data,
    })
}

/// Loads the close-to-tray preference; a broken file keeps the default (on).
pub fn load_preference(path: &std::path::Path) -> bool {
    anivox_core::tray_settings::load(path).unwrap_or_else(|err| {
        eprintln!("Не удалось загрузить настройку трея: {err}");
        true
    })
}

/// Starts the icon in the background and forwards its events to `on_event`
/// on the GTK main thread. `on_ready` receives the handle once registered.
pub fn start(
    minimize_to_tray: bool,
    on_ready: impl FnOnce(Result<TrayHandle, String>) + 'static,
    on_event: impl Fn(TrayEvent) + 'static,
) {
    let (tx, rx) = async_channel::unbounded();
    let tray = AnivoxTray::new(tx, minimize_to_tray);
    glib::spawn_future_local(async move {
        let result = gtk4::gio::spawn_blocking(move || spawn(tray))
            .await
            .unwrap_or_else(|_| Err("поток трея завершился аварийно".into()));
        on_ready(result);
        while let Ok(event) = rx.recv().await {
            on_event(event);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use ksni::Tray;

    #[test]
    fn bundled_icons_decode_to_argb() {
        let icons = load_icons();
        assert_eq!(icons.len(), 2);
        for icon in icons {
            assert_eq!(icon.data.len(), (icon.width * icon.height * 4) as usize);
        }
    }

    #[test]
    fn menu_items_send_their_events() {
        let (tx, rx) = async_channel::unbounded();
        let mut tray = AnivoxTray::new(tx, false);
        tray.activate(0, 0);
        let menu = tray.menu();
        for item in &menu {
            match item {
                MenuItem::Standard(item) => (item.activate)(&mut tray),
                MenuItem::Checkmark(item) => {
                    assert!(!item.checked);
                    (item.activate)(&mut tray);
                }
                _ => {}
            }
        }
        let events: Vec<_> = std::iter::from_fn(|| rx.try_recv().ok()).collect();
        assert_eq!(
            events,
            [
                TrayEvent::Toggle,
                TrayEvent::Show,
                TrayEvent::ToggleMinimize,
                TrayEvent::Quit
            ]
        );
    }
}
