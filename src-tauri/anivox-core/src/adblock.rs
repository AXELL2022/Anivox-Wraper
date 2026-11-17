//! Ad blocking with Brave's `adblock` engine and the RU AdList + EasyList filter list.
//!
//! The list is downloaded into a cache folder and refreshed once a day; with no
//! cache and no network nothing is blocked. Requests to the site itself
//! (`anivox.fun`, `anivox.me`) and top-level documents are never blocked, so a
//! bad rule cannot break the site.
//!
//! WinUI asks [`Blocker::should_block`] for every request; WebKit shells
//! (feature `adblock-webkit`) compile [`content_blocker_json`] into a native
//! content filter instead.

use crate::site_hosts;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

pub struct FilterList {
    /// File name in the cache folder.
    pub file: &'static str,
    pub url: &'static str,
}

/// RU AdList bundled with EasyList by its maintainers: Russian ad networks plus the global list.
pub const LISTS: &[FilterList] = &[FilterList {
    file: "ruadlist-easylist.txt",
    url: "https://easylist-downloads.adblockplus.org/ruadlist+easylist.txt",
}];

/// Lists older than this are downloaded again (the lists declare `Expires: 1 days`).
pub const REFRESH_AFTER: Duration = Duration::from_secs(24 * 60 * 60);

/// A downloaded list larger than this is rejected (the real one is about 3 MB).
const MAX_LIST_BYTES: usize = 32 * 1024 * 1024;

/// Cache folder for the lists inside the shell's local data folder.
pub fn cache_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("adblock")
}

/// Texts of the cached lists (missing or unreadable ones are skipped).
pub fn cached_lists(dir: &Path) -> Vec<String> {
    LISTS
        .iter()
        .filter_map(|list| std::fs::read_to_string(dir.join(list.file)).ok())
        .filter(|text| looks_like_filter_list(text))
        .collect()
}

/// True when a list is missing or older than [`REFRESH_AFTER`].
pub fn is_stale(dir: &Path, now: SystemTime) -> bool {
    LISTS.iter().any(|list| {
        std::fs::metadata(dir.join(list.file))
            .and_then(|meta| meta.modified())
            .ok()
            .and_then(|modified| now.duration_since(modified).ok())
            .is_none_or(|age| age >= REFRESH_AFTER)
    })
}

/// Adblock Plus list header check: rejects HTML error pages and captive portals.
pub fn looks_like_filter_list(text: &str) -> bool {
    let text = text.trim_start_matches('\u{feff}').trim_start();
    text.starts_with("[Adblock")
        || text
            .lines()
            .take(30)
            .any(|line| line.starts_with("! Title:"))
}

fn save_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    use std::io::Write;
    let parent = path.parent().ok_or("Не определён каталог фильтров")?;
    std::fs::create_dir_all(parent).map_err(|err| err.to_string())?;
    let mut file = tempfile::NamedTempFile::new_in(parent).map_err(|err| err.to_string())?;
    file.write_all(bytes).map_err(|err| err.to_string())?;
    file.as_file().sync_all().map_err(|err| err.to_string())?;
    file.persist(path)
        .map_err(|err| format!("Не удалось сохранить список фильтров: {}", err.error))?;
    Ok(())
}

/// Downloads every list into `dir` when the cache is stale. Returns true when a
/// list changed (rebuild the blocker), false when the cache was fresh or the
/// downloads matched it. A failed download keeps the old file.
#[cfg(feature = "net")]
pub async fn refresh(dir: &Path, user_agent: &str) -> Result<bool, String> {
    if !is_stale(dir, SystemTime::now()) {
        return Ok(false);
    }
    let _ = rustls::crypto::ring::default_provider().install_default();
    let client = reqwest::Client::builder()
        .user_agent(user_agent)
        .timeout(Duration::from_secs(60))
        .build()
        .map_err(|err| err.to_string())?;
    let mut changed = false;
    let mut errors = Vec::new();
    for list in LISTS {
        let path = dir.join(list.file);
        let result = async {
            let bytes = client
                .get(list.url)
                .send()
                .await
                .and_then(|response| response.error_for_status())
                .map_err(|err| err.to_string())?
                .bytes()
                .await
                .map_err(|err| err.to_string())?;
            if bytes.len() > MAX_LIST_BYTES {
                return Err("список слишком большой".to_owned());
            }
            let text = std::str::from_utf8(&bytes).map_err(|_| "не UTF-8".to_owned())?;
            if !looks_like_filter_list(text) {
                return Err("ответ не похож на список фильтров".to_owned());
            }
            let same = std::fs::read(&path).is_ok_and(|old| old == bytes.as_ref());
            save_atomic(&path, &bytes)?;
            Ok(!same)
        }
        .await;
        match result {
            Ok(list_changed) => changed |= list_changed,
            Err(err) => errors.push(format!("{}: {err}", list.url)),
        }
    }
    if errors.is_empty() || changed {
        Ok(changed)
    } else {
        Err(format!(
            "Не удалось обновить фильтры рекламы: {}",
            errors.join("; ")
        ))
    }
}

