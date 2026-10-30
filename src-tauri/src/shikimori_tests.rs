use super::{
    add_episode_total, shikimori_info_episodes, shikimori_info_label, shikimori_navigation_allowed,
    shikimori_new_window_response, shikimori_overlay_url, strip_episode,
};
use tauri::webview::NewWindowResponse;

#[test]
fn social_login_can_leave_shikimori_and_return_to_its_callback() {
    for address in [
        "https://shikimori.io/users/auth/vkontakte",
        "https://oauth.vk.com/authorize",
        "https://passport.yandex.ru/auth",
        "https://accounts.google.com/o/oauth2/v2/auth",
        "https://shikimori.io/users/auth/yandex/callback",
    ] {
        let url = tauri::Url::parse(address).unwrap();
        assert!(shikimori_navigation_allowed(&url), "Blocked {address}");
        assert!(
            matches!(
                shikimori_new_window_response(&url),
                NewWindowResponse::Allow
            ),
            "Blocked popup {address}"
        );
    }
}

#[test]
fn social_login_cannot_launch_local_files_or_insecure_navigation() {
    for address in [
        "http://shikimori.io/",
        "file:///C:/Windows/win.ini",
        "javascript:alert(1)",
        "data:text/html,hello",
        "tauri://localhost/index.html",
    ] {
        let url = tauri::Url::parse(address).unwrap();
        assert!(!shikimori_navigation_allowed(&url), "Allowed {address}");
        assert!(
            matches!(shikimori_new_window_response(&url), NewWindowResponse::Deny),
            "Allowed popup {address}"
        );
    }
}

#[test]
fn overlay_opens_the_same_title_as_the_anivox_page() {
    for (content, expected) in [
        (
            "https://anivox.fun/anime/38524-ataka-titanov-3-chast-2",
            "https://shikimori.io/animes/38524",
        ),
        (
            "https://anivox.fun/anime/1",
            "https://shikimori.io/animes/1",
        ),
        ("https://anivox.fun/", "https://shikimori.io/"),
        ("https://anivox.fun/anime/abc", "https://shikimori.io/"),
        ("https://anivox.fun/anime/12x-slug", "https://shikimori.io/"),
        ("https://anivox.fun/anime/0-slug", "https://shikimori.io/"),
        (
            "https://evil.example/anime/38524-x",
            "https://shikimori.io/",
        ),
    ] {
        let url = tauri::Url::parse(content).unwrap();
        assert_eq!(shikimori_overlay_url(&url).as_str(), expected, "{content}");
    }
}

#[test]
fn info_label_shows_score_kind_and_year() {
    let info = serde_json::json!({"score": "9.05", "kind": "tv", "aired_on": "2019-04-29"});
    assert_eq!(
        shikimori_info_label(&info).as_deref(),
        Some("★ 9.05 · TV · 2019")
    );
    let unrated = serde_json::json!({"score": "0.0", "kind": "movie", "aired_on": null});
    assert_eq!(shikimori_info_label(&unrated).as_deref(), Some("Фильм"));
    assert_eq!(shikimori_info_label(&serde_json::json!({})), None);
}

#[tokio::test]
#[ignore = "requires network access to shikimori.io"]
async fn live_api_returns_info_label() {
    let data = super::fetch_shikimori_data(38524).await.unwrap();
    assert_eq!(data.label.as_deref(), Some("★ 9.05 · TV · 2019"));
    assert_eq!(data.episodes, Some(10));
}

#[test]
fn episode_total_is_added_to_the_episode_number() {
    assert_eq!(
        add_episode_total("Серия 8 · MPV · Смотрит", 10),
        "Серия 8/10 · MPV · Смотрит"
    );
    assert_eq!(add_episode_total("Серия 8", 10), "Серия 8/10");
    assert_eq!(
        add_episode_total("Серия 11 · На паузе", 10),
        "Серия 11 · На паузе"
    );
    assert_eq!(add_episode_total("Серия 8/10", 10), "Серия 8/10");
    assert_eq!(add_episode_total("Выбирает серию", 10), "Выбирает серию");
    assert_eq!(
        shikimori_info_episodes(&serde_json::json!({"episodes": 10})),
        Some(10)
    );
    assert_eq!(
        shikimori_info_episodes(&serde_json::json!({"episodes": 0})),
        None
    );
}

#[test]
fn films_drop_the_episode_number() {
    assert_eq!(strip_episode("Серия 1 · MPV · Смотрит"), "MPV · Смотрит");
    assert_eq!(strip_episode("Серия 1"), "");
    assert_eq!(strip_episode("На паузе"), "На паузе");
}
