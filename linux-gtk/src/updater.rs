//! Update check, changelog review and installation of the pacman package.
//!
//! The release manifest (`latest.json`, the same one the Tauri updater reads)
//! carries the version, the notes from CHANGELOG and the signed package. When the
//! release has a package for this build, the button downloads it: the core checks
//! the manifest again (a changed version shows its own notes and needs a fresh
//! confirmation), downloads only from this repository's releases and keeps the
//! file only with a valid release-key signature. "Установить" then runs
//! `pkexec pacman -U` (the system asks for the administrator password) and
//! "Перезапустить" starts the new version. Without a package, or when pkexec is
//! missing, the release page and the manual `pacman -U` command remain.

use crate::ipc::Ipc;
use anivox_core::updater::{self as core, Download, UpdateInfo, RELEASE_PAGE};
use gtk4::prelude::*;
use gtk4::{gio, glib};
use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

pub const CURRENT: &str = env!("CARGO_PKG_VERSION");
const APP_DIR: &str = "com.yasin.anivox";

/// `pkexec pacman -U` for a verified package. `--ask=4` answers pacman's
/// "remove the conflicting package?" with yes: the package replaces the retired
/// Tauri package `anivox` (ALPM_QUESTION_CONFLICT_PKG).
fn install_command(package: &Path) -> Vec<std::ffi::OsString> {
    let mut args: Vec<std::ffi::OsString> = ["pkexec", "pacman", "-U", "--noconfirm", "--ask=4"]
        .into_iter()
        .map(Into::into)
        .collect();
    args.push(package.as_os_str().to_owned());
    args
}

fn manual_command(package: &Path) -> String {
    format!("sudo pacman -U '{}'", package.display())
}

/// Message for a failed `pkexec pacman` run.
fn install_error(status: i32, stderr: &str) -> String {
    match status {
        // pkexec: the authentication dialog was dismissed / not authorized.
        126 => "Установка отменена: пароль не введён.".to_owned(),
        127 => "Нет прав на установку или polkit недоступен.".to_owned(),
        _ => {
            let detail: Vec<&str> = stderr
                .lines()
                .map(str::trim)
                .filter(|line| !line.is_empty())
                .collect();
            let tail = detail[detail.len().saturating_sub(3)..].join(" ");
            if tail.is_empty() {
                format!("pacman завершился с ошибкой {status}.")
            } else {
                format!("pacman завершился с ошибкой {status}: {tail}")
            }
        }
    }
}

/// The running binary; after pacman replaced it, Linux reports "<path> (deleted)".
fn current_binary() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let text = exe.to_string_lossy();
    Some(PathBuf::from(
        text.strip_suffix(" (deleted)").unwrap_or(&text).to_owned(),
    ))
}

/// Shell snippet that waits until process `$1` is gone and then starts `$2`:
/// GApplication would hand a second instance over to the one still running.
const RELAUNCH_SCRIPT: &str = "while kill -0 \"$1\" 2>/dev/null; do sleep 0.2; done; exec \"$2\"";

struct Review {
    button: gtk4::MenuButton,
    title: gtk4::Label,
    notes: gtk4::TextView,
    status: gtk4::Label,
    progress: gtk4::ProgressBar,
    action: gtk4::Button,
    cancel: gtk4::Button,
    retry: gtk4::Button,
    /// The release whose notes are on screen: only it can be downloaded or opened.
    reviewed: RefCell<Option<UpdateInfo>>,
    /// The release has a package this build may download and install.
    installable: Cell<bool>,
    busy: Cell<bool>,
    /// A verified package for the reviewed version.
    downloaded: RefCell<Option<(String, PathBuf)>>,
    installed: Cell<bool>,
    stop: Arc<AtomicBool>,
    received: Arc<AtomicU64>,
    total: Arc<AtomicU64>,
    ipc: Arc<Ipc>,
    /// Saves the session and quits the application.
    quit: Rc<dyn Fn()>,
}

