//! DevTools' `CSS` domain as the renderer answers it (Blink's
//! `InspectorCSSAgent`), for the Elements panel's Computed and Styles: an
//! element's computed style, what `getComputedStyle` reads; the rules it
//! matches, its presentational hints and its `style` attribute's
//! declarations, as the style engine cascades them; the fonts its text
//! was laid out in; and, while the Computed pane follows a node, word when
//! its style changes.
//!
//! The sheets are told as they come and go, with their texts; a rule an
//! element matches comes with its sheet, where it and its declarations are
//! in the sheet's text (the text parsed again, as Blink's
//! `InspectorStyleSheet` does), and the `@media` and `@supports` it is in.
//! An element's `style` attribute is a sheet of its own, not told.

use std::cell::{Cell, RefCell};
use std::ops::Range;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::{Arc, Weak};

use computed_style::ComputedStyle;
use crossbeam_channel::Sender;
use css::inspector::{SourceDeclaration, SourceRule};
use css::{CssPropertyValueSet, CssRule, StyleSheetContents as CssStyleSheetContents};
use dom::{Cx, Dom, NodeId, NodeKind, NodeRef};
use dom_api::{DevToolsPage, FrameLayout};
use protocol::renderer::RendererHostMsg;
use serde_json::{Value, json};

use super::dom_agent::{DomAgent, with_layout};
use super::page_agent::PageAgent;

/// The commands the agent answers, without `CSS.`.
const COMMANDS: &[&str] = &[
    "enable",
    "disable",
    "getComputedStyleForNode",
    "getInlineStylesForNode",
    "getMatchedStylesForNode",
    "getPlatformFontsForNode",
    "getStyleSheetText",
    "getAnimatedStylesForNode",
    "trackComputedStyleUpdatesForNode",
];

/// A session's `CSS`, while it has it on.
#[derive(Default)]
struct Session {
    /// The node the Computed pane follows (`trackComputedStyleUpdatesForNode`),
    /// by the session's id of it and the backend's, with its style as last
    /// seen.
    tracked: Option<Tracked>,
    /// The sheets it was told of (`styleSheetAdded`), not gone since: their
    /// ids and their contents' addresses as last told (others now: script
    /// edited them, `styleSheetChanged`).
    sheets: Vec<(u64, usize)>,
}

struct Tracked {
    id: i64,
    node: NodeId,
    style: Option<Arc<ComputedStyle>>,
}

/// Blink's `InspectorCSSAgent`, for every session of the page.
pub(crate) struct CssAgent {
    to_browser: Sender<RendererHostMsg>,
    dom_agent: Rc<DomAgent>,
    /// What has the documents' markup, which an inline sheet's position is
    /// found in.
    page_agent: Rc<PageAgent>,
    /// The renderer's, in the sheets' ids (`style-sheet-<prefix>-<id>`, as
    /// Chrome's are of its process).
    prefix: u64,
    /// The sessions that turned `CSS` on.
    sessions: RefCell<HashMap<String, Session>>,
    /// The documents' styles' counts of their sheets' collections when the
    /// sheets were last listed (`style::StyleEngine::sheets_version`): they
    /// are listed again once one moves.
    sheets_versions: RefCell<Vec<(NodeId, u64)>>,
    /// `<link>` sheets' texts as they came, by their contents: kept once a
    /// DevTools session is attached (`keep_link_text`), or fetched again
    /// for one loaded before (`refetched`). `None`: what came again isn't
    /// what the rules are now (CSSOM writes them, as for a sheet with no
    /// text).
    link_texts: RefCell<Vec<(Weak<CssStyleSheetContents>, Option<Rc<str>>)>>,
    /// The `<link>` sheets to fetch again for their texts, and the commands
    /// that wait for them.
    refetches: RefCell<Vec<Refetch>>,
    /// Sheets' texts parsed for where their rules are, as DevTools asked:
    /// kept while their contents live and their texts stay.
    sources: RefCell<Vec<Rc<SheetSource>>>,
    /// The elements whose `style` attributes DevTools has as sheets (Blink's
    /// `InspectorStyleSheetForInlineStyle`), and the last number given one.
    inline_styles: RefCell<Vec<InlineStyle>>,
    inline_count: Cell<u64>,
}

/// An element's `style` attribute as a sheet of its own: its id, and its
/// text as DevTools last had it (`styleSheetChanged` once it changes).
struct InlineStyle {
    node: NodeId,
    id: u64,
    text: String,
}

/// A sheet's text and where its rules are in it (Blink's
/// `CSSRuleSourceData`): its rules parsed again, which the parser keeps in
/// the order its contents' are, rule for rule (its contents are what this
/// text parsed to: script didn't edit them).
struct SheetSource {
    contents: Weak<CssStyleSheetContents>,
    text: String,
    rules: Vec<SourceRule>,
    lines: Lines,
}

/// Where each line of a text starts, for DevTools' positions (lines from 0,
/// columns in UTF-16 code units).
struct Lines {
    starts: Vec<usize>,
}

impl Lines {
    fn new(text: &str) -> Self {
        let mut starts = vec![0];
        // After each line break: within the text, nothing to overflow.
        starts.extend(text.match_indices('\n').map(|(at, _)| at.saturating_add(1)));
        Self { starts }
    }

    /// `range` of `text` as a `CSS.SourceRange`.
    fn range(&self, text: &str, range: &Range<usize>) -> Value {
        let (start_line, start_column) = self.position(text, range.start);
        let (end_line, end_column) = self.position(text, range.end);
        json!({
            "startLine": start_line,
            "startColumn": start_column,
            "endLine": end_line,
            "endColumn": end_column,
        })
    }

    fn position(&self, text: &str, offset: usize) -> (u32, u32) {
        let line = self.starts.partition_point(|&start| start <= offset).saturating_sub(1);
        let start = self.starts.get(line).copied().unwrap_or_default();
        let column = text
            .get(start..offset)
            .map_or(0, |before| before.encode_utf16().count());
        (
            u32::try_from(line).unwrap_or(u32::MAX),
            u32::try_from(column).unwrap_or(u32::MAX),
        )
    }
}

/// A `<link>` sheet loaded before DevTools came, fetched again for its text
/// as the first command asks for it (`getStyleSheetText`).
struct Refetch {
    contents: Weak<CssStyleSheetContents>,
    /// Its `href`, its `<link>`'s CORS credentials (none: no CORS), and its
    /// document.
    url: String,
    cors: Option<bool>,
    document: NodeId,
    /// Asked of the network already.
    started: bool,
    /// The commands waiting: their sessions and ids.
    waiting: Vec<(String, Value)>,
}

impl CssAgent {
    pub(crate) fn new(
        to_browser: Sender<RendererHostMsg>,
        dom_agent: Rc<DomAgent>,
        page_agent: Rc<PageAgent>,
        prefix: u64,
    ) -> Self {
        Self {
            to_browser,
            dom_agent,
            page_agent,
            prefix,
            sessions: RefCell::new(HashMap::new()),
            sheets_versions: RefCell::new(Vec::new()),
            link_texts: RefCell::new(Vec::new()),
            refetches: RefCell::new(Vec::new()),
            sources: RefCell::new(Vec::new()),
            inline_styles: RefCell::new(Vec::new()),
            inline_count: Cell::new(0),
        }
    }

    /// A `<link>`'s sheet came with `text`, parsed into `contents`, while
    /// a DevTools session is attached: kept as its text.
    pub(crate) fn keep_link_text(&self, contents: &Arc<CssStyleSheetContents>, text: &str) {
        let mut texts = self.link_texts.borrow_mut();
        texts.retain(|(kept, _)| kept.strong_count() > 0);
        texts.push((Arc::downgrade(contents), Some(Rc::from(text))));
    }

