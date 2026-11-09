//! `NodeIterator`, `TreeWalker` and `NodeFilter`
//! (https://dom.spec.whatwg.org/#traversal; Blink's `NodeIterator`,
//! `TreeWalker`): DOM's algorithms, each written once over a `Walk`. A
//! traversal made without a filter walks in one operation (`InOp`): only
//! `whatToShow` decides, as Blink's loop does. One with a filter calls it
//! out of any operation (`Between`, `ScriptHost::call_node_filter`), each
//! step its own operation, as the filter may change the tree; what the
//! filter throws is thrown on. `dom` keeps their state and runs the
//! node iterators' pre-removing steps.

use dom::{NodeIterator, NodeRef, NodeRoot, Traversal, TraversalRoot, TreeWalker};
use elise::{Gc, Persistent};

use crate::exception::ExceptionName;
use crate::typed::{CallbackArg, OpFront, ScriptHost};
use crate::{ApiCx, ApiError};

/// `NodeFilter`'s results.
pub(crate) const FILTER_ACCEPT: u16 = 1;
const FILTER_REJECT: u16 = 2;
const FILTER_SKIP: u16 = 3;

/// A node's relative a walk steps to.
#[derive(Clone, Copy)]
enum Relative {
    Parent,
    FirstChild,
    LastChild,
    PreviousSibling,
    NextSibling,
}

/// What DOM's traversal algorithms walk by: nodes (in the operation, or
/// rooted between operations), their relatives, the filter, and the
/// traversal's state.
trait Walk {
    type Node: Clone;

    fn same(&mut self, a: &Self::Node, b: &Self::Node) -> bool;
    fn relative(&mut self, node: &Self::Node, relative: Relative) -> Option<Self::Node>;
    /// DOM's "filter" of the traversal: `FILTER_SKIP` unless `whatToShow`
    /// has the node's type, else the filter's result (`FILTER_ACCEPT`
    /// without one).
    fn filter(&mut self, node: &Self::Node) -> Result<u16, ApiError>;
    fn root(&mut self) -> Self::Node;
    /// A tree walker's current node.
    fn current(&mut self) -> Self::Node;
    fn set_current(&mut self, node: &Self::Node);
    /// A node iterator's reference, and whether its pointer is before it.
    fn position(&mut self) -> (Self::Node, bool);
    fn set_position(&mut self, position: (&Self::Node, bool));
    /// A node iterator's position being filtered, set before the filter
    /// runs (so that removals move it), and where it is after.
    fn filtering(&mut self, position: (&Self::Node, bool));
    fn filtered(&mut self) -> (Self::Node, bool);
}

/// Whether `traversal`'s `whatToShow` has `node`'s type.
fn shown<'gc>(cx: &ApiCx<'_, 'gc>, traversal: Traversal<'gc>, node: NodeRef<'gc>) -> bool {
    // A node's type is 1 to 12: the shift fits.
    let bit = u32::from(cx.node_type(node)).saturating_sub(1);
    cx.traversal_what_to_show(traversal) & (1 << bit) != 0
}

/// A walk in one operation, of a traversal without a filter.
struct InOp<'a, 'b, 'gc> {
    cx: &'a ApiCx<'b, 'gc>,
    traversal: Traversal<'gc>,
    filtering: Option<(NodeRef<'gc>, bool)>,
}

impl<'gc> Walk for InOp<'_, '_, 'gc> {
    type Node = NodeRef<'gc>;

    fn same(&mut self, a: &NodeRef<'gc>, b: &NodeRef<'gc>) -> bool {
        Gc::ptr_eq(*a, *b)
    }

    fn relative(&mut self, node: &NodeRef<'gc>, relative: Relative) -> Option<NodeRef<'gc>> {
        relative_of(self.cx, *node, relative)
    }

    fn filter(&mut self, node: &NodeRef<'gc>) -> Result<u16, ApiError> {
        Ok(if shown(self.cx, self.traversal, *node) {
            FILTER_ACCEPT
        } else {
            FILTER_SKIP
        })
    }

    fn root(&mut self) -> NodeRef<'gc> {
        self.cx.traversal_root(self.traversal)
    }