impl Review {
    fn show(&self, info: Option<UpdateInfo>) {
        match &info {
            Some(info) => {
                self.button.set_label(&format!("v{}", info.version));
                self.button
                    .set_tooltip_text(Some(&format!("Доступно обновление v{}", info.version)));
                self.title.set_text(&format!("Anivox v{}", info.version));
                let notes = if info.notes.trim().is_empty() {
                    "Описание изменений не указано."
                } else {
                    info.notes.trim()
                };
                self.notes.buffer().set_text(notes);
            }
            None => self.title.set_text("Обновление Anivox"),
        }
        self.installable.set(
            info.as_ref()
                .is_some_and(|info| info.installable().is_some()),
        );
        let version = info.as_ref().map(|info| info.version.clone());
        let keep = matches!(
            (&*self.downloaded.borrow(), &version),
            (Some((done, path)), Some(version)) if done == version && path.exists()
        );
        if !keep {
            *self.downloaded.borrow_mut() = None;
            self.installed.set(false);
        }
        self.button
            .set_visible(info.is_some() || self.button.is_visible());
        *self.reviewed.borrow_mut() = info;
        self.refresh();
    }

    /// Label and sensitivity of the main button for the current state.
    fn refresh(&self) {
        let label = if self.installed.get() {
            "Перезапустить"
        } else if self.downloaded.borrow().is_some() {
            "Установить"
        } else if self.installable.get() {
            "Скачать и установить"
        } else {
            "Открыть страницу обновления"
        };
        self.action.set_label(label);
        self.action
            .set_sensitive(!self.busy.get() && self.reviewed.borrow().is_some());
        self.retry.set_sensitive(!self.busy.get());
    }

    fn set_busy(&self, busy: bool) {
        self.busy.set(busy);
        self.refresh();
    }

    fn check(
        self: &Rc<Self>,
        then: impl FnOnce(&Rc<Self>, Result<Option<UpdateInfo>, String>) + 'static,
    ) {
        self.set_busy(true);
        let ipc = self.ipc.clone();
        let this = self.clone();
        glib::spawn_future_local(async move {
            let result = gio::spawn_blocking(move || ipc.block_on(core::check(CURRENT)))
                .await
                .unwrap_or_else(|_| Err("Проверка завершилась аварийно".into()));
            this.set_busy(false);
            then(&this, result);
        });
    }

    fn act(self: &Rc<Self>) {
        if self.busy.get() {
            return;
        }
        if self.installed.get() {
            self.relaunch();
            return;
        }
        let downloaded = self.downloaded.borrow().clone();
        if let Some((_, path)) = downloaded {
            self.install(path);
        } else if self.installable.get() {
            self.download();
        } else {
            self.open_page_after_check();
        }
    }

    /// Downloads and verifies the reviewed package; a changed release is shown again.
    fn download(self: &Rc<Self>) {
        let Some(reviewed) = self.reviewed.borrow().clone() else {
            return;
        };
        let dir = glib::user_cache_dir().join(APP_DIR).join("updates");
        self.stop.store(false, Ordering::Relaxed);
        self.received.store(0, Ordering::Relaxed);
        self.total.store(0, Ordering::Relaxed);
        self.set_busy(true);
        self.cancel.set_visible(true);
        self.cancel.set_sensitive(true);
        self.progress.set_fraction(0.0);
        self.progress.set_visible(true);
        self.status.set_text("Проверка версии…");

        let this = self.clone();
        glib::timeout_add_local(Duration::from_millis(200), move || {
            if !this.cancel.is_visible() {
                return glib::ControlFlow::Break;
            }
            this.show_progress();
            glib::ControlFlow::Continue
        });

        let (ipc, stop, received, total) = (
            self.ipc.clone(),
            self.stop.clone(),
            self.received.clone(),
            self.total.clone(),
        );
        let this = self.clone();
        glib::spawn_future_local(async move {
            let version = reviewed.version.clone();
            let result = gio::spawn_blocking(move || {
                ipc.block_on(core::download_reviewed(
                    &reviewed,
                    CURRENT,
                    &dir,
                    |done, size| {
                        received.store(done, Ordering::Relaxed);
                        total.store(size.unwrap_or(0), Ordering::Relaxed);
                    },
                    &stop,
                ))
            })
            .await
            .unwrap_or_else(|_| Err("Загрузка завершилась аварийно".into()));
            this.cancel.set_visible(false);
            this.progress.set_visible(false);
            this.busy.set(false);
            match result {
                Ok(Download::Ready(path)) => {
                    *this.downloaded.borrow_mut() = Some((version.clone(), path));
                    this.status.set_text(&format!(
                        "Пакет v{version} скачан, подпись проверена. Нажмите «Установить»: \
                         система спросит пароль администратора."
                    ));
                    this.refresh();
                }
                Ok(Download::Changed(Some(info))) => {
                    this.show(Some(info));
                    this.status.set_text(
                        "Вышла другая версия. Ознакомьтесь с изменениями и подтвердите ещё раз.",
                    );
                }
                Ok(Download::Changed(None)) => {
                    this.show(None);
                    this.status.set_text("Обновление больше не доступно.");
                }
                Err(err) if err == "Загрузка отменена" => {
                    this.status.set_text("Загрузка отменена.");
                    this.refresh();
                }
                Err(err) => {
                    this.status.set_text(&format!(
                        "{err}. Попробуйте ещё раз или скачайте пакет со страницы выпуска."
                    ));
                    this.refresh();
                }
            }
        });
    }

