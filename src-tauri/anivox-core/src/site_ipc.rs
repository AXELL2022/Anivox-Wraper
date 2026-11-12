//! Site command dispatcher for the native shells (GTK, WinUI).
//!
//! Site scripts call `window.__TAURI__.core.invoke`; the shell forwards the
//! command here. Commands are blocking and must run off the UI thread; anything
//! that touches the window goes back to the shell as a [`SiteEvent`] through
//! the [`EventSink`] it passed in.

use crate::discord_presence::{update_discord_presence, DiscordPresence};
use crate::mpv::{PlaybackMedia, ResumePoint};
use crate::mpv_session::{self as session, MpvHost, MpvLaunchRequest, MpvState};
use crate::player_settings::{load_player_settings, PlayerSettings};
use crate::segments::SegmentStats;
use crate::shikimori::{presence_title_id, ShikimoriCache};
use crate::site_hosts;
use crate::site_load::{self, SharedLoad};
use discord_rich_presence::DiscordIpcClient;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use url::Url;

/// Requests for the UI thread. The WinUI shell receives them as JSON.
#[derive(Debug, Serialize, PartialEq)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum SiteEvent {
    #[serde(rename_all = "camelCase")]
    Status {
        text: String,
        color: String,
        reset_after_ms: u64,
        detail: Option<String>,
    },
    Fullscreen {
        on: bool,
    },
    /// `site-lifecycle.js` saw the site app mount for this load attempt.
    SiteReady {
        generation: u64,
    },
}

/// Delivers [`SiteEvent`]s to the shell's UI thread; called from background threads.
pub type EventSink = Arc<dyn Fn(SiteEvent) + Send + Sync>;

struct Shared {
    mpv: MpvState,
    discord: Mutex<Option<DiscordIpcClient>>,
    shikimori: ShikimoriCache,
    user_agent: &'static str,
    config_dir: PathBuf,
    data_dir: PathBuf,
    /// Current page address; set by the shell, used for the Referer origin.
    page: Mutex<Option<String>>,
    ui: EventSink,
    /// Latest video segment summary from `segment-monitor.js`.
    segments: Mutex<Option<SegmentStats>>,
    site_load: SharedLoad,
    /// Page kind shown in Discord while the Shikimori overlay is open.
    shikimori_activity: Mutex<Option<String>>,
}

#[derive(Clone)]
pub struct SiteHost(Arc<Shared>);

impl SiteHost {
    /// `user_agent` is the shell's `Anivox/<version>`.
    pub fn new(
        ui: EventSink,
        user_agent: &'static str,
        config_dir: PathBuf,
        data_dir: PathBuf,
    ) -> Self {
        Self(Arc::new(Shared {
            mpv: MpvState::default(),
            discord: Mutex::new(None),
            shikimori: ShikimoriCache::default(),
            user_agent,
            config_dir,
            data_dir,
            page: Mutex::new(None),
            ui,
            segments: Mutex::new(None),
            site_load: SharedLoad::default(),
            shikimori_activity: Mutex::new(None),
        }))
    }

    pub fn set_page(&self, uri: Option<String>) {
        if let Ok(mut page) = self.0.page.lock() {
            *page = uri;
        }
    }

    /// Load watchdog state shared with the recovery screen.
    pub fn site_load(&self) -> SharedLoad {
        self.0.site_load.clone()
    }

    pub fn config_dir(&self) -> &PathBuf {
        &self.0.config_dir
    }

    fn page(&self) -> Option<String> {
        self.0.page.lock().ok().and_then(|page| page.clone())
    }

    fn send(&self, event: SiteEvent) {
        (self.0.ui)(event);
    }
}