    fn current(&mut self) -> NodeRef<'gc> {
        self.cx.tree_walker_current(walker_of(self.traversal))
    }

    fn set_current(&mut self, node: &NodeRef<'gc>) {
        self.cx
            .set_tree_walker_current(walker_of(self.traversal), *node);
    }

    fn position(&mut self) -> (NodeRef<'gc>, bool) {
        self.cx.node_iterator_position(iterator_of(self.traversal))
    }

    fn set_position(&mut self, (node, before): (&NodeRef<'gc>, bool)) {
        self.cx
            .set_node_iterator_position(iterator_of(self.traversal), (*node, before));
    }

    fn filtering(&mut self, (node, before): (&NodeRef<'gc>, bool)) {
        // No filter runs: nothing moves it.
        self.filtering = Some((*node, before));
    }

    fn filtered(&mut self) -> (NodeRef<'gc>, bool) {
        self.filtering.take().expect("a position being filtered")
    }
}

fn relative_of<'gc>(
    cx: &ApiCx<'_, 'gc>,
    node: NodeRef<'gc>,
    relative: Relative,
) -> Option<NodeRef<'gc>> {
    match relative {
        Relative::Parent => cx.parent(node),
        Relative::FirstChild => cx.first_child(node),
        Relative::LastChild => cx.last_child(node),
        Relative::PreviousSibling => cx.previous_sibling(node),
        Relative::NextSibling => cx.next_sibling(node),
    }
}

fn walker_of(traversal: Traversal<'_>) -> Gc<'_, TreeWalker> {
    match traversal {
        Traversal::TreeWalker(walker) => walker,
        Traversal::NodeIterator(_) => unreachable!("a tree walker's walk"),
    }
}

fn iterator_of(traversal: Traversal<'_>) -> Gc<'_, NodeIterator> {
    match traversal {
        Traversal::NodeIterator(iterator) => iterator,
        Traversal::TreeWalker(_) => unreachable!("a node iterator's walk"),
    }
}

/// A walk between operations, of a traversal with a filter: each step an
/// operation of its own, the nodes rooted.
struct Between<'h, H> {
    host: &'h mut H,
    traversal: TraversalRoot,
}

impl<H: ScriptHost> Walk for Between<'_, H> {
    type Node = NodeRoot;

    fn same(&mut self, a: &NodeRoot, b: &NodeRoot) -> bool {
        self.host.read(|cx| Gc::ptr_eq(a.get(cx.gc), b.get(cx.gc)))
    }

    fn relative(&mut self, node: &NodeRoot, relative: Relative) -> Option<NodeRoot> {
        self.host
            .read(|cx| relative_of(cx, node.get(cx.gc), relative).map(|node| cx.root(node)))
    }

    fn filter(&mut self, node: &NodeRoot) -> Result<u16, ApiError> {
        let traversal = &self.traversal;
        let (active, shown) = self.host.read(|cx| {
            let traversal = traversal.get(cx.gc);
            (
                cx.traversal_active(traversal),
                shown(cx, traversal, node.get(cx.gc)),
            )
        });
        if active {
            return Err(already_running());
        }
        if !shown {
            return Ok(FILTER_SKIP);
        }
        self.host
            .mutate(|cx| cx.set_traversal_active(traversal.get(cx.gc), true));
        let result = self.host.call_node_filter(traversal, node);
        self.host
            .mutate(|cx| cx.set_traversal_active(traversal.get(cx.gc), false));
        result
    }

    fn root(&mut self) -> NodeRoot {
        let traversal = &self.traversal;
        self.host
            .read(|cx| cx.root(cx.traversal_root(traversal.get(cx.gc))))
    }

    fn current(&mut self) -> NodeRoot {
        let traversal = &self.traversal;
        self.host
            .read(|cx| cx.root(cx.tree_walker_current(walker_of(traversal.get(cx.gc)))))
    }

    fn set_current(&mut self, node: &NodeRoot) {
        let traversal = &self.traversal;
        self.host.mutate(|cx| {
            cx.set_tree_walker_current(walker_of(traversal.get(cx.gc)), node.get(cx.gc));
        });
    }

    fn position(&mut self) -> (NodeRoot, bool) {
        let traversal = &self.traversal;
        self.host.read(|cx| {
            let (node, before) = cx.node_iterator_position(iterator_of(traversal.get(cx.gc)));
            (cx.root(node), before)
        })
    }

    fn set_position(&mut self, (node, before): (&NodeRoot, bool)) {
        let traversal = &self.traversal;
        self.host.mutate(|cx| {
            cx.set_node_iterator_position(
                iterator_of(traversal.get(cx.gc)),
                (node.get(cx.gc), before),
            );
        });
    }

    fn filtering(&mut self, (node, before): (&NodeRoot, bool)) {
        let traversal = &self.traversal;
        self.host.mutate(|cx| {
            cx.set_node_iterator_candidate(
                iterator_of(traversal.get(cx.gc)),
                Some((node.get(cx.gc), before)),
            );
        });
    }

    fn filtered(&mut self) -> (NodeRoot, bool) {
        let traversal = &self.traversal;
        self.host.mutate(|cx| {
            let iterator = iterator_of(traversal.get(cx.gc));
            let (node, before) = cx
                .node_iterator_candidate(iterator)
                .expect("a position being filtered");
            cx.set_node_iterator_candidate(iterator, None);
            (cx.root(node), before)
        })
    }
}

