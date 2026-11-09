//! DevTools' `CSS` domain as the renderer answers it (Blink's
//! `InspectorCSSAgent`), for the Elements panel's Computed and Styles: an
//! element's computed style, what `getComputedStyle` reads; the rules it
//! matches, its presentational hints and its `style` attribute's
//! declarations, as the style engine cascades them; the fonts its text
//! was laid out in; and, while the Computed pane follows a node, word when
//! its style changes.
//!
//! The rules come without the sheets they are in (no `styleSheetId`, no
//! `range`): machopo keeps no sheet's text, and `enable` tells of no
//! sheet. Their declarations are as parsed, as Chrome lists a rule it has
//! no text of (a user-agent rule).

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use computed_style::ComputedStyle;
use css::CssPropertyValueSet;
use crossbeam_channel::Sender;
use dom::{Cx, Dom, NodeId, NodeKind, NodeRef};
use dom_api::{DevToolsPage, FrameLayout};
use protocol::renderer::RendererHostMsg;
use serde_json::{Value, json};

use super::dom_agent::{DomAgent, with_layout};

/// The commands the agent answers, without `CSS.`.
const COMMANDS: &[&str] = &[
    "enable",
    "disable",
    "getComputedStyleForNode",
    "getInlineStylesForNode",
    "getMatchedStylesForNode",
    "getPlatformFontsForNode",
    "trackComputedStyleUpdatesForNode",
];

/// A session's `CSS`, while it has it on.
#[derive(Default)]
struct Session {
    /// The node the Computed pane follows (`trackComputedStyleUpdatesForNode`),
    /// by the session's id of it and the backend's, with its style as last
    /// seen.
    tracked: Option<Tracked>,
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
    /// The sessions that turned `CSS` on.
    sessions: RefCell<HashMap<String, Session>>,
}

impl CssAgent {
    pub(crate) fn new(to_browser: Sender<RendererHostMsg>, dom_agent: Rc<DomAgent>) -> Self {
        Self {
            to_browser,
            dom_agent,
            sessions: RefCell::new(HashMap::new()),
        }
    }

    fn send(&self, session: &str, message: &Value) {
        let _ = self.to_browser.send(RendererHostMsg::DevToolsMessage {
            session: session.to_owned(),
            message: message.to_string(),
        });
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
        let answer = self.answer(session, method, &params, page);
        let message = match answer {
            Ok(result) => json!({ "id": id, "result": result }),
            Err(error) => json!({ "id": id, "error": { "code": -32000, "message": error } }),
        };
        self.send(session, &message);
        true
    }

