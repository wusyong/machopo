//! Who asked for a request, as DevTools' `Network.Initiator` tells it
//! (Blink's `FetchInitiatorInfo`, which `InspectorNetworkAgent`'s
//! `BuildInitiatorObject` makes into one).
//!
//! The renderer starts most of a document's loads once script is done or
//! the parser stops, not as script asks for them as Blink does: what script
//! asked for, the engine captured the stack of then (the DOM keeps it,
//! `dom::StackAsker`), and the load takes it as it starts; what the
//! parser made, the line of its start tag (`Parser::take_load_lines`).

use std::rc::Rc;
use std::sync::Arc;

use serde_json::{Value, json};

/// Who asked for a request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Initiator {
    /// Script, running: its stack (`Runtime.StackTrace`, JSON).
    Script(Rc<str>),
    /// A module's `import`: the importing module's URL.
    Import(String),
    /// A stylesheet's `url()`: the stylesheet's URL (its document's for one
    /// in the document, a `<style>` or a `style` attribute).
    Css(Arc<str>),
    /// The document's parser, at the element's start tag: the document's
    /// URL and the line (0-based). machopo's tokenizer counts no columns.
    Parser { url: String, line: u32 },
}

impl Initiator {
    /// As DevTools' `Network.Initiator`: `other` for none.
    pub(crate) fn to_json(initiator: Option<&Self>) -> Value {
        match initiator {
            None => json!({ "type": "other" }),
            Some(Self::Script(stack)) => {
                let stack: Value = serde_json::from_str(stack).unwrap_or(Value::Null);
                json!({ "type": "script", "stack": stack })
            }
            Some(Self::Import(url)) => json!({ "type": "script", "url": url }),
            // Without a position, as Blink's (its `url()`s keep none).
            Some(Self::Css(url)) => json!({ "type": "parser", "url": &**url }),
            Some(Self::Parser { url, line }) => {
                json!({ "type": "parser", "url": url, "lineNumber": line })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Each as DevTools takes it: a stack parsed (one that isn't JSON is
    /// null), a stylesheet's without a line, none `other`.
    #[test]
    fn initiators_are_told_as_devtools_takes_them() {
        let stack = Initiator::Script(Rc::from(r#"{"callFrames":[]}"#));
        assert_eq!(
            Initiator::to_json(Some(&stack)),
            json!({ "type": "script", "stack": { "callFrames": [] } })
        );
        let broken = Initiator::Script(Rc::from("{"));
        assert_eq!(
            Initiator::to_json(Some(&broken)),
            json!({ "type": "script", "stack": null })
        );
        assert_eq!(
            Initiator::to_json(Some(&Initiator::Import("https://a.test/m.js".into()))),
            json!({ "type": "script", "url": "https://a.test/m.js" })
        );
        assert_eq!(
            Initiator::to_json(Some(&Initiator::Css(Arc::from("https://a.test/s.css")))),
            json!({ "type": "parser", "url": "https://a.test/s.css" })
        );
        assert_eq!(
            Initiator::to_json(Some(&Initiator::Parser {
                url: "https://a.test/".into(),
                line: 3,
            })),
            json!({ "type": "parser", "url": "https://a.test/", "lineNumber": 3 })
        );
        assert_eq!(Initiator::to_json(None), json!({ "type": "other" }));
    }
}
