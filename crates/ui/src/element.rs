//! The declarative layer over the tree: the app describes its UI from its
//! state as a tree of [`Element`]s, and the tree updates the views it keeps
//! to match ([`crate::ViewTree::describe`]). An element names a view type
//! with its props, its style and its children; described again, a view of
//! the same type at the same key (or place, among siblings without keys) is
//! kept and given the new props only if they changed, one that moved is
//! moved, and the others are made or removed (React's and Flutter's rule:
//! the same type and key).

use std::any::{Any, TypeId};
use std::marker::PhantomData;

use taffy::Style;

use crate::view::{App, Container, CreateCtx, EventCtx, View};

/// A view an element can make and update: its props are all an element
/// says of it.
pub trait Component<A: App>: View<A> + Sized {
    /// What the view shows, as the app describes it. Equal props from one
    /// description to the next leave the view untouched.
    type Props: PartialEq + 'static;

    /// The view, for `props`.
    fn create(props: &Self::Props, ctx: &mut CreateCtx<'_, A>) -> Self;

    /// The props changed: the view takes them, and asks to be painted (or
    /// laid out) again if what it shows changed.
    fn set_props(&mut self, props: &Self::Props, ctx: &mut EventCtx<'_, A>);
}

impl<A: App> Component<A> for Container {
    type Props = ();

    fn create(_: &(), _: &mut CreateCtx<'_, A>) -> Self {
        Container
    }

    fn set_props(&mut self, _: &(), _: &mut EventCtx<'_, A>) {}
}

/// What tells siblings apart from one description to the next.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ElementKey {
    Name(&'static str),
    Number(u64),
}

impl From<&'static str> for ElementKey {
    fn from(name: &'static str) -> Self {
        ElementKey::Name(name)
    }
}

impl From<u64> for ElementKey {
    fn from(number: u64) -> Self {
        ElementKey::Number(number)
    }
}

/// A view as the app describes it: its type and props, its style, its
/// children.
pub struct Element<A: App> {
    pub(crate) key: Option<ElementKey>,
    pub(crate) name: Option<&'static str>,
    pub(crate) style: Style,
    pub(crate) props: Box<dyn AnyProps<A>>,
    pub(crate) children: Vec<Element<A>>,
}

impl<A: App> Element<A> {
    /// A view of type `V` with `props`, the default style and no children.
    pub fn new<V: Component<A>>(props: V::Props) -> Self {
        Self {
            key: None,
            name: None,
            style: Style::default(),
            props: Box::new(PropsOf::<A, V> {
                props,
                _types: PhantomData,
            }),
            children: Vec::new(),
        }
    }

    /// A [`Container`]: only holds others.
    pub fn container() -> Self {
        Self::new::<Container>(())
    }

    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// What tells it apart from its siblings: a sibling with the same key
    /// in the next description is it, wherever it is among them.
    pub fn key(mut self, key: impl Into<ElementKey>) -> Self {
        self.key = Some(key.into());
        self
    }

    /// A name, unique in the description, to find the view by
    /// ([`crate::ViewTree::find`]).
    pub fn name(mut self, name: &'static str) -> Self {
        self.name = Some(name);
        self
    }

    pub fn child(mut self, child: Element<A>) -> Self {
        self.children.push(child);
        self
    }

    pub fn children(mut self, children: impl IntoIterator<Item = Element<A>>) -> Self {
        self.children.extend(children);
        self
    }
}

/// An element's props, whatever its view's type.
pub(crate) trait AnyProps<A: App>: Any {
    fn view_type(&self) -> TypeId;

    fn create(&self, ctx: &mut CreateCtx<'_, A>) -> Box<dyn View<A>>;

    /// Whether these are `old`'s props: the same type and equal.
    fn same(&self, old: &dyn AnyProps<A>) -> bool;

    /// Gives `view` (made from props of the same type) these props.
    fn update(&self, view: &mut dyn View<A>, ctx: &mut EventCtx<'_, A>);
}

struct PropsOf<A: App, V: Component<A>> {
    props: V::Props,
    // `fn() -> …`: holds neither.
    _types: PhantomData<fn() -> (A, V)>,
}

impl<A: App, V: Component<A>> AnyProps<A> for PropsOf<A, V> {
    fn view_type(&self) -> TypeId {
        TypeId::of::<V>()
    }

    fn create(&self, ctx: &mut CreateCtx<'_, A>) -> Box<dyn View<A>> {
        Box::new(V::create(&self.props, ctx))
    }

    fn same(&self, old: &dyn AnyProps<A>) -> bool {
        let old: &dyn Any = old;
        old.downcast_ref::<Self>()
            .is_some_and(|old| old.props == self.props)
    }

    fn update(&self, view: &mut dyn View<A>, ctx: &mut EventCtx<'_, A>) {
        let view: &mut dyn Any = view;
        let view = view
            .downcast_mut::<V>()
            .expect("a view is kept only for props of its own type");
        view.set_props(&self.props, ctx);
    }
}