    fn answer(
        &self,
        name: &str,
        method: &str,
        params: &Value,
        page: &mut dyn DevToolsPage,
    ) -> Result<Value, String> {
        match method {
            "enable" => {
                if !self.dom_agent.enabled(name) {
                    return Err("DOM agent needs to be enabled first.".to_owned());
                }
                self.sessions.borrow_mut().entry(name.to_owned()).or_default();
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
                let node = self.node(name, params, page)?;
                let computed = with_layout(page, node, |cx, frame, node, _| {
                    let element = styled_element(cx, node)?;
                    Some(dom_api::computed_style_for_devtools(cx, frame, element))
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
                let node = self.element(name, params, page)?;
                let explained = explain(page, node);
                let mut result = json!({ "inlineStyle": inline_json(explained.as_ref()) });
                if let Some(hints) = explained.as_ref().and_then(|e| e.presentation.as_deref()) {
                    result["attributesStyle"] = style_json(hints);
                }
                Ok(result)
            }
            "getMatchedStylesForNode" => {
                let node = self.element(name, params, page)?;
                let explained = explain(page, node);
                let rules: Vec<Value> = explained
                    .iter()
                    .flat_map(|explained| &explained.rules)
                    .map(|matched| {
                        json!({
                            "rule": rule_json(matched),
                            "matchingSelectors": matched.matching_selectors,
                        })
                    })
                    .collect();
                let mut result = json!({
                    "matchedCSSRules": rules,
                    "pseudoElements": [],
                    "inherited": [],
                    "inheritedPseudoElements": [],
                    "cssKeyframesRules": [],
                    "cssPropertyRules": [],
                    "cssPropertyRegistrations": [],
                    "cssAtRules": [],
                });
                result["inlineStyle"] = inline_json(explained.as_ref());
                if let Some(hints) = explained.as_ref().and_then(|e| e.presentation.as_deref()) {
                    result["attributesStyle"] = style_json(hints);
                }
                // Its parent in the layout tree, which Styles reads flex
                // and grid of (Blink's `LayoutParentElement`), sent first.
                let parent = with_layout(page, node, |cx, frame, element, _| {
                    let mut at = cx.flat_tree_parent(element);
                    while let Some(parent) = at.filter(|&parent| cx.kind(parent) == NodeKind::Element) {
                        let contents = frame
                            .computed_style(cx, parent)
                            .is_some_and(|style| style.display == computed_style::Display::Contents);
                        if !contents {
                            return Some(cx.node_id(parent));
                        }
                        at = cx.flat_tree_parent(parent);
                    }
                    None
                });
                if let Some(id) = parent.and_then(|parent| self.dom_agent.push_node(name, parent, page.dom())) {
                    result["parentLayoutNodeId"] = json!(id);
                }
                Ok(result)
            }
            "getPlatformFontsForNode" => {
                let node = self.node(name, params, page)?;
                let fonts = with_layout(page, node, |cx, frame, node, document| {
                    let counted = counted_text(cx, node);
                    let element = (cx.kind(node) == NodeKind::Element).then(|| cx.node_id(node));
                    let view = frame.view.view_of(document)?;
                    Some(view.platform_fonts(&|run| {
                        match &run.source {
                            Some((text, _)) => counted.contains(text),
                            // Generated content's: the element's own
                            // pseudo-elements'.
                            None => run.pseudo.is_some() && run.node == element && element.is_some(),
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

    /// The element a command names by its `nodeId` (Blink's
    /// `AssertElement`).
    fn element(
        &self,
        name: &str,
        params: &Value,
        page: &mut dyn DevToolsPage,
    ) -> Result<NodeId, String> {
        let node = self.node(name, params, page)?;
        let element = page
            .dom()
            .read(|cx| cx.node_by_id(node).is_some_and(|node| cx.kind(node) == NodeKind::Element));
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
    let children = |node| std::iter::successors(cx.first_child(node), |&child| cx.next_sibling(child));
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

/// A rule an element matches, as DevTools' `CSS.CSSRule`.
fn rule_json(matched: &style::ExplainedRule) -> Value {
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
    json!({
        "selectorList": { "selectors": selectors, "text": matched.rule.selector_text() },
        "origin": origin,
        "style": style_json(matched.rule.style()),
    })
}

/// An element's inline style, as DevTools' `CSS.CSSStyle`: one with no
/// declarations without a `style` attribute (Blink's is its inline style
/// sheet's, which every element has).
fn inline_json(explained: Option<&style::Explained>) -> Value {
    match explained.and_then(|explained| explained.inline.as_deref()) {
        Some(inline) => style_json(inline),
        None => json!({ "cssProperties": [], "shorthandEntries": [] }),
    }
}

/// A declaration block DevTools has no text of, as its `CSS.CSSStyle`
/// (Blink's `InspectorStyle::BuildObjectForStyle` without source data):
/// each declaration, and the shorthands that set them.
fn style_json(block: &CssPropertyValueSet) -> Value {
    let declarations = block.inspector_declarations();
    let mut shorthands: Vec<&str> = Vec::new();
    for shorthand in declarations.iter().filter_map(|declaration| declaration.shorthand) {
        if !shorthands.contains(&shorthand) {
            shorthands.push(shorthand);
        }
    }
    let properties: Vec<Value> = declarations
        .iter()
        .map(|declaration| {
            let mut property = json!({ "name": declaration.name, "value": declaration.value });
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