    /// The text kept for the `<link>` sheet of `contents`: `Some(None)` if
    /// what came again isn't what its rules are.
    fn link_text(&self, contents: &Arc<CssStyleSheetContents>) -> Option<Option<Rc<str>>> {
        self.link_texts
            .borrow()
            .iter()
            .find(|(kept, _)| std::ptr::eq(kept.as_ptr(), Arc::as_ptr(contents)))
            .map(|(_, text)| text.clone())
    }

    /// The `<link>` sheets to fetch again now, each once: by a key of their
    /// own (their contents' address), their `href`, CORS credentials
    /// (`true`: `use-credentials`) and document.
    pub(crate) fn take_refetches(&self) -> Vec<(usize, String, Option<bool>, NodeId)> {
        let mut refetches = self.refetches.borrow_mut();
        refetches
            .iter_mut()
            .filter(|refetch| !refetch.started)
            .map(|refetch| {
                refetch.started = true;
                let key = refetch.contents.as_ptr().addr();
                (key, refetch.url.clone(), refetch.cors, refetch.document)
            })
            .collect()
    }

    /// The `<link>` sheet fetched again (`take_refetches`' `key`) came with
    /// `text` (none: it failed), parsed against `base`: kept as its text if
    /// it has the rules the sheet has now; the commands waiting answered.
    pub(crate) fn refetched(&self, key: usize, text: Option<String>, base: Option<css::UrlContext>) {
        let refetch = {
            let mut refetches = self.refetches.borrow_mut();
            let Some(at) = refetches
                .iter()
                .position(|refetch| refetch.contents.as_ptr().addr() == key)
            else {
                return;
            };
            refetches.remove(at)
        };
        let Some(contents) = refetch.contents.upgrade() else {
            return;
        };
        let kept = text_if_same(text, base, &contents);
        {
            let mut texts = self.link_texts.borrow_mut();
            texts.retain(|(known, _)| known.strong_count() > 0);
            texts.push((Arc::downgrade(&contents), kept.clone()));
        }
        let text = kept.map_or_else(|| serialized(&contents), |text| text.to_string());
        for (session, id) in refetch.waiting {
            self.send(&session, &json!({ "id": id, "result": { "text": text } }));
        }
    }

    /// A sheet's id as DevTools has it.
    fn sheet_id(&self, sheet: u64) -> String {
        format!("style-sheet-{}-{sheet}", self.prefix)
    }

    /// Every sheet of the frames' documents now, with its header and text.
    fn list_sheets(&self, dom: &Dom) -> Vec<SheetInfo> {
        let frames = self.dom_agent.frames();
        dom.read(|cx| {
            let mut sheets = Vec::new();
            for frame in &frames {
                let Some(document) = cx.node_by_id(frame.document) else {
                    continue;
                };
                let markup = self.page_agent.source_of(&frame.id);
                let markup = markup.as_deref();
                let frame = frame.id.as_str();
                for scope in scopes(cx, document) {
                    let all = cx
                        .owner_sheets_in(scope)
                        .into_iter()
                        .chain(cx.adopted_style_sheets(scope));
                    // Each sheet's header (`CSS.CSSStyleSheetHeader`, as
                    // Blink's `InspectorStyleSheet::BuildObjectForStyleSheetInfo`
                    // has it) and text.
                    for sheet in all {
                        let id = cx.style_sheet_id(sheet);
                        let constructed = cx.style_sheet_is_constructed(sheet);
                        let owner = cx.style_sheet_owner(sheet);
                        let href = cx.style_sheet_href(sheet);
                        let inline = owner.is_some() && href.is_none();
                        let contents = cx.style_sheet_contents(sheet);
                        // Its source while script hasn't edited its rules;
                        // after, as CSSOM writes them.
                        let edited = cx.style_sheet_edited(sheet);
                        let text = match owner {
                            Some(owner) if inline && !edited => {
                                cx.child_text_content(owner).to_string()
                            }
                            // A `<link>`'s as it came, if DevTools has it
                            // (none of an edited one's: an edit's contents
                            // are others); else, and a constructed one's
                            // (Chrome's too), as CSSOM writes its rules.
                            _ => match self.link_text(contents) {
                                Some(Some(text)) => text.to_string(),
                                _ => serialized(contents),
                            },
                        };
                        // A `<link>`'s whose text DevTools hasn't: what to
                        // fetch it again with.
                        let refetch = owner
                            .zip(href)
                            .filter(|_| !edited && self.link_text(contents).is_none())
                            .map(|(link, href)| Refetch {
                                contents: Arc::downgrade(contents),
                                url: href.to_owned(),
                                cors: cx.get_attribute(link, "crossorigin").map(|value| {
                                    value.to_str().eq_ignore_ascii_case("use-credentials")
                                }),
                                document: cx.node_id(document),
                                started: false,
                                waiting: Vec::new(),
                            });
                        let source_url = if constructed {
                            String::new()
                        } else {
                            match href {
                                Some(href) => href.to_owned(),
                                None => cx
                                    .document_url(document)
                                    .map(|url| url.to_string())
                                    .unwrap_or_default(),
                            }
                        };
                        // An inline sheet's text where its `<style>`'s is in
                        // the markup; another's at the start.
                        let (start_line, start_column) = owner
                            .filter(|_| inline)
                            .and_then(|owner| {
                                let line = cx.style_parser_line(owner)?;
                                text_position(markup?, line, &text)
                            })
                            .unwrap_or((0, 0));
                        let (end_line, end_column) = text_end(start_line, start_column, &text);
                        let mut header = json!({
                            "styleSheetId": self.sheet_id(id),
                            "frameId": frame,
                            "sourceURL": source_url,
                            "origin": "regular",
                            "title": cx.style_sheet_title(sheet).unwrap_or_default(),
                            "disabled": cx.style_sheet_disabled(sheet),
                            "isInline": inline,
                            "isMutable": constructed,
                            "isConstructed": constructed,
                            "startLine": start_line,
                            "startColumn": start_column,
                            "length": text.encode_utf16().count(),
                            "endLine": end_line,
                            "endColumn": end_column,
                        });
                        if let Some(owner) = owner {
                            header["ownerNode"] = cx.node_id(owner).get().into();
                        }
                        if !inline {
                            header["loadingFailed"] = false.into();
                        }
                        let media = cx.style_sheet_media(sheet).0;
                        sheets.push(SheetInfo {
                            id,
                            header,
                            text,
                            refetch,
                            contents: contents.clone(),
                            media: (owner.is_some() && !media.is_empty())
                                .then(|| media.to_owned()),
                        });
                    }
                }
            }
            sheets
        })
    }

    /// `sheet`'s text parsed for where its rules are: the one kept, if its
    /// contents and text are the same.
    fn source(&self, sheet: &SheetInfo) -> Rc<SheetSource> {
        let mut sources = self.sources.borrow_mut();
        if let Some(source) = sources.iter().find(|source| {
            std::ptr::eq(source.contents.as_ptr(), Arc::as_ptr(&sheet.contents))
                && source.text == sheet.text
        }) {
            return source.clone();
        }
        sources.retain(|source| source.contents.strong_count() > 0);
        let source = Rc::new(SheetSource {
            contents: Arc::downgrade(&sheet.contents),
            rules: css::inspector::rules_for_inspector(&sheet.text),
            lines: Lines::new(&sheet.text),
            text: sheet.text.clone(),
        });
        sources.push(source.clone());
        source
    }

