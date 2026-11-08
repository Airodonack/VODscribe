//! Web links in what a drag offers as text: a browser's link, or text that is
//! only links. Read within caps (`MAX_TEXT`, `MAX_LINKS`): another app
//! chooses what a drag carries. Kept apart from the platforms' calls so that it's tested
//! everywhere.

/// The most text read for links, in characters (UTF-16 units on Windows): a
/// few dozen links take a few kilobytes, so a drag offering more is a
/// document, not links, and isn't read whole on every move over the window.
pub(crate) const MAX_TEXT: usize = 64 * 1024;

/// The most links taken from one drag. Each is a row in the app and a lookup
/// of its own, one after another: more than anyone drags at once, and few
/// enough that a stray drag (a long list selected by mistake) doesn't queue
/// minutes of them.
pub(crate) const MAX_LINKS: usize = 50;

/// `text` as a web link (http or https), trimmed, if that's all it is.
pub(crate) fn web_link(text: &str) -> Option<String> {
    let text = text.trim();
    let scheme = text.split_once("://")?.0;
    let web = scheme.eq_ignore_ascii_case("http") || scheme.eq_ignore_ascii_case("https");
    let rest = &text[scheme.len() + 3..];
    (web && !rest.is_empty() && !text.contains(char::is_whitespace)).then(|| text.to_string())
}

/// The web links of `text` when it's links only, one a line (blank lines
/// left out, as are the comments of a text/uri-list), at most `MAX_LINKS`;
/// else none: a sentence with a link in it isn't a link dragged, nor is text
/// longer than `MAX_TEXT`.
pub(crate) fn links_in(text: &str) -> Vec<String> {
    if text.chars().count() > MAX_TEXT {
        return vec![];
    }
    let lines = text.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('#'));
    let mut links = lines.map(web_link).collect::<Option<Vec<_>>>().unwrap_or_default();
    links.truncate(MAX_LINKS);
    links
}

/// The web links among `urls`, at most `MAX_LINKS`, none read past it.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(crate) fn web_links(urls: impl IntoIterator<Item = String>) -> Vec<String> {
    urls.into_iter().filter_map(|u| web_link(&u)).take(MAX_LINKS).collect()
}

/// `texts` (a pasteboard's strings) joined a line each, if they come to at
/// most `MAX_TEXT` characters; none read past it. A text given as None is
/// one its source measured as longer than that, and not copied out.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(crate) fn joined(texts: impl IntoIterator<Item = Option<String>>) -> Option<String> {
    let mut all = String::new();
    let mut count = 0;
    for text in texts {
        let text = text?;
        count += text.chars().count() + 1;
        if count > MAX_TEXT + 1 {
            return None;
        }
        if !all.is_empty() {
            all.push('\n');
        }
        all.push_str(&text);
    }
    Some(all)
}

/// The text of UTF-16 `units` up to its first NUL, if that's at most
/// `MAX_TEXT` units long; `units` may be longer than the text (a memory
/// block's size), and only `MAX_TEXT + 1` of them are looked at.
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn text_of_utf16(units: &[u16]) -> Option<String> {
    let units = &units[..units.len().min(MAX_TEXT + 1)];
    let end = units.iter().position(|&u| u == 0).unwrap_or(units.len());
    (end <= MAX_TEXT).then(|| String::from_utf16_lossy(&units[..end]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_web_link_is_one() {
        assert_eq!(web_link(" https://www.twitch.tv/videos/1 ").as_deref(), Some("https://www.twitch.tv/videos/1"));
        assert_eq!(web_link("HTTP://example.com").as_deref(), Some("HTTP://example.com"));
    }

    #[test]
    fn other_links_and_text_are_not() {
        for text in ["file:///tmp/a.mp4", "mailto:a@example.com", "ftp://example.com", "https://", "twitch.tv/x", ""] {
            assert_eq!(web_link(text), None, "{text}");
        }
        assert_eq!(web_link("see https://example.com"), None);
        assert_eq!(web_link("https://example.com and more"), None);
    }

    #[test]
    fn text_of_links_only_is_its_links() {
        let text = "# a comment\r\nhttps://a.example/1\r\n\r\nhttp://b.example/2\n";
        assert_eq!(links_in(text), ["https://a.example/1", "http://b.example/2"]);
    }

    #[test]
    fn text_over_the_cap_is_no_links() {
        let long = format!("https://a.example/{}", "x".repeat(MAX_TEXT));
        assert!(links_in(&long).is_empty());
        let mut many = "https://a.example/1\n".repeat(MAX_TEXT / 20);
        many.truncate(MAX_TEXT);
        assert!(!links_in(&many).is_empty(), "just under the cap: read");
    }

    #[test]
    fn links_past_the_cap_are_left() {
        let text: String = (0..MAX_LINKS + 10).map(|i| format!("https://a.example/{i}\n")).collect();
        let links = links_in(&text);
        assert_eq!(links.len(), MAX_LINKS);
        assert_eq!(links[0], "https://a.example/0");
        let urls = (0..MAX_LINKS + 10).map(|i| format!("https://a.example/{i}"));
        assert_eq!(web_links(urls).len(), MAX_LINKS);
    }

    #[test]
    fn joined_texts_over_the_cap_are_none() {
        let some = |texts: Vec<String>| texts.into_iter().map(Some);
        assert_eq!(joined(some(vec!["a".into(), "b".into()])).as_deref(), Some("a\nb"));
        assert_eq!(joined(some(vec!["x".repeat(MAX_TEXT)])).map(|t| t.len()), Some(MAX_TEXT));
        assert_eq!(joined(some(vec!["x".repeat(MAX_TEXT), "y".into()])), None);
        // An endless source is read only as far as the cap.
        assert_eq!(joined(std::iter::repeat(Some("x".repeat(1000)))), None);
        // One measured as too long by its source: none, and nothing after it read.
        assert_eq!(joined([Some("a".to_string()), None, Some("b".to_string())]), None);
    }

    #[test]
    fn utf16_text_ends_at_its_nul_and_its_cap() {
        let units: Vec<u16> = "https://a.example/1\0junk".encode_utf16().collect();
        assert_eq!(text_of_utf16(&units).as_deref(), Some("https://a.example/1"));
        // A large block with short text in it: read.
        let mut block = "ab".encode_utf16().collect::<Vec<_>>();
        block.resize(MAX_TEXT * 4, 0);
        assert_eq!(text_of_utf16(&block).as_deref(), Some("ab"));
        // Text past the cap, NUL or not: none.
        assert_eq!(text_of_utf16(&vec![b'x' as u16; MAX_TEXT * 4]), None);
        let mut over = vec![b'x' as u16; MAX_TEXT + 1];
        over.push(0);
        assert_eq!(text_of_utf16(&over), None);
        let mut at = vec![b'x' as u16; MAX_TEXT];
        at.push(0);
        assert_eq!(text_of_utf16(&at).map(|t| t.len()), Some(MAX_TEXT));
    }

    #[test]
    fn text_with_other_lines_is_no_links() {
        assert!(links_in("Watch this:\nhttps://a.example/1").is_empty());
        assert!(links_in("https://a.example/1\nfile:///tmp/a.mp4").is_empty());
        assert!(links_in("").is_empty());
    }
}
