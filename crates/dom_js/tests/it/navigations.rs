//! A frame client that keeps the navigations script asked for, as the
//! host's is told them (`FrameClient::navigate`, `navigate_child`): what
//! tests of what script navigates watch.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use dom::FrameNavigation;
use dom_api::{ChildNavigation, FrameId, FrameToken, ScriptEngine};

/// A navigation script asked for: the frame navigated, the one whose
/// script asked (an iframe's: its parent), and what.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Navigated {
    pub(crate) frame: FrameId,
    pub(crate) initiator: FrameId,
    pub(crate) navigation: Navigation,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Navigation {
    /// Script's (`location`, `history`, a link).
    Frame(FrameNavigation),
    /// An iframe's (`src`, `srcdoc`).
    Child(ChildNavigation),
}

type Told = (Option<FrameToken>, Option<FrameToken>, Navigation);

/// The client, sending nothing (or, `numbering`, numbering each
/// navigation to another document or reload as a host sending it would:
/// 1, 2...); each navigation told, by its frames' tokens (none: the
/// top-level frame's, which has none in these tests).
#[derive(Clone, Default)]
pub(crate) struct Navigations {
    told: Rc<RefCell<Vec<Told>>>,
    sent: Option<Rc<Cell<u64>>>,
}

impl Navigations {
    /// `engine`'s frame client from now on.
    pub(crate) fn watch(engine: &mut dom_js::V8Engine) -> Self {
        let navigations = Self::default();
        engine.set_frame_client(Box::new(navigations.clone()));
        navigations
    }

    /// `engine`'s frame client from now on, numbering what it sends.
    pub(crate) fn numbering(engine: &mut dom_js::V8Engine) -> Self {
        let navigations = Self {
            sent: Some(Rc::default()),
            ..Self::default()
        };
        engine.set_frame_client(Box::new(navigations.clone()));
        navigations
    }

    /// The next number, if it numbers them.
    fn send(&self) -> Option<u64> {
        let sent = self.sent.as_ref()?;
        sent.set(sent.get() + 1);
        Some(sent.get())
    }

    /// Those told since, taken.
    pub(crate) fn take(&self, engine: &mut dom_js::V8Engine) -> Vec<Navigated> {
        let top = engine.top_frame();
        let mut frame = |token: Option<FrameToken>| {
            token
                .and_then(|token| engine.frame_of_token(token))
                .unwrap_or(top)
        };
        std::mem::take(&mut *self.told.borrow_mut())
            .into_iter()
            .map(|(navigated, initiator, navigation)| Navigated {
                frame: frame(navigated),
                initiator: frame(initiator),
                navigation,
            })
            .collect()
    }
}

impl dom_api::FrameClient for Navigations {
    fn child_frame_created(&mut self, _: &dom_api::ChildFrameMade<'_>, _: &dom::Dom) {}

    fn child_document_created(&mut self, _: FrameToken, _: dom::NodeId) {}

    fn frame_detached(&mut self, _: FrameToken) {}

    fn navigate(&mut self, asked: &dom_api::NavigationAsked<'_>, _: &dom::Dom) -> Option<u64> {
        self.told.borrow_mut().push((
            asked.frame,
            asked.initiator,
            Navigation::Frame(asked.navigation.clone()),
        ));
        match asked.navigation {
            FrameNavigation::To { .. } | FrameNavigation::Reload => self.send(),
            _ => None,
        }
    }

    fn navigate_child(
        &mut self,
        asked: &dom_api::ChildNavigationAsked<'_>,
        _: &dom::Dom,
    ) -> Option<u64> {
        self.told.borrow_mut().push((
            asked.frame,
            asked.parent,
            Navigation::Child(asked.navigation.clone()),
        ));
        self.send()
    }
}