    /// A rule an element matches, as DevTools' `CSS.CSSRule`: with where it
    /// is in its sheet (one of `sheets`), and the group rules it is in.
    fn rule_json(&self, matched: &style::ExplainedRule, sheets: &[SheetInfo]) -> Value {
        let selectors: Vec<Value> = matched
            .rule
            .selectors_for_inspector()
            .into_iter()
            .map(|(text, [a, b, c])| json!({ "text": text, "specificity": { "a": a, "b": b, "c": c } }))
            .collect();
        let origin = match matched.origin {
            css::stylesheet::Origin::UserAgent => "user-agent",
            css::stylesheet::Origin::Author => "regular",
        };
        let mut rule = json!({
            "selectorList": { "selectors": selectors, "text": matched.rule.selector_text() },
            "origin": origin,
            "style": style_json(matched.rule.style()),
        });
        // The sheet it is of: none for the UA's.
        let Some(sheet) = matched.sheet else {
            return rule;
        };
        let id = self.sheet_id(sheet);
        rule["styleSheetId"] = id.clone().into();
        rule["style"]["styleSheetId"] = id.clone().into();
        let Some(sheet) = sheets.iter().find(|info| info.id == sheet) else {
            return rule;
        };
        let Some(path) = rule_path(sheet.contents.rules(), &matched.rule) else {
            return rule;
        };
        let source = self.source(sheet);
        let source_url = sheet.header["sourceURL"].clone();
        // The group rules it is in, the outermost first, and where it is.
        let (mut media, mut supports, mut types) = (Vec::new(), Vec::new(), Vec::new());
        let mut rules = sheet.contents.rules();
        let mut sources = Some(&source.rules[..]);
        let mut found = None;
        for &at in &path {
            let here = sources.and_then(|sources| sources.get(at));
            let prelude = here.map(|here| (&source.text[here.prelude.clone()], &here.prelude));
            match &rules[at] {
                CssRule::Media(group) => {
                    let mut entry = json!({
                        "text": prelude.map_or_else(|| group.media.to_css(), |(text, _)| text.to_owned()),
                        "source": "mediaRule",
                        "sourceURL": source_url,
                        "styleSheetId": id,
                        "mediaList": [],
                    });
                    if let Some((_, range)) = prelude {
                        entry["range"] = source.lines.range(&source.text, range);
                    }
                    media.push(entry);
                    types.push("MediaRule");
                    rules = &group.rules;
                }
                CssRule::Supports(group) => {
                    let mut entry = json!({
                        "text": prelude.map_or_else(|| group.condition.clone(), |(text, _)| text.to_owned()),
                        "active": group.supported,
                        "styleSheetId": id,
                    });
                    if let Some((_, range)) = prelude {
                        entry["range"] = source.lines.range(&source.text, range);
                    }
                    supports.push(entry);
                    types.push("SupportsRule");
                    rules = &group.rules;
                }
                _ => found = here,
            }
            sources = here.map(|here| &here.rules[..]);
        }
        // The innermost first, the sheet's own media last (Blink's
        // `BuildMediaListChain`).
        media.reverse();
        supports.reverse();
        types.reverse();
        if let Some(text) = &sheet.media {
            media.push(json!({
                "text": text,
                "source": "linkedSheet",
                "sourceURL": source_url,
            }));
            types.push("MediaRule");
        }
        rule["media"] = media.into();
        rule["supports"] = supports.into();
        rule["ruleTypes"] = types.into();
        let Some(found) = found else {
            return rule;
        };
        // Its selectors as written, if the parser found as many.
        let selectors = &mut rule["selectorList"]["selectors"];
        if selectors.as_array().map(Vec::len) == Some(found.selectors.len()) {
            for (selector, range) in selectors
                .as_array_mut()
                .into_iter()
                .flatten()
                .zip(&found.selectors)
            {
                selector["text"] = source.text[range.clone()].trim_end().into();
                selector["range"] = source.lines.range(&source.text, range);
            }
            let texts: Vec<&str> = found
                .selectors
                .iter()
                .map(|range| source.text[range.clone()].trim_end())
                .collect();
            rule["selectorList"]["text"] = texts.join(", ").into();
        }
        let style = &mut rule["style"];
        sourced(style, &source.text, &found.block, &source.lines);
        rule
    }

    /// The `@keyframes` `element`'s animations run, as DevTools'
    /// `CSS.CSSKeyframesRule`s (Blink's `AnimationsForNode`): each name of
    /// its `animation-name` once, in order, that finds one, with where it
    /// and its keyframes are in its sheet (one of `sheets`).
    fn keyframes_json(
        &self,
        page: &mut dyn DevToolsPage,
        element: NodeId,
        sheets: &[SheetInfo],
    ) -> Vec<Value> {
        let Some(style) = current_style(page, element) else {
            return Vec::new();
        };
        let mut names: Vec<&str> = Vec::new();
        for name in style.rare.animations.name.iter() {
            if let computed_style::AnimationName::Name(name) = name
                && !names.contains(&&**name)
            {
                names.push(name);
            }
        }
        let mut found = Vec::new();
        for name in names {
            let Some(rule) = with_layout(page, element, |cx, frame, element, document| {
                frame
                    .view
                    .view_of(document)?
                    .keyframes_rule(cx, element, name)
            }) else {
                continue;
            };
            // Where it is: its sheet's, and its place in it.
            let place = sheets.iter().find_map(|sheet| {
                let path = keyframes_path(sheet.contents.rules(), &rule)?;
                Some((sheet, path))
            });
            let id = place.as_ref().map(|(sheet, _)| self.sheet_id(sheet.id));
            let source = place.map(|(sheet, path)| (self.source(sheet), path));
            let source_rule = source.as_ref().and_then(|(source, path)| {
                let mut rules = &source.rules[..];
                let mut at = None;
                for &index in path {
                    let rule = rules.get(index)?;
                    rules = &rule.rules;
                    at = Some(rule);
                }
                at.map(|rule| (source, rule))
            });
            let mut animation_name = json!({ "text": name });
            if let Some((source, rule)) = &source_rule {
                animation_name["range"] = source.lines.range(&source.text, &rule.prelude);
            }
            let keyframes: Vec<Value> = rule
                .keyframes()
                .enumerate()
                .map(|(at, keyframe)| {
                    let mut key_text = json!({ "text": keyframe.key_text() });
                    let mut style = style_json(&keyframe.block);
                    if let Some((source, rule)) = &source_rule
                        && let Some(of) = rule.rules.get(at)
                    {
                        key_text["range"] = source.lines.range(&source.text, &of.prelude);
                        sourced(&mut style, &source.text, &of.block, &source.lines);
                    }
                    let mut entry = json!({
                        "origin": "regular",
                        "keyText": key_text,
                        "style": style,
                    });
                    if let Some(id) = &id {
                        entry["styleSheetId"] = id.clone().into();
                        entry["style"]["styleSheetId"] = id.clone().into();
                    }
                    entry
                })
                .collect();
            found.push(json!({ "animationName": animation_name, "keyframes": keyframes }));
        }
        found
    }

    /// An element's inline style as DevTools' `CSS.CSSStyle`: its `style`
    /// attribute a sheet of its own (one with no declarations without the
    /// attribute: Blink's is its inline style sheet's, which every element
    /// has), with where its declarations are in the attribute.
    fn inline_json(
        &self,
        page: &mut dyn DevToolsPage,
        node: NodeId,
        explained: Option<&style::Explained>,
    ) -> Value {
        let text = page
            .dom()
            .read(|cx| {
                let element = cx.node_by_id(node)?;
                cx.get_attribute(element, "style")
                    .map(|value| value.to_str().into_owned())
            })
            .unwrap_or_default();
        let mut style = match explained.and_then(|explained| explained.inline.as_deref()) {
            Some(inline) => style_json(inline),
            None => json!({ "cssProperties": [], "shorthandEntries": [] }),
        };
        let lines = Lines::new(&text);
        sourced(&mut style, &text, &(0..text.len()), &lines);
        style["styleSheetId"] = self.inline_id(node, &text).into();
        style
    }

