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

    /// 0.7.3: "Drop links from your browser. A VOD link dragged onto VODscribe from a browser or
    /// another app adds it". `web_link`: "`text` as a web link (http or https), trimmed".
    #[test]
    fn a_web_link_is_one_trimmed() {
        assert_eq!(web_link(" https://www.twitch.tv/videos/1 ").as_deref(), Some("https://www.twitch.tv/videos/1"));
        assert_eq!(web_link("http://example.com\r\n").as_deref(), Some("http://example.com"));
    }

    /// `web_link`: "(http or https)", whatever the case of the scheme.
    #[test]
    fn a_web_links_scheme_may_be_in_any_case() {
        assert_eq!(web_link("HTTP://example.com").as_deref(), Some("HTTP://example.com"));
        assert_eq!(web_link("hTtPs://example.com").as_deref(), Some("hTtPs://example.com"));
    }

    /// `web_link`: only http and https links are web links.
    #[test]
    fn a_link_of_another_scheme_is_not_a_web_link() {
        for text in ["file:///tmp/a.mp4", "mailto:a@example.com", "ftp://example.com", "javascript://x", "twitch.tv/x"]
        {
            assert_eq!(web_link(text), None, "{text}");
        }
    }

    /// `web_link`: a scheme with nothing after it, or no text, is not a link.
    #[test]
    fn a_scheme_with_nothing_after_it_is_not_a_link() {
        for text in ["https://", "http://", "", "  "] {
            assert_eq!(web_link(text), None, "{text:?}");
        }
    }

    /// `web_link`: "if that's all it is": text around a link, or a space inside it, is not a link.
    #[test]
    fn text_around_a_link_makes_it_no_link() {
        assert_eq!(web_link("see https://example.com"), None);
        assert_eq!(web_link("https://example.com and more"), None);
        assert_eq!(web_link("https://exa mple.com"), None);
    }

    /// `links_in`: "The web links of `text` when it's links only, one a line (blank lines left out,
    /// as are the comments of a text/uri-list)".
    #[test]
    fn text_of_links_only_is_its_links_without_blank_lines_and_comments() {
        let text = "# a comment\r\nhttps://a.example/1\r\n\r\nhttp://b.example/2\n";
        assert_eq!(links_in(text), ["https://a.example/1", "http://b.example/2"]);
    }

    /// `links_in`: "a sentence with a link in it isn't a link dragged": any line that isn't a link
    /// makes the whole text none.
    #[test]
    fn text_with_other_lines_is_no_links() {
        assert!(links_in("Watch this:\nhttps://a.example/1").is_empty());
        assert!(links_in("https://a.example/1\nfile:///tmp/a.mp4").is_empty());
        assert!(links_in("").is_empty());
        assert!(links_in("# only a comment").is_empty());
    }

    /// `MAX_TEXT`: "a drag offering more is a document, not links": text of more than that many
    /// characters is none, and text of exactly that many is read.
    #[test]
    fn text_over_the_cap_is_no_links_and_text_at_it_is_read() {
        // Lines of 20 characters, then a last link to make the cap exactly.
        let at = format!("{}https://a.exampl", "https://a.example/1\n".repeat(3276));
        assert_eq!(at.chars().count(), MAX_TEXT);
        assert_eq!(links_in(&at).len(), MAX_LINKS);
        assert!(links_in(&format!("{at}e")).is_empty());
    }

    /// `MAX_LINKS`: "The most links taken from one drag": the first of them are, and a drag of
    /// exactly that many keeps all.
    #[test]
    fn links_past_the_cap_are_left() {
        let lines = |n: usize| -> String { (0..n).map(|i| format!("https://a.example/{i}\n")).collect() };
        let links = links_in(&lines(MAX_LINKS + 10));
        assert_eq!(links.len(), MAX_LINKS);
        assert_eq!(links[0], "https://a.example/0");
        assert_eq!(links[MAX_LINKS - 1], format!("https://a.example/{}", MAX_LINKS - 1));
        assert_eq!(links_in(&lines(MAX_LINKS)).len(), MAX_LINKS);
    }

    /// `web_links`: "The web links among `urls`, at most `MAX_LINKS`, none read past it": the
    /// others are skipped, not stopping it.
    #[test]
    fn the_web_links_among_urls_skip_the_others_and_stop_at_the_cap() {
        let urls = ["file:///a.mp4", "https://a.example/1", "mailto:x@example.com", "http://b.example/2"];
        assert_eq!(web_links(urls.map(String::from)), ["https://a.example/1", "http://b.example/2"]);
        let endless = (0..).map(|i| format!("https://a.example/{i}"));
        assert_eq!(web_links(endless).len(), MAX_LINKS);
    }

    /// `joined`: "`texts` (a pasteboard's strings) joined a line each".
    #[test]
    fn texts_are_joined_a_line_each() {
        let some = |texts: &[&str]| texts.iter().map(|t| Some(t.to_string())).collect::<Vec<_>>();
        assert_eq!(joined(some(&["a", "b"])).as_deref(), Some("a\nb"));
        assert_eq!(joined(some(&[])).as_deref(), Some(""));
    }

    /// `joined`: "if they come to at most `MAX_TEXT` characters; none read past it".
    #[test]
    fn joined_texts_over_the_cap_are_none_and_an_endless_source_is_read_only_that_far() {
        let some = |texts: Vec<String>| texts.into_iter().map(Some);
        assert_eq!(joined(some(vec!["x".repeat(MAX_TEXT)])).map(|t| t.len()), Some(MAX_TEXT));
        assert_eq!(joined(some(vec!["x".repeat(MAX_TEXT), "y".into()])), None);
        assert_eq!(joined(std::iter::repeat(Some("x".repeat(1000)))), None);
    }

    /// `joined`: "A text given as None is one its source measured as longer than that, and not
    /// copied out": none, and nothing after it read.
    #[test]
    fn a_text_its_source_measured_too_long_makes_them_none() {
        let mut read = 0;
        let texts = [Some("a".to_string()), None, Some("b".to_string())].into_iter().inspect(|_| read += 1);
        assert_eq!(joined(texts), None);
        assert_eq!(read, 2);
    }

    /// `text_of_utf16`: "The text of UTF-16 `units` up to its first NUL ... `units` may be longer
    /// than the text (a memory block's size)".
    #[test]
    fn utf16_text_ends_at_its_nul_even_in_a_larger_block() {
        let units: Vec<u16> = "https://a.example/1\0junk".encode_utf16().collect();
        assert_eq!(text_of_utf16(&units).as_deref(), Some("https://a.example/1"));
        let mut block = "ab".encode_utf16().collect::<Vec<_>>();
        block.resize(MAX_TEXT * 4, 0);
        assert_eq!(text_of_utf16(&block).as_deref(), Some("ab"));
    }

    /// `text_of_utf16`: "if that's at most `MAX_TEXT` units long ... only `MAX_TEXT + 1` of them are
    /// looked at": text past the cap, NUL or not, is none; text of exactly the cap is read.
    #[test]
    fn utf16_text_past_the_cap_is_none() {
        assert_eq!(text_of_utf16(&vec![b'x' as u16; MAX_TEXT * 4]), None);
        let mut over = vec![b'x' as u16; MAX_TEXT + 1];
        over.push(0);
        assert_eq!(text_of_utf16(&over), None);
        let mut at = vec![b'x' as u16; MAX_TEXT];
        at.push(0);
        assert_eq!(text_of_utf16(&at).map(|t| t.len()), Some(MAX_TEXT));
    }

    /// `text_of_utf16`: text with no NUL, within the cap, is all of it.
    #[test]
    fn utf16_text_with_no_nul_is_all_of_it() {
        let units: Vec<u16> = "https://a.example/1".encode_utf16().collect();
        assert_eq!(text_of_utf16(&units).as_deref(), Some("https://a.example/1"));
        assert_eq!(text_of_utf16(&[]).as_deref(), Some(""));
    }
}
