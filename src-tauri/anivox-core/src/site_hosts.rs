//! Single list of Anivox hosts (main domain and mirrors) that get the injected
//! scripts, IPC capabilities and trusted-page treatment. Keep in sync with
//! `capabilities/default.json` and the origin guards in `scripts/*.js`
//! (`tests/site_hosts.test.mjs` fails when they diverge).

/// The first entry is the primary site and the default for "home".
pub const SITE_HOSTS: [&str; 2] = ["anivox.fun", "anivox.me"];

pub fn is_site_host(host: &str) -> bool {
    SITE_HOSTS.contains(&host)
}

/// `https://<site host>` on the default port, regardless of path.
pub fn is_site_origin(url: &url::Url) -> bool {
    url.scheme() == "https"
        && url.port_or_known_default() == Some(443)
        && url.host_str().is_some_and(is_site_host)
}

/// JS array literal of site origins, e.g. `["https://anivox.fun","https://anivox.me"]`.
pub fn js_origins() -> String {
    let origins: Vec<String> = SITE_HOSTS
        .iter()
        .map(|host| format!("https://{host}"))
        .collect();
    serde_json::to_string(&origins).expect("origins serialize")
}

/// Origin (`https://host`) for a page on a site host, else the primary site.
pub fn site_origin_for(url: Option<&url::Url>) -> String {
    let host = url
        .filter(|url| is_site_origin(url))
        .and_then(|url| url.host_str())
        .unwrap_or(SITE_HOSTS[0]);
    format!("https://{host}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn url(value: &str) -> url::Url {
        url::Url::parse(value).unwrap()
    }

    #[test]
    fn only_exact_https_site_hosts_match() {
        for ok in ["https://anivox.fun/", "https://anivox.me/anime/1-x"] {
            assert!(is_site_origin(&url(ok)), "{ok}");
        }
        for bad in [
            "http://anivox.me/",
            "https://anivox.me:444/",
            "https://anivox.me.evil.test/",
            "https://sub.anivox.me/",
            "https://anivox.com/",
        ] {
            assert!(!is_site_origin(&url(bad)), "{bad}");
        }
    }

    #[test]
    fn origin_follows_current_site_and_defaults_to_primary() {
        assert_eq!(
            site_origin_for(Some(&url("https://anivox.me/x"))),
            "https://anivox.me"
        );
        assert_eq!(
            site_origin_for(Some(&url("https://evil.test/"))),
            "https://anivox.fun"
        );
        assert_eq!(site_origin_for(None), "https://anivox.fun");
    }
}