impl MpvHost for SiteHost {
    fn mpv(&self) -> &MpvState {
        &self.0.mpv
    }
    fn discord(&self) -> &Mutex<Option<DiscordIpcClient>> {
        &self.0.discord
    }
    fn shikimori(&self) -> &ShikimoriCache {
        &self.0.shikimori
    }
    fn user_agent(&self) -> &'static str {
        self.0.user_agent
    }
    fn site_origin(&self) -> String {
        site_hosts::site_origin_for(self.page().and_then(|uri| Url::parse(&uri).ok()).as_ref())
    }
    fn player_settings(&self) -> Result<PlayerSettings, String> {
        load_player_settings(&self.0.config_dir.join("settings.json"))
    }
    fn logs_dir(&self) -> Result<PathBuf, String> {
        Ok(self.0.data_dir.join("logs"))
    }
    fn history_path(&self) -> Result<PathBuf, String> {
        Ok(self.0.config_dir.join("playback-history.json"))
    }
    fn report_status(&self, text: &str, color: &str, reset_after_ms: u64, detail: Option<String>) {
        self.send(SiteEvent::Status {
            text: text.into(),
            color: color.into(),
            reset_after_ms,
            detail,
        });
    }
}

pub struct SiteIpc {
    host: SiteHost,
    runtime: tokio::runtime::Runtime,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct OpenInMpv {
    url: String,
    title: Option<String>,
    start_time: Option<f64>,
    sub_url: Option<String>,
    token: Option<String>,
    media: Option<PlaybackMedia>,
}

#[derive(Deserialize)]
struct DocumentState {
    href: String,
    loaded: bool,
    generation: u64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PlayerStatus {
    text: String,
    color: String,
    #[serde(default)]
    reset_after_ms: u64,
    detail: Option<String>,
}

impl SiteIpc {
    pub fn new(host: SiteHost) -> Result<Self, String> {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .map_err(|err| err.to_string())?;
        Ok(Self { host, runtime })
    }

    pub fn host(&self) -> &SiteHost {
        &self.host
    }

    /// Runs a future on the background runtime; call off the UI thread.
    pub fn block_on<F: std::future::Future>(&self, future: F) -> F::Output {
        self.runtime.block_on(future)
    }

    /// For the native connection panel.
    pub fn segment_stats(&self) -> Option<SegmentStats> {
        self.host.0.segments.lock().ok()?.clone()
    }

    /// The Shikimori page kind Discord currently shows, if the overlay is open.
    pub fn shikimori_activity(&self) -> Option<String> {
        self.host.0.shikimori_activity.lock().ok()?.clone()
    }

    /// While the Shikimori overlay is open and MPV is not playing, Discord shows
    /// the kind of Shikimori page instead of the embedded player; `None`
    /// restores the embedded player's last status.
    pub fn set_shikimori_activity(&self, activity: Option<String>) -> Result<(), String> {
        {
            let mut current = self.host.0.shikimori_activity.lock().map_err(err)?;
            if *current == activity {
                return Ok(());
            }
            current.clone_from(&activity);
        }
        let mut client = self.host.discord().lock().map_err(err)?;
        if self.mpv_owns_discord()? {
            return Ok(());
        }
        let presence = match activity {
            Some(text) => DiscordPresence {
                details: "Shikimori".into(),
                state_text: text,
                ..DiscordPresence::default()
            },
            None => {
                let mut presence = self.host.mpv().embedded.lock().map_err(err)?.clone();
                self.host.shikimori().decorate(&mut presence);
                presence
            }
        };
        update_discord_presence(&mut client, &presence)
    }

    fn mpv_owns_discord(&self) -> Result<bool, String> {
        Ok(self
            .host
            .mpv()
            .current
            .lock()
            .map_err(err)?
            .as_ref()
            .is_some_and(|session| session.playback.ready))
    }

    /// Used by the native settings panel only; remote pages have no such command.
    pub fn clear_playback_history(&self) -> Result<(), String> {
        session::clear_playback_history(&self.host)
    }

