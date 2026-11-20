//! Turning a navigation's body into a document, and reading back the parts
//! the browser needs.

/// A document's text as the HTML parser reads it, a piece at a time as
/// it comes: HTML as it is; XHTML through the HTML parser too (there is no
/// XML parser for documents yet, and the WPT tests we run are well-formed
/// documents it reads the same, except for CDATA sections, usually around
/// `<style>` contents, whose markers an XML parser drops: so they are
/// dropped first); any other type as text in a `<pre>` (Blink's
/// `TextDocumentParser`).
pub(crate) enum MarkupFilter {
    Html,
    /// The end of what came that may be the start of a CDATA marker, held
    /// for what comes next.
    Xhtml { held: String },
    /// Whether the `<pre>` has gone.
    Text { started: bool },
}

const CDATA_START: &str = "<![CDATA[";
const CDATA_END: &str = "]]>";

impl MarkupFilter {
    pub(crate) fn new(mime_type: &str) -> Self {
        match mime_type {
            "text/html" => Self::Html,
            "application/xhtml+xml" => Self::Xhtml {
                held: String::new(),
            },
            _ => Self::Text { started: false },
        }
    }

    /// The markup of the text's next piece, `text` (`last`: the end of
    /// it).
    pub(crate) fn push(&mut self, text: &str, last: bool) -> String {
        match self {
            Self::Html => text.to_owned(),
            Self::Xhtml { held } => {
                let mut markup = std::mem::take(held);
                markup.push_str(text);
                let mut markup = markup.replace(CDATA_START, "").replace(CDATA_END, "");
                if !last {
                    let keep = marker_start_at_end(&markup);
                    *held = markup.split_off(markup.len().saturating_sub(keep));
                }
                markup
            }
            Self::Text { started } => {
                let escaped = text
                    .replace('&', "&amp;")
                    .replace('<', "&lt;")
                    .replace('>', "&gt;");
                if std::mem::replace(started, true) {
                    escaped
                } else {
                    // The parser drops a newline right after `<pre>`, so
                    // add one.
                    format!("<pre>\n{escaped}")
                }
            }
        }
    }
}

/// How long the end of `markup` is that starts a CDATA marker (and
/// isn't one): 0 if none does. The markers are ASCII, so it ends at a
/// character's boundary.
fn marker_start_at_end(markup: &str) -> usize {
    [CDATA_START, CDATA_END]
        .iter()
        .flat_map(|marker| (1..marker.len()).filter(|&len| markup.ends_with(&marker[..len])))
        .max()
        .unwrap_or(0)
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
        let mut filter = MarkupFilter::new("text/plain");
        let markup = [filter.push("<title>t", false), filter.push("</title>\nx", true)].concat();
        let document = html::parser::parse_document(&mut dom, &markup, true);
        let dump = html::parser::dump_children(&mut dom, &document.root());
        assert!(
            dump.contains("<pre>\n|       \"<title>t</title>\nx\""),
            "{dump}"
        );
    }

    #[test]
    fn xhtml_drops_cdata_markers_cut_between_pieces() {
        let mut filter = MarkupFilter::new("application/xhtml+xml");
        let pieces = ["<style><![CD", "ATA[p{}]", "]></style>]", "]"];
        let markup: String = pieces
            .iter()
            .enumerate()
            .map(|(at, piece)| filter.push(piece, at + 1 == pieces.len()))
            .collect();
        // What only looked like a marker's start comes out at the end.
        assert_eq!(markup, "<style>p{}</style>]]");
    }
}