/// A traversal's filter using it.
fn already_running() -> ApiError {
    ApiError::Dom(crate::exception::DomException {
        name: ExceptionName::InvalidState,
        message: "The filter is already running.".into(),
    })
}

/// A traversal's algorithm walked: in one operation without a filter
/// (`in_op`), its result rooted; else between operations (`between`), a
/// filter running already an `InvalidStateError` if `check` (a node
/// iterator's: before its position being filtered is touched, which is
/// the running traversal's).
fn run<H: ScriptHost>(
    host: &mut H,
    traversal: TraversalRoot,
    check: bool,
    in_op: impl for<'a, 'b, 'gc> FnOnce(
        &mut InOp<'a, 'b, 'gc>,
    ) -> Result<Option<NodeRef<'gc>>, ApiError>,
    between: impl FnOnce(&mut Between<'_, H>) -> Result<Option<NodeRoot>, ApiError>,
) -> Result<Option<NodeRoot>, ApiError> {
    let (has_filter, active) = host.read(|cx| {
        let traversal = traversal.get(cx.gc);
        (
            cx.traversal_has_filter(traversal),
            cx.traversal_active(traversal),
        )
    });
    if !has_filter {
        return host.mutate(|cx| {
            let mut walk = InOp {
                cx,
                traversal: traversal.get(cx.gc),
                filtering: None,
            };
            Ok(in_op(&mut walk)?.map(|node| cx.root(node)))
        });
    }
    if check && active {
        return Err(already_running());
    }
    between(&mut Between { host, traversal })
}

/// The first node after `node` in tree order in `root`'s subtree (a node
/// iterator's collection's next).
fn following<W: Walk>(walk: &mut W, node: &W::Node, root: &W::Node) -> Option<W::Node> {
    if let Some(child) = walk.relative(node, Relative::FirstChild) {
        return Some(child);
    }
    let mut at = node.clone();
    loop {
        if walk.same(&at, root) {
            return None;
        }
        if let Some(next) = walk.relative(&at, Relative::NextSibling) {
            return Some(next);
        }
        at = walk.relative(&at, Relative::Parent)?;
    }
}

/// The first node before `node` in tree order in `root`'s subtree.
fn preceding<W: Walk>(walk: &mut W, node: &W::Node, root: &W::Node) -> Option<W::Node> {
    if walk.same(node, root) {
        return None;
    }
    match walk.relative(node, Relative::PreviousSibling) {
        // Its last inclusive descendant.
        Some(mut previous) => {
            while let Some(last) = walk.relative(&previous, Relative::LastChild) {
                previous = last;
            }
            Some(previous)
        }
        None => walk.relative(node, Relative::Parent),
    }
}

// --- NodeIterator ------------------------------------------------------------