    /// Site-facing commands, the same set in every native shell.
    pub fn handle(&self, command: &str, args: Value) -> Result<Value, String> {
        match command {
            "set_discord_rpc" => self.set_discord_rpc(args),
            "get_mpv_state" => to_json(self.host.mpv().current.lock().map_err(err)?.clone()),
            "get_playback_resume" => {
                let key = args["key"].as_str().ok_or("Нет ключа серии")?;
                let point: Option<ResumePoint> = session::playback_resume(&self.host, key)?;
                to_json(point)
            }
            "open_in_mpv" => {
                let request: OpenInMpv = from_json(args)?;
                let launched = self.runtime.block_on(session::open_in_mpv(
                    self.host.clone(),
                    MpvLaunchRequest {
                        url: request.url,
                        title: request.title,
                        start_time: request.start_time,
                        sub_url: request.sub_url,
                        token: request.token,
                        media: request.media,
                    },
                ))?;
                to_json(launched)
            }
            "report_player_status" => {
                let status: PlayerStatus = from_json(args)?;
                self.host.send(SiteEvent::Status {
                    text: status.text,
                    color: status.color,
                    reset_after_ms: status.reset_after_ms,
                    detail: status.detail,
                });
                Ok(Value::Null)
            }
            "report_segment_stats" => {
                let stats: SegmentStats =
                    from_json(args.get("stats").cloned().unwrap_or(Value::Null))?;
                *self.host.0.segments.lock().map_err(err)? = Some(stats.sanitized());
                Ok(Value::Null)
            }
            "get_site_document_generation" => {
                let info = self.host.0.site_load.lock().map_err(err)?;
                Ok(Value::from(info.generation))
            }
            "site_document_state" => {
                let report: DocumentState = from_json(args)?;
                let page = self.host.page().ok_or("Страница неизвестна")?;
                let current = self.host.0.site_load.lock().map_err(err)?.generation;
                if site_load::document_report_matches(&page, &report.href)?
                    && report.generation == current
                    && report.loaded
                {
                    self.host.send(SiteEvent::SiteReady {
                        generation: report.generation,
                    });
                }
                Ok(Value::Null)
            }
            "content_fullscreen" => {
                let on = args["fullscreen"].as_bool().unwrap_or(false);
                self.host.send(SiteEvent::Fullscreen { on });
                Ok(Value::Null)
            }
            other => Err(format!("Команда недоступна в этой оболочке: {other}")),
        }
    }

    fn set_discord_rpc(&self, args: Value) -> Result<Value, String> {
        let mut presence: DiscordPresence =
            from_json(args.get("presence").cloned().unwrap_or(Value::Null))?;
        presence.truncate_texts();
        if let Some(id) = presence_title_id(&presence) {
            self.runtime
                .block_on(self.host.shikimori().ensure(id, self.host.user_agent()));
        }
        let mut client = self.host.discord().lock().map_err(err)?;
        // The embedded player keeps its last status; an active MPV session owns Discord.
        *self.host.mpv().embedded.lock().map_err(err)? = presence.clone();
        if self.mpv_owns_discord()?
            || self
                .host
                .0
                .shikimori_activity
                .lock()
                .map_err(err)?
                .is_some()
        {
            return Ok(Value::Null);
        }
        self.host.shikimori().decorate(&mut presence);
        update_discord_presence(&mut client, &presence)?;
        Ok(Value::Null)
    }
}

fn err<E: ToString>(error: E) -> String {
    error.to_string()
}

fn from_json<T: serde::de::DeserializeOwned>(value: Value) -> Result<T, String> {
    serde_json::from_value(value).map_err(err)
}

fn to_json<T: serde::Serialize>(value: T) -> Result<Value, String> {
    serde_json::to_value(value).map_err(err)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // Discord itself is not exercised here: on Windows its named pipe belongs to
    // the user's running client, and a test must not change their status.

    fn ipc(dir: &std::path::Path) -> (SiteIpc, Arc<Mutex<Vec<SiteEvent>>>) {
        let events: Arc<Mutex<Vec<SiteEvent>>> = Arc::default();
        let sink = events.clone();
        let host = SiteHost::new(
            Arc::new(move |event| sink.lock().unwrap().push(event)),
            "Anivox-test",
            dir.join("config"),
            dir.join("data"),
        );
        (SiteIpc::new(host).unwrap(), events)
    }

    #[test]
    fn unknown_commands_are_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let (ipc, _events) = ipc(dir.path());
        let err = ipc.handle("close_shikimori", json!({})).unwrap_err();
        assert!(err.contains("close_shikimori"), "{err}");
    }

    #[test]
    fn malformed_presence_is_rejected_before_touching_discord() {
        let dir = tempfile::tempdir().unwrap();
        let (ipc, _events) = ipc(dir.path());
        assert!(ipc
            .handle("set_discord_rpc", json!({"presence": 5}))
            .is_err());
        assert!(ipc.host().discord().lock().unwrap().is_none());
    }