/// For a shell's background thread: hands the cached lists to `publish` right
/// away, then refreshes a stale cache and publishes again when a list changed.
#[cfg(feature = "net")]
pub fn load_and_refresh(
    dir: &Path,
    user_agent: &str,
    mut publish: impl FnMut(Vec<String>),
) -> Result<(), String> {
    let cached = cached_lists(dir);
    if !cached.is_empty() {
        publish(cached);
    }
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|err| err.to_string())?;
    if runtime.block_on(refresh(dir, user_agent))? {
        publish(cached_lists(dir));
    }
    Ok(())
}

/// Request kinds as WebView2 reports them, mapped to adblock types.
fn request_type(kind: &str) -> &'static str {
    match kind {
        "document" => "document",
        "subdocument" | "sub_frame" => "sub_frame",
        "stylesheet" => "stylesheet",
        "image" => "image",
        "media" => "media",
        "font" => "font",
        "script" => "script",
        "xhr" | "xmlhttprequest" | "fetch" | "eventsource" => "xmlhttprequest",
        "websocket" => "websocket",
        "ping" | "beacon" | "cspviolationreport" => "ping",
        _ => "other",
    }
}

/// Never blocked: the site origins and everything under them.
fn is_site_request(url: &url::Url) -> bool {
    url.host_str().is_some_and(|host| {
        site_hosts::SITE_HOSTS
            .iter()
            .any(|site| host == *site || host.ends_with(&format!(".{site}")))
    })
}

/// Compiled filter engine. Building it from the full list takes about a second;
/// do it off the UI thread.
pub struct Blocker {
    engine: adblock::Engine,
}

impl Blocker {
    pub fn new(lists: &[String]) -> Self {
        let mut set = adblock::lists::FilterSet::new(false);
        for list in lists {
            set.add_filter_list(list.clone(), Default::default());
        }
        Self {
            engine: adblock::Engine::new_with_filter_set(set),
        }
    }

    /// Whether a request should be cancelled. `source` is the page that made it
    /// (the top page is a fine approximation), `kind` a WebView2-style resource
    /// kind (`script`, `image`, `xhr`, `subdocument`, `document`...).
    pub fn should_block(&self, url: &str, source: &str, kind: &str) -> bool {
        let kind = request_type(kind);
        if kind == "document" {
            return false;
        }
        let Ok(parsed) = url::Url::parse(url) else {
            return false;
        };
        if !matches!(parsed.scheme(), "http" | "https" | "ws" | "wss") || is_site_request(&parsed) {
            return false;
        }
        adblock::request::Request::new(url, source, kind, "GET")
            .map(|request| self.engine.check_network_request(&request).should_block())
            .unwrap_or(false)
    }

    /// Page-specific element hiding for `page_url` as a style sheet, or `None`.
    /// Generic hiding rules are not applied (they need DOM observation).
    pub fn hide_css(&self, page_url: &str) -> Option<String> {
        let resources = self.engine.url_cosmetic_resources(page_url);
        if resources.hide_selectors.is_empty() {
            return None;
        }
        let mut selectors: Vec<_> = resources.hide_selectors.into_iter().collect();
        selectors.sort();
        Some(
            selectors
                .iter()
                .map(|selector| format!("{selector}{{display:none!important}}"))
                .collect::<Vec<_>>()
                .join("\n"),
        )
    }
}

/// Stable cache key for compiled content filters: FNV-1a over the list texts.
pub fn lists_key(lists: &[String]) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in lists.iter().flat_map(|list| list.bytes().chain([0])) {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    format!("anivox-adblock-{hash:016x}")
}