    /// The id of `node`'s `style` attribute as a sheet (`<prefix>.<n>`, as
    /// Chrome's are of its process), whose text is `text` now.
    fn inline_id(&self, node: NodeId, text: &str) -> String {
        let mut inline = self.inline_styles.borrow_mut();
        let id = match inline.iter().find(|style| style.node == node) {
            Some(style) => style.id,
            None => {
                let id = self.inline_count.get();
                self.inline_count.set(id.wrapping_add(1));
                inline.push(InlineStyle {
                    node,
                    id,
                    text: text.to_owned(),
                });
                id
            }
        };
        format!("{}.{id}", self.prefix)
    }

    /// The sheets now told to each session: those gone (`styleSheetRemoved`),
    /// those come (`styleSheetAdded`).
    fn tell_sheets(&self, sheets: &[SheetInfo]) {
        let mut told = Vec::new();
        for (name, session) in self.sessions.borrow_mut().iter_mut() {
            for (gone, _) in session
                .sheets
                .iter()
                .filter(|(id, _)| !sheets.iter().any(|sheet| sheet.id == *id))
            {
                told.push((
                    name.clone(),
                    json!({ "method": "CSS.styleSheetRemoved", "params": { "styleSheetId": self.sheet_id(*gone) } }),
                ));
            }
            for sheet in sheets {
                let method = match session.sheets.iter().find(|(id, _)| *id == sheet.id) {
                    None => "CSS.styleSheetAdded",
                    // Script edited its rules (Blink's `StyleSheetChanged`).
                    Some(&(_, contents)) if contents != sheet.key() => "CSS.styleSheetChanged",
                    Some(_) => continue,
                };
                let params = if method == "CSS.styleSheetAdded" {
                    json!({ "header": sheet.header })
                } else {
                    json!({ "styleSheetId": self.sheet_id(sheet.id) })
                };
                told.push((name.clone(), json!({ "method": method, "params": params })));
            }
            session.sheets = sheets.iter().map(|sheet| (sheet.id, sheet.key())).collect();
        }
        for (name, message) in told {
            self.send(&name, &message);
        }
    }

    /// After a frame's style recalc: the sheets listed again and the
    /// sessions told of what changed, if a document's style collected its
    /// sheets since they were last listed (Blink's `ActiveStyleSheetsUpdated`
    /// probe).
    pub(crate) fn sheets_updated(&self, dom: &Dom, frame: &crate::frame_view::Frame) {
        if self.sessions.borrow().is_empty() {
            return;
        }
        let mut versions = Vec::new();
        frame.view.sheets_versions(&mut versions);
        if *self.sheets_versions.borrow() == versions {
            return;
        }
        *self.sheets_versions.borrow_mut() = versions;
        let sheets = self.list_sheets(dom);
        self.tell_sheets(&sheets);
    }

    fn send(&self, session: &str, message: &Value) {
        let _ = self.to_browser.send(RendererHostMsg::DevToolsMessage {
            session: session.to_owned(),
            message: message.to_string(),
        });
    }

    /// The `DOM` agent it answers with (what the lifecycle tells of
    /// pseudo-elements too).
    pub(crate) fn dom_agent(&self) -> &DomAgent {
        &self.dom_agent
    }

    /// `session` has gone.
    pub(crate) fn detach(&self, session: &str) {
        self.sessions.borrow_mut().remove(session);
    }

    /// The DOM is going: the styles kept of its nodes go.
    pub(crate) fn dom_gone(&self) {
        for session in self.sessions.borrow_mut().values_mut() {
            session.tracked = None;
        }
        self.inline_styles.borrow_mut().clear();
    }

    /// The text of the `style` attribute whose sheet's id is `id`, if it is
    /// one's.
    fn inline_text(&self, id: &str, dom: &Dom) -> Option<String> {
        let number = id
            .strip_prefix(&format!("{}.", self.prefix))?
            .parse::<u64>()
            .ok()?;
        let node = self
            .inline_styles
            .borrow()
            .iter()
            .find(|style| style.id == number)?
            .node;
        dom.read(|cx| {
            let element = cx.node_by_id(node)?;
            Some(
                cx.get_attribute(element, "style")
                    .map(|value| value.to_str().into_owned())
                    .unwrap_or_default(),
            )
        })
    }

    /// Answers `message`, a command of `session`'s, if it is one of the
    /// agent's: whether it was. The DOM's changes go to the sessions first,
    /// so the nodes the answer names are theirs.
    pub(crate) fn handle(&self, session: &str, message: &str, page: &mut dyn DevToolsPage) -> bool {
        let Ok(command) = serde_json::from_str::<Value>(message) else {
            return false;
        };
        let Some(method) = command
            .get("method")
            .and_then(Value::as_str)
            .and_then(|method| method.strip_prefix("CSS."))
        else {
            return false;
        };
        if !COMMANDS.contains(&method) {
            return false;
        }
        self.dom_agent.take_changes(page.dom());
        let id = command.get("id").cloned().unwrap_or(Value::Null);
        let params = command.get("params").cloned().unwrap_or_else(|| json!({}));
        let answer = self.answer(session, method, &params, &id, page);
        let message = match answer {
            // Answered later.
            Ok(Value::Null) => return true,
            Ok(result) => json!({ "id": id, "result": result }),
            Err(error) => json!({ "id": id, "error": { "code": -32000, "message": error } }),
        };
        self.send(session, &message);
        true
    }