/// DOM's "traverse" of a node iterator (`next` or previous). The position
/// being filtered is the iterator's while the filter runs, so that
/// removing nodes then moves it: the walk goes on from where it was moved
/// to, and an accepted node's reference is there too, though the node
/// returned is the one filtered (WPT's
/// `NodeIterator-removal-during-filtering`; Blink and WebKit leave the
/// position on the removed node).
fn traverse<W: Walk>(walk: &mut W, next: bool) -> Result<Option<W::Node>, ApiError> {
    let root = walk.root();
    let (mut node, mut before) = walk.position();
    loop {
        if next {
            if before {
                before = false;
            } else {
                let Some(following) = following(walk, &node, &root) else {
                    return Ok(None);
                };
                node = following;
            }
        } else if before {
            let Some(preceding) = preceding(walk, &node, &root) else {
                return Ok(None);
            };
            node = preceding;
        } else {
            before = true;
        }
        walk.filtering((&node, before));
        let result = walk.filter(&node);
        let (at, at_before) = walk.filtered();
        if result? == FILTER_ACCEPT {
            walk.set_position((&at, at_before));
            return Ok(Some(node));
        }
        (node, before) = (at, at_before);
    }
}

pub(crate) fn node_iterator_root<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, NodeIterator>,
) -> Result<NodeRef<'gc>, ApiError> {
    Ok(cx.traversal_root(Traversal::NodeIterator(this)))
}

pub(crate) fn reference_node<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, NodeIterator>,
) -> Result<NodeRef<'gc>, ApiError> {
    Ok(cx.node_iterator_position(this).0)
}

pub(crate) fn pointer_before_reference_node<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, NodeIterator>,
) -> Result<bool, ApiError> {
    Ok(cx.node_iterator_position(this).1)
}

pub(crate) fn node_iterator_what_to_show<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, NodeIterator>,
) -> Result<u32, ApiError> {
    Ok(cx.traversal_what_to_show(Traversal::NodeIterator(this)))
}

/// `filter`: the object it was made with (null for none).
pub(crate) fn node_iterator_filter<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, NodeIterator>,
) -> Result<&'gc elise::JsEdge, ApiError> {
    Ok(cx.traversal_filter_edge(Traversal::NodeIterator(this)))
}

/// `nextNode()`.
pub(crate) fn next_node<H: ScriptHost>(
    host: &mut H,
    this: Persistent<NodeIterator>,
) -> Result<Option<NodeRoot>, ApiError> {
    run(
        host,
        TraversalRoot::NodeIterator(this),
        true,
        |walk| traverse(walk, true),
        |walk| traverse(walk, true),
    )
}

/// `previousNode()`.
pub(crate) fn previous_node<H: ScriptHost>(
    host: &mut H,
    this: Persistent<NodeIterator>,
) -> Result<Option<NodeRoot>, ApiError> {
    run(
        host,
        TraversalRoot::NodeIterator(this),
        true,
        |walk| traverse(walk, false),
        |walk| traverse(walk, false),
    )
}

/// `detach()`: nothing (DOM keeps it for compatibility).
pub(crate) fn detach<'gc>(_: &ApiCx<'_, 'gc>, _: Gc<'gc, NodeIterator>) -> Result<(), ApiError> {
    Ok(())
}

// --- TreeWalker --------------------------------------------------------------

pub(crate) fn tree_walker_root<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, TreeWalker>,
) -> Result<NodeRef<'gc>, ApiError> {
    Ok(cx.traversal_root(Traversal::TreeWalker(this)))
}

pub(crate) fn tree_walker_what_to_show<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, TreeWalker>,
) -> Result<u32, ApiError> {
    Ok(cx.traversal_what_to_show(Traversal::TreeWalker(this)))
}

pub(crate) fn tree_walker_filter<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, TreeWalker>,
) -> Result<&'gc elise::JsEdge, ApiError> {
    Ok(cx.traversal_filter_edge(Traversal::TreeWalker(this)))
}

pub(crate) fn current_node<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, TreeWalker>,
) -> Result<NodeRef<'gc>, ApiError> {
    Ok(cx.tree_walker_current(this))
}