    fn show_progress(&self) {
        let received = self.received.load(Ordering::Relaxed);
        let total = self.total.load(Ordering::Relaxed);
        let mb = |bytes: u64| format!("{:.1}", bytes as f64 / 1_048_576.0);
        if total > 0 {
            self.progress
                .set_fraction((received as f64 / total as f64).min(1.0));
            self.status
                .set_text(&format!("Скачивание… {} из {} МБ", mb(received), mb(total)));
        } else if received > 0 {
            self.progress.pulse();
            self.status
                .set_text(&format!("Скачивание… {} МБ", mb(received)));
        }
    }

    /// `pkexec pacman -U` for the verified package.
    fn install(self: &Rc<Self>, path: PathBuf) {
        if !path.exists() {
            *self.downloaded.borrow_mut() = None;
            self.status
                .set_text("Скачанный пакет не найден. Скачайте обновление ещё раз.");
            self.refresh();
            return;
        }
        let args = install_command(&path);
        let argv: Vec<&std::ffi::OsStr> = args.iter().map(|arg| arg.as_os_str()).collect();
        let process = match gio::Subprocess::newv(&argv, gio::SubprocessFlags::STDERR_PIPE) {
            Ok(process) => process,
            Err(err) => {
                self.status.set_text(&format!(
                    "Не удалось запустить pkexec ({err}). Установите пакет вручную:\n{}",
                    manual_command(&path)
                ));
                return;
            }
        };
        self.set_busy(true);
        self.status
            .set_text("Установка… Система спросит пароль администратора.");
        let this = self.clone();
        glib::spawn_future_local(async move {
            let output = process.communicate_utf8_future(None).await;
            this.set_busy(false);
            let stderr = output
                .ok()
                .and_then(|(_, stderr)| stderr)
                .map(|text| text.to_string())
                .unwrap_or_default();
            let status = if process.has_exited() {
                process.exit_status()
            } else {
                -1
            };
            if status == 0 {
                this.installed.set(true);
                this.status.set_text(
                    "Обновление установлено. Нажмите «Перезапустить», чтобы открыть новую версию.",
                );
            } else {
                this.status.set_text(&format!(
                    "{}\nУстановить вручную: {}",
                    install_error(status, &stderr),
                    manual_command(&path)
                ));
            }
            this.refresh();
        });
    }

    /// Starts the new binary once this process has exited, then quits.
    fn relaunch(&self) {
        let Some(binary) = current_binary() else {
            self.status
                .set_text("Не удалось определить путь к Anivox. Запустите его заново вручную.");
            return;
        };
        let pid = std::process::id().to_string();
        let argv: [&std::ffi::OsStr; 6] = [
            "sh".as_ref(),
            "-c".as_ref(),
            RELAUNCH_SCRIPT.as_ref(),
            "anivox-relaunch".as_ref(),
            pid.as_ref(),
            binary.as_os_str(),
        ];
        if let Err(err) = gio::Subprocess::newv(&argv, gio::SubprocessFlags::NONE) {
            self.status.set_text(&format!(
                "Не удалось перезапустить Anivox ({err}). Запустите его заново вручную."
            ));
            return;
        }
        (self.quit)();
    }