    /// The answer to the command `method` (with `params`, numbered
    /// `command_id`) of the session `name`; `Value::Null` for one answered
    /// later.
    fn answer(
        &self,
        name: &str,
        method: &str,
        params: &Value,
        command_id: &Value,
        page: &mut dyn DevToolsPage,
    ) -> Result<Value, String> {
        match method {
            "enable" => {
                if !self.dom_agent.enabled(name) {
                    return Err("DOM agent needs to be enabled first.".to_owned());
                }
                self.sessions
                    .borrow_mut()
                    .entry(name.to_owned())
                    .or_default();
                // Every sheet now, before the answer (Blink's `enable`).
                let sheets = self.list_sheets(page.dom());
                self.tell_sheets(&sheets);
                Ok(json!({}))
            }
            "disable" => {
                self.sessions.borrow_mut().remove(name);
                Ok(json!({}))
            }
            _ if !self.sessions.borrow().contains_key(name) => {
                Err("CSS agent was not enabled".to_owned())
            }
            "getComputedStyleForNode" => {
                let (node, pseudo) = self.inspected(name, params, page)?;
                let computed = with_layout(page, node, |cx, frame, node, _| {
                    let element = styled_element(cx, node)?;
                    Some(dom_api::computed_style_for_devtools(
                        cx, frame, element, pseudo,
                    ))
                })
                .unwrap_or_default();
                let computed: Vec<Value> = computed
                    .into_iter()
                    .map(|(name, value)| json!({ "name": name, "value": value }))
                    .collect();
                Ok(json!({
                    "computedStyle": computed,
                    "extraFields": { "isAppearanceBase": false },
                }))
            }
            "getInlineStylesForNode" => {
                // A pseudo-element has none (Chrome's words).
                if self.inspected(name, params, page)?.1.is_some() {
                    return Err("Element is not a style sheet".to_owned());
                }
                let node = self.element(name, params, page)?;
                let explained = explain(page, node);
                let inline = self.inline_json(page, node, explained.as_ref());
                let mut result = json!({ "inlineStyle": inline });
                if let Some(hints) = explained.as_ref().and_then(|e| e.presentation.as_deref()) {
                    result["attributesStyle"] = style_json(hints);
                }
                Ok(result)
            }
            "getMatchedStylesForNode" => {
                // A pseudo-element's: its rules, its element's inherited.
                let (node, of_pseudo) = match self.inspected(name, params, page)? {
                    (node, Some(pseudo)) => (node, Some(pseudo)),
                    _ => (self.element(name, params, page)?, None),
                };
                let explained = explain(page, node);
                let sheets = self.list_sheets(page.dom());
                let matches = |rules: &[style::ExplainedRule]| -> Vec<Value> {
                    rules
                        .iter()
                        .map(|matched| {
                            json!({
                                "rule": self.rule_json(matched, &sheets),
                                "matchingSelectors": matched.matching_selectors,
                            })
                        })
                        .collect()
                };
                let rules = explained.as_ref().map_or_else(Vec::new, |explained| {
                    match of_pseudo.and_then(|pseudo| {
                        PSEUDO_TYPES.iter().position(|(_, of)| *of == Some(pseudo))
                    }) {
                        Some(at) => matches(&explained.pseudo[at]),
                        None => matches(&explained.rules),
                    }
                });
                // Its pseudo-elements' rules, those it can have (a marker a
                // list item's, as style has it) with any (Blink's
                // `BuildPseudoElementMatches`).
                let list_item = current_style(page, node)
                    .is_some_and(|style| style.display == computed_style::Display::ListItem);
                let pseudo_elements = |explained: &style::Explained, highlights_only: bool| {
                    PSEUDO_TYPES
                        .iter()
                        .zip(&explained.pseudo)
                        .filter(|((kind, _), rules)| {
                            !rules.is_empty()
                                && (!highlights_only || *kind == "selection")
                                && (*kind != "marker" || list_item)
                        })
                        .map(|((kind, _), rules)| {
                            json!({ "pseudoType": kind, "matches": matches(rules) })
                        })
                        .collect::<Vec<Value>>()
                };
                let pseudo = explained
                    .as_ref()
                    .filter(|_| of_pseudo.is_none())
                    .map_or_else(Vec::new, |explained| pseudo_elements(explained, false));
                // Each of its ancestors', the nearest first (Blink's
                // `MatchedStylesForNode`'s `inherited`): every rule it
                // matches (the frontend picks out the inherited
                // properties), its inline style if it has one, and its
                // highlight pseudo-elements' rules.
                let ancestors = page.dom().read(|cx| {
                    let element = cx.node_by_id(node)?;
                    // A pseudo-element's: its element first.
                    let first = if of_pseudo.is_some() {
                        Some(element)
                    } else {
                        cx.flat_tree_parent(element)
                    };
                    Some(
                        std::iter::successors(first, |&at| cx.flat_tree_parent(at))
                        .filter(|&at| cx.kind(at) == NodeKind::Element)
                        .map(|at| cx.node_id(at))
                        .collect::<Vec<NodeId>>(),
                    )
                });
                let (mut inherited, mut inherited_pseudo) = (Vec::new(), Vec::new());
                for ancestor in ancestors.unwrap_or_default() {
                    let Some(of) = explain(page, ancestor) else {
                        continue;
                    };
                    let mut entry = json!({ "matchedCSSRules": matches(&of.rules) });
                    if of.inline.is_some() {
                        entry["inlineStyle"] = self.inline_json(page, ancestor, Some(&of));
                    }
                    inherited.push(entry);
                    inherited_pseudo.push(json!({ "pseudoElements": pseudo_elements(&of, true) }));
                }
                let keyframes = if of_pseudo.is_none() {
                    self.keyframes_json(page, node, &sheets)
                } else {
                    Vec::new()
                };
                let mut result = json!({
                    "matchedCSSRules": rules,
                    "pseudoElements": pseudo,
                    "inherited": inherited,
                    "inheritedPseudoElements": inherited_pseudo,
                    "cssKeyframesRules": keyframes,
                    "cssPropertyRules": [],
                    "cssPropertyRegistrations": [],
                    "cssAtRules": [],
                });
                if of_pseudo.is_none() {
                    result["inlineStyle"] = self.inline_json(page, node, explained.as_ref());
                    if let Some(hints) =
                        explained.as_ref().and_then(|e| e.presentation.as_deref())
                    {
                        result["attributesStyle"] = style_json(hints);
                    }
                }
                // Its parent in the layout tree, which Styles reads flex
                // and grid of (Blink's `LayoutParentElement`), sent first:
                // a pseudo-element's, its element.
                let parent = with_layout(page, node, |cx, frame, element, _| {
                    if of_pseudo.is_some() {
                        return Some(cx.node_id(element));
                    }
                    let mut at = cx.flat_tree_parent(element);
                    while let Some(parent) =
                        at.filter(|&parent| cx.kind(parent) == NodeKind::Element)
                    {
                        let contents = frame.computed_style(cx, parent).is_some_and(|style| {
                            style.display == computed_style::Display::Contents
                        });
                        if !contents {
                            return Some(cx.node_id(parent));
                        }
                        at = cx.flat_tree_parent(parent);
                    }
                    None
                });
                if let Some(id) =
                    parent.and_then(|parent| self.dom_agent.push_node(name, parent, page.dom()))
                {
                    result["parentLayoutNodeId"] = json!(id);
                }
                Ok(result)
            }
            "getStyleSheetText" => {
                let id = params["styleSheetId"].as_str().unwrap_or_default();
                // An element's `style` attribute's.
                if let Some(text) = self.inline_text(id, page.dom()) {
                    return Ok(json!({ "text": text }));
                }
                let sheets = self.list_sheets(page.dom());
                let sheet = sheets
                    .into_iter()
                    .find(|sheet| self.sheet_id(sheet.id) == id)
                    .ok_or("No style sheet with given id found")?;
                // A `<link>`'s loaded before DevTools came: fetched again,
                // answered once it comes.
                if let Some(refetch) = sheet.refetch {
                    let mut refetches = self.refetches.borrow_mut();
                    let known = refetches
                        .iter_mut()
                        .find(|known| known.contents.ptr_eq(&refetch.contents));
                    let waiting = (name.to_owned(), command_id.clone());
                    match known {
                        Some(known) => known.waiting.push(waiting),
                        None => refetches.push(Refetch {
                            waiting: vec![waiting],
                            ..refetch
                        }),
                    }
                    return Ok(Value::Null);
                }
                Ok(json!({ "text": sheet.text }))
            }
            "getAnimatedStylesForNode" => {
                // Its animations' values (a pseudo-element's, its), then
                // each of its ancestors' (its element first).
                let (node, pseudo) = self.inspected(name, params, page)?;
                Ok(page
                    .dom()
                    .read(|cx| {
                        let element = cx.node_by_id(node)?;
                        let now = cx.animation_time(cx.document_of(element));
                        let mut result = animated_styles(cx, element, pseudo, now);
                        let first = if pseudo.is_some() {
                            Some(element)
                        } else {
                            cx.flat_tree_parent(element)
                        };
                        let inherited: Vec<Value> =
                            std::iter::successors(first, |&at| cx.flat_tree_parent(at))
                                .filter(|&at| cx.kind(at) == NodeKind::Element)
                                .map(|at| animated_styles(cx, at, None, now))
                                .collect();
                        result["inherited"] = inherited.into();
                        Some(result)
                    })
                    .unwrap_or_else(|| json!({ "animationStyles": [], "inherited": [] })))
            }
            "getPlatformFontsForNode" => {
                let (node, pseudo) = self.inspected(name, params, page)?;
                let fonts = with_layout(page, node, |cx, frame, node, document| {
                    let counted = counted_text(cx, node);
                    let element = (cx.kind(node) == NodeKind::Element).then(|| cx.node_id(node));
                    let view = frame.view.view_of(document)?;
                    Some(view.platform_fonts(&|run| {
                        // A pseudo-element's: its content's.
                        if pseudo.is_some() {
                            return run.source.is_none()
                                && run.pseudo == pseudo
                                && run.node == element;
                        }
                        match &run.source {
                            Some((text, _)) => counted.contains(text),
                            // Generated content's: the element's own
                            // pseudo-elements'.
                            None => {
                                run.pseudo.is_some() && run.node == element && element.is_some()
                            }
                        }
                    }))
                })
                .unwrap_or_default();
                let fonts: Vec<Value> = fonts
                    .into_iter()
                    .map(|font| {
                        json!({
                            "familyName": font.family_name,
                            "postScriptName": font.postscript_name,
                            // machopo has no `@font-face`: every font is
                            // the platform's.
                            "isCustomFont": false,
                            "glyphCount": font.glyph_count,
                        })
                    })
                    .collect();
                Ok(json!({ "fonts": fonts }))
            }
            "trackComputedStyleUpdatesForNode" => {
                let tracked = match params["nodeId"].as_i64() {
                    None => None,
                    Some(id) => {
                        let node = self.node(name, params, page)?;
                        let style = current_style(page, node);
                        Some(Tracked { id, node, style })
                    }
                };
                if let Some(session) = self.sessions.borrow_mut().get_mut(name) {
                    session.tracked = tracked;
                }
                Ok(json!({}))
            }
            _ => unreachable!("one of COMMANDS"),
        }
    }

