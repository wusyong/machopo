//! The `<link>`s' sheets asked for (`send_sheet_fetches`): each of a
//! frame's document's tree and its shadow trees once, through the load
//! sink, its URL against the document's; what each blocks as the DOM keeps
//! it (the parser's of media that match blocks its scripts, one not
//! alternate and not a shadow tree's its rendering); an `href` that
//! doesn't parse, or a request that went nowhere, noted as failed.

use std::any::Any;
use std::sync::Arc;

use computed_style::ComputedStyle;
use css::media::MediaEnvironment;
use dom::{
    Cx, Dom, NodeId, NodeRef, NodeRoot, QualName, ScriptCors, ScriptFlags, ShadowRootInit,
    ShadowRootMode, SlotAssignmentMode,
};
use dom_api::{FetchOrigin, FrameId, FrameLayout, LoadSink};
use gfx::{ElementBox, PointF, RectF, Viewport};

/// A sink that keeps what was asked: each sheet's URL and CORS settings,
/// and the request's origin; a request that goes nowhere for `refused`.
#[derive(Default)]
struct Sink {
    sheets: Vec<(String, Option<ScriptCors>, Option<String>)>,
    refused: Option<String>,
    next: u64,
}

impl LoadSink for Sink {
    fn start_fetch(&mut self, _: FrameId, _: &dom::FetchRequest, _: &FetchOrigin) -> Option<u64> {
        None
    }

    fn cancel_fetch(&mut self, _: u64) {}

    fn start_sheet_fetch(
        &mut self,
        _: FrameId,
        _: NodeId,
        url: &str,
        cors: Option<ScriptCors>,
        request_origin: Option<&str>,
        _: &FetchOrigin,
    ) -> Option<u64> {
        if self.refused.as_deref() == Some(url) {
            return None;
        }
        self.sheets
            .push((url.to_owned(), cors, request_origin.map(String::from)));
        self.next += 1;
        Some(self.next)
    }

    fn register_blob_url(&self, _: FrameId, _: u64, _: &str, _: &dom::BlobData) {}

    fn revoke_blob_url(&self, _: &str) {}

    fn frame_gone(&self, _: FrameId) {}
}

/// A layout whose frames' media see a screen `width` wide.
struct Layout {
    width: f32,
}

impl FrameLayout for Layout {
    fn shows(&mut self, _: NodeId) -> bool {
        true
    }

    fn update_style_and_layout(&mut self, _: &Cx<'_, '_>) {}

    fn element_box(&self, _: NodeId) -> Option<ElementBox> {
        None
    }

    fn client_rects(&self, _: NodeId) -> Vec<RectF> {
        Vec::new()
    }

    // Of the layout, only the media are asked of it.
    fn viewport(&self) -> Viewport {
        unreachable!("not asked")
    }

    fn computed_style<'gc>(&self, _: &Cx<'_, 'gc>, _: NodeRef<'gc>) -> Option<Arc<ComputedStyle>> {
        None
    }

    fn window_screen_rect(&self) -> Option<RectF> {
        None
    }

    fn element_at(&mut self, _: PointF) -> Option<NodeId> {
        None
    }

    fn elements_at(&mut self, _: PointF) -> Vec<NodeId> {
        Vec::new()
    }

    fn media_environment(&self, _: NodeId) -> Option<MediaEnvironment> {
        Some(MediaEnvironment {
            width: self.width,
            height: 600.0,
            device_pixel_ratio: 1.0,
        })
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// A `<link>` in `parent` with `attributes`, the parser's if
/// `parser_inserted`.
fn link<'gc>(
    cx: &Cx<'_, 'gc>,
    parent: NodeRef<'gc>,
    attributes: &[(&str, &str)],
    parser_inserted: bool,
) -> NodeRef<'gc> {
    let link = cx.create_element(cx.document_of(parent), QualName::html("link"));
    for (name, value) in attributes {
        cx.set_attribute(link, name, value);
    }
    cx.set_script_flags(
        link,
        ScriptFlags {
            parser_inserted,
            ..ScriptFlags::default()
        },
    );
    cx.append_child(parent, link).unwrap();
    link
}

