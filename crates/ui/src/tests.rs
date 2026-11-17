//! The tree's behavior, with small views that record what they hear.

use compositor_paint::{DisplayItemList, PaintFlags, PaintOp};
use gfx::{Color, PointF, RectF, SizeF};
use taffy::prelude::{auto, length};
use taffy::{AlignItems, AvailableSpace, FlexDirection, Size, Style};

use crate::{
    AnyViewId, App, Cursor, EventCtx, Handled, Key, KeyEvent, MeasureCtx, Modifiers, PaintCtx,
    PointerButton, PointerEvent, TextContexts, View, ViewId, ViewTree,
};

struct TestApp;

impl App for TestApp {
    type Message = String;
    /// The color views paint with when they have none of their own.
    type Theme = Color;
}

/// A leaf of a fixed size that fills itself with a color, and records the
/// events it hears.
struct Swatch {
    name: &'static str,
    size: SizeF,
    color: Option<Color>,
    /// Whether it handles the pointer and keys.
    handles: bool,
    paints: u32,
    heard: Vec<String>,
}

impl Swatch {
    fn new(name: &'static str, width: f32, height: f32) -> Self {
        Self {
            name,
            size: SizeF::new(width, height),
            color: None,
            handles: true,
            paints: 0,
            heard: Vec::new(),
        }
    }

    fn passive(mut self) -> Self {
        self.handles = false;
        self
    }

    /// A new color, asking to be painted again as a view should.
    fn set_color(&mut self, color: Color, ctx: &mut EventCtx<'_, TestApp>) {
        if self.color != Some(color) {
            self.color = Some(color);
            ctx.request_paint();
        }
    }

    fn handled(&self) -> Handled {
        if self.handles {
            Handled::Yes
        } else {
            Handled::No
        }
    }
}

impl View<TestApp> for Swatch {
    fn measure(
        &mut self,
        _: &mut MeasureCtx<'_, TestApp>,
        _: Size<Option<f32>>,
        _: Size<AvailableSpace>,
    ) -> SizeF {
        self.size
    }

    fn paint(&mut self, ctx: &mut PaintCtx<'_, TestApp>, list: &mut DisplayItemList) {
        self.paints += 1;
        let color = self.color.unwrap_or(*ctx.theme());
        list.push_op(PaintOp::DrawRect {
            rect: RectF::from_size(ctx.size()),
            flags: PaintFlags::fill(color),
        });
    }

    fn on_pointer(&mut self, ctx: &mut EventCtx<'_, TestApp>, event: &PointerEvent) -> Handled {
        let p = event.position();
        let what = match event {
            PointerEvent::Move { .. } => "move",
            PointerEvent::Down { .. } => "down",
            PointerEvent::Up { .. } => "up",
        };
        self.heard.push(format!("{what} {} {}", p.x, p.y));
        if self.handles && matches!(event, PointerEvent::Down { .. }) {
            ctx.submit(format!("{} pressed", self.name));
            ctx.request_focus();
        }
        self.handled()
    }

    fn on_hover(&mut self, ctx: &mut EventCtx<'_, TestApp>, hovered: bool) {
        self.heard.push(format!("hover {hovered}"));
        // In the order every view hears it.
        ctx.submit(format!("{} hover {hovered}", self.name));
    }

    fn on_key(&mut self, _: &mut EventCtx<'_, TestApp>, event: &KeyEvent) -> Handled {
        self.heard.push(format!("key {:?}", event.key));
        self.handled()
    }

    fn on_focus(&mut self, _: &mut EventCtx<'_, TestApp>, focused: bool) {
        self.heard.push(format!("focus {focused}"));
    }

    fn cursor(&self, _: PointF) -> Cursor {
        Cursor::Text
    }
}

/// A tree 200 × 100, not checking its recordings on every paint: the
/// tests count paints, and check with `assert_fresh`.
fn tree() -> ViewTree<TestApp> {
    let mut tree = ViewTree::new(Color::WHITE, TextContexts::new(true));
    tree.set_size(SizeF::new(200.0, 100.0));
    tree.set_check_recordings(false);
    tree
}

/// A row of children at their own heights.
fn row() -> Style {
    Style {
        flex_direction: FlexDirection::Row,
        align_items: AlignItems::FLEX_START,
        size: Size {
            width: auto(),
            height: auto(),
        },
        ..Style::default()
    }
}

/// A row `width` × `height`: a view with children is sized by its style
/// and its children, not by its `measure`.
fn sized_row(width: f32, height: f32) -> Style {
    Style {
        size: Size {
            width: length(width),
            height: length(height),
        },
        ..row()
    }
}