    /// The node a command names by its `nodeId` (Blink's `AssertNode`).
    fn node(
        &self,
        name: &str,
        params: &Value,
        page: &mut dyn DevToolsPage,
    ) -> Result<NodeId, String> {
        let id = params["nodeId"].as_i64().unwrap_or(0);
        self.dom_agent
            .node_param(name, &json!({ "nodeId": id }), page)
    }

    /// The node or pseudo-element a command names by its `nodeId`: a
    /// pseudo-element's element and kind.
    fn inspected(
        &self,
        name: &str,
        params: &Value,
        page: &mut dyn DevToolsPage,
    ) -> Result<(NodeId, Option<computed_style::PseudoId>), String> {
        let id = params["nodeId"].as_i64().unwrap_or(0);
        self.dom_agent
            .inspected_param(name, &json!({ "nodeId": id }), page)
    }

    /// The element a command names by its `nodeId` (Blink's
    /// `AssertElement`).
    fn element(
        &self,
        name: &str,
        params: &Value,
        page: &mut dyn DevToolsPage,
    ) -> Result<NodeId, String> {
        let node = self.node(name, params, page)?;
        let element = page.dom().read(|cx| {
            cx.node_by_id(node)
                .is_some_and(|node| cx.kind(node) == NodeKind::Element)
        });
        if !element {
            return Err("Node is not an Element".to_owned());
        }
        Ok(node)
    }

    /// After a frame's style recalc: each session whose Computed pane
    /// follows a node whose style changed is told (`computedStyleUpdated`;
    /// Blink's `DidUpdateComputedStyle` probe). A style worked out again
    /// the same is a new one too: the pane reads it again for nothing.
    pub(crate) fn styles_updated(&self, dom: &Dom, layout: &dyn FrameLayout) {
        let mut told = Vec::new();
        for (name, session) in self.sessions.borrow_mut().iter_mut() {
            let Some(tracked) = &mut session.tracked else {
                continue;
            };
            let style = dom.read(|cx| {
                let element = styled_element(cx, cx.node_by_id(tracked.node)?)?;
                layout.computed_style(cx, element)
            });
            let same = match (&style, &tracked.style) {
                (Some(new), Some(old)) => Arc::ptr_eq(new, old),
                (None, None) => true,
                _ => false,
            };
            if !same {
                tracked.style = style;
                told.push((name.clone(), tracked.id));
            }
        }
        for (name, id) in told {
            self.send(
                &name,
                &json!({ "method": "CSS.computedStyleUpdated", "params": { "nodeId": id } }),
            );
        }
        // The `style` attributes DevTools has as sheets, changed since.
        let mut changed = Vec::new();
        self.inline_styles.borrow_mut().retain_mut(|style| {
            let Some(text) = dom.read(|cx| {
                let element = cx.node_by_id(style.node)?;
                Some(
                    cx.get_attribute(element, "style")
                        .map(|value| value.to_str().into_owned())
                        .unwrap_or_default(),
                )
            }) else {
                return false;
            };
            if text != style.text {
                style.text = text;
                changed.push(format!("{}.{}", self.prefix, style.id));
            }
            true
        });
        let names: Vec<String> = self.sessions.borrow().keys().cloned().collect();
        for id in changed {
            for name in &names {
                self.send(
                    name,
                    &json!({ "method": "CSS.styleSheetChanged", "params": { "styleSheetId": id } }),
                );
            }
        }
    }
}

/// `element`'s (or its pseudo-element `pseudo`'s) animations' values at
/// `now` as DevTools' `getAnimatedStylesForNode` has them (Blink's
/// `BuildAnimationStyles`): each animation's alone over its base style, its
/// name a CSS animation's, the computed values of the properties it
/// animates; the transitions' together (for an element with animations,
/// none running too).
fn animated_styles<'gc>(
    cx: &Cx<'_, 'gc>,
    element: NodeRef<'gc>,
    pseudo: Option<computed_style::PseudoId>,
    now: f64,
) -> Value {
    let target = match pseudo {
        None => 0,
        Some(computed_style::PseudoId::Before) => 1,
        Some(computed_style::PseudoId::After) => 2,
        Some(computed_style::PseudoId::Marker) => 3,
    };
    let sampled = cx
        .read_animations(element, |targets| {
            targets[target]
                .as_ref()
                .map(|target| target.animations.sampled_alone(now, &target.base))
        })
        .flatten();
    let style = |style: &ComputedStyle, properties: &[css::properties::LonghandId]| {
        let properties: Vec<Value> = properties
            .iter()
            .map(|property| {
                let name = property.name();
                let value = css::computed::computed_value(style, name).unwrap_or_default();
                json!({ "name": name, "value": value })
            })
            .collect();
        json!({ "cssProperties": properties, "shorthandEntries": [] })
    };
    let animations: Vec<Value> = sampled
        .iter()
        .flat_map(|sampled| &sampled.animations)
        .map(|animation| {
            let mut entry = json!({ "style": style(&animation.style, &animation.properties) });
            if let Some(name) = &animation.name {
                entry["name"] = name.as_ref().into();
            }
            entry
        })
        .collect();
    let mut result = json!({ "animationStyles": animations });
    // An element with animations has its transitions' style, empty with
    // none running (Blink's, of its `ElementAnimations`).
    if let Some(sampled) = &sampled {
        result["transitionsStyle"] = match &sampled.transitions {
            Some(transitions) => style(&transitions.style, &transitions.properties),
            None => json!({ "cssProperties": [], "shorthandEntries": [] }),
        };
    }
    result
}

/// DevTools' names of the pseudo-elements `style::Explained::pseudo` has
/// rules of, in its order, and their kinds.
const PSEUDO_TYPES: [(&str, Option<computed_style::PseudoId>); 4] = [
    ("before", Some(computed_style::PseudoId::Before)),
    ("after", Some(computed_style::PseudoId::After)),
    ("marker", Some(computed_style::PseudoId::Marker)),
    ("selection", None),
];

