//! "MPV" toolbar button: hands the current episode to MPV and shows the
//! player status reported by the site script (`report_player_status`), like
//! the Tauri toolbar. Longer explanations go to a message next to it.

use gtk4::glib;
use gtk4::prelude::*;
use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;
use webkit6::prelude::*;
use webkit6::WebView;

const LABEL: &str = "MPV";
const FALLBACK: Duration = Duration::from_secs(10);
const MESSAGE: Duration = Duration::from_secs(5);

/// Page-provided colors go into Pango markup: accept only `#rgb`/`#rrggbb[aa]`.
pub fn safe_color(value: &str) -> &str {
    let hex = value.strip_prefix('#').unwrap_or("");
    if matches!(hex.len(), 3 | 6 | 8) && hex.bytes().all(|c| c.is_ascii_hexdigit()) {
        value
    } else {
        "#cccccc"
    }
}

/// The button has its own icon: drop the status symbol the site puts first.
pub fn button_text(text: &str) -> &str {
    let trimmed = text.trim_start_matches(['▶', '✔', '⚠', '✨']);
    if trimmed.len() == text.len() {
        text
    } else {
        trimmed.trim_start()
    }
}

pub struct PlayerButton {
    pub button: gtk4::Button,
    label: gtk4::Label,
    pub message: gtk4::Label,
    button_generation: Cell<u64>,
    message_generation: Cell<u64>,
}

impl PlayerButton {
    pub fn new(content: &WebView) -> Rc<Self> {
        let label = gtk4::Label::new(Some(LABEL));
        let inner = gtk4::Box::new(gtk4::Orientation::Horizontal, 6);
        inner.append(&gtk4::Image::from_icon_name(
            "media-playback-start-symbolic",
        ));
        inner.append(&label);
        let button = gtk4::Button::builder()
            .child(&inner)
            .tooltip_text("Открыть текущее видео в MPV (M / Ь)")
            .build();
        let message = gtk4::Label::new(None);
        message.add_css_class("error");
        message.set_ellipsize(gtk4::pango::EllipsizeMode::End);
        message.set_max_width_chars(48);
        message.set_visible(false);
        let player = Rc::new(Self {
            button,
            label,
            message,
            button_generation: Cell::new(0),
            message_generation: Cell::new(0),
        });
        player.button.connect_clicked(glib::clone!(
            #[weak]
            player,
            #[weak]
            content,
            move |_| player.launch(&content)
        ));
        player
    }

    fn launch(self: &Rc<Self>, content: &WebView) {
        self.button.set_sensitive(false);
        self.label.set_text("MPV · …");
        let mine = self.bump_button();
        let player = Rc::downgrade(self);
        glib::timeout_add_local_once(FALLBACK, move || {
            if let Some(player) = player.upgrade() {
                if player.button_generation.get() == mine {
                    player.reset();
                    player.show_message("MPV: сначала загрузите сайт и включите серию.");
                }
            }
        });
        content.evaluate_javascript(
            "window.__anivoxLaunchMpv?.()",
            None,
            None,
            gtk4::gio::Cancellable::NONE,
            |_| {},
        );
    }

    fn bump_button(&self) -> u64 {
        let mine = self.button_generation.get() + 1;
        self.button_generation.set(mine);
        mine
    }

    fn reset(&self) {
        self.label.set_text(LABEL);
        self.button.set_sensitive(true);
    }

    pub fn show_message(self: &Rc<Self>, text: &str) {
        self.message.set_text(text);
        self.message.set_tooltip_text(Some(text));
        self.message.set_visible(true);
        let mine = self.message_generation.get() + 1;
        self.message_generation.set(mine);
        let player = Rc::downgrade(self);
        glib::timeout_add_local_once(MESSAGE, move || {
            if let Some(player) = player.upgrade() {
                if player.message_generation.get() == mine {
                    player.message.set_visible(false);
                }
            }
        });
    }

    /// `resetAfterMs == 0` keeps the button busy until the next status.
    pub fn show_status(
        self: &Rc<Self>,
        text: &str,
        color: &str,
        reset_after_ms: u64,
        detail: Option<&str>,
    ) {
        if let Some(detail) = detail.filter(|detail| !detail.is_empty()) {
            self.show_message(detail);
        }
        self.label.set_markup(&format!(
            "<span foreground=\"{}\">{}</span>",
            safe_color(color),
            glib::markup_escape_text(button_text(text))
        ));
        self.button.set_sensitive(reset_after_ms > 0);
        let mine = self.bump_button();
        let delay = if reset_after_ms > 0 {
            Duration::from_millis(reset_after_ms)
        } else {
            FALLBACK
        };
        let player = Rc::downgrade(self);
        glib::timeout_add_local_once(delay, move || {
            if let Some(player) = player.upgrade() {
                if player.button_generation.get() == mine {
                    player.reset();
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colors_from_the_page_are_validated() {
        assert_eq!(safe_color("#4ade80"), "#4ade80");
        assert_eq!(safe_color("#fff"), "#fff");
        assert_eq!(safe_color("red\" font=\"99"), "#cccccc");
        assert_eq!(safe_color("#12345"), "#cccccc");
    }

    #[test]
    fn status_symbols_are_dropped_from_the_button() {
        assert_eq!(button_text("▶ MPV · играет"), "MPV · играет");
        assert_eq!(button_text("⚠ MPV"), "MPV");
        assert_eq!(button_text("MPV · пауза"), "MPV · пауза");
    }
}