/// Rules in the Safari content blocker format for WebKitGTK and WKWebView. The
/// site origins get an exception for sub-resources, as in [`Blocker::should_block`];
/// rules WebKit cannot express are dropped by the converter.
#[cfg(feature = "adblock-webkit")]
pub fn content_blocker_json(lists: &[String]) -> Result<String, String> {
    use adblock::content_blocking::{CbAction, CbResourceType, CbRule, CbTrigger, CbType};
    let mut set = adblock::lists::FilterSet::new(true);
    for list in lists {
        set.add_filter_list(list.clone(), Default::default());
    }
    let (mut rules, _) = set
        .into_content_blocking()
        .map_err(|_| "Не удалось преобразовать фильтры рекламы".to_owned())?;
    let hosts = site_hosts::SITE_HOSTS
        .iter()
        .map(|host| host.replace('.', "\\."))
        .collect::<Vec<_>>()
        .join("|");
    rules.push(CbRule {
        action: CbAction {
            typ: CbType::IgnorePreviousRules,
            selector: None,
        },
        trigger: CbTrigger {
            url_filter: format!("^[a-z]+://([^/:]+\\.)?({hosts})[/:]"),
            resource_type: Some(
                [
                    CbResourceType::Image,
                    CbResourceType::StyleSheet,
                    CbResourceType::Script,
                    CbResourceType::Font,
                    CbResourceType::Raw,
                    CbResourceType::SvgDocument,
                    CbResourceType::Media,
                    CbResourceType::Popup,
                ]
                .into_iter()
                .collect(),
            ),
            ..Default::default()
        },
    });
    serde_json::to_string(&rules).map_err(|err| err.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    const LIST: &str = "[Adblock Plus 2.0]\n! Title: test\n||ads.example^\n||mc.yandex.ru^\n||anivox.fun/ad/*\n/banner.\nanivox.fun##.promo-banner\n";

    fn blocker() -> Blocker {
        Blocker::new(&[LIST.to_owned()])
    }

    #[test]
    fn blocks_listed_third_party_requests() {
        let blocker = blocker();
        let page = "https://anivox.fun/anime/1";
        assert!(blocker.should_block("https://ads.example/x.js", page, "script"));
        assert!(blocker.should_block("https://mc.yandex.ru/metrika/tag.js", page, "script"));
        assert!(blocker.should_block("https://cdn.example/banner.png", page, "image"));
        assert!(blocker.should_block("https://ads.example/frame", page, "subdocument"));
        assert!(!blocker.should_block("https://anistorage.su/poster.webp", page, "image"));
    }

    #[test]
    fn never_blocks_the_site_or_documents() {
        let blocker = blocker();
        let page = "https://anivox.fun/";
        assert!(!blocker.should_block("https://anivox.fun/ad/x.js", page, "script"));
        assert!(!blocker.should_block("https://cdn.anivox.me/banner.png", page, "image"));
        assert!(!blocker.should_block("https://ads.example/", page, "document"));
        assert!(!blocker.should_block("not a url", page, "script"));
        assert!(!blocker.should_block("data:image/png;base64,AA", page, "image"));
    }

    #[test]
    fn hides_page_specific_elements() {
        let blocker = blocker();
        assert_eq!(
            blocker.hide_css("https://anivox.fun/").as_deref(),
            Some(".promo-banner{display:none!important}")
        );
        assert_eq!(blocker.hide_css("https://other.example/"), None);
    }

    #[test]
    fn checks_list_headers() {
        assert!(looks_like_filter_list(LIST));
        assert!(looks_like_filter_list("\u{feff}[Adblock Plus 2.0]\n"));
        assert!(!looks_like_filter_list(
            "<!doctype html><title>Portal</title>"
        ));
    }

    #[test]
    fn cache_reads_and_staleness() {
        let dir = tempfile::tempdir().unwrap();
        assert!(cached_lists(dir.path()).is_empty());
        assert!(is_stale(dir.path(), SystemTime::now()));
        save_atomic(&dir.path().join(LISTS[0].file), LIST.as_bytes()).unwrap();
        assert_eq!(cached_lists(dir.path()), vec![LIST.to_owned()]);
        assert!(!is_stale(dir.path(), SystemTime::now()));
        assert!(is_stale(dir.path(), SystemTime::now() + REFRESH_AFTER));
        save_atomic(&dir.path().join(LISTS[0].file), b"<html>").unwrap();
        assert!(cached_lists(dir.path()).is_empty());
    }

    #[test]
    fn lists_key_is_stable() {
        let a = lists_key(&[LIST.to_owned()]);
        assert_eq!(a, lists_key(&[LIST.to_owned()]));
        assert_ne!(a, lists_key(&[format!("{LIST}x")]));
        assert!(a.starts_with("anivox-adblock-"));
    }

    #[cfg(feature = "adblock-webkit")]
    #[test]
    fn content_blocker_rules_end_with_site_exception() {
        let json = content_blocker_json(&[LIST.to_owned()]).unwrap();
        let rules: Vec<serde_json::Value> = serde_json::from_str(&json).unwrap();
        assert!(rules.iter().any(|rule| rule["trigger"]["url-filter"]
            .as_str()
            .is_some_and(|f| f.contains("ads"))));
        let last = rules.last().unwrap();
        assert_eq!(last["action"]["type"], "ignore-previous-rules");
        assert!(last["trigger"]["url-filter"]
            .as_str()
            .unwrap()
            .contains("anivox\\.fun"));
    }

    /// Real list from the cache folder of a local install, if present:
    /// `cargo test -p anivox-core --features adblock -- --ignored real_list`.
    #[test]
    #[ignore]
    fn real_list_build_time() {
        let path = std::env::var("ANIVOX_ADBLOCK_LIST").expect("ANIVOX_ADBLOCK_LIST");
        let text = std::fs::read_to_string(path).unwrap();
        let started = std::time::Instant::now();
        let blocker = Blocker::new(&[text]);
        eprintln!("build: {:?}", started.elapsed());
        let page = "https://anivox.fun/anime/59217-grotesk";
        let started = std::time::Instant::now();
        for url in [
            "https://mc.yandex.ru/metrika/tag.js",
            "https://anistorage.su/posters/1.webp",
            "https://www.gstatic.com/cv/js/sender/v1/cast_sender.js",
            "https://an.yandex.ru/system/context.js",
            "https://googleads.g.doubleclick.net/pagead/id",
        ] {
            eprintln!("{url}: {}", blocker.should_block(url, page, "script"));
        }
        eprintln!("5 checks: {:?}", started.elapsed());
    }
}
