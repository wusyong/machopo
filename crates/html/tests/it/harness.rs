//! What the tests share: a DOM (`dom::Dom`) to parse into, a
//! fragment's context element, the html5lib dump of what was parsed, and a
//! document's parser that pauses at scripts with the engine it is lent
//! (`TestHost`).

use std::cell::RefCell;
use std::rc::Rc;

use dom::{Dom, NodeRoot, QualName, QuirksMode};
use html::parser::{Parser, ParserState, SinkHandle};

/// A DOM for the tests, shared by their closures.
pub fn dom() -> Rc<RefCell<Dom>> {
    Rc::new(RefCell::new(Dom::new()))
}

/// An element `name` in a new HTML document in `quirks` mode, and the
/// document: a fragment's context.
pub fn context_named(
    dom: &mut Dom,
    name: QualName<'_>,
    quirks: QuirksMode,
) -> (NodeRoot, NodeRoot) {
    dom.mutate(|cx| {
        let document = cx.create_document(true);
        cx.set_quirks_mode(document, quirks);
        let element = cx.create_element(document, name);
        (cx.root(element), cx.root(document))
    })
}

/// A `<div>` context in a new document in `quirks` mode, and the document.
pub fn context(dom: &Rc<RefCell<Dom>>, quirks: QuirksMode) -> (NodeRoot, NodeRoot) {
    context_named(&mut dom.borrow_mut(), QualName::html("div"), quirks)
}

/// `node`'s children in the html5lib tree-construction format.
pub fn dump(dom: &mut Dom, node: &SinkHandle) -> String {
    html::parser::dump_children(dom, &node.root())
}

/// `input` parsed as the contents of a new document's HTML element
/// `context`, in no-quirks mode, dumped.
pub fn dump_fragment(context: &str, input: &str, scripting: bool) -> String {
    let mut dom = Dom::new();
    let (context, _) = context_named(&mut dom, QualName::html(context), QuirksMode::NoQuirks);
    let fragment = html::parser::parse_fragment(&mut dom, &context, input, scripting);
    dump(&mut dom, &fragment)
}

/// A document parser's engine for the tests: its DOM, and the custom
/// elements it is asked to construct, each made as a plain element with
/// its attributes (and its name logged).
pub struct TestHost {
    pub dom: Dom,
    pub constructed: Vec<String>,
}

impl TestHost {
    pub fn new() -> Box<Self> {
        Box::new(Self {
            dom: Dom::new(),
            constructed: Vec::new(),
        })
    }
}

impl html::parser::ParserHost for TestHost {
    fn parser_dom(&mut self) -> &mut Dom {
        &mut self.dom
    }

    fn create_custom_element(
        &mut self,
        document: &NodeRoot,
        local: &str,
        attributes: &[(QualName<'_>, &str)],
    ) -> Option<NodeRoot> {
        let names: Vec<String> = attributes
            .iter()
            .map(|(name, value)| format!("{}={value}", name.local))
            .collect();
        self.constructed
            .push(format!("{local} {}", names.join(" ")));
        Some(self.dom.mutate(|cx| {
            let element = cx.create_element(document.get(cx.gc), QualName::html(local));
            for (name, value) in attributes {
                cx.set_attribute_ns(element, *name, value);
            }
            cx.root(element)
        }))
    }

    fn run_custom_element_reactions(&mut self) {
        self.constructed.push("reactions".to_owned());
    }
}

/// A document's parser (scripting enabled) into a new document of its
/// engine's DOM, with the engine lent to it for each run and taken back
/// after, so the test uses the DOM while the parser waits.
pub struct DocumentParser {
    // Fields drop in order: the roots before the DOM they are of.
    pub document: NodeRoot,
    parser: Option<Parser<TestHost>>,
    host: Option<Box<TestHost>>,
}

impl DocumentParser {
    pub fn new() -> Self {
        let mut host = TestHost::new();
        let document = host.dom.mutate(|cx| cx.root(cx.create_document(true)));
        Self {
            parser: Some(Parser::new(&host.dom, &document)),
            document,
            host: Some(host),
        }
    }

    pub fn push(&mut self, text: &str) {
        self.parser.as_mut().expect("not finished").push(text);
    }

    pub fn resume(&mut self) -> ParserState {
        let parser = self.parser.as_mut().expect("not finished");
        let (state, host) = parser.resume(self.host.take().expect("the engine"));
        self.host = Some(host);
        state
    }

    pub fn finish(&mut self) {
        let parser = self.parser.take().expect("not finished");
        self.host = Some(parser.finish(self.host.take().expect("the engine")));
    }

    /// The DOM, the parser waiting.
    pub fn dom(&mut self) -> &mut Dom {
        &mut self.host.as_mut().expect("the engine").dom
    }

    /// The document so far, dumped.
    pub fn dump(&mut self) -> String {
        let document = self.document.clone();
        html::parser::dump_children(self.dom(), &document)
    }

    /// A script element's text.
    pub fn script_text(&mut self, script: &SinkHandle) -> String {
        self.dom().read(|cx| {
            cx.text_content(script.root().get(cx.gc))
                .map(|text| text.to_string())
                .unwrap_or_default()
        })
    }

    /// The scripts it stops at until it needs input, by their text.
    pub fn scripts(&mut self) -> Vec<String> {
        let mut scripts = Vec::new();
        while let ParserState::Script { element, .. } = self.resume() {
            scripts.push(self.script_text(&element));
        }
        scripts
    }
}