/// A sheet as `CSS` tells it: its id (`dom::Cx::style_sheet_id`), its
/// header and its text.
struct SheetInfo {
    id: u64,
    header: Value,
    text: String,
    /// A `<link>`'s whose text DevTools hasn't: to fetch it again with.
    refetch: Option<Refetch>,
    /// Its rules: what its text is the source of, where its rules are found.
    contents: Arc<CssStyleSheetContents>,
    /// Its own media as written (an element's `media`), if any.
    media: Option<String>,
}

impl SheetInfo {
    /// Its contents' address: another once script edits its rules.
    fn key(&self) -> usize {
        Arc::as_ptr(&self.contents).addr()
    }
}

/// `text`, a `<link>`'s fetched again (none: it failed), if it parses
/// (against `base`) to the rules the sheet has, `contents`.
fn text_if_same(
    text: Option<String>,
    base: Option<css::UrlContext>,
    contents: &CssStyleSheetContents,
) -> Option<Rc<str>> {
    let text = text?;
    (serialized(&CssStyleSheetContents::parse(&text, base)) == serialized(contents))
        .then(|| Rc::from(text))
}

/// Where `target` is in `rules`: its index in each list down to it, through
/// the group rules it is in. None if it isn't there.
fn rule_path(rules: &[CssRule], target: &Arc<css::StyleRule>) -> Option<Vec<usize>> {
    for (at, rule) in rules.iter().enumerate() {
        let inner = match rule {
            CssRule::Style(style) if Arc::ptr_eq(style, target) => return Some(vec![at]),
            CssRule::Media(group) => &group.rules,
            CssRule::Supports(group) => &group.rules,
            _ => continue,
        };
        if let Some(mut path) = rule_path(inner, target) {
            path.insert(0, at);
            return Some(path);
        }
    }
    None
}

/// Where `target` is in `rules` (`rule_path` of a `@keyframes`).
fn keyframes_path(rules: &[CssRule], target: &Arc<css::KeyframesRule>) -> Option<Vec<usize>> {
    for (at, rule) in rules.iter().enumerate() {
        let inner = match rule {
            CssRule::Keyframes(keyframes) if Arc::ptr_eq(keyframes, target) => {
                return Some(vec![at]);
            }
            CssRule::Media(group) => &group.rules,
            CssRule::Supports(group) => &group.rules,
            _ => continue,
        };
        if let Some(mut path) = keyframes_path(inner, target) {
            path.insert(0, at);
            return Some(path);
        }
    }
    None
}

/// `style`, a `CSS.CSSStyle` of a block's declarations as parsed, with
/// where the block is (`block` of `text`): its range and text, and its
/// declarations as written first (Blink's `InspectorStyle::PopulateAllProperties`),
/// those parsed after them.
fn sourced(style: &mut Value, text: &str, block: &Range<usize>, lines: &Lines) {
    let written = css::inspector::declarations_for_inspector(text, block.clone());
    let mut properties: Vec<Value> = written
        .iter()
        .map(|declaration| declaration_json(declaration, text, lines))
        .collect();
    // Those parsed, not told implicit (Blink's, with the block's text).
    if let Some(parsed) = style["cssProperties"].as_array() {
        properties.extend(parsed.iter().cloned().map(|mut property| {
            if let Some(property) = property.as_object_mut() {
                property.remove("implicit");
            }
            property
        }));
    }
    style["cssProperties"] = properties.into();
    style["range"] = lines.range(text, block);
    style["cssText"] = text.get(block.clone()).unwrap_or_default().into();
}

/// A declaration as written, as DevTools' `CSS.CSSProperty`: a shorthand's
/// with the longhands it sets.
fn declaration_json(declaration: &SourceDeclaration, text: &str, lines: &Lines) -> Value {
    let mut property = json!({
        "name": declaration.name,
        "value": declaration.value,
        "text": text.get(declaration.range.clone()).unwrap_or_default(),
        "range": lines.range(text, &declaration.range),
        "disabled": declaration.disabled,
    });
    // One turned off isn't told implicit or not (Blink's).
    if !declaration.disabled {
        property["implicit"] = false.into();
    }
    if declaration.important {
        property["important"] = true.into();
    }
    if !declaration.parsed_ok {
        property["parsedOk"] = false.into();
    }
    let parsed =
        CssPropertyValueSet::parse(&format!("{}: {}", declaration.name, declaration.value));
    let longhands = parsed.inspector_declarations();
    // A shorthand's longhands; none while a `var()` keeps them from having
    // values (Blink's).
    if longhands.iter().any(|longhand| longhand.shorthand.is_some())
        && longhands.iter().all(|longhand| !longhand.value.is_empty())
    {
        let longhands: Vec<Value> = longhands
            .iter()
            .map(|longhand| json!({ "name": longhand.name, "value": longhand.value }))
            .collect();
        property["longhandProperties"] = longhands.into();
    }
    property
}

/// `document` and the shadow roots in it, at any depth, in tree order: the
/// scopes whose sheets there are.
fn scopes<'gc>(cx: &Cx<'_, 'gc>, document: NodeRef<'gc>) -> Vec<NodeRef<'gc>> {
    let mut scopes = vec![document];
    let mut at = 0;
    while let Some(&scope) = scopes.get(at) {
        let mut node = cx.first_child(scope);
        while let Some(here) = node {
            if let Some(root) = cx.shadow_root(here) {
                scopes.push(root);
            }
            node = cx.first_child(here).or_else(|| {
                let mut up = Some(here);
                while let Some(climb) = up.filter(|&climb| cx.node_id(climb) != cx.node_id(scope)) {
                    if let Some(next) = cx.next_sibling(climb) {
                        return Some(next);
                    }
                    up = cx.parent(climb);
                }
                None
            });
        }
        at = at.saturating_add(1);
    }
    scopes
}

/// A sheet's contents as CSSOM writes its rules, one a line (Blink's
/// `InspectorStyleSheet` text of a sheet it has no source of).
fn serialized(contents: &css::StyleSheetContents) -> String {
    let mut text = String::new();
    for rule in contents.rules() {
        text.push_str(&rule.css_text());
        text.push('\n');
    }
    text
}

/// Where a `<style>`'s text starts in its document's `markup`, as a line
/// and a column (UTF-16) from 0: after the `>` that ends a `<style` start
/// tag on `line` (from 1, the one its start tag ends on); of several there,
/// the one its `text` follows (a line break in the markup as CR LF, as LF in
/// the text), else the first (its text changed since: Blink's place is its
/// element's, whatever its text). None if no start tag ends there.
fn text_position(markup: &str, line: u32, text: &str) -> Option<(u32, u32)> {
    let index = usize::try_from(line.checked_sub(1)?).ok()?;
    let start = if index == 0 {
        0
    } else {
        markup.match_indices('\n').nth(index - 1)?.0.checked_add(1)?
    };
    let line_text = markup[start..].split('\n').next()?;
    // The `>`s that end a `<style` start tag: a `<style` before them in the
    // markup with no other `>` between.
    let ends: Vec<usize> = line_text
        .match_indices('>')
        .map(|(at, _)| at)
        .filter(|&at| {
            let before = &markup[..start.saturating_add(at)];
            before.rfind('<').is_some_and(|open| {
                let tag = &before[open..];
                tag.len() >= 6
                    && tag.as_bytes()[1..6].eq_ignore_ascii_case(b"style")
                    && !tag.contains('>')
            })
        })
        .collect();
    let at = ends
        .iter()
        .copied()
        .find(|&at| follows(&markup[start.saturating_add(at).saturating_add(1)..], text))
        .or_else(|| ends.first().copied())?;
    let column = line_text[..=at].encode_utf16().count();
    Some((u32::try_from(index).ok()?, u32::try_from(column).ok()?))
}

