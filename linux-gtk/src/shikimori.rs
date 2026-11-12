//! Shikimori overlay: a lazily created WebView above the site.
//!
//! Opening shows the same title as the current Anivox page (`/anime/<id>` has
//! the Shikimori id) or the Shikimori home page. The button, Escape and a click
//! on the backdrop close it, and closing destroys the view, so the next opening
//! starts fresh. Shikimori gets no Anivox IPC: its only message handler closes
//! the overlay. Engine-managed popups stay available for social sign-in.

use crate::ipc::Ipc;
use anivox_core::shikimori_overlay::{page_activity, start_url, ORIGIN};
use gtk4::prelude::*;
use gtk4::{gdk, gio, glib};
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;
use url::Url;
use webkit6::prelude::*;
use webkit6::{
    NavigationPolicyDecision, PolicyDecisionType, UserContentInjectedFrames, UserContentManager,
    UserScript, UserScriptInjectionTime, WebView,
};

const CLOSE_HANDLER: &str = "anivoxShikimoriClose";
const MARGIN: i32 = 16;
/// Escape in the page closes the overlay unless the page handled the key itself.
const ESCAPE_SCRIPT: &str = r#"(() => {
    if (location.origin !== 'https://shikimori.io' || window !== window.top) return;
    const handler = window.webkit?.messageHandlers?.anivoxShikimoriClose;
    if (!handler) return;
    document.addEventListener('keydown', event => {
        if (event.key === 'Escape' && !event.defaultPrevented) {
            event.preventDefault();
            handler.postMessage('close');
        }
    });
})();"#;
const CSS: &str = "
.shikimori-backdrop { background: alpha(black, 0.55); }
.shikimori-panel { background: @window_bg_color; border-radius: 12px; box-shadow: 0 12px 48px alpha(black, 0.5); }
.shikimori-panel .shikimori-header { padding: 4px 6px; }
";

struct Open {
    backdrop: gtk4::Box,
    panel: gtk4::Box,
    view: WebView,
    poll: glib::SourceId,
}

pub struct Shikimori {
    overlay: gtk4::Overlay,
    content: WebView,
    ipc: Arc<Ipc>,
    pub button: gtk4::ToggleButton,
    open: RefCell<Option<Open>>,
}

fn https_only(decision: &webkit6::PolicyDecision, kind: PolicyDecisionType) -> bool {
    if kind != PolicyDecisionType::NavigationAction && kind != PolicyDecisionType::NewWindowAction {
        return false;
    }
    let uri = decision
        .downcast_ref::<NavigationPolicyDecision>()
        .and_then(|nav| nav.navigation_action())
        .and_then(|action| action.request())
        .and_then(|request| request.uri());
    // Social sign-in redirects away from Shikimori and back, always over HTTPS.
    if uri.is_some_and(|uri| uri.starts_with("https://") || uri == "about:blank") {
        return false;
    }
    decision.ignore();
    true
}

/// Login popups (`window.open`) get a real related view so cookies and
/// `window.opener` keep working; they have no message handlers at all.
fn popup_for(view: &WebView, parent: &gtk4::Window) -> WebView {
    let popup = WebView::builder().related_view(view).build();
    popup.connect_decide_policy(|_, decision, kind| https_only(decision, kind));
    let window = gtk4::Window::builder()
        .transient_for(parent)
        .title("Shikimori — вход")
        .default_width(520)
        .default_height(680)
        .child(&popup)
        .build();
    popup.connect_ready_to_show(glib::clone!(
        #[weak]
        window,
        move |_| window.present()
    ));
    popup.connect_close(glib::clone!(
        #[weak]
        window,
        move |_| window.destroy()
    ));
    popup.connect_title_notify(glib::clone!(
        #[weak]
        window,
        move |popup| {
            if let Some(title) = popup.title().filter(|title| !title.is_empty()) {
                window.set_title(Some(&title));
            }
        }
    ));
    popup
}