/// A document at `https://example.test/dir/` with a `<head>`: the
/// document, rooted.
fn page(dom: &mut Dom) -> NodeRoot {
    dom.mutate(|cx| {
        let document = cx.create_document(true);
        cx.create_window(document);
        cx.set_document_url(document, "https://example.test/dir/");
        let html = cx.create_element(document, QualName::html("html"));
        cx.append_child(document, html).unwrap();
        let head = cx.create_element(document, QualName::html("head"));
        cx.append_child(html, head).unwrap();
        cx.root(document)
    })
}

fn head<'gc>(cx: &Cx<'_, 'gc>, document: &NodeRoot) -> NodeRef<'gc> {
    let html = cx.first_child(document.get(cx.gc)).unwrap();
    cx.first_child(html).unwrap()
}

/// The DOM's record of the sheet sent for `url`: whether it blocks
/// rendering and scripts.
fn blocks(dom: &Dom, request_id: u64) -> (bool, bool) {
    let sent = dom.sent_sheet(request_id).expect("sent");
    (sent.blocks_render, sent.blocks_scripts)
}

#[test]
fn each_links_sheet_is_asked_for_once_blocking_as_it_is() {
    let mut dom = Dom::new();
    let document = page(&mut dom);
    dom.mutate(|cx| {
        let head = head(cx, &document);
        link(cx, head, &[("rel", "stylesheet"), ("href", "a.css")], true);
        link(
            cx,
            head,
            &[("rel", "alternate stylesheet"), ("href", "b.css")],
            true,
        );
        link(
            cx,
            head,
            &[("rel", "stylesheet"), ("href", "c.css"), ("media", "print")],
            true,
        );
        link(
            cx,
            head,
            &[
                ("rel", "stylesheet"),
                ("href", "d.css"),
                ("crossorigin", "use-credentials"),
            ],
            false,
        );
        link(
            cx,
            head,
            &[("rel", "stylesheet"), ("href", "http://[")],
            true,
        );
        link(cx, head, &[("rel", "icon"), ("href", "e.png")], true);
        let host = cx.create_element(cx.document_of(head), QualName::html("div"));
        cx.append_child(head, host).unwrap();
        let shadow = cx.attach_shadow(
            host,
            ShadowRootInit {
                mode: ShadowRootMode::Open,
                delegates_focus: false,
                slot_assignment: SlotAssignmentMode::Named,
                clonable: false,
                serializable: false,
            },
        );
        link(
            cx,
            shadow,
            &[
                ("rel", "stylesheet"),
                ("href", "f.css"),
                ("crossorigin", ""),
            ],
            true,
        );
    });
    let mut sink = Sink::default();
    let mut layout = Layout { width: 800.0 };
    let id = dom.read(|cx| cx.node_id(document.get(cx.gc)));
    dom_api::send_sheet_fetches(
        &mut dom,
        Some(&mut layout),
        &mut sink,
        FrameId(1),
        None,
        &document,
    );
    let origin = Some("https://example.test".to_owned());
    assert_eq!(
        sink.sheets,
        [
            (
                "https://example.test/dir/a.css".to_owned(),
                None,
                origin.clone()
            ),
            (
                "https://example.test/dir/b.css".to_owned(),
                None,
                origin.clone()
            ),
            (
                "https://example.test/dir/c.css".to_owned(),
                None,
                origin.clone()
            ),
            (
                "https://example.test/dir/d.css".to_owned(),
                Some(ScriptCors::UseCredentials),
                origin.clone()
            ),
            (
                "https://example.test/dir/f.css".to_owned(),
                Some(ScriptCors::Anonymous),
                origin
            ),
        ]
    );
    // The parser's of media that match blocks its scripts; an alternate
    // one and a shadow tree's block nothing.
    assert_eq!(blocks(&dom, 1), (true, true));
    assert_eq!(blocks(&dom, 2), (false, false));
    assert_eq!(blocks(&dom, 3), (true, false), "print doesn't match");
    assert_eq!(blocks(&dom, 4), (true, false), "script's");
    assert_eq!(blocks(&dom, 5), (false, false), "a shadow tree's");
    assert_eq!(dom.sent_sheets_of(id), 5);
    dom.mutate(|cx| {
        let document = document.get(cx.gc);
        assert_eq!(cx.take_failed_loads(document), ["http://["]);
        assert!(cx.has_load_requests(document));
    });

    // Asked for once: none again, but a new one.
    dom.mutate(|cx| {
        let head = head(cx, &document);
        link(cx, head, &[("rel", "stylesheet"), ("href", "g.css")], false);
    });
    dom_api::send_sheet_fetches(
        &mut dom,
        Some(&mut layout),
        &mut sink,
        FrameId(1),
        None,
        &document,
    );
    assert_eq!(sink.sheets.len(), 6);
    assert_eq!(sink.sheets[5].0, "https://example.test/dir/g.css");
    assert!(dom.read(|cx| cx.take_failed_loads(document.get(cx.gc)).is_empty()));
}

