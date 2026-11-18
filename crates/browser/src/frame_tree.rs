//! A tab's frames as the browser knows them (Chromium's `FrameTree` and
//! its `FrameTreeNode`s): the main frame, and each child frame its
//! renderer told of, with its parent and children, by token. The renderer
//! makes a child frame on its own (script reads its window next) and tells
//! of it after (`RendererHostMsg::DidCreateChildFrame`); a frame goes only
//! as the renderer tells (`DidDetachFrame`, the frames in it with it), or
//! with the renderer itself. What the renderer tells is checked: a message
//! the tree can't take is a bad message (`BadMessageReason`).

use std::collections::HashMap;

use protocol::FrameToken;

use crate::bad_message::BadMessageReason;

/// A frame of the tree: its parent (none for the main frame) and its
/// child frames, in the order they were made.
#[derive(Debug)]
struct FrameTreeNode {
    parent: Option<FrameToken>,
    children: Vec<FrameToken>,
}

/// The tree of a tab's frames, the main frame its root.
#[derive(Debug)]
pub struct FrameTree {
    root: FrameToken,
    nodes: HashMap<FrameToken, FrameTreeNode>,
}

impl FrameTree {
    /// A tree of the main frame alone, whose token is `root`.
    pub fn new(root: FrameToken) -> Self {
        let mut tree = Self {
            root,
            nodes: HashMap::new(),
        };
        tree.reset();
        tree
    }

    /// The main frame's token.
    pub fn root(&self) -> FrameToken {
        self.root
    }

    /// Whether `frame` is one of the tree's.
    pub fn contains(&self, frame: FrameToken) -> bool {
        self.nodes.contains_key(&frame)
    }

    /// `frame`'s parent: none for the main frame, or a frame not in the
    /// tree.
    pub fn parent(&self, frame: FrameToken) -> Option<FrameToken> {
        self.nodes.get(&frame)?.parent
    }

    /// `frame`'s child frames, in the order they were made.
    pub fn children(&self, frame: FrameToken) -> &[FrameToken] {
        self.nodes
            .get(&frame)
            .map_or(&[], |node| node.children.as_slice())
    }

    /// How many frames there are, the main frame's included.
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// Never: the main frame is always there.
    pub fn is_empty(&self) -> bool {
        false
    }

    /// `frame` was made, the last child of `parent`
    /// (`FrameTreeNode::AddChild`). A parent the tree doesn't have, or a
    /// token one of its frames has already, is a bad message.
    pub fn add_child(
        &mut self,
        parent: FrameToken,
        frame: FrameToken,
    ) -> Result<(), BadMessageReason> {
        if self.nodes.contains_key(&frame) {
            return Err(BadMessageReason::FrameTokenInUse);
        }
        let Some(node) = self.nodes.get_mut(&parent) else {
            return Err(BadMessageReason::FrameParentUnknown);
        };
        node.children.push(frame);
        self.nodes.insert(
            frame,
            FrameTreeNode {
                parent: Some(parent),
                children: Vec::new(),
            },
        );
        Ok(())
    }

    /// `frame` has gone, and the frames in it with it
    /// (`RenderFrameHostImpl::Detach`). A frame the tree doesn't have, or
    /// the main frame (Chromium's `RFH_DETACH_MAIN_FRAME`), is a bad
    /// message.
    pub fn remove(&mut self, frame: FrameToken) -> Result<(), BadMessageReason> {
        let Some(node) = self.nodes.get(&frame) else {
            return Err(BadMessageReason::DetachUnknownFrame);
        };
        let Some(parent) = node.parent else {
            return Err(BadMessageReason::DetachMainFrame);
        };
        if let Some(parent) = self.nodes.get_mut(&parent) {
            parent.children.retain(|child| *child != frame);
        }
        let mut gone = vec![frame];
        while let Some(at) = gone.pop() {
            if let Some(node) = self.nodes.remove(&at) {
                gone.extend(node.children);
            }
        }
        Ok(())
    }

    /// The main frame alone again: its renderer has gone, or another took
    /// its place (`RenderFrameHostImpl::ResetChildren` as the process is
    /// gone).
    pub fn reset(&mut self) {
        self.nodes.clear();
        self.nodes.insert(
            self.root,
            FrameTreeNode {
                parent: None,
                children: Vec::new(),
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn token(n: u8) -> FrameToken {
        FrameToken([n; 16])
    }

    /// Child frames hang from the frame they were made in, in the order
    /// they were made; a frame removed takes the frames in it with it.
    #[test]
    fn frames_come_and_go_with_the_frames_in_them() {
        let mut tree = FrameTree::new(token(0));
        assert_eq!(tree.root(), token(0));
        assert_eq!(tree.len(), 1);
        tree.add_child(token(0), token(1)).unwrap();
        tree.add_child(token(0), token(2)).unwrap();
        tree.add_child(token(1), token(3)).unwrap();
        tree.add_child(token(3), token(4)).unwrap();
        assert_eq!(tree.children(token(0)), [token(1), token(2)]);
        assert_eq!(tree.children(token(1)), [token(3)]);
        assert_eq!(tree.parent(token(4)), Some(token(3)));
        assert_eq!(tree.parent(token(0)), None);
        assert_eq!(tree.len(), 5);
        tree.remove(token(1)).unwrap();
        assert_eq!(tree.children(token(0)), [token(2)]);
        for gone in [token(1), token(3), token(4)] {
            assert!(!tree.contains(gone), "{gone:?}");
        }
        assert_eq!(tree.len(), 2);
        // Gone, its token may be made again.
        tree.add_child(token(2), token(1)).unwrap();
        assert_eq!(tree.parent(token(1)), Some(token(2)));
        tree.reset();
        assert_eq!(tree.len(), 1);
        assert!(tree.children(token(0)).is_empty());
        assert!(!tree.contains(token(2)));
    }

    /// What the tree can't take: a parent it doesn't have, a token in use
    /// (the main frame's too), a frame it doesn't have gone, the main
    /// frame gone. The tree stays as it was.
    #[test]
    fn what_the_tree_cant_take_is_a_bad_message() {
        let mut tree = FrameTree::new(token(0));
        tree.add_child(token(0), token(1)).unwrap();
        assert_eq!(
            tree.add_child(token(9), token(2)),
            Err(BadMessageReason::FrameParentUnknown)
        );
        assert_eq!(
            tree.add_child(token(0), token(1)),
            Err(BadMessageReason::FrameTokenInUse)
        );
        assert_eq!(
            tree.add_child(token(1), token(0)),
            Err(BadMessageReason::FrameTokenInUse)
        );
        assert_eq!(
            tree.remove(token(9)),
            Err(BadMessageReason::DetachUnknownFrame)
        );
        assert_eq!(
            tree.remove(token(0)),
            Err(BadMessageReason::DetachMainFrame)
        );
        assert_eq!(tree.len(), 2);
        assert_eq!(tree.children(token(0)), [token(1)]);
        assert!(tree.children(token(1)).is_empty());
    }
}
