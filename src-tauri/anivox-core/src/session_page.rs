//! Which Anivox pages may be remembered and restored at startup.

/// Primary site home page: the startup page when nothing else is restored.
pub const HOME_URL: &str = "https://anivox.fun/";

/// Strict origin and conservative page allowlist. Never persist credentials,
/// fragments (OAuth implicit responses), authentication paths or unknown query
/// parameters. Rejected navigation leaves the previous trusted page untouched.
pub fn trusted_page(value: &str) -> Option<url::Url> {
    if value.len() > 8192 || value.chars().any(char::is_control) {
        return None;
    }
    let url = url::Url::parse(value).ok()?;
    if url.scheme() != "https"
        || !crate::site_hosts::is_site_origin(&url)
        // Mirrors are opened only by explicit choice, never restored at startup.
        || url.host_str() != Some(crate::site_hosts::SITE_HOSTS[0])
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
    {
        return None;
    }
    // Also reject empty userinfo, which URL parsing can otherwise normalize away.
    if value
        .split("://")
        .nth(1)?
        .split(['/', '?', '#'])
        .next()?
        .contains('@')
    {
        return None;
    }
    let path = decode_ascii_path(url.path())?;
    if path
        .split(|c: char| !c.is_ascii_alphanumeric())
        .any(|part| {
            matches!(
                part,
                "auth"
                    | "oauth"
                    | "oauth2"
                    | "authorize"
                    | "authorization"
                    | "callback"
                    | "signin"
                    | "signout"
                    | "login"
                    | "logout"
                    | "token"
                    | "tokens"
                    | "password"
                    | "reset"
                    | "verify"
                    | "session"
                    | "sso"
            )
        })
    {
        return None;
    }
    if url.query_pairs().any(|(key, _)| {
        !matches!(
            key.as_ref(),
            "page"
                | "search"
                | "q"
                | "sort"
                | "order"
                | "genre"
                | "year"
                | "season"
                | "status"
                | "type"
                | "episode"
                | "translation"
        )
    }) {
        return None;
    }
    Some(url)
}

fn decode_ascii_path(value: &str) -> Option<String> {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = std::str::from_utf8(bytes.get(i + 1..i + 3)?).ok()?;
            let byte = u8::from_str_radix(hex, 16).ok()?;
            // Reject double encoding rather than trying to interpret server rules.
            if byte == b'%' {
                return None;
            }
            out.push(byte);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    Some(String::from_utf8(out).ok()?.to_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_plain_primary_site_pages_are_trusted() {
        for value in [
            "https://anivox.me/anime/42",
            "https://anivox.fun/login",
            "https://anivox.fun/anime/1#access_token=x",
            "https://user@anivox.fun/",
            "https://@anivox.fun/",
            "https://anivox.fun/?code=secret",
            "https://anivox.fun/%2561uth",
            "http://anivox.fun/",
        ] {
            assert!(trusted_page(value).is_none(), "{value}");
        }
        for value in [HOME_URL, "https://anivox.fun/anime/42-slug?episode=3"] {
            assert!(trusted_page(value).is_some(), "{value}");
        }
    }
}
