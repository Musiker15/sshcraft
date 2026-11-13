//! Links from terminal output: only web addresses are opened.

pub const MAX_LINK: usize = 2048;

/// The link to open, or `None` when it is not an `http`/`https` address with a host.
pub fn validate_link(url: &str) -> Option<String> {
    if url.len() > MAX_LINK || url.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return None;
    }
    let parsed = url::Url::parse(url).ok()?;
    let web = matches!(parsed.scheme(), "http" | "https");
    let has_host = parsed.host_str().is_some_and(|h| !h.is_empty());
    (web && has_host).then(|| parsed.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn web_links_are_accepted() {
        assert_eq!(
            validate_link("https://example.com/a?b=c#d").as_deref(),
            Some("https://example.com/a?b=c#d")
        );
        assert_eq!(
            validate_link("http://10.0.0.1:8080/").as_deref(),
            Some("http://10.0.0.1:8080/")
        );
    }

    #[test]
    fn other_schemes_and_broken_links_are_refused() {
        let long = format!("https://example.com/{}", "a".repeat(MAX_LINK));
        for bad in [
            "file:///C:/Windows/system32/calc.exe",
            "javascript:alert(1)",
            "ftp://example.com/",
            "https://",
            "mailto:a@example.com",
            "https://exa mple.com",
            "ms-settings:privacy",
            "",
            long.as_str(),
        ] {
            assert_eq!(validate_link(bad), None, "{bad}");
        }
    }
}