/// Whether `markup` starts with `text`, CR LF in it as LF.
fn follows(markup: &str, text: &str) -> bool {
    let mut markup = markup.chars().peekable();
    for want in text.chars() {
        if want == '\n' && markup.peek() == Some(&'\r') {
            markup.next();
        }
        if markup.next() != Some(want) {
            return false;
        }
    }
    true
}

/// Where `text` that starts at `line`, `column` ends (UTF-16 columns).
fn text_end(line: u32, column: u32, text: &str) -> (u32, u32) {
    let breaks = text.matches('\n').count();
    let last = text
        .rsplit('\n')
        .next()
        .unwrap_or_default()
        .encode_utf16()
        .count();
    let last = u32::try_from(last).unwrap_or(u32::MAX);
    if breaks == 0 {
        (line, column.saturating_add(last))
    } else {
        (
            line.saturating_add(u32::try_from(breaks).unwrap_or(u32::MAX)),
            last,
        )
    }
}

/// The element whose style is `node`'s: itself, or a text node's parent
/// in the flat tree (Blink's `CSSComputedStyleDeclaration::StyledElement`).
fn styled_element<'gc>(cx: &Cx<'_, 'gc>, node: NodeRef<'gc>) -> Option<NodeRef<'gc>> {
    match cx.kind(node) {
        NodeKind::Element => Some(node),
        NodeKind::Text => cx
            .flat_tree_parent(node)
            .filter(|&parent| cx.kind(parent) == NodeKind::Element),
        _ => None,
    }
}

/// `node`'s style as last worked out, nothing brought up to date.
fn current_style(page: &mut dyn DevToolsPage, node: NodeId) -> Option<Arc<ComputedStyle>> {
    let (dom, layout) = page.dom_and_layout();
    let layout = layout?;
    dom.read(|cx| {
        let element = styled_element(cx, cx.node_by_id(node)?)?;
        layout.computed_style(cx, element)
    })
}

/// What `element`'s own style is cascaded from, its style brought up to
/// date first (Blink's `getMatchedStylesForNode` updates it).
fn explain(page: &mut dyn DevToolsPage, element: NodeId) -> Option<style::Explained> {
    with_layout(page, element, |cx, frame, element, document| {
        frame.view.view_of_mut(document)?.explain_style(cx, element)
    })
}

/// The text nodes whose fonts `getPlatformFontsForNode` counts for
/// `node`: itself, a text node; else its children's and theirs (Blink
/// goes two levels down the layout tree).
fn counted_text<'gc>(cx: &Cx<'_, 'gc>, node: NodeRef<'gc>) -> Vec<NodeId> {
    if cx.kind(node) == NodeKind::Text {
        return vec![cx.node_id(node)];
    }
    let children =
        |node| std::iter::successors(cx.first_child(node), |&child| cx.next_sibling(child));
    let mut counted = Vec::new();
    for child in children(node) {
        match cx.kind(child) {
            NodeKind::Text => counted.push(cx.node_id(child)),
            NodeKind::Element => counted.extend(
                children(child)
                    .filter(|&grandchild| cx.kind(grandchild) == NodeKind::Text)
                    .map(|grandchild| cx.node_id(grandchild)),
            ),
            _ => {}
        }
    }
    counted
}

/// A declaration block DevTools has no text of, as its `CSS.CSSStyle`
/// (Blink's `InspectorStyle::BuildObjectForStyle` without source data):
/// each declaration, and the shorthands that set them.
fn style_json(block: &CssPropertyValueSet) -> Value {
    let declarations = block.inspector_declarations();
    let mut shorthands: Vec<&str> = Vec::new();
    for shorthand in declarations
        .iter()
        .filter_map(|declaration| declaration.shorthand)
    {
        if !shorthands.contains(&shorthand) {
            shorthands.push(shorthand);
        }
    }
    let properties: Vec<Value> = declarations
        .iter()
        .map(|declaration| {
            // An important one's value with its `!important` (Blink's).
            let value = if declaration.important {
                format!("{} !important", declaration.value)
            } else {
                declaration.value.clone()
            };
            let mut property = json!({ "name": declaration.name, "value": value });
            if declaration.important {
                property["important"] = json!(true);
            }
            if declaration.implicit {
                property["implicit"] = json!(true);
            }
            property
        })
        .collect();
    let shorthands: Vec<Value> = shorthands
        .into_iter()
        .map(|shorthand| {
            let mut entry = json!({ "name": shorthand, "value": block.property_value(shorthand) });
            if block.property_priority(shorthand) == "important" {
                entry["important"] = json!(true);
            }
            entry
        })
        .collect();
    json!({ "cssProperties": properties, "shorthandEntries": shorthands })
}

#[cfg(test)]
mod tests {
    use super::{CssStyleSheetContents, text_end, text_if_same, text_position};

    /// What came again is the text if it has the sheet's rules (its
    /// comments and white space aside); not if they differ, or nothing
    /// came.
    #[test]
    fn a_link_fetched_again_is_its_text_if_its_rules_are_the_same() {
        let contents = CssStyleSheetContents::parse("p { color: red }", None);
        let same = "/* again */\np {\n  color: red;\n}\n";
        assert_eq!(
            text_if_same(Some(same.to_owned()), None, &contents).as_deref(),
            Some(same)
        );
        assert_eq!(
            text_if_same(Some("p { color: blue }".to_owned()), None, &contents),
            None
        );
        assert_eq!(text_if_same(None, None, &contents), None);
    }

    /// After the `>` of a `<style` start tag on the line asked, in UTF-16
    /// columns; of several on that line, the one its text follows; one
    /// whose text isn't there now, the first; a line with none, none.
    #[test]
    fn inline_sheets_text_is_found_in_the_markup() {
        let markup = "<!DOCTYPE html>\n<p title='>'>é</p><style>a {}</style><style id=b>b {}</style>\n";
        assert_eq!(text_position(markup, 2, "a {}"), Some((1, 25)));
        assert_eq!(text_position(markup, 2, "b {}"), Some((1, 49)));
        assert_eq!(text_position(markup, 2, "changed"), Some((1, 25)));
        assert_eq!(text_position(markup, 1, "a {}"), None);
        assert_eq!(text_position(markup, 3, "a {}"), None);
        assert_eq!(text_position(markup, 0, "a {}"), None);
        // A tag over two lines: where it ends; its text's line break in the
        // markup as CR LF.
        let crlf = "<style\r\n media=all><style>\r\nb {}\r\n</style><style>\nb {}\n</style>";
        assert_eq!(text_position(crlf, 2, "\nb {}\n"), Some((1, 18)));
        // `<styles>` isn't one.
        assert_eq!(text_position("<styles>x", 1, "x"), Some((0, 8)));
        assert_eq!(text_position("<b>x", 1, "x"), None);
        // A `>` in the text doesn't end the start tag.
        assert_eq!(text_position("<style>a > b</style>", 1, " b"), Some((0, 7)));
    }

    #[test]
    fn text_ends_after_its_last_line() {
        assert_eq!(text_end(2, 7, "a {}"), (2, 11));
        assert_eq!(text_end(2, 7, "\na {}\n"), (4, 0));
        assert_eq!(text_end(2, 7, "\na {}é"), (3, 5));
        assert_eq!(text_end(2, 7, ""), (2, 7));
    }
}
