//! The user agent string (Chromium's `embedder_support::GetUserAgent`):
//! Chrome's reduced UA, exactly, so that what tells
//! browsers apart by it takes Chrome's path. Its version is the milestone
//! of the V8 machopo links (`RUSTY_V8_MIRROR_TAG`, `machopo-v152.2.0-29`: Chrome
//! 152); the platform is the one machopo was built for, frozen as Chrome's is.
//! The one source: HTTP requests send it, and renderers answer
//! `navigator.userAgent` with it.

/// The tag of the V8 release machopo links, from `.cargo/config.toml`.
const V8_TAG: &str = env!("RUSTY_V8_MIRROR_TAG");

/// The user agent, as Chrome's on this platform.
pub fn user_agent() -> String {
    format!(
        "Mozilla/5.0 ({}) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/{}.0.0.0 Safari/537.36",
        platform(),
        chrome_major(V8_TAG)
    )
}

/// The reduced UA's platform (Chromium's `GetUnifiedPlatform`): fixed
/// values, not the machine's OS version.
fn platform() -> &'static str {
    if cfg!(target_os = "macos") {
        "Macintosh; Intel Mac OS X 10_15_7"
    } else if cfg!(target_os = "windows") {
        "Windows NT 10.0; Win64; x64"
    } else {
        "X11; Linux x86_64"
    }
}

/// The Chrome milestone of a V8 release tag: V8 `M.m` ships in Chrome
/// `M m` (V8 15.2 in Chrome 152), and the fork's tags are `machopo-vMmm.…`
/// (`kun-vMmm.…` until the project was renamed).
fn chrome_major(tag: &str) -> &str {
    let version = tag.split_once("-v").map_or(tag, |(_, version)| version);
    version.split('.').next().unwrap_or(version)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_user_agent_is_chrome_s_at_v8_s_milestone() {
        assert_eq!(chrome_major("machopo-v152.2.0-29"), "152");
        assert_eq!(chrome_major("kun-v152.2.0-28"), "152");
        let agent = user_agent();
        assert!(agent.starts_with("Mozilla/5.0 ("), "{agent}");
        assert!(
            agent.ends_with(&format!(
                ") AppleWebKit/537.36 (KHTML, like Gecko) Chrome/{}.0.0.0 Safari/537.36",
                chrome_major(V8_TAG)
            )),
            "{agent}"
        );
        if cfg!(target_os = "macos") {
            assert!(agent.contains("(Macintosh; Intel Mac OS X 10_15_7)"));
        }
    }
}