/// The root as a row of `swatches`, each `width` × 20.
fn row_of(tree: &mut ViewTree<TestApp>, names: &[&'static str], width: f32) -> Vec<ViewId<Swatch>> {
    tree.set_style(tree.root(), row());
    names
        .iter()
        .map(|name| {
            let id = tree.insert(Swatch::new(name, width, 20.0));
            tree.append_child(tree.root(), id);
            id
        })
        .collect()
}

fn down(x: f32, y: f32) -> PointerEvent {
    PointerEvent::Down {
        position: PointF::new(x, y),
        button: PointerButton::Primary,
        click_count: 1,
        modifiers: Modifiers::NONE,
    }
}

fn up(x: f32, y: f32) -> PointerEvent {
    PointerEvent::Up {
        position: PointF::new(x, y),
        button: PointerButton::Primary,
        click_count: 1,
        modifiers: Modifiers::NONE,
    }
}

fn move_to(x: f32, y: f32) -> PointerEvent {
    PointerEvent::Move {
        position: PointF::new(x, y),
        modifiers: Modifiers::NONE,
    }
}

fn heard(tree: &ViewTree<TestApp>, id: ViewId<Swatch>) -> Vec<String> {
    tree.get(id).unwrap().heard.clone()
}

fn take_heard(tree: &mut ViewTree<TestApp>, id: ViewId<Swatch>) -> Vec<String> {
    tree.update(id, |swatch, _| std::mem::take(&mut swatch.heard))
        .unwrap()
}

// --- Ids ----------------------------------------------------------------------

#[test]
fn a_removed_view_s_id_finds_nothing_even_once_its_slot_is_used_again() {
    let mut tree = tree();
    let a = tree.insert(Swatch::new("a", 10.0, 10.0));
    tree.append_child(tree.root(), a);
    assert_eq!(tree.get(a).unwrap().name, "a");
    tree.remove(a);
    assert!(tree.get(a).is_none());
    assert!(tree.update(a, |_, _| ()).is_none());
    assert!(tree.children(tree.root()).is_empty());
    // The same slot, another generation.
    let b = tree.insert(Swatch::new("b", 10.0, 10.0));
    assert_eq!(b.any().index, a.any().index);
    assert_ne!(b.any(), a.any());
    assert!(tree.get(a).is_none());
    assert_eq!(tree.get(b).unwrap().name, "b");
}

#[test]
fn removing_a_view_removes_the_views_under_it() {
    let mut tree = tree();
    let group = tree.insert(crate::Container);
    let child = tree.insert(Swatch::new("child", 10.0, 10.0));
    tree.append_child(tree.root(), group);
    tree.append_child(group, child);
    tree.remove(group);
    assert!(!tree.contains(group));
    assert!(tree.get(child).is_none());
}

// --- Layout -------------------------------------------------------------------

#[test]
fn a_flex_row_lays_its_children_out_in_their_parent_s_coordinates() {
    let mut tree = tree();
    let [a, b] = row_of(&mut tree, &["a", "b"], 30.0)[..] else {
        unreachable!()
    };
    // A group to the right, padded, with a child of its own.
    let group = tree.insert(crate::Container);
    tree.set_style(
        group,
        Style {
            padding: taffy::Rect {
                left: length(5.0),
                right: length(0.0),
                top: length(3.0),
                bottom: length(0.0),
            },
            ..row()
        },
    );
    let inner = tree.insert(Swatch::new("inner", 10.0, 10.0));
    tree.append_child(tree.root(), group);
    tree.append_child(group, inner);
    tree.layout();
    assert_eq!(tree.bounds(a), Some(RectF::new(0.0, 0.0, 30.0, 20.0)));
    assert_eq!(tree.bounds(b), Some(RectF::new(30.0, 0.0, 30.0, 20.0)));
    assert_eq!(tree.bounds(group).unwrap().x, 60.0);
    // The child's bounds are its parent's coordinates; its absolute ones
    // add the parent's.
    assert_eq!(tree.bounds(inner), Some(RectF::new(5.0, 3.0, 10.0, 10.0)));
    assert_eq!(
        tree.absolute_bounds(inner),
        Some(RectF::new(65.0, 3.0, 10.0, 10.0))
    );
}

#[test]
fn a_new_size_or_style_lays_out_again() {
    let mut tree = tree();
    let [a] = row_of(&mut tree, &["a"], 30.0)[..] else {
        unreachable!()
    };
    tree.set_style(
        a,
        Style {
            flex_grow: 1.0,
            ..Style::default()
        },
    );
    tree.layout();
    assert_eq!(tree.bounds(a).unwrap().width, 200.0);
    tree.set_size(SizeF::new(150.0, 100.0));
    tree.layout();
    assert_eq!(tree.bounds(a).unwrap().width, 150.0);
    tree.set_style(a, Style::default());
    tree.layout();
    assert_eq!(tree.bounds(a).unwrap().width, 30.0);
}

/// A leaf whose size its content decides, and that says so when it
/// changes.
struct Growing {
    width: f32,
}

impl View<TestApp> for Growing {
    fn measure(
        &mut self,
        _: &mut MeasureCtx<'_, TestApp>,
        _: Size<Option<f32>>,
        _: Size<AvailableSpace>,
    ) -> SizeF {
        SizeF::new(self.width, 10.0)
    }
}

#[test]
fn a_view_that_asks_is_measured_again_and_its_siblings_move() {
    let mut tree = tree();
    tree.set_style(tree.root(), row());
    let growing = tree.insert(Growing { width: 20.0 });
    let after = tree.insert(Swatch::new("after", 10.0, 10.0));
    tree.append_child(tree.root(), growing);
    tree.append_child(tree.root(), after);
    tree.layout();
    assert_eq!(tree.bounds(after).unwrap().x, 20.0);
    // Without asking, taffy's cache keeps the old size.
    tree.update(growing, |view, _| view.width = 50.0);
    tree.layout();
    assert_eq!(tree.bounds(after).unwrap().x, 20.0);
    tree.update(growing, |_, ctx| ctx.request_layout());
    tree.layout();
    assert_eq!(tree.bounds(after).unwrap().x, 50.0);
}

// --- Hit testing --------------------------------------------------------------

#[test]
fn the_deepest_view_hit_is_found_with_the_point_in_its_coordinates() {
    let mut tree = tree();
    let [a, b] = row_of(&mut tree, &["a", "b"], 30.5)[..] else {
        unreachable!()
    };
    tree.layout();
    assert_eq!(
        tree.hit_test(PointF::new(10.0, 5.0)),
        Some((a.any(), PointF::new(10.0, 5.0)))
    );
    assert_eq!(
        tree.hit_test(PointF::new(31.0, 7.0)),
        Some((b.any(), PointF::new(0.5, 7.0)))
    );
    // Past the swatches: the root, a container, isn't hit itself.
    assert_eq!(tree.hit_test(PointF::new(100.0, 5.0)), None);
    // Out of the tree.
    assert_eq!(tree.hit_test(PointF::new(10.0, 150.0)), None);
}

#[test]
fn a_later_child_is_hit_before_an_earlier_one_over_it() {
    let mut tree = tree();
    let under = tree.insert(Swatch::new("under", 50.0, 50.0));
    let over = tree.insert(Swatch::new("over", 50.0, 50.0));
    tree.set_style(tree.root(), row());
    tree.append_child(tree.root(), under);
    tree.append_child(tree.root(), over);
    // Pulled back over the first.
    tree.set_style(
        over,
        Style {
            margin: taffy::Rect {
                left: length(-50.0),
                right: length(0.0),
                top: length(0.0),
                bottom: length(0.0),
            },
            ..Style::default()
        },
    );
    tree.layout();
    assert_eq!(tree.hit_test(PointF::new(5.0, 5.0)).unwrap().0, over.any());
    // Hidden, it isn't hit.
    tree.set_style(
        over,
        Style {
            display: taffy::Display::None,
            ..Style::default()
        },
    );
    // Hidden, at once: before it is laid out again.
    assert_eq!(tree.hit_test(PointF::new(5.0, 5.0)).unwrap().0, under.any());
    tree.layout();
    assert_eq!(tree.hit_test(PointF::new(5.0, 5.0)).unwrap().0, under.any());
}

// --- Events -------------------------------------------------------------------

#[test]
fn a_press_not_handled_goes_up_to_the_parent() {
    let mut tree = tree();
    tree.set_style(tree.root(), row());
    let group = tree.insert(Swatch::new("group", 100.0, 50.0));
    tree.append_child(tree.root(), group);
    tree.set_style(group, sized_row(100.0, 50.0));
    let child = tree.insert(Swatch::new("child", 10.0, 10.0).passive());
    tree.append_child(group, child);
    tree.set_style(
        child,
        Style {
            margin: taffy::Rect {
                left: length(4.0),
                right: length(0.0),
                top: length(2.0),
                bottom: length(0.0),
            },
            ..Style::default()
        },
    );
    tree.layout();
    assert_eq!(tree.pointer(down(6.0, 3.0)), Handled::Yes);
    // Each at its own coordinates.
    assert!(heard(&tree, child).contains(&"down 2 1".to_owned()));
    assert!(heard(&tree, group).contains(&"down 6 3".to_owned()));
    let messages = tree.take_messages();
    let pressed: Vec<&String> = messages.iter().filter(|m| m.ends_with("pressed")).collect();
    assert_eq!(pressed, ["group pressed"]);
    // The handler captured the pointer.
    assert_eq!(tree.captured(), Some(group.any()));
}

#[test]
fn the_view_that_handled_a_press_gets_the_pointer_until_its_button_comes_up() {
    let mut tree = tree();
    let [a, b] = row_of(&mut tree, &["a", "b"], 30.0)[..] else {
        unreachable!()
    };
    tree.layout();
    tree.pointer(down(10.0, 10.0));
    take_heard(&mut tree, a);
    take_heard(&mut tree, b);
    // Over b, the moves and the up still go to a, at a's coordinates; b is
    // hovered.
    tree.pointer(move_to(40.0, 10.0));
    tree.pointer(up(40.0, 10.0));
    assert_eq!(
        take_heard(&mut tree, a),
        ["hover false", "move 40 10", "up 40 10"]
    );
    assert_eq!(take_heard(&mut tree, b), ["hover true"]);
    assert_eq!(tree.captured(), None);
    // Not captured any more: a move over b goes to b.
    tree.pointer(move_to(41.0, 10.0));
    assert_eq!(take_heard(&mut tree, b), ["move 11 10"]);
    assert!(take_heard(&mut tree, a).is_empty());
}

#[test]
fn a_captured_view_removed_lets_go_of_the_pointer() {
    let mut tree = tree();
    let [a, b] = row_of(&mut tree, &["a", "b"], 30.0)[..] else {
        unreachable!()
    };
    tree.layout();
    tree.pointer(down(10.0, 10.0));
    tree.remove(a);
    assert_eq!(tree.captured(), None);
    tree.layout();
    // b is where a was: the up goes to it, as the view under the pointer.
    tree.pointer(up(10.0, 10.0));
    assert!(heard(&tree, b).contains(&"up 10 10".to_owned()));
}

#[test]
fn hovering_tells_the_views_left_deepest_first_then_those_come_outermost_first() {
    let mut tree = tree();
    tree.set_style(tree.root(), row());
    let outer = tree.insert(Swatch::new("outer", 100.0, 50.0));
    tree.set_style(outer, sized_row(100.0, 50.0));
    let inner = tree.insert(Swatch::new("inner", 10.0, 10.0));
    tree.append_child(tree.root(), outer);
    tree.append_child(outer, inner);
    tree.layout();
    tree.pointer(move_to(5.0, 5.0));
    assert_eq!(take_heard(&mut tree, outer), ["hover true"]);
    assert_eq!(take_heard(&mut tree, inner), ["hover true", "move 5 5"]);
    // Off the inner one, still over the outer one.
    tree.pointer(move_to(50.0, 5.0));
    assert_eq!(take_heard(&mut tree, inner), ["hover false"]);
    assert_eq!(take_heard(&mut tree, outer), ["move 50 5"]);
    tree.pointer_left();
    assert_eq!(take_heard(&mut tree, outer), ["hover false"]);
    // From the inner one to nothing: the inner one hears first; back, the
    // outer one first.
    tree.take_messages();
    tree.pointer(move_to(5.0, 5.0));
    tree.pointer_left();
    assert_eq!(
        tree.take_messages(),
        [
            "outer hover true",
            "inner hover true",
            "inner hover false",
            "outer hover false"
        ]
    );
}

#[test]
fn keys_go_to_the_focused_view_then_up_and_nowhere_without_one() {
    let mut tree = tree();
    tree.set_style(tree.root(), row());
    let group = tree.insert(Swatch::new("group", 100.0, 50.0));
    tree.set_style(group, sized_row(100.0, 50.0));
    let field = tree.insert(Swatch::new("field", 10.0, 10.0).passive());
    tree.append_child(tree.root(), group);
    tree.append_child(group, field);
    let key = KeyEvent {
        key: Key::Character("a".into()),
        text: Some("a".into()),
        modifiers: Modifiers::NONE,
        pressed: true,
        repeat: false,
    };
    assert_eq!(tree.key(&key), Handled::No);
    tree.set_focus(Some(field.any()));
    assert_eq!(tree.key(&key), Handled::Yes);
    assert_eq!(
        take_heard(&mut tree, field),
        ["focus true", "key Character(\"a\")"]
    );
    assert_eq!(take_heard(&mut tree, group), ["key Character(\"a\")"]);
    // A press that asks for the focus takes it; the other view hears.
    tree.layout();
    tree.pointer(down(50.0, 5.0));
    assert_eq!(tree.focused(), Some(group.any()));
    assert_eq!(take_heard(&mut tree, field), ["focus false"]);
    // The focused view removed, no view has the keys.
    tree.remove(group);
    assert_eq!(tree.focused(), None);
    assert_eq!(tree.key(&key), Handled::No);
}

#[test]
fn the_cursor_is_the_view_s_under_the_pointer() {
    let mut tree = tree();
    row_of(&mut tree, &["a"], 30.0);
    tree.layout();
    assert_eq!(tree.cursor(PointF::new(10.0, 10.0)), Cursor::Text);
    assert_eq!(tree.cursor(PointF::new(100.0, 10.0)), Cursor::Default);
}

// --- Painting -----------------------------------------------------------------

fn paints(tree: &ViewTree<TestApp>, id: ViewId<Swatch>) -> u32 {
    tree.get(id).unwrap().paints
}

/// The frame's one layer's invalidation.
fn paint(tree: &mut ViewTree<TestApp>) -> Option<Option<Vec<RectF>>> {
    let frame = tree.paint()?;
    let [layer] = &frame.layers[..] else {
        panic!("one layer")
    };
    Some(layer.invalidation.as_ref().map(|rects| rects.to_vec()))
}

/// No view's kept recording is stale. It paints every view again (so after
/// counting paints).
fn assert_fresh(tree: &mut ViewTree<TestApp>) {
    assert_eq!(tree.stale_views(), Vec::<AnyViewId>::new());
}

#[test]
fn the_first_frame_is_whole_and_an_unchanged_tree_paints_nothing() {
    let mut tree = tree();
    let [a, b] = row_of(&mut tree, &["a", "b"], 30.0)[..] else {
        unreachable!()
    };
    assert_eq!(paint(&mut tree), Some(None));
    assert_eq!(tree.paint().map(|_| ()), None);
    assert_eq!((paints(&tree, a), paints(&tree, b)), (1, 1));
    assert_fresh(&mut tree);
}

#[test]
fn a_view_that_asks_is_painted_again_alone_and_only_where_it_is() {
    let mut tree = tree();
    let [a, b] = row_of(&mut tree, &["a", "b"], 30.0)[..] else {
        unreachable!()
    };
    paint(&mut tree);
    tree.update(b, |swatch, ctx| {
        swatch.set_color(Color::rgb(0, 0, 255), ctx)
    });
    let damage = paint(&mut tree).unwrap().unwrap();
    assert!(!damage.is_empty());
    let b_at = RectF::new(30.0, 0.0, 30.0, 20.0);
    assert!(damage.iter().all(|rect| *rect == b_at));
    assert_eq!((paints(&tree, a), paints(&tree, b)), (1, 2));
    // The same color again: it doesn't ask, nothing is painted.
    tree.update(b, |swatch, ctx| {
        swatch.set_color(Color::rgb(0, 0, 255), ctx)
    });
    assert_eq!(tree.paint().map(|_| ()), None);
    assert_fresh(&mut tree);
}

#[test]
fn a_view_moved_is_drawn_where_it_was_and_is_without_painting_again() {
    let mut tree = tree();
    let [a, b] = row_of(&mut tree, &["a", "b"], 30.0)[..] else {
        unreachable!()
    };
    paint(&mut tree);
    // a hidden: b moves left.
    tree.set_style(
        a,
        Style {
            display: taffy::Display::None,
            ..Style::default()
        },
    );
    let damage = paint(&mut tree).unwrap().unwrap();
    let was = RectF::new(30.0, 0.0, 30.0, 20.0);
    let is = RectF::new(0.0, 0.0, 30.0, 20.0);
    assert!(damage.contains(&was) && damage.contains(&is));
    assert_eq!(paints(&tree, b), 1);
    assert_fresh(&mut tree);
}

#[test]
fn a_view_removed_leaves_damage_where_it_was() {
    let mut tree = tree();
    let [_, b] = row_of(&mut tree, &["a", "b"], 30.0)[..] else {
        unreachable!()
    };
    paint(&mut tree);
    tree.remove(b);
    let damage = paint(&mut tree).unwrap().unwrap();
    assert_eq!(damage, [RectF::new(30.0, 0.0, 30.0, 20.0)]);
    assert_fresh(&mut tree);
}

#[test]
fn a_new_size_paints_the_whole_layer() {
    let mut tree = tree();
    row_of(&mut tree, &["a"], 30.0);
    paint(&mut tree);
    tree.set_size(SizeF::new(300.0, 100.0));
    assert_eq!(paint(&mut tree), Some(None));
    assert_fresh(&mut tree);
}

#[test]
fn a_view_changed_without_asking_is_found_stale() {
    let mut tree = tree();
    let [a] = row_of(&mut tree, &["a"], 30.0)[..] else {
        unreachable!()
    };
    paint(&mut tree);
    tree.update(a, |swatch, _| swatch.color = Some(Color::rgb(255, 0, 0)));
    assert_eq!(tree.paint().map(|_| ()), None);
    assert_eq!(tree.stale_views(), [a.any()]);
}

#[test]
fn the_layer_has_each_view_s_recording_where_it_is_parents_under_children() {
    let mut tree = tree();
    tree.set_style(tree.root(), row());
    let group = tree.insert(Swatch::new("group", 100.0, 50.0));
    tree.set_style(
        group,
        Style {
            padding: taffy::Rect {
                left: length(10.0),
                right: length(0.0),
                top: length(0.0),
                bottom: length(0.0),
            },
            ..sized_row(100.0, 50.0)
        },
    );
    let inner = tree.insert(Swatch::new("inner", 20.0, 20.0));
    tree.append_child(tree.root(), group);
    tree.append_child(group, inner);
    tree.update(inner, |swatch, ctx| {
        swatch.set_color(Color::rgb(0, 128, 0), ctx)
    });
    let frame = tree.paint().unwrap();
    let rects: Vec<(RectF, Color)> = frame.layers[0]
        .display_list
        .ops()
        .iter()
        .filter_map(|op| match op {
            PaintOp::DrawRect { rect, flags } => Some((*rect, flags.color)),
            _ => None,
        })
        .collect();
    // Each drawn in its own coordinates, translated: the group first.
    assert_eq!(
        rects,
        [
            (RectF::new(0.0, 0.0, 100.0, 50.0), Color::WHITE),
            (RectF::new(0.0, 0.0, 20.0, 20.0), Color::rgb(0, 128, 0)),
        ]
    );
    assert_eq!(
        frame.layers[0].display_list.bounds(),
        RectF::new(0.0, 0.0, 100.0, 50.0)
    );
    assert_fresh(&mut tree);
}

#[test]
fn a_view_whose_size_changed_paints_again() {
    let mut tree = tree();
    let [a] = row_of(&mut tree, &["a"], 30.0)[..] else {
        unreachable!()
    };
    tree.set_style(
        a,
        Style {
            flex_grow: 1.0,
            ..Style::default()
        },
    );
    paint(&mut tree);
    assert_eq!(paints(&tree, a), 1);
    tree.set_size(SizeF::new(150.0, 100.0));
    paint(&mut tree);
    assert_eq!(paints(&tree, a), 2);
    assert_fresh(&mut tree);
}

#[test]
fn a_view_that_shrinks_leaves_damage_where_it_was() {
    let mut tree = tree();
    let [a] = row_of(&mut tree, &["a"], 30.0)[..] else {
        unreachable!()
    };
    paint(&mut tree);
    tree.set_style(
        a,
        Style {
            size: Size {
                width: length(10.0),
                height: length(20.0),
            },
            ..Style::default()
        },
    );
    // Painted again at its new size: where it drew before is damage too,
    // though nothing else covers it now.
    let damage = paint(&mut tree).unwrap().unwrap();
    assert!(damage.contains(&RectF::new(0.0, 0.0, 30.0, 20.0)));
    assert_fresh(&mut tree);
}

#[test]
fn a_view_hidden_leaves_damage_where_it_was() {
    let mut tree = tree();
    let [_, b] = row_of(&mut tree, &["a", "b"], 30.0)[..] else {
        unreachable!()
    };
    paint(&mut tree);
    tree.set_style(
        b,
        Style {
            display: taffy::Display::None,
            ..Style::default()
        },
    );
    let damage = paint(&mut tree).unwrap().unwrap();
    assert_eq!(damage, [RectF::new(30.0, 0.0, 30.0, 20.0)]);
    // Shown again, it paints again.
    tree.set_style(b, Style::default());
    let damage = paint(&mut tree).unwrap().unwrap();
    assert!(damage.contains(&RectF::new(30.0, 0.0, 30.0, 20.0)));
    assert_fresh(&mut tree);
}

#[test]
fn a_new_theme_paints_every_view_again() {
    let mut tree = tree();
    let [a, b] = row_of(&mut tree, &["a", "b"], 30.0)[..] else {
        unreachable!()
    };
    paint(&mut tree);
    tree.set_theme(Color::rgb(10, 20, 30));
    assert!(paint(&mut tree).is_some());
    assert_eq!((paints(&tree, a), paints(&tree, b)), (2, 2));
    assert_fresh(&mut tree);
}

/// A leaf that draws a rect without saying where (a visual rect of
/// everywhere), off to the right of itself.
struct Unbounded {
    x: f32,
}

impl View<TestApp> for Unbounded {
    fn measure(
        &mut self,
        _: &mut MeasureCtx<'_, TestApp>,
        _: Size<Option<f32>>,
        _: Size<AvailableSpace>,
    ) -> SizeF {
        SizeF::new(10.0, 10.0)
    }

    fn paint(&mut self, _: &mut PaintCtx<'_, TestApp>, list: &mut DisplayItemList) {
        list.push(
            PaintOp::DrawRect {
                rect: RectF::new(self.x, 0.0, 10.0, 10.0),
                flags: PaintFlags::fill(Color::WHITE),
            },
            None,
        );
    }
}

#[test]
fn an_op_recorded_without_a_rect_damages_the_whole_layer() {
    let mut tree = tree();
    let id = tree.insert(Unbounded { x: 100.0 });
    tree.append_child(tree.root(), id);
    paint(&mut tree);
    tree.update(id, |view, ctx| {
        view.x = 150.0;
        ctx.request_paint();
    });
    let damage = paint(&mut tree).unwrap().unwrap();
    assert_eq!(damage, [RectF::new(0.0, 0.0, 200.0, 100.0)]);
}

#[test]
#[should_panic(expected = "views changed without asking to be painted again")]
fn checking_recordings_a_paint_panics_at_a_view_changed_without_asking() {
    let mut tree = tree();
    tree.set_check_recordings(true);
    let [a] = row_of(&mut tree, &["a"], 30.0)[..] else {
        unreachable!()
    };
    tree.paint();
    tree.update(a, |swatch, _| swatch.color = Some(Color::rgb(255, 0, 0)));
    tree.paint();
}

#[test]
fn checking_recordings_a_paint_of_fresh_views_passes() {
    let mut tree = tree();
    tree.set_check_recordings(true);
    let [a] = row_of(&mut tree, &["a"], 30.0)[..] else {
        unreachable!()
    };
    tree.paint();
    tree.update(a, |swatch, ctx| {
        swatch.set_color(Color::rgb(255, 0, 0), ctx)
    });
    assert!(tree.paint().is_some());
}

// --- Description ----------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
struct TileProps {
    width: f32,
    color: Color,
}

/// A leaf made from its props, counting the props it was given after.
struct Tile {
    props: TileProps,
    sets: u32,
}

impl View<TestApp> for Tile {
    fn measure(
        &mut self,
        _: &mut MeasureCtx<'_, TestApp>,
        _: Size<Option<f32>>,
        _: Size<AvailableSpace>,
    ) -> SizeF {
        SizeF::new(self.props.width, 10.0)
    }

    fn paint(&mut self, ctx: &mut PaintCtx<'_, TestApp>, list: &mut DisplayItemList) {
        list.push_op(PaintOp::DrawRect {
            rect: RectF::from_size(ctx.size()),
            flags: PaintFlags::fill(self.props.color),
        });
    }
}

impl crate::Component<TestApp> for Tile {
    type Props = TileProps;

    fn create(props: &TileProps, _: &mut crate::CreateCtx<'_, TestApp>) -> Self {
        Self {
            props: props.clone(),
            sets: 0,
        }
    }

    fn set_props(&mut self, props: &TileProps, ctx: &mut EventCtx<'_, TestApp>) {
        self.sets += 1;
        if props.width != self.props.width {
            ctx.request_layout();
        }
        if props.color != self.props.color {
            ctx.request_paint();
        }
        self.props = props.clone();
    }
}

type El = crate::Element<TestApp>;

fn tile(width: f32) -> El {
    El::new::<Tile>(TileProps {
        width,
        color: Color::rgb(0, 0, 255),
    })
}

/// A row of `children` as the root.
fn described(tree: &mut ViewTree<TestApp>, children: Vec<El>) {
    tree.describe(El::container().style(row()).children(children));
}

/// The root's children's ids.
fn ids(tree: &ViewTree<TestApp>) -> Vec<AnyViewId> {
    tree.children(tree.root()).to_vec()
}

fn tile_at(tree: &ViewTree<TestApp>, index: usize) -> &Tile {
    let id = ids(tree)[index];
    tree.get(ViewId::<Tile>::new(id)).unwrap()
}

#[test]
fn a_description_makes_its_views_with_their_props_and_styles() {
    let mut tree = tree();
    described(
        &mut tree,
        vec![
            tile(30.0),
            El::container()
                .style(sized_row(50.0, 20.0))
                .child(tile(10.0).name("inner")),
        ],
    );
    tree.layout();
    let [first, group] = ids(&tree)[..] else {
        panic!("two children")
    };
    assert_eq!(tree.bounds(first), Some(RectF::new(0.0, 0.0, 30.0, 10.0)));
    assert_eq!(tree.bounds(group), Some(RectF::new(30.0, 0.0, 50.0, 20.0)));
    let inner = tree.find::<Tile>("inner").unwrap();
    assert_eq!(tree.parent(inner), Some(group));
    assert_eq!(tree.get(inner).unwrap().props.width, 10.0);
    // Found only as what it is.
    assert!(tree.find::<crate::Container>("inner").is_none());
    assert!(tree.find::<Tile>("nobody").is_none());
}

#[test]
fn equal_props_leave_a_view_alone_and_new_ones_are_given_to_it() {
    let mut tree = tree();
    described(&mut tree, vec![tile(30.0), tile(40.0)]);
    let before = ids(&tree);
    paint(&mut tree);
    described(&mut tree, vec![tile(30.0), tile(40.0)]);
    assert_eq!(ids(&tree), before, "the same views");
    assert_eq!((tile_at(&tree, 0).sets, tile_at(&tree, 1).sets), (0, 0));
    assert_eq!(tree.paint().map(|_| ()), None);
    // Another color for the second: given to it, painted again.
    described(
        &mut tree,
        vec![
            tile(30.0),
            El::new::<Tile>(TileProps {
                width: 40.0,
                color: Color::rgb(255, 0, 0),
            }),
        ],
    );
    assert_eq!(ids(&tree), before);
    assert_eq!((tile_at(&tree, 0).sets, tile_at(&tree, 1).sets), (0, 1));
    let damage = paint(&mut tree).unwrap().unwrap();
    assert_eq!(damage, [RectF::new(30.0, 0.0, 40.0, 10.0)]);
    // Wider: laid out again, the first stays.
    described(&mut tree, vec![tile(30.0), tile(60.0)]);
    tree.layout();
    assert_eq!(tree.bounds(ids(&tree)[1]).unwrap().width, 60.0);
    paint(&mut tree);
    assert_fresh(&mut tree);
}

#[test]
fn keyed_views_moved_are_kept_and_moved() {
    let mut tree = tree();
    described(
        &mut tree,
        vec![tile(10.0).key(1), tile(20.0).key(2), tile(30.0).key(3)],
    );
    let [a, b, c] = ids(&tree)[..] else {
        panic!("three")
    };
    described(
        &mut tree,
        vec![tile(30.0).key(3), tile(10.0).key(1), tile(20.0).key(2)],
    );
    assert_eq!(ids(&tree), [c, a, b]);
    // One taken out of the middle, one new at the end.
    described(
        &mut tree,
        vec![tile(30.0).key(3), tile(20.0).key(2), tile(5.0).key(4)],
    );
    let after = ids(&tree);
    assert_eq!(after[..2], [c, b]);
    assert!(!tree.contains(a));
    assert!(![a, b, c].contains(&after[2]));
    tree.layout();
    assert_eq!(tree.bounds(after[2]).unwrap().width, 5.0);
}

#[test]
fn views_without_keys_are_matched_by_place_and_another_type_is_made_anew() {
    let mut tree = tree();
    described(&mut tree, vec![tile(10.0), tile(20.0)]);
    let [a, b] = ids(&tree)[..] else {
        panic!("two")
    };
    // The first stays (another width given to it); a container where the
    // second was is new; the second's gone.
    described(&mut tree, vec![tile(15.0), El::container()]);
    let [first, second] = ids(&tree)[..] else {
        panic!("two")
    };
    assert_eq!(first, a);
    assert_eq!(tile_at(&tree, 0).sets, 1);
    assert_ne!(second, b);
    assert!(!tree.contains(b));
    // Fewer: the rest removed.
    described(&mut tree, vec![tile(15.0)]);
    assert_eq!(ids(&tree), [a]);
    assert!(!tree.contains(second));
}

#[test]
fn children_are_described_within_their_parent() {
    let mut tree = tree();
    let group = |children: Vec<El>| {
        El::container()
            .key("group")
            .style(sized_row(100.0, 20.0))
            .children(children)
    };
    described(&mut tree, vec![group(vec![tile(10.0).key(1), tile(20.0).key(2)])]);
    let parent = ids(&tree)[0];
    let inner = tree.children(parent).to_vec();
    described(&mut tree, vec![group(vec![tile(20.0).key(2), tile(10.0).key(1)])]);
    assert_eq!(ids(&tree), [parent]);
    assert_eq!(tree.children(parent), [inner[1], inner[0]]);
    // A new style for the group: laid out with it.
    tree.describe(
        El::container().style(row()).child(
            El::container()
                .key("group")
                .style(sized_row(80.0, 20.0))
                .children(vec![tile(20.0).key(2), tile(10.0).key(1)]),
        ),
    );
    tree.layout();
    assert_eq!(tree.bounds(parent).unwrap().width, 80.0);
}

#[test]
fn a_name_described_no_more_finds_nothing() {
    let mut tree = tree();
    described(&mut tree, vec![tile(10.0).name("first").key(1)]);
    let first = tree.find::<Tile>("first").unwrap();
    // The view gone, then a new one of the same type in its slot, unnamed.
    described(&mut tree, vec![]);
    described(&mut tree, vec![tile(20.0).key(2)]);
    assert!(!tree.contains(first));
    assert_eq!(ids(&tree)[0].index, first.any().index);
    assert!(tree.find::<Tile>("first").is_none());
}
