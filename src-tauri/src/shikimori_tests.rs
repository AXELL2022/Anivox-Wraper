use super::{shikimori_navigation_allowed, shikimori_new_window_response};
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