pub(crate) fn set_current_node<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, TreeWalker>,
    node: NodeRef<'gc>,
) -> Result<(), ApiError> {
    cx.set_tree_walker_current(this, node);
    Ok(())
}

/// `node` made the walker's current node, and given.
fn accept<W: Walk>(walk: &mut W, node: W::Node) -> Result<Option<W::Node>, ApiError> {
    walk.set_current(&node);
    Ok(Some(node))
}

/// `parentNode()`.
fn parent_node_of<W: Walk>(walk: &mut W) -> Result<Option<W::Node>, ApiError> {
    let root = walk.root();
    let mut node = walk.current();
    while !walk.same(&node, &root) {
        let Some(parent) = walk.relative(&node, Relative::Parent) else {
            break;
        };
        node = parent;
        if walk.filter(&node)? == FILTER_ACCEPT {
            return accept(walk, node);
        }
    }
    Ok(None)
}

/// DOM's "traverse children" (`first`, or last).
fn traverse_children<W: Walk>(walk: &mut W, first: bool) -> Result<Option<W::Node>, ApiError> {
    let (child, sibling) = if first {
        (Relative::FirstChild, Relative::NextSibling)
    } else {
        (Relative::LastChild, Relative::PreviousSibling)
    };
    let root = walk.root();
    let current = walk.current();
    let mut node = walk.relative(&current, child);
    while let Some(at) = node {
        let result = walk.filter(&at)?;
        if result == FILTER_ACCEPT {
            return accept(walk, at);
        }
        if result == FILTER_SKIP
            && let Some(grandchild) = walk.relative(&at, child)
        {
            node = Some(grandchild);
            continue;
        }
        let mut up = at;
        node = loop {
            if let Some(next) = walk.relative(&up, sibling) {
                break Some(next);
            }
            match walk.relative(&up, Relative::Parent) {
                Some(parent) if !walk.same(&parent, &root) && !walk.same(&parent, &current) => {
                    up = parent
                }
                _ => return Ok(None),
            }
        };
    }
    Ok(None)
}

/// DOM's "traverse siblings" (`next`, or previous).
fn traverse_siblings<W: Walk>(walk: &mut W, next: bool) -> Result<Option<W::Node>, ApiError> {
    let (sibling_of, child_of) = if next {
        (Relative::NextSibling, Relative::FirstChild)
    } else {
        (Relative::PreviousSibling, Relative::LastChild)
    };
    let root = walk.root();
    let mut node = walk.current();
    if walk.same(&node, &root) {
        return Ok(None);
    }
    loop {
        let mut sibling = walk.relative(&node, sibling_of);
        while let Some(at) = sibling {
            node = at;
            let result = walk.filter(&node)?;
            if result == FILTER_ACCEPT {
                return accept(walk, node);
            }
            sibling = walk.relative(&node, child_of);
            if result == FILTER_REJECT || sibling.is_none() {
                sibling = walk.relative(&node, sibling_of);
            }
        }
        node = match walk.relative(&node, Relative::Parent) {
            Some(parent) if !walk.same(&parent, &root) => parent,
            _ => return Ok(None),
        };
        if walk.filter(&node)? == FILTER_ACCEPT {
            return Ok(None);
        }
    }
}

/// `previousNode()`.
fn previous_node_of<W: Walk>(walk: &mut W) -> Result<Option<W::Node>, ApiError> {
    let root = walk.root();
    let mut node = walk.current();
    while !walk.same(&node, &root) {
        let mut sibling = walk.relative(&node, Relative::PreviousSibling);
        while let Some(at) = sibling {
            node = at;
            let mut result = walk.filter(&node)?;
            while result != FILTER_REJECT
                && let Some(last) = walk.relative(&node, Relative::LastChild)
            {
                node = last;
                result = walk.filter(&node)?;
            }
            if result == FILTER_ACCEPT {
                return accept(walk, node);
            }
            sibling = walk.relative(&node, Relative::PreviousSibling);
        }
        if walk.same(&node, &root) {
            return Ok(None);
        }
        let Some(parent) = walk.relative(&node, Relative::Parent) else {
            return Ok(None);
        };
        node = parent;
        if walk.filter(&node)? == FILTER_ACCEPT {
            return accept(walk, node);
        }
    }
    Ok(None)
}