    /// Re-checks right before acting so an unreviewed version is never opened.
    fn open_page_after_check(self: &Rc<Self>) {
        let Some(expected) = self.reviewed.borrow().clone() else {
            return;
        };
        self.status.set_text("Проверка версии…");
        self.check(move |this, result| match result {
            Ok(Some(info)) if info.version == expected.version => this.open_page(
                "Открыта страница выпуска. Скачайте пакет и установите его \
                 (pacman -U), затем перезапустите Anivox.",
            ),
            Ok(Some(info)) => {
                this.show(Some(info));
                this.status.set_text(
                    "Вышла другая версия. Ознакомьтесь с изменениями и подтвердите ещё раз.",
                );
            }
            Ok(None) => {
                this.show(None);
                this.status.set_text("Обновление больше не доступно.");
            }
            Err(err) => this.status.set_text(&err),
        });
    }

    fn open_page(&self, done: &str) {
        match gio::AppInfo::launch_default_for_uri(RELEASE_PAGE, gio::AppLaunchContext::NONE) {
            Ok(()) => self.status.set_text(done),
            Err(err) => self
                .status
                .set_text(&format!("Не удалось открыть браузер: {err}")),
        }
    }
}

/// Header button that appears when a newer version is published. `quit` saves the
/// session and quits; it is used to restart into the installed version.
pub fn button(ipc: &Arc<Ipc>, quit: Rc<dyn Fn()>) -> gtk4::MenuButton {
    let content = gtk4::Box::new(gtk4::Orientation::Vertical, 10);
    content.set_margin_top(12);
    content.set_margin_bottom(12);
    content.set_margin_start(12);
    content.set_margin_end(12);
    content.set_width_request(440);
    let title = gtk4::Label::new(Some("Обновление Anivox"));
    title.add_css_class("title-4");
    title.set_xalign(0.0);
    content.append(&title);
    let current = gtk4::Label::new(Some(&format!("Установлена версия v{CURRENT}")));
    current.add_css_class("dim-label");
    current.set_xalign(0.0);
    content.append(&current);
    let notes = gtk4::TextView::builder()
        .editable(false)
        .cursor_visible(false)
        .wrap_mode(gtk4::WrapMode::WordChar)
        .top_margin(8)
        .bottom_margin(8)
        .left_margin(8)
        .right_margin(8)
        .build();
    let scroller = gtk4::ScrolledWindow::builder()
        .child(&notes)
        .min_content_height(220)
        .max_content_height(360)
        .propagate_natural_height(true)
        .build();
    scroller.add_css_class("card");
    content.append(&scroller);
    let status = gtk4::Label::new(Some("Перед установкой ознакомьтесь с изменениями."));
    status.set_wrap(true);
    status.set_xalign(0.0);
    // The manual pacman command must be copyable.
    status.set_selectable(true);
    content.append(&status);
    let progress = gtk4::ProgressBar::new();
    progress.set_visible(false);
    content.append(&progress);
    let actions = gtk4::Box::new(gtk4::Orientation::Horizontal, 6);
    let action = gtk4::Button::with_label("Открыть страницу обновления");
    action.add_css_class("suggested-action");
    let cancel = gtk4::Button::with_label("Отмена");
    cancel.set_visible(false);
    let retry = gtk4::Button::with_label("Проверить снова");
    let later = gtk4::Button::with_label("Позже");
    actions.append(&action);
    actions.append(&cancel);
    actions.append(&retry);
    actions.append(&later);
    content.append(&actions);
    let page = gtk4::LinkButton::with_label(RELEASE_PAGE, "Страница выпуска");
    page.set_halign(gtk4::Align::Start);
    content.append(&page);

    let popover = gtk4::Popover::builder().child(&content).build();
    let button = gtk4::MenuButton::builder()
        .label("Обновление")
        .popover(&popover)
        .visible(false)
        .build();
    button.add_css_class("suggested-action");

    let review = Rc::new(Review {
        button: button.clone(),
        title,
        notes,
        status,
        progress,
        action: action.clone(),
        cancel: cancel.clone(),
        retry: retry.clone(),
        reviewed: RefCell::new(None),
        installable: Cell::new(false),
        busy: Cell::new(false),
        downloaded: RefCell::new(None),
        installed: Cell::new(false),
        stop: Arc::new(AtomicBool::new(false)),
        received: Arc::new(AtomicU64::new(0)),
        total: Arc::new(AtomicU64::new(0)),
        ipc: ipc.clone(),
        quit,
    });
    action.connect_clicked(glib::clone!(
        #[strong]
        review,
        move |_| review.act()
    ));
    cancel.connect_clicked(glib::clone!(
        #[strong]
        review,
        move |cancel| {
            review.stop.store(true, Ordering::Relaxed);
            cancel.set_sensitive(false);
            review.status.set_text("Отмена…");
        }
    ));
    retry.connect_clicked(glib::clone!(
        #[strong]
        review,
        move |_| {
            review.status.set_text("Проверка обновлений…");
            review.check(|this, result| match result {
                Ok(info) => {
                    let found = info.is_some();
                    this.show(info);
                    this.status.set_text(if found {
                        "Перед установкой ознакомьтесь с изменениями."
                    } else {
                        "У вас последняя версия."
                    });
                }
                Err(err) => this.status.set_text(&err),
            });
        }
    ));
    later.connect_clicked(glib::clone!(
        #[weak]
        popover,
        move |_| popover.popdown()
    ));

    // Background check shortly after launch.
    glib::timeout_add_local_once(
        Duration::from_secs(3),
        glib::clone!(
            #[strong]
            review,
            move || {
                review.check(|this, result| match result {
                    Ok(info) => this.show(info),
                    Err(err) => eprintln!("{err}"),
                })
            }
        ),
    );
    button
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_package_is_installed_with_pkexec_and_replaces_the_tauri_package() {
        let args = install_command(Path::new("/tmp/anivox gtk.pkg.tar.zst"));
        let args: Vec<_> = args.iter().map(|arg| arg.to_string_lossy()).collect();
        assert_eq!(
            args,
            [
                "pkexec",
                "pacman",
                "-U",
                "--noconfirm",
                "--ask=4",
                "/tmp/anivox gtk.pkg.tar.zst"
            ]
        );
        assert_eq!(
            manual_command(Path::new("/tmp/a.pkg.tar.zst")),
            "sudo pacman -U '/tmp/a.pkg.tar.zst'"
        );
    }

    #[test]
    fn install_failures_explain_themselves() {
        assert!(install_error(126, "").contains("отменена"));
        assert!(install_error(127, "").contains("polkit"));
        assert_eq!(
            install_error(1, "\nerror: one\nerror: two\n"),
            "pacman завершился с ошибкой 1: error: one error: two"
        );
        assert_eq!(install_error(1, ""), "pacman завершился с ошибкой 1.");
    }

    #[test]
    fn relaunch_waits_for_this_process_and_runs_the_binary() {
        let marker = std::env::temp_dir().join(format!("anivox-relaunch-{}", std::process::id()));
        let _ = std::fs::remove_file(&marker);
        let mut waited = std::process::Command::new("sleep")
            .arg("1")
            .spawn()
            .unwrap();
        let pid = waited.id();
        // Reap the child as soon as it ends: `kill -0` succeeds on a zombie.
        let reaper = std::thread::spawn(move || waited.wait());
        let target =
            std::env::temp_dir().join(format!("anivox-relaunch-{}.sh", std::process::id()));
        std::fs::write(
            &target,
            format!("#!/bin/sh\ntouch '{}'\n", marker.display()),
        )
        .unwrap();
        std::process::Command::new("chmod")
            .arg("+x")
            .arg(&target)
            .status()
            .unwrap();
        let started = std::time::Instant::now();
        let status = std::process::Command::new("sh")
            .args(["-c", RELAUNCH_SCRIPT, "anivox-relaunch"])
            .arg(pid.to_string())
            .arg(&target)
            .status()
            .unwrap();
        assert!(status.success());
        assert!(started.elapsed() >= Duration::from_millis(800));
        assert!(marker.exists());
        let _ = reaper.join();
        let _ = std::fs::remove_file(&marker);
        let _ = std::fs::remove_file(&target);
    }
}
