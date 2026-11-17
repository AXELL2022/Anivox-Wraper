//! Ad blocking in the site webview through a WebKit content filter.
//!
//! The filter list (RU AdList + EasyList) is loaded and refreshed by
//! `anivox_core::adblock` on a background thread, converted into the content
//! blocker format and compiled by WebKit once per list version; the compiled
//! filter is cached in `adblock/compiled`. The site origins are excepted by the
//! converter, and the Shikimori overlay has its own content manager, so it is
//! not filtered.

use anivox_core::adblock;
use gtk4::glib;
use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use webkit6::prelude::*;
use webkit6::{UserContentFilter, UserContentFilterStore, UserContentManager, WebView};

pub struct Adblock {
    manager: UserContentManager,
    store: UserContentFilterStore,
    compiled_dir: PathBuf,
    data_dir: PathBuf,
    filter: RefCell<Option<UserContentFilter>>,
    enabled: Cell<bool>,
    started: Cell<bool>,
}

impl Adblock {
    pub fn install(webview: &WebView, data_dir: &Path, enabled: bool) -> Rc<Self> {
        let compiled_dir = adblock::cache_dir(data_dir).join("compiled");
        let _ = std::fs::create_dir_all(&compiled_dir);
        let adblock = Rc::new(Self {
            manager: webview
                .user_content_manager()
                .expect("у WebView нет менеджера содержимого"),
            store: UserContentFilterStore::new(&compiled_dir.to_string_lossy()),
            compiled_dir,
            data_dir: data_dir.to_owned(),
            filter: RefCell::default(),
            enabled: Cell::new(false),
            started: Cell::new(false),
        });
        adblock.set_enabled(enabled);
        adblock
    }

    /// Turns the filter on or off for new loads (reload the page to apply it fully).
    pub fn set_enabled(self: &Rc<Self>, enabled: bool) {
        if self.enabled.replace(enabled) == enabled {
            return;
        }
        if let Some(filter) = self.filter.borrow().as_ref() {
            if enabled {
                self.manager.add_filter(filter);
            } else {
                self.manager.remove_filter(filter);
            }
        }
        if enabled && !self.started.replace(true) {
            self.start();
        }
    }

    /// Cached lists first, then a refreshed list when the cache was stale.
    fn start(self: &Rc<Self>) {
        let (tx, rx) = async_channel::unbounded::<Vec<String>>();
        let dir = adblock::cache_dir(&self.data_dir);
        std::thread::spawn(move || {
            let user_agent = concat!("Anivox/", env!("CARGO_PKG_VERSION"));
            if let Err(err) = adblock::load_and_refresh(&dir, user_agent, |lists| {
                let _ = tx.send_blocking(lists);
            }) {
                eprintln!("{err}");
            }
        });
        let this = Rc::downgrade(self);
        glib::spawn_future_local(async move {
            while let Ok(lists) = rx.recv().await {
                let Some(this) = this.upgrade() else {
                    return;
                };
                this.load(lists).await;
            }
        });
    }

    async fn load(&self, lists: Vec<String>) {
        let key = adblock::lists_key(&lists);
        let filter = match self.store.load_future(&key).await {
            Ok(filter) => filter,
            Err(_) => match self.compile(&key, lists).await {
                Ok(filter) => filter,
                Err(err) => {
                    eprintln!("Не удалось подготовить фильтры рекламы: {err}");
                    return;
                }
            },
        };
        self.remove_stale(&key);
        if let Some(old) = self.filter.replace(Some(filter.clone())) {
            self.manager.remove_filter(&old);
        }
        if self.enabled.get() {
            self.manager.add_filter(&filter);
        }
    }

    /// Converts the lists off the main thread and lets WebKit compile the rules.
    async fn compile(&self, key: &str, lists: Vec<String>) -> Result<UserContentFilter, String> {
        let json = gtk4::gio::spawn_blocking(move || adblock::content_blocker_json(&lists))
            .await
            .map_err(|_| "преобразование прервано".to_owned())??;
        self.store
            .save_future(key, &glib::Bytes::from_owned(json.into_bytes()))
            .await
            .map_err(|err| err.to_string())
    }

    /// Compiled filters of older list versions.
    fn remove_stale(&self, key: &str) {
        let Ok(entries) = std::fs::read_dir(&self.compiled_dir) else {
            return;
        };
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name.contains("anivox-adblock-") && !name.contains(key) {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }
}
