//! Shikimori overlay rules for the native shells: the start page for the
//! current Anivox page, the page kind Discord may show and who may close it.

use crate::shikimori::site_title_id;
use url::Url;

pub const ORIGIN: &str = "https://shikimori.io";

/// Overlay start page for the page shown in the site view.
pub fn start_url(content: Option<&str>) -> String {
    content
        .and_then(|uri| Url::parse(uri).ok())
        .and_then(|url| site_title_id(&url))
        // `/animes/<id>` is 404 for some titles; the `z` form always redirects.
        .map(|id| format!("{ORIGIN}/animes/z{id}"))
        .unwrap_or_else(|| format!("{ORIGIN}/"))
}

/// What Discord shows while the overlay is open. Only the kind of page:
/// profile names, search queries and other hosts (sign-in) never leak.
pub fn page_activity(uri: &str) -> String {
    let browsing = "Листает Shikimori".to_owned();
    let Ok(url) = Url::parse(uri) else {
        return browsing;
    };
    if url.host_str() != Some("shikimori.io") {
        return browsing;
    }
    let mut segments = url.path_segments().into_iter().flatten();
    let first = segments.next().unwrap_or("");
    let rest: Vec<&str> = segments.collect();
    let ranked = rest.windows(2).any(|pair| pair == ["order-by", "ranked"])
        || url
            .query_pairs()
            .any(|(key, value)| key == "order" && value == "ranked");
    let text = match first {
        "animes"
            if rest
                .first()
                .map(|id| id.trim_start_matches('z'))
                .is_some_and(|id| id.starts_with(|c: char| c.is_ascii_digit())) =>
        {
            "Читает об аниме на Shikimori"
        }
        "animes" if ranked => "Смотрит топ аниме",
        "animes" => "Ищет аниме в каталоге Shikimori",
        "mangas" => "Листает мангу на Shikimori",
        "ranobe" => "Листает ранобэ на Shikimori",
        "forum" => "Читает форум Shikimori",
        "clubs" => "Смотрит клубы Shikimori",
        "characters" | "people" => "Смотрит персонажей на Shikimori",
        "ongoings" | "calendar" => "Смотрит календарь онгоингов",
        _ => return browsing,
    };
    text.to_owned()
}

/// Only Shikimori itself may close the overlay, never a sign-in provider.
pub fn is_shikimori(uri: &str) -> bool {
    Url::parse(uri).is_ok_and(|url| url.origin().ascii_serialization() == ORIGIN)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overlay_opens_the_same_title_or_the_home_page() {
        assert_eq!(
            start_url(Some("https://anivox.fun/anime/38524-shingeki")),
            "https://shikimori.io/animes/z38524"
        );
        assert_eq!(
            start_url(Some("https://anivox.me/anime/5114")),
            "https://shikimori.io/animes/z5114"
        );
        for other in [
            Some("https://anivox.fun/"),
            Some("https://evil.test/anime/1"),
            None,
        ] {
            assert_eq!(start_url(other), "https://shikimori.io/");
        }
    }

    #[test]
    fn discord_sees_only_the_kind_of_page() {
        let cases = [
            ("https://shikimori.io/", "Листает Shikimori"),
            (
                "https://shikimori.io/animes/z5114-fma",
                "Читает об аниме на Shikimori",
            ),
            (
                "https://shikimori.io/animes/order-by/ranked",
                "Смотрит топ аниме",
            ),
            (
                "https://shikimori.io/animes?search=secret",
                "Ищет аниме в каталоге Shikimori",
            ),
            ("https://shikimori.io/SomeUserName", "Листает Shikimori"),
            ("https://shikimori.io/forum/news", "Читает форум Shikimori"),
            ("https://accounts.google.com/o/oauth2", "Листает Shikimori"),
        ];
        for (uri, expected) in cases {
            let activity = page_activity(uri);
            assert_eq!(activity, expected, "{uri}");
            assert!(!activity.contains("secret") && !activity.contains("SomeUser"));
        }
    }

    #[test]
    fn only_the_shikimori_origin_may_close() {
        assert!(is_shikimori("https://shikimori.io/animes"));
        for other in [
            "http://shikimori.io/",
            "https://shikimori.io.evil.test/",
            "https://oauth.vk.com/authorize",
        ] {
            assert!(!is_shikimori(other), "{other}");
        }
    }
}