/// `nextNode()`.
fn next_node_of<W: Walk>(walk: &mut W) -> Result<Option<W::Node>, ApiError> {
    let root = walk.root();
    let mut node = walk.current();
    let mut result = FILTER_ACCEPT;
    loop {
        while result != FILTER_REJECT
            && let Some(child) = walk.relative(&node, Relative::FirstChild)
        {
            node = child;
            result = walk.filter(&node)?;
            if result == FILTER_ACCEPT {
                return accept(walk, node);
            }
        }
        // The next sibling of it or of an ancestor, not past the root.
        let mut at = Some(node.clone());
        let mut sibling = None;
        while let Some(up) = at {
            if walk.same(&up, &root) {
                return Ok(None);
            }
            sibling = walk.relative(&up, Relative::NextSibling);
            if sibling.is_some() {
                break;
            }
            at = walk.relative(&up, Relative::Parent);
        }
        let Some(sibling) = sibling else {
            return Ok(None);
        };
        node = sibling;
        result = walk.filter(&node)?;
        if result == FILTER_ACCEPT {
            return accept(walk, node);
        }
    }
}

/// A tree walker's method: `algorithm` run.
macro_rules! walker_method {
    ($(#[$doc:meta])* $name:ident, $algorithm:expr) => {
        $(#[$doc])*
        pub(crate) fn $name<H: ScriptHost>(
            host: &mut H,
            this: Persistent<TreeWalker>,
        ) -> Result<Option<NodeRoot>, ApiError> {
            run(host, TraversalRoot::TreeWalker(this), false, |walk| $algorithm(walk), |walk| {
                $algorithm(walk)
            })
        }
    };
}

walker_method!(
    /// `parentNode()`.
    parent_node,
    parent_node_of
);
walker_method!(
    /// `firstChild()`.
    first_child,
    |walk| traverse_children(walk, true)
);
walker_method!(
    /// `lastChild()`.
    last_child,
    |walk| traverse_children(walk, false)
);
walker_method!(
    /// `previousSibling()`.
    previous_sibling,
    |walk| traverse_siblings(walk, false)
);
walker_method!(
    /// `nextSibling()`.
    next_sibling,
    |walk| traverse_siblings(walk, true)
);
walker_method!(
    /// `previousNode()`.
    walker_previous_node,
    previous_node_of
);
walker_method!(
    /// `nextNode()`.
    walker_next_node,
    next_node_of
);

// --- Construction ------------------------------------------------------------

/// `createNodeIterator(root, whatToShow, filter)`.
pub(crate) fn create_node_iterator<'gc, O: OpFront>(
    op: &mut O,
    cx: &ApiCx<'_, 'gc>,
    _: NodeRef<'gc>,
    root: NodeRef<'gc>,
    what_to_show: u32,
    filter: Option<CallbackArg>,
) -> Result<Gc<'gc, NodeIterator>, ApiError> {
    let iterator = cx.create_node_iterator(root, what_to_show, filter.is_some());
    if let Some(filter) = filter {
        op.store_callback(
            cx,
            cx.traversal_filter_edge(Traversal::NodeIterator(iterator)),
            filter,
        );
    }
    Ok(iterator)
}

/// `createTreeWalker(root, whatToShow, filter)`.
pub(crate) fn create_tree_walker<'gc, O: OpFront>(
    op: &mut O,
    cx: &ApiCx<'_, 'gc>,
    _: NodeRef<'gc>,
    root: NodeRef<'gc>,
    what_to_show: u32,
    filter: Option<CallbackArg>,
) -> Result<Gc<'gc, TreeWalker>, ApiError> {
    let walker = cx.create_tree_walker(root, what_to_show, filter.is_some());
    if let Some(filter) = filter {
        op.store_callback(
            cx,
            cx.traversal_filter_edge(Traversal::TreeWalker(walker)),
            filter,
        );
    }
    Ok(walker)
}
