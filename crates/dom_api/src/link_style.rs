//! The `<link>`s' sheets asked for (Blink's `LinkStyle::Process`, as a
//! `<link>` is inserted or changed): each one of a sheet in its frame's
//! document's tree, or a shadow tree in it, once, its request sent
//! through the host's load sink as the DOM's script and image requests
//! are; the DOM keeps what it blocks while it loads (`dom`'s
//! `link_sheet`). In the engine's calls (the parser's, before a script it
//! stopped at runs, so a sheet before it blocks it) and as the host asks
//! (`ScriptEngine::send_sheet_fetches`, after a task).

use dom::{Dom, NodeId, NodeRef, NodeRoot, ScriptCors, SentSheet};

use crate::{FrameId, FrameLayout, LoadSink};

/// A `<link>` whose sheet is to be asked for: its `href`, its CORS
/// settings (none: no CORS), whether it blocks nothing (an alternate one,
/// a shadow tree's), whether the parser made it, and its `media`.
struct LinkToLoad {
    link: NodeRoot,
    id: NodeId,
    href: String,
    cors: Option<ScriptCors>,
    blocks_nothing: bool,
    parser_inserted: bool,
    media: Option<String>,
}

/// The `<link>`s of `frame`'s `document` (its token `token`) whose sheets
/// weren't asked for, each asked for now: from the document's tree, then
/// its shadow trees' whose sheets may have changed, as style collects
/// them, while its style says its sheets may have changed. One the parser
/// made, of media that match as it starts loading (`layout`'s
/// environment: one of a frame given no size yet matches), blocks the
/// parser's scripts (HTML's "a style sheet that is blocking scripts";
/// Blink's `PendingSheetType::kBlocking` looks at its media then too);
/// one not alternate and not a shadow tree's blocks its rendering; each
/// delays its `load`. An `href` that doesn't parse sends nothing: the
/// DOM notes it failed.
pub fn send_sheet_fetches(
    dom: &mut Dom,
    mut layout: Option<&mut dyn FrameLayout>,
    sink: &mut dyn LoadSink,
    frame: FrameId,
    token: Option<String>,
    document: &NodeRoot,
) {
    let links = dom.read(|cx| {
        let document = document.get(cx.gc);
        let mut roots: Vec<(NodeRef<'_>, bool)> = cx
            .sheet_changed_roots(document)
            .into_iter()
            .map(|root| (root, true))
            .collect();
        if cx
            .style_flags(document)
            .contains(dom::StyleFlags::STYLE_SHEETS_CHANGED)
        {
            roots.insert(0, (document, false));
        }
        let mut links = Vec::new();
        for (root, in_shadow) in roots {
            let mut node = cx.first_child(root);
            while let Some(at) = node {
                if let Some(href) = cx.stylesheet_link_href(at)
                    && !cx.sheet_requested(at)
                {
                    // HTML's CORS settings attribute: by CORS with one,
                    // `use-credentials` including them.
                    let cors = cx.get_attribute(at, "crossorigin").map(|value| {
                        if value.to_str().eq_ignore_ascii_case("use-credentials") {
                            ScriptCors::UseCredentials
                        } else {
                            ScriptCors::Anonymous
                        }
                    });
                    links.push(LinkToLoad {
                        link: cx.root(at),
                        id: cx.node_id(at),
                        href,
                        cors,
                        blocks_nothing: in_shadow || cx.is_alternate_stylesheet(at),
                        parser_inserted: cx.script_flags(at).parser_inserted,
                        media: cx
                            .get_attribute(at, "media")
                            .map(|media| media.to_str().into_owned()),
                    });
                }
                node = next_in_tree_order(cx, at, root);
            }
        }
        links
    });
    for link in links {
        let (document_id, link_document, url) = dom.read(|cx| {
            let element = link.link.get(cx.gc);
            let document = document.get(cx.gc);
            (
                cx.node_id(document),
                cx.node_id(cx.document_of(element)),
                cx.resolve_url(document, link.href.trim()),
            )
        });
        let blocks_scripts = !link.blocks_nothing
            && link.parser_inserted
            && link.media.as_deref().is_none_or(|media| {
                layout
                    .as_deref_mut()
                    .and_then(|layout| layout.media_environment(link_document))
                    .is_none_or(|environment| {
                        css::media::MediaQueryList::parse(media).matches(&environment)
                    })
            });
        dom.read(|cx| cx.set_sheet_requested(link.link.get(cx.gc)));
        let Some(url) = url else {
            dom.mutate(|cx| cx.note_load_failed(document.get(cx.gc), link.href));
            continue;
        };
        let (origin, request_origin) = crate::engine::load_origin(
            dom,
            document_id,
            token.clone(),
            Some(document),
            &dom::LoadInitiator::Element(link.id),
        );
        let Some(request_id) = sink.start_sheet_fetch(
            frame,
            document_id,
            url.as_str(),
            link.cors,
            request_origin.as_deref(),
            &origin,
        ) else {
            dom.mutate(|cx| cx.note_load_failed(document.get(cx.gc), url.to_string()));
            continue;
        };
        dom.mutate(|cx| {
            cx.sheet_sent(
                link.link.get(cx.gc),
                request_id,
                SentSheet {
                    link: link.id,
                    document: document_id,
                    url: url.to_string(),
                    blocks_render: !link.blocks_nothing,
                    blocks_scripts,
                },
            );
        });
    }
}

/// The node after `node` in tree order, within `root`'s subtree.
fn next_in_tree_order<'gc>(
    cx: &dom::Cx<'_, 'gc>,
    node: NodeRef<'gc>,
    root: NodeRef<'gc>,
) -> Option<NodeRef<'gc>> {
    if let Some(child) = cx.first_child(node) {
        return Some(child);
    }
    let mut at = node;
    loop {
        if NodeRef::ptr_eq(at, root) {
            return None;
        }
        if let Some(next) = cx.next_sibling(at) {
            return Some(next);
        }
        at = cx.parent(at)?;
    }
}
