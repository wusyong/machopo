//! Turning a navigation's body into a document, and reading back the parts
//! the browser needs.

/// The HTML to parse for a response's decoded text (see `encoding`).
/// Other types are shown as plain text, the way Blink's `TextDocument`
/// puts them in a `<pre>`.
pub(crate) fn markup_for(mime_type: &str, text: &str) -> String {
    if mime_type == "text/html" {
        return text.to_owned();
    }
    // XHTML goes through the HTML parser too: there is no XML parser yet,
    // and the WPT tests we run are well-formed documents it reads the same,
    // except for CDATA sections (usually around `<style>` contents), whose
    // markers an XML parser drops. So drop them first.
    if mime_type == "application/xhtml+xml" {
        return text.replace("<![CDATA[", "").replace("]]>", "");
    }
    let escaped = text
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;");
    // The parser drops a newline right after `<pre>`, so add one.
    format!("<pre>\n{escaped}")
}

/// Whether the document element's `class` has `class` (web tests'
/// `reftest-wait`).
pub(crate) fn root_has_class<'gc>(
    cx: &dom::Cx<'_, 'gc>,
    document: dom::NodeRef<'gc>,
    class: &str,
) -> bool {
    std::iter::successors(cx.first_child(document), |&child| cx.next_sibling(child))
        .find(|&child| cx.kind(child) == dom::NodeKind::Element)
        .and_then(|root| cx.get_attribute(root, "class"))
        .is_some_and(|classes| {
            classes
                .to_str()
                .split_ascii_whitespace()
                .any(|c| c == class)
        })
}

/// `%XX` sequences decoded, then read as UTF-8 (lossily).
pub(crate) fn percent_decode(text: &str) -> String {
    let mut rest = text.as_bytes();
    let mut out = Vec::with_capacity(rest.len());
    while let Some((&first, after)) = rest.split_first() {
        let decoded = (first == b'%')
            .then(|| after.split_first_chunk::<2>())
            .flatten()
            .and_then(|(hex, after)| {
                let hex = std::str::from_utf8(hex).ok()?;
                Some((u8::from_str_radix(hex, 16).ok()?, after))
            });
        match decoded {
            Some((byte, after)) => {
                out.push(byte);
                rest = after;
            }
            None => {
                out.push(first);
                rest = after;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percent_decoding() {
        assert_eq!(percent_decode("%C3%A9%20x"), "é x");
        assert_eq!(percent_decode("%74op"), "top");
        // What isn't `%` and two hex digits stays as it is.
        assert_eq!(percent_decode("100%"), "100%");
        assert_eq!(percent_decode("%zz%4"), "%zz%4");
    }

    #[test]
    fn plain_text_is_not_parsed_as_markup() {
        let mut dom = dom::Dom::new();
        let markup = markup_for("text/plain", "<title>t</title>\nx");
        let document = html::parser::parse_document(&mut dom, &markup, true);
        let dump = html::parser::dump_children(&mut dom, &document.root());
        assert!(
            dump.contains("<pre>\n|       \"<title>t</title>\nx\""),
            "{dump}"
        );
    }
}