    #[test]
    fn mpv_state_is_empty_and_resume_needs_a_key() {
        let dir = tempfile::tempdir().unwrap();
        let (ipc, _events) = ipc(dir.path());
        assert_eq!(ipc.handle("get_mpv_state", json!({})).unwrap(), Value::Null);
        assert!(ipc.handle("get_playback_resume", json!({})).is_err());
        let none = ipc
            .handle("get_playback_resume", json!({"key": "1:2:3"}))
            .unwrap();
        assert_eq!(none, Value::Null);
    }

    #[test]
    fn status_and_fullscreen_are_forwarded_to_the_ui() {
        let dir = tempfile::tempdir().unwrap();
        let (ipc, events) = ipc(dir.path());
        ipc.handle(
            "report_player_status",
            json!({"text": "MPV", "color": "#fff", "resetAfterMs": 5, "detail": null}),
        )
        .unwrap();
        ipc.handle("content_fullscreen", json!({"fullscreen": true}))
            .unwrap();
        let events = events.lock().unwrap();
        assert!(matches!(events[0], SiteEvent::Status { .. }));
        assert_eq!(events[1], SiteEvent::Fullscreen { on: true });
        assert_eq!(
            serde_json::to_value(&events[1]).unwrap(),
            json!({"type": "fullscreen", "on": true})
        );
        assert_eq!(
            serde_json::to_value(&events[0]).unwrap(),
            json!({"type": "status", "text": "MPV", "color": "#fff", "resetAfterMs": 5, "detail": null})
        );
    }

    #[test]
    fn segment_stats_are_stored_for_the_connection_panel() {
        let dir = tempfile::tempdir().unwrap();
        let (ipc, _events) = ipc(dir.path());
        assert!(ipc.segment_stats().is_none());
        assert!(ipc
            .handle("report_segment_stats", json!({"stats": 1}))
            .is_err());
        ipc.handle(
            "report_segment_stats",
            json!({"stats": {"host": "cdn", "active": true, "mbps": 12.5, "updatedAt": 5.0}}),
        )
        .unwrap();
        let stats = ipc.segment_stats().unwrap();
        assert_eq!((stats.host.as_str(), stats.mbps), ("cdn", 12.5));
    }

    #[test]
    fn only_the_current_page_and_attempt_can_end_the_loading_screen() {
        let dir = tempfile::tempdir().unwrap();
        let (ipc, events) = ipc(dir.path());
        ipc.host().site_load().lock().unwrap().begin();
        ipc.host()
            .set_page(Some("https://anivox.fun/anime/1".into()));
        assert_eq!(
            ipc.handle("get_site_document_generation", json!({}))
                .unwrap(),
            json!(1)
        );
        let report = |href: &str, generation: u64| json!({"href": href, "loaded": true, "generation": generation});
        ipc.handle("site_document_state", report("https://anivox.fun/", 1))
            .unwrap();
        ipc.handle(
            "site_document_state",
            report("https://anivox.fun/anime/1", 0),
        )
        .unwrap();
        assert!(ipc
            .handle(
                "site_document_state",
                report("https://evil.test/anime/1", 1)
            )
            .is_err());
        assert!(events.lock().unwrap().is_empty());
        ipc.handle(
            "site_document_state",
            report("https://anivox.fun/anime/1", 1),
        )
        .unwrap();
        assert_eq!(
            events.lock().unwrap()[0],
            SiteEvent::SiteReady { generation: 1 }
        );
    }

    #[test]
    fn launching_a_missing_player_fails_with_a_clear_message() {
        let dir = tempfile::tempdir().unwrap();
        let (ipc, _events) = ipc(dir.path());
        std::fs::create_dir_all(dir.path().join("config")).unwrap();
        let missing = dir.path().join("missing").join("mpv-player");
        let settings = json!({"mpvPath": missing.to_string_lossy()});
        std::fs::write(
            dir.path().join("config/settings.json"),
            settings.to_string(),
        )
        .unwrap();
        let err = ipc
            .handle("open_in_mpv", json!({"url": "https://example.test/v.m3u8"}))
            .unwrap_err();
        assert!(err.contains("MPV"), "{err}");
    }
}