#[test]
fn media_match_as_the_frame_sees_them() {
    let mut dom = Dom::new();
    let document = page(&mut dom);
    dom.mutate(|cx| {
        let head = head(cx, &document);
        let wide = [
            ("rel", "stylesheet"),
            ("href", "wide.css"),
            ("media", "(min-width: 1000px)"),
        ];
        link(cx, head, &wide, true);
    });
    let mut sink = Sink::default();
    let mut layout = Layout { width: 800.0 };
    dom_api::send_sheet_fetches(
        &mut dom,
        Some(&mut layout),
        &mut sink,
        FrameId(1),
        None,
        &document,
    );
    assert_eq!(blocks(&dom, 1), (true, false), "too narrow");
    dom.mutate(|cx| {
        let head = head(cx, &document);
        let wide = [
            ("rel", "stylesheet"),
            ("href", "wide2.css"),
            ("media", "(min-width: 1000px)"),
        ];
        link(cx, head, &wide, true);
    });
    let mut wider = Layout { width: 1200.0 };
    dom_api::send_sheet_fetches(
        &mut dom,
        Some(&mut wider),
        &mut sink,
        FrameId(1),
        None,
        &document,
    );
    assert_eq!(blocks(&dom, 2), (true, true), "wide enough");
    // No layout to ask (a frame given no size yet): it matches.
    dom.mutate(|cx| {
        let head = head(cx, &document);
        let print = [
            ("rel", "stylesheet"),
            ("href", "print.css"),
            ("media", "print"),
        ];
        link(cx, head, &print, true);
    });
    dom_api::send_sheet_fetches(&mut dom, None, &mut sink, FrameId(1), None, &document);
    assert_eq!(blocks(&dom, 3), (true, true));
}

#[test]
fn a_request_that_went_nowhere_is_noted_failed() {
    let mut dom = Dom::new();
    let document = page(&mut dom);
    dom.mutate(|cx| {
        let head = head(cx, &document);
        link(cx, head, &[("rel", "stylesheet"), ("href", "a.css")], true);
    });
    let mut sink = Sink {
        refused: Some("https://example.test/dir/a.css".to_owned()),
        ..Sink::default()
    };
    dom_api::send_sheet_fetches(&mut dom, None, &mut sink, FrameId(1), None, &document);
    assert!(sink.sheets.is_empty());
    dom.mutate(|cx| {
        let document = document.get(cx.gc);
        assert_eq!(
            cx.take_failed_loads(document),
            ["https://example.test/dir/a.css"]
        );
        assert!(!cx.has_load_requests(document));
        assert!(!cx.sheets_block_scripts(document));
    });
}