impl Shikimori {
    pub fn install(overlay: &gtk4::Overlay, content: &WebView, ipc: &Arc<Ipc>) -> Rc<Self> {
        let provider = gtk4::CssProvider::new();
        provider.load_from_string(CSS);
        gtk4::style_context_add_provider_for_display(
            &WidgetExt::display(overlay),
            &provider,
            gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
        let button = gtk4::ToggleButton::builder()
            .label("Shikimori")
            .tooltip_text("Открыть Shikimori поверх сайта")
            .build();
        let shikimori = Rc::new(Self {
            overlay: overlay.clone(),
            content: content.clone(),
            ipc: ipc.clone(),
            button: button.clone(),
            open: RefCell::new(None),
        });
        button.connect_toggled(glib::clone!(
            #[weak]
            shikimori,
            move |button| {
                if button.is_active() {
                    shikimori.show();
                } else {
                    shikimori.close();
                }
            }
        ));
        shikimori
    }

    fn show(self: &Rc<Self>) {
        if self.open.borrow().is_some() {
            return;
        }
        let manager = UserContentManager::new();
        manager.add_script(&UserScript::new(
            ESCAPE_SCRIPT,
            UserContentInjectedFrames::TopFrame,
            UserScriptInjectionTime::Start,
            &["https://shikimori.io/*"],
            &[],
        ));
        let view = WebView::builder().user_content_manager(&manager).build();
        crate::disable_60fps_preference(&view);
        if manager.register_script_message_handler(CLOSE_HANDLER, None) {
            let weak_view = view.downgrade();
            let this = Rc::downgrade(self);
            manager.connect_script_message_received(Some(CLOSE_HANDLER), move |_, _| {
                // Only Shikimori itself may close the overlay, never a sign-in provider.
                let on_shikimori = weak_view
                    .upgrade()
                    .and_then(|view| view.uri())
                    .and_then(|uri| Url::parse(&uri).ok())
                    .is_some_and(|url| url.origin().ascii_serialization() == ORIGIN);
                if let (true, Some(this)) = (on_shikimori, this.upgrade()) {
                    this.button.set_active(false);
                }
            });
        }
        view.connect_decide_policy(|_, decision, kind| https_only(decision, kind));
        let parent = self.overlay.root().and_downcast::<gtk4::Window>();
        view.connect_create(move |view, _action| {
            let parent = parent.as_ref()?;
            Some(popup_for(view, parent).upcast())
        });
        view.set_vexpand(true);

        let backdrop = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
        backdrop.add_css_class("shikimori-backdrop");
        let click = gtk4::GestureClick::new();
        click.connect_released(glib::clone!(
            #[weak(rename_to = button)]
            self.button,
            move |_, _, _, _| button.set_active(false)
        ));
        backdrop.add_controller(click);

        let panel = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
        panel.add_css_class("shikimori-panel");
        panel.set_overflow(gtk4::Overflow::Hidden);
        panel.set_margin_top(MARGIN);
        panel.set_margin_bottom(MARGIN);
        panel.set_margin_start(MARGIN);
        panel.set_margin_end(MARGIN);
        panel.append(&self.header(&view));
        panel.append(&view);
        let keys = gtk4::EventControllerKey::new();
        keys.connect_key_pressed(glib::clone!(
            #[weak(rename_to = button)]
            self.button,
            #[upgrade_or]
            glib::Propagation::Proceed,
            move |_, key, _, _| {
                if key == gdk::Key::Escape {
                    button.set_active(false);
                    glib::Propagation::Stop
                } else {
                    glib::Propagation::Proceed
                }
            }
        ));
        // Bubble phase: Escape the page consumed itself does not reach this.
        panel.add_controller(keys);

        self.overlay.add_overlay(&backdrop);
        self.overlay.add_overlay(&panel);
        view.load_uri(&start_url(self.content.uri().as_deref()));
        view.grab_focus();

        let poll = glib::timeout_add_local(
            Duration::from_secs(2),
            glib::clone!(
                #[weak(rename_to = this)]
                self,
                #[upgrade_or]
                glib::ControlFlow::Break,
                move || {
                    this.report_activity();
                    glib::ControlFlow::Continue
                }
            ),
        );
        *self.open.borrow_mut() = Some(Open {
            backdrop,
            panel,
            view,
            poll,
        });
        self.button.set_tooltip_text(Some("Закрыть Shikimori"));
        self.report_activity();
    }

    fn header(&self, view: &WebView) -> gtk4::Box {
        let header = gtk4::Box::new(gtk4::Orientation::Horizontal, 4);
        header.add_css_class("shikimori-header");
        let back = gtk4::Button::from_icon_name("go-previous-symbolic");
        back.set_tooltip_text(Some("Назад"));
        let forward = gtk4::Button::from_icon_name("go-next-symbolic");
        forward.set_tooltip_text(Some("Вперёд"));
        let reload = gtk4::Button::from_icon_name("view-refresh-symbolic");
        reload.set_tooltip_text(Some("Обновить"));
        let title = gtk4::Label::new(Some("Shikimori"));
        title.add_css_class("heading");
        title.set_hexpand(true);
        title.set_ellipsize(gtk4::pango::EllipsizeMode::End);
        let close = gtk4::Button::from_icon_name("window-close-symbolic");
        close.set_tooltip_text(Some("Закрыть Shikimori (Esc)"));
        for button in [&back, &forward, &reload] {
            button.add_css_class("flat");
            header.append(button);
        }
        header.append(&title);
        close.add_css_class("flat");
        header.append(&close);

        back.connect_clicked(glib::clone!(
            #[weak]
            view,
            move |_| view.go_back()
        ));
        forward.connect_clicked(glib::clone!(
            #[weak]
            view,
            move |_| view.go_forward()
        ));
        reload.connect_clicked(glib::clone!(
            #[weak]
            view,
            move |_| view.reload()
        ));
        close.connect_clicked(glib::clone!(
            #[weak(rename_to = button)]
            self.button,
            move |_| button.set_active(false)
        ));
        let sync = glib::clone!(
            #[weak]
            back,
            #[weak]
            forward,
            move |view: &WebView| {
                back.set_sensitive(view.can_go_back());
                forward.set_sensitive(view.can_go_forward());
            }
        );
        view.connect_uri_notify(sync.clone());
        view.connect_load_changed(move |view, _| sync(view));
        view.connect_title_notify(glib::clone!(
            #[weak]
            title,
            move |view| {
                let text = view.title().filter(|text| !text.is_empty());
                title.set_text(text.as_deref().unwrap_or("Shikimori"));
            }
        ));
        header
    }

    fn report_activity(&self) {
        let activity = self
            .open
            .borrow()
            .as_ref()
            .map(|open| page_activity(open.view.uri().as_deref().unwrap_or("")));
        self.send_activity(activity);
    }

    fn send_activity(&self, activity: Option<String>) {
        let ipc = self.ipc.clone();
        glib::spawn_future_local(async move {
            let result = gio::spawn_blocking(move || ipc.set_shikimori_activity(activity)).await;
            if let Ok(Err(err)) = result {
                eprintln!("Discord: {err}");
            }
        });
    }

    pub fn close(&self) {
        let Some(open) = self.open.borrow_mut().take() else {
            return;
        };
        open.poll.remove();
        self.overlay.remove_overlay(&open.panel);
        self.overlay.remove_overlay(&open.backdrop);
        open.view.try_close();
        if self.button.is_active() {
            self.button.set_active(false);
        }
        self.button
            .set_tooltip_text(Some("Открыть Shikimori поверх сайта"));
        self.content.grab_focus();
        self.send_activity(None);
    }
}
