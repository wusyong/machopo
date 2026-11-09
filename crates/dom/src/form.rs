//! HTML's form owner of the listed elements
//! (https://html.spec.whatwg.org/#form-owner): Blink's `ListedElement`
//! and the list of them an `HTMLFormElement` keeps. A listed element's
//! owner is in its data, reset in the DOM's operations as each change
//! happens (Blink's `InsertedInto`, `RemovedFrom` and `ParseAttribute`): a
//! subtree inserted or removed, a `form` or `id` attribute changed. A
//! form's elements in tree order are found when asked and kept until an
//! owner changes (Blink's `listed_elements_` and
//! `listed_elements_are_dirty_`).
//!
//! Until a form is made, or a listed element given a `form` attribute, no
//! element can have an owner: a subtree inserted or removed reads one flag
//! (`FormState::seen`) and does nothing more. The same flag says whether a
//! fieldset or a radio button was made, for what they need of an
//! insertion.
//!
//! A form control is disabled by its `disabled` attribute, or by a
//! disabled fieldset ancestor (Blink's `ListedElement::IsActuallyDisabled`):
//! what its ancestors say is found once and kept in its data until a
//! subtree with it comes or goes, a fieldset's `disabled` or `<legend>`
//! children change (Blink's `ancestor_disabled_state_` and
//! `AncestorDisabledStateWasChanged`).
//!
//! A `<label>`'s labeled control, and a labelable element's `labels`, are
//! found at each ask (a walk of the element's tree; Blink's
//! `LabelsNodeList` keeps its place until the tree's version changes): the
//! DOM's operations keep nothing for them.

use std::cell::Cell;

use elise::{Gc, GcObject, HeapVector, LocalExternal, Member};
use gfx::NodeId;

use crate::{
    Cx, DomString, HtmlCollection, InputKind, NameId, Node, NodeList, NodeRef, PARSER_INSERTED,
    QualifiedName, names, sites,
};

/// What an HTML element is to forms (`Cx::form_kind`, by its local name
/// from the names' table): a listed element (HTML's: a form's `elements`,
/// and the only ones machopo gives an owner; `<img>` is form-associated but
/// not listed), a labelable one, a form.
pub(crate) const LISTED: u8 = 1 << 0;
pub(crate) const LABELABLE: u8 = 1 << 1;
pub(crate) const FORM: u8 = 1 << 2;
pub(crate) const INPUT: u8 = 1 << 3;
pub(crate) const FIELDSET: u8 = 1 << 4;
/// An attribute's name's (the same table): a form element's state follows
/// the attribute (`form`, `type`, `checked`, `name`, `disabled`), or an
/// `<img>`'s image data (`src`, `srcset`, `sizes`, `crossorigin`,
/// `referrerpolicy`: `image`'s relevant mutations).
pub(crate) const CONTROL_ATTRIBUTE: u8 = 1 << 5;
/// A form control that a `disabled` attribute disables (HTML's `button`,
/// `fieldset`, `input`, `select`, `textarea`).
pub(crate) const DISABLABLE: u8 = 1 << 6;
pub(crate) const TEXTAREA: u8 = 1 << 7;

/// `FormState::seen`'s: a form was made, or a listed element given a `form`
/// attribute, or the parser associated one; a fieldset was made; an input
/// became a radio button. An input or a textarea was made: connected
/// subtrees are looked at for controls whose UA shadow tree is to be built
/// (`ua_shadow`; not a bit `form_inserted` reads).
const FORMS_SEEN: u16 = 1 << 0;
const FIELDSETS_SEEN: u16 = 1 << 1;
pub(crate) const RADIOS_SEEN: u16 = 1 << 2;
pub(crate) const CONTROLS_SEEN: u16 = 1 << 3;
/// A select was made: subtrees inserted and removed are looked at for
/// options (`select`).
pub(crate) const SELECTS_SEEN: u16 = 1 << 4;
/// A `<picture>` was made: what is inserted into and removed from one is
/// looked at for `<img>`s and `<source>`s (`image`, out of line in
/// `form_inserted` and `form_removed`).
pub(crate) const PICTURES_SEEN: u16 = 1 << 5;
/// An `<iframe>` was made: what is connected, removed and changed is
/// looked at for iframes (`frame`; the word insertion reads for
/// `CONTROLS_SEEN` already).
pub(crate) const IFRAMES_SEEN: u16 = 1 << 6;
/// A live range or a node iterator was made: inserting, removing and
/// changing data look at their document's (`range`, `traversal`).
pub(crate) const LIVE_SEEN: u16 = 1 << 7;
/// A script element was made that may run as it is inserted (not
/// parser-inserted, not already started: script made it, or the parser
/// left one unprepared): what is connected is looked at for script
/// elements, what is inserted into a connected one too (`script`; the
/// word insertion reads for `IFRAMES_SEEN` already, one test for both).
/// The parser's scripts don't set it.
pub(crate) const SCRIPTS_SEEN: u16 = 1 << 8;
/// A `<link>` was given a `rel` with `modulepreload`: what is connected,
/// and its attributes changed while connected, are looked at for such
/// links (`script`; the word the iframes' checks read, one test for all).
pub(crate) const PRELOADS_SEEN: u16 = 1 << 9;
/// A `<base>` was made: what is connected, removed and changed is looked
/// at for them (`base`; the word the iframes' checks read).
pub(crate) const BASES_SEEN: u16 = 1 << 10;
/// An HTML `embed`, `form`, `iframe`, `img` or `object` was given a
/// `name`: what is connected and removed is looked at for their names
/// (`ids`' tables for the window's named access).
pub(crate) const NAMES_SEEN: u16 = 1 << 11;
/// An element was given an `autofocus` attribute: what is connected is
/// looked at for it (`frame`'s connection steps; the word they read).
pub(crate) const AUTOFOCUS_SEEN: u16 = 1 << 12;
/// An element was given an `http-equiv` attribute: what is connected, and
/// its attributes changed while connected, are looked at for a
/// `<meta http-equiv="content-language">` (`language`; few pages have one,
/// most a `<meta>`).
pub(crate) const PRAGMAS_SEEN: u16 = 1 << 13;

/// `FormAssociation::ancestor_disabled`'s.
const UNKNOWN: u8 = 0;
const ENABLED: u8 = 1;
const DISABLED: u8 = 2;

thread_local! {
    /// Walks finding a form's elements, for the tests.
    static FORM_WALKS: Cell<u64> = const { Cell::new(0) };
    /// Walks finding whether a fieldset ancestor disables a form control.
    static DISABLED_WALKS: Cell<u64> = const { Cell::new(0) };
}

/// How many walks the thread made finding a form's elements (tests: a
/// form keeps them until an owner changes).
#[doc(hidden)]
pub fn form_walks() -> u64 {
    FORM_WALKS.get()
}

/// How many walks the thread made finding whether a fieldset ancestor
/// disables a form control (tests: what it found is kept until the tree or
/// a fieldset changes).
#[doc(hidden)]
pub fn disabled_walks() -> u64 {
    DISABLED_WALKS.get()
}

/// A listed element's, a labelable element's or a form's (its data's
/// `form`, made at the first).
#[derive(GcObject)]
pub struct FormAssociation {
    /// A listed element's form owner.
    owner: Member<Node>,
    /// A form's listed elements whose owner it is, in tree order, as last
    /// found; none until asked for again (an owner changed).
    elements: Member<HeapVector<Node>>,
    /// A form's: an element out of its subtree may have it as owner (the
    /// parser associated one, or a `form` attribute names it; Blink's
    /// `has_elements_associated_by_parser_` and
    /// `has_elements_associated_by_form_attribute_`): its elements are
    /// looked for in its whole tree, and those left out when it is removed
    /// are reset. Never unset.
    outside: Cell<bool>,
    /// A form's or a fieldset's `elements`, a select's or a datalist's
    /// `options`, the same each time.
    pub(crate) collection: Member<HtmlCollection>,
    /// A select's `selectedOptions`, the same each time.
    pub(crate) selected_options: Member<HtmlCollection>,
    /// A form's `relList` or an output's `htmlFor`, the same each time.
    pub(crate) token_list: Member<crate::DomTokenList>,
    /// A labelable element's `labels`, the same each time.
    labels: Member<NodeList>,
    /// An `<input>`'s type and state (`input`), made with the element.
    pub(crate) input: Member<crate::InputType>,
    /// A `<textarea>`'s value and selection (`text_control`), made with
    /// the element.
    pub(crate) textarea: Member<crate::TextArea>,
    /// An `<output>`'s default value override (`select`), made with the
    /// element.
    pub(crate) output: Member<crate::Output>,
    /// A form control's: whether a fieldset ancestor disables it, if known
    /// (`UNKNOWN`, `ENABLED`, `DISABLED`).
    ancestor_disabled: Cell<u8>,
    /// A form's: HTML's locked for reset, while `reset()` resets it.
    locked_for_reset: Cell<bool>,
    /// A select's: its popup is open (`select`; Blink's
    /// `popup_is_visible_`).
    pub(crate) popup_open: Cell<bool>,
    /// A list box's state between the user's events, made at the first
    /// (`select`).
    pub(crate) list_box: Member<crate::ListBox>,
    /// A form's default button (`form_state`), kept as its submit buttons
    /// change.
    default_button: Member<Node>,
    /// A form's past names map, made at its first entry.
    past_names: Member<PastNames>,
}

/// A form's past names map (HTML's): the names `form[name]` found one
/// element by, and the element (by id, weak: an element that stops being
/// the form's leaves the map as its owner changes).
#[derive(GcObject)]
pub struct PastNames {
    entries: LocalExternal<Cell<PastNameEntries>>,
}

/// The past names map's entries: each name and its element's id.
type PastNameEntries = Vec<(DomString, NodeId)>;

/// A `RadioNodeList`'s: the elements of a form's `elements` whose `id` or
/// `name` is its name, live.
#[derive(GcObject)]
pub struct RadioList {
    form: Member<Node>,
    name: LocalExternal<DomString>,
}

/// What `form[name]` and `elements.namedItem(name)` find: one element, or
/// a list of several.
#[derive(Clone, Copy)]
pub enum FormItem<'gc> {
    Element(NodeRef<'gc>),
    List(Gc<'gc, NodeList>),
}

/// What the DOM keeps for forms.
#[derive(Default)]
pub(crate) struct FormState {
    /// What has been seen (`FORMS_SEEN`, `FIELDSETS_SEEN`, `RADIOS_SEEN`):
    /// from then on, subtrees inserted and removed are looked at for it.
    /// Never unset.
    pub(crate) seen: Cell<u16>,
    /// The listed elements with a `form` attribute (by id, weak): reset
    /// when an id may name another form, as a connected subtree comes or
    /// goes or an id changes (Blink's `IdTargetObserver` finds those an id
    /// names; few pages have any, and these are few).
    attributed: Cell<Vec<NodeId>>,
    /// The controls connected since the last style update whose UA shadow
    /// tree is to be built then (`ua_shadow`, by id, weak): a set, as
    /// Blink's, so that scheduling one is a step, not a look at each
    /// scheduled before (a page inserting a list of checkboxes).
    pub(crate) scheduled: Cell<crate::NodeIdHashSet>,
    /// Whether a "replace all" is replacing a node's children: a select's
    /// options' steps wait for its end (`select`).
    pub(crate) replacing: Cell<bool>,
    /// What the selects asked of their popups since the host last took
    /// them (`take_popup_requests`), in order.
    pub(crate) popup_requests: Cell<Vec<crate::PopupRequest>>,
}

impl<'gc> Cx<'_, 'gc> {
    /// What `node` is to forms (`LISTED`, `LABELABLE`, `FORM`): an HTML
    /// element's of a fixed name, by its local name; 0 for others.
    #[inline]
    pub(crate) fn form_kind(&self, node: NodeRef<'gc>) -> u8 {
        // A node that isn't an element has no namespace: 0 too.
        let name = node.qualified_name().ids();
        if name.namespace != names::HTML_NAMESPACE {
            return 0;
        }
        names::form_kind(name.local)
    }

    /// Whether `element` is listed (HTML's listed elements: `button`,
    /// `fieldset`, `input`, `object`, `output`, `select`, `textarea`).
    pub fn is_listed(&self, element: NodeRef<'gc>) -> bool {
        self.form_kind(element) & LISTED != 0
    }

    /// Whether `element` is labelable (HTML's: `button`, `input` but a
    /// hidden one, `meter`, `output`, `progress`, `select`, `textarea`).
    pub fn is_labelable(&self, element: NodeRef<'gc>) -> bool {
        self.form_kind(element) & LABELABLE != 0
            && self.input_kind(element) != Some(InputKind::Hidden)
    }

    fn is_form(&self, node: NodeRef<'gc>) -> bool {
        self.form_kind(node) & FORM != 0
    }

    pub(crate) fn see(&self, seen: u16) {
        let state = &self.changes.forms.seen;
        state.set(state.get() | seen);
    }

    pub(crate) fn has_seen(&self, seen: u16) -> bool {
        self.changes.forms.seen.get() & seen != 0
    }

    /// An `<input>` in the Image Button state: not in its form's
    /// `elements`.
    fn is_image_button(&self, element: NodeRef<'gc>) -> bool {
        self.input_kind(element) == Some(InputKind::Image)
    }

    pub(crate) fn association(&self, element: NodeRef<'gc>) -> Option<Gc<'gc, FormAssociation>> {
        self.existing_element_data(element)?.form.get(self.gc)
    }

    pub(crate) fn association_or_new(&self, element: NodeRef<'gc>) -> Gc<'gc, FormAssociation> {
        let data = self.element_data(element);
        if let Some(association) = data.form.get(self.gc) {
            return association;
        }
        let association = self.gc.alloc_beside(
            element,
            &sites::OTHER,
            FormAssociationInit {
                owner: None,
                elements: None,
                outside: Cell::new(false),
                collection: None,
                selected_options: None,
                token_list: None,
                labels: None,
                input: None,
                textarea: None,
                output: None,
                ancestor_disabled: Cell::new(UNKNOWN),
                popup_open: Cell::new(false),
                list_box: None,
                locked_for_reset: Cell::new(false),
                default_button: None,
                past_names: None,
            },
        );
        data.form.set(self.gc, Some(association));
        association
    }

    /// A form, a fieldset, an input, a textarea, a select, an output or
    /// another listed or disablable element was made
    /// (`create_element_named`, with what it is to forms, `kind`): from a
    /// form, a fieldset or a select on, subtrees inserted and removed are
    /// looked at; an input has its type, a textarea and an output their
    /// state.
    #[cold]
    #[inline(never)]
    pub(crate) fn form_element_made(&self, element: NodeRef<'gc>, kind: u8) {
        if kind & FORM != 0 {
            self.see(FORMS_SEEN);
        }
        if kind & FIELDSET != 0 {
            self.see(FIELDSETS_SEEN);
        }
        if kind & INPUT != 0 {
            self.see(CONTROLS_SEEN);
            self.input_made(element);
        }
        if kind & TEXTAREA != 0 {
            self.see(CONTROLS_SEEN);
            self.textarea_made(element);
        }
        if self.is_select(element) {
            self.select_made();
        } else if self.is_html_named(element, names::local::PICTURE) {
            self.see(PICTURES_SEEN);
        } else if self.qualified_name(element) == Some(QualifiedName::html(names::local::OUTPUT)) {
            self.output_made(element);
        }
    }

    /// `element`'s form owner (a listed element's); none for others.
    pub fn form_owner(&self, element: NodeRef<'gc>) -> Option<NodeRef<'gc>> {
        self.association(element)?.owner.get(self.gc)
    }

    /// `element`'s owner is `form` now: the forms' elements found again
    /// when next asked for; the old one's past names forget it.
    fn set_form_owner(&self, element: NodeRef<'gc>, form: Option<NodeRef<'gc>>) {
        let old = self.form_owner(element);
        let same = match (old, form) {
            (Some(old), Some(form)) => Gc::ptr_eq(old, form),
            (old, form) => old.is_none() && form.is_none(),
        };
        if same {
            return;
        }
        if let Some(old) = old {
            self.form_elements_changed(old);
            self.forget_past_names(old, element);
        }
        match form {
            Some(form) => {
                self.form_elements_changed(form);
                self.association_or_new(element)
                    .owner
                    .set(self.gc, Some(form));
            }
            None => {
                if let Some(association) = self.association(element) {
                    association.owner.set(self.gc, None);
                }
            }
        }
        // HTML's: a checked radio button whose owner changed unchecks its
        // new group.
        self.uncheck_radio_group(element);
        if self.input_kind(element) == Some(InputKind::Radio) {
            self.radio_groups_changed(element);
        }
        if self.is_submit_button(element) {
            for form in [old, form].into_iter().flatten() {
                self.default_button_changed(form);
            }
        }
    }

    /// `form`'s default button, as last found.
    pub(crate) fn default_button(&self, form: NodeRef<'gc>) -> Option<NodeRef<'gc>> {
        self.association(form)?.default_button.get(self.gc)
    }

    pub(crate) fn set_default_button(&self, form: NodeRef<'gc>, button: Option<NodeRef<'gc>>) {
        self.association_or_new(form)
            .default_button
            .set(self.gc, button);
    }

    /// `form`'s elements are found again when next asked for.
    fn form_elements_changed(&self, form: NodeRef<'gc>) {
        if let Some(association) = self.association(form) {
            association.elements.set(self.gc, None);
        }
    }

    /// HTML's "reset the form owner" of a listed `element`: its parser
    /// inserted flag unset; its owner the form its `form` attribute names,
    /// if it has one and is connected, else its nearest ancestor form.
    fn reset_form_owner(&self, element: NodeRef<'gc>) {
        self.set_flag(element, PARSER_INSERTED, false);
        let form = match self.form_attribute_target(element) {
            Some(named) => named,
            None => self.ancestor_form(element),
        };
        self.set_form_owner(element, form);
    }

    /// The form `element`'s `form` attribute names, where it counts (it has
    /// one, and is connected): the first element in its tree with the id,
    /// if a form (`Some(None)` if not).
    fn form_attribute_target(&self, element: NodeRef<'gc>) -> Option<Option<NodeRef<'gc>>> {
        if !self.is_connected(element) {
            return None;
        }
        let id = self
            .attribute_value_by_id(element, names::local::FORM)?
            .to_owned();
        let root = self.root_node(element, false);
        let form = self
            .element_by_id(root, &id.to_str())
            .filter(|found| self.is_form(*found));
        if let Some(form) = form {
            self.association_or_new(form).outside.set(true);
        }
        Some(form)
    }

    /// The nearest form among `element`'s ancestors (in its tree: not
    /// through a shadow root to its host).
    fn ancestor_form(&self, element: NodeRef<'gc>) -> Option<NodeRef<'gc>> {
        let mut at = self.parent(element);
        while let Some(node) = at {
            if self.is_form(node) {
                return Some(node);
            }
            at = self.parent(node);
        }
        None
    }

    /// Whether `node` is `root` or one of its descendants.
    fn is_within(&self, node: NodeRef<'gc>, root: NodeRef<'gc>) -> bool {
        let mut at = Some(node);
        while let Some(current) = at {
            if Gc::ptr_eq(current, root) {
                return true;
            }
            at = self.parent(current);
        }
        false
    }

    /// The parser associated `element`, made while its form element
    /// pointer was `form`, with that form (HTML's "create an element for a
    /// token"): its parser inserted flag set, so that its insertion keeps
    /// the owner. Only a listed element: `<img>` has no owner here.
    pub fn set_parser_form_owner(&self, element: NodeRef<'gc>, form: NodeRef<'gc>) {
        if !self.is_listed(element) || !self.is_form(form) {
            return;
        }
        self.see(FORMS_SEEN);
        self.association_or_new(form).outside.set(true);
        self.set_form_owner(element, Some(form));
        self.set_flag(element, PARSER_INSERTED, true);
    }

    /// `node` was inserted into `parent` (`link`): HTML's insertion steps
    /// of the listed elements among its inclusive descendants (Blink's
    /// `ListedElement::InsertedInto`). Nothing until a form, a fieldset or
    /// a radio button is seen.
    #[inline]
    pub(crate) fn form_inserted(
        &self,
        parent: NodeRef<'gc>,
        node: NodeRef<'gc>,
        next: Option<NodeRef<'gc>>,
    ) {
        if self.changes.forms.seen.get() & !CONTROLS_SEEN != 0 {
            self.form_subtree_inserted(parent, node, next);
        }
    }

    /// `form_inserted`'s, out of line: a `<picture>`'s relevant mutations
    /// too (`image`), from the first `<picture>`.
    #[inline(never)]
    fn form_subtree_inserted(
        &self,
        parent: NodeRef<'gc>,
        node: NodeRef<'gc>,
        next: Option<NodeRef<'gc>>,
    ) {
        if self.has_seen(PICTURES_SEEN) {
            self.picture_children_changed(parent, node, next);
        }
        if self.changes.forms.seen.get() & !(CONTROLS_SEEN | PICTURES_SEEN) == 0 {
            return;
        }
        let (forms, fieldsets, radios) = (
            self.has_seen(FORMS_SEEN),
            self.has_seen(FIELDSETS_SEEN),
            self.has_seen(RADIOS_SEEN),
        );
        let connected = self.is_connected(node);
        let mut radio = false;
        self.for_each_inclusive_descendant(node, |at| {
            if !self.is_listed(at) {
                return;
            }
            radio |= radios && self.input_kind(at) == Some(InputKind::Radio);
            if fieldsets {
                self.ancestor_disabled_changed(at);
            }
            // HTML's: the parser's association stands, the flag set until
            // a reset (its removal out of its owner's tree, a `form`
            // attribute). It was associated before it was in its form's
            // tree, which a default button's search found its elements in
            // without it: they, and the default button, are found again.
            if forms && !self.flag(at, PARSER_INSERTED) {
                self.reset_form_owner(at);
            } else if forms && let Some(form) = self.form_owner(at) {
                self.form_elements_changed(form);
                if self.is_submit_button(at) {
                    self.default_button_changed(form);
                }
            }
            // HTML's: a checked radio button that becomes connected
            // unchecks its group.
            if radios && connected {
                self.uncheck_radio_group(at);
            }
        });
        if fieldsets {
            self.fieldset_children_changed(parent, node);
        }
        self.options_inserted(parent, node);
        if self.has_seen(SELECTS_SEEN) && self.has_text(node) {
            self.option_text_changed(parent);
        }
        // Radio buttons came into the tree's groups.
        if radio {
            self.radio_groups_changed(node);
        }
        // A form, or an id, may have come into a document: what `form`
        // attributes name.
        if forms && connected {
            self.reset_attributed();
        }
    }

    /// `node` was removed from `parent` (`remove_noting`), connected
    /// before if `was_connected`: HTML's removing steps of the listed
    /// elements among its inclusive descendants (Blink's
    /// `ListedElement::RemovedFrom`), and of the forms (Blink's
    /// `HTMLFormElement::RemovedFrom`).
    #[inline]
    pub(crate) fn form_removed(
        &self,
        parent: NodeRef<'gc>,
        node: NodeRef<'gc>,
        next: Option<NodeRef<'gc>>,
        was_connected: bool,
    ) {
        if self.changes.forms.seen.get() & !CONTROLS_SEEN != 0 {
            self.form_subtree_removed(parent, node, next, was_connected);
        }
    }

    /// `form_removed`'s, out of line: a `<picture>`'s relevant mutations
    /// too (`image`), from the first `<picture>`.
    #[inline(never)]
    fn form_subtree_removed(
        &self,
        parent: NodeRef<'gc>,
        node: NodeRef<'gc>,
        next: Option<NodeRef<'gc>>,
        was_connected: bool,
    ) {
        if self.has_seen(PICTURES_SEEN) {
            self.picture_children_changed(parent, node, next);
        }
        if self.changes.forms.seen.get() & !(CONTROLS_SEEN | PICTURES_SEEN) == 0 {
            return;
        }
        let fieldsets = self.has_seen(FIELDSETS_SEEN);
        if fieldsets {
            self.fieldset_children_changed(parent, node);
        }
        self.options_removed(parent, node);
        if self.has_seen(SELECTS_SEEN) && self.has_text(node) {
            self.option_text_changed(parent);
        }
        // Radio buttons left the tree's groups.
        if self.has_seen(RADIOS_SEEN) && was_connected {
            let mut radio = false;
            self.for_each_inclusive_descendant(node, |at| {
                radio |= self.input_kind(at) == Some(InputKind::Radio);
            });
            if radio {
                self.radio_groups_changed(parent);
            }
        }
        if !self.has_seen(FORMS_SEEN) {
            if fieldsets {
                self.for_each_inclusive_descendant(node, |at| {
                    if self.is_listed(at) {
                        self.ancestor_disabled_changed(at);
                    }
                });
            }
            return;
        }
        let mut forms = Vec::new();
        self.for_each_inclusive_descendant(node, |at| {
            let kind = self.form_kind(at);
            if kind & LISTED != 0 && fieldsets {
                self.ancestor_disabled_changed(at);
            }
            if kind & LISTED != 0 {
                // Its owner left behind. (One whose `form` attribute no
                // longer counts, out of the document, is reset with the
                // others below.)
                if self
                    .form_owner(at)
                    .is_some_and(|owner| !self.is_within(owner, node))
                {
                    self.reset_form_owner(at);
                }
            } else if kind & FORM != 0
                && self
                    .association(at)
                    .is_some_and(|association| association.outside.get())
            {
                forms.push(at);
            }
        });
        // The elements outside whose owner is a form removed: no longer in
        // its tree.
        if !forms.is_empty() {
            let mut left = Vec::new();
            self.for_each_inclusive_descendant(self.root_node(parent, false), |at| {
                if self.is_listed(at)
                    && let Some(owner) = self.form_owner(at)
                    && forms.iter().any(|form| Gc::ptr_eq(*form, owner))
                {
                    left.push(at);
                }
            });
            for element in left {
                self.reset_form_owner(element);
            }
        }
        if was_connected {
            self.reset_attributed();
        }
    }

    /// An attribute whose name has `CONTROL_ATTRIBUTE` changed
    /// (`attribute_changed`; `local` its local name, a plain name's):
    /// what a form element's state follows of it.
    #[cold]
    #[inline(never)]
    pub(crate) fn control_attribute_changed(
        &self,
        element: NodeRef<'gc>,
        local: NameId,
        present: bool,
    ) {
        let kind = self.form_kind(element);
        if kind == 0 {
            self.image_attribute_changed(element, local);
            self.script_attribute_changed(element, local, present);
            self.link_rel_changed(element, local);
        }
        match local {
            names::local::FORM => self.form_attribute_changed(element, present),
            // A button's type: its form's default button may be another.
            names::local::TYPE if kind & INPUT == 0 => {
                if let Some(form) = self.form_owner(element) {
                    self.default_button_changed(form);
                }
            }
            names::local::DISABLED if kind & FIELDSET != 0 => {
                self.fieldset_disabled_changed(element)
            }
            // Disabled by its own attribute and read-only: its native look
            // shows them (a select's options are disabled with it).
            names::local::DISABLED if kind & DISABLABLE != 0 => {
                if self.is_connected(element) {
                    self.control_look_changed(element);
                }
                self.select_attribute_changed(element, local, present);
            }
            names::local::READONLY if kind & (INPUT | TEXTAREA) != 0 => {
                if self.is_connected(element) {
                    self.control_look_changed(element);
                }
            }
            _ if kind & INPUT != 0 => self.input_attribute_changed(element, local, present),
            // An option's, an optgroup's, a select's.
            names::local::SELECTED
            | names::local::DISABLED
            | names::local::MULTIPLE
            | names::local::SIZE
            | names::local::LABEL => {
                self.select_attribute_changed(element, local, present);
            }
            names::local::PLACEHOLDER if kind & TEXTAREA != 0 => self.shadow_tree_changed(element),
            _ => {}
        }
    }

    /// `element`'s `form` attribute was set or removed (`present`): a
    /// listed element's owner is reset (Blink's
    /// `ListedElement::FormAttributeChanged`).
    fn form_attribute_changed(&self, element: NodeRef<'gc>, present: bool) {
        if !self.is_listed(element) {
            return;
        }
        let state = &self.changes.forms;
        self.see(FORMS_SEEN);
        // (One that lost it is forgotten at the next reset of them.)
        if present {
            let id = self.node_id(element);
            let mut attributed = state.attributed.take();
            if !attributed.contains(&id) {
                attributed.push(id);
            }
            state.attributed.set(attributed);
        }
        self.reset_form_owner(element);
    }

    /// An id changed in a connected tree (`attribute_changed`): what `form`
    /// attributes name may be another form.
    #[inline]
    pub(crate) fn form_id_changed(&self) {
        if self.has_seen(FORMS_SEEN) {
            self.reset_attributed();
        }
    }

    /// The owner of each listed element with a `form` attribute reset;
    /// those collected or with none since forgotten.
    fn reset_attributed(&self) {
        let state = &self.changes.forms;
        let mut attributed = state.attributed.take();
        attributed.retain(|id| match self.node_by_id(*id) {
            Some(element)
                if self
                    .attribute_value_by_id(element, names::local::FORM)
                    .is_some() =>
            {
                self.reset_form_owner(element);
                true
            }
            _ => false,
        });
        // A reset changes no `form` attribute: none were added meanwhile.
        state.attributed.set(attributed);
    }

    /// `form`'s listed elements whose owner it is, in tree order: found in
    /// its subtree (its tree if one outside may be), and kept until an
    /// owner changes.
    pub(crate) fn form_listed_elements(&self, form: NodeRef<'gc>) -> Gc<'gc, HeapVector<Node>> {
        let association = self.association_or_new(form);
        if let Some(elements) = association.elements.get(self.gc) {
            return elements;
        }
        FORM_WALKS.set(FORM_WALKS.get().wrapping_add(1));
        let root = if association.outside.get() {
            self.root_node(form, false)
        } else {
            form
        };
        let elements = HeapVector::new(self.gc, 0);
        self.for_each_inclusive_descendant(root, |at| {
            if self.is_listed(at)
                && self
                    .form_owner(at)
                    .is_some_and(|owner| Gc::ptr_eq(owner, form))
            {
                elements.push(self.gc, at);
            }
        });
        association.elements.set(self.gc, Some(elements));
        elements
    }

    /// `f` of each element of `form`'s `elements` (its listed elements but
    /// image buttons), in tree order, until it says stop (`false`).
    pub(crate) fn for_each_form_control(
        &self,
        form: NodeRef<'gc>,
        mut f: impl FnMut(NodeRef<'gc>) -> bool,
    ) {
        let elements = self.form_listed_elements(form);
        for index in 0..elements.len() {
            if let Some(element) = elements.get(self.gc, index)
                && !self.is_image_button(element)
                && !f(element)
            {
                return;
            }
        }
    }

    /// `document.forms`: its `<form>`s, the same each time.
    pub fn document_forms(&self, document: NodeRef<'gc>) -> Gc<'gc, HtmlCollection> {
        let data = self.ensure_document_data(document);
        if let Some(forms) = data.forms.get(self.gc) {
            return forms;
        }
        let forms = self.new_collection(document, crate::collection::FORMS, None);
        data.forms.set(self.gc, Some(forms));
        forms
    }

    /// `form.elements`: the same each time.
    pub fn form_elements(&self, form: NodeRef<'gc>) -> Gc<'gc, HtmlCollection> {
        let association = self.association_or_new(form);
        if let Some(collection) = association.collection.get(self.gc) {
            return collection;
        }
        let collection = self.new_collection(form, crate::collection::FORM_ELEMENTS, None);
        association.collection.set(self.gc, Some(collection));
        collection
    }

    /// Whether `element` has `key` as its `id` or `name` (a listed
    /// element's: HTML).
    fn is_named(&self, element: NodeRef<'gc>, key: &str) -> bool {
        let has = |local| {
            self.attribute_value_by_id(element, local)
                .is_some_and(|value| *value.borrow() == key)
        };
        has(names::ID) || has(names::NAME)
    }

    /// The elements of `form`'s `elements` named `key` (`is_named`): none,
    /// one, or a list of several. None for the empty key.
    fn form_controls_named(&self, form: NodeRef<'gc>, key: &str) -> Option<FormItem<'gc>> {
        if key.is_empty() {
            return None;
        }
        let (mut first, mut several) = (None, false);
        self.for_each_form_control(form, |element| {
            if !self.is_named(element, key) {
                return true;
            }
            if first.is_some() {
                several = true;
                return false;
            }
            first = Some(element);
            true
        });
        let first = first?;
        Some(if several {
            FormItem::List(self.new_radio_node_list(form, key))
        } else {
            FormItem::Element(first)
        })
    }

    /// `HTMLFormControlsCollection`'s `namedItem(key)` (and named getter).
    pub fn form_controls_named_item(
        &self,
        collection: Gc<'gc, HtmlCollection>,
        key: &str,
    ) -> Option<FormItem<'gc>> {
        self.form_controls_named(self.collection_root(collection), key)
    }

    /// `form[name]` (HTML's named getter of a form): the elements of its
    /// `elements` named `name`, one or a list; one is kept in its past
    /// names map, which finds it by the name when none is named so (it
    /// still being the form's). `<img>`s aren't looked at: no owner here.
    pub fn form_named_item(&self, form: NodeRef<'gc>, name: &str) -> Option<FormItem<'gc>> {
        match self.form_controls_named(form, name) {
            Some(FormItem::Element(element)) => {
                self.remember_past_name(form, name, element);
                Some(FormItem::Element(element))
            }
            Some(list) => Some(list),
            None => self.past_name(form, name).map(FormItem::Element),
        }
    }

    fn remember_past_name(&self, form: NodeRef<'gc>, name: &str, element: NodeRef<'gc>) {
        let association = self.association_or_new(form);
        let past = match association.past_names.get(self.gc) {
            Some(past) => past,
            None => {
                let past = self.gc.alloc_in(
                    &sites::OTHER,
                    PastNamesInit {
                        entries: LocalExternal::new(Cell::new(Vec::new())),
                    },
                );
                association.past_names.set(self.gc, Some(past));
                past
            }
        };
        let id = self.node_id(element);
        let cell = past.into_ref().entries.get();
        let mut entries = cell.take();
        match entries.iter_mut().find(|(entry, _)| *entry == name) {
            Some(entry) => entry.1 = id,
            None => entries.push((DomString::new(name), id)),
        }
        cell.set(entries);
    }

    fn past_name(&self, form: NodeRef<'gc>, name: &str) -> Option<NodeRef<'gc>> {
        let past = self.association(form)?.past_names.get(self.gc)?;
        let cell = past.into_ref().entries.get();
        let entries = cell.take();
        let id = entries
            .iter()
            .find(|(entry, _)| *entry == name)
            .map(|(_, id)| *id);
        cell.set(entries);
        self.node_by_id(id?)
    }

    /// `element` is no longer `form`'s: out of its past names map.
    fn forget_past_names(&self, form: NodeRef<'gc>, element: NodeRef<'gc>) {
        let Some(past) = self
            .association(form)
            .and_then(|association| association.past_names.get(self.gc))
        else {
            return;
        };
        let id = self.node_id(element);
        let cell = past.into_ref().entries.get();
        let mut entries = cell.take();
        entries.retain(|(_, entry)| *entry != id);
        cell.set(entries);
    }

    /// A `RadioNodeList` of the elements of `form`'s `elements` named
    /// `name`, live.
    fn new_radio_node_list(&self, form: NodeRef<'gc>, name: &str) -> Gc<'gc, NodeList> {
        let radio = self.gc.alloc_in(
            &sites::OTHER,
            RadioListInit {
                form: Some(form),
                name: LocalExternal::new(DomString::new(name)),
            },
        );
        self.new_live_node_list(radio)
    }

    /// `radio`'s elements, each given to `f` until it says stop.
    pub(crate) fn for_each_radio_list_node(
        &self,
        radio: Gc<'gc, RadioList>,
        mut f: impl FnMut(NodeRef<'gc>) -> bool,
    ) {
        let form = radio.form.get(self.gc).expect("a radio list's form");
        let name = radio.into_ref().name.get().to_str();
        self.for_each_form_control(form, |element| !self.is_named(element, &name) || f(element));
    }

    /// `fieldset.elements`: its listed descendants, the same each time.
    pub fn fieldset_elements(&self, fieldset: NodeRef<'gc>) -> Gc<'gc, HtmlCollection> {
        let association = self.association_or_new(fieldset);
        if let Some(collection) = association.collection.get(self.gc) {
            return collection;
        }
        let collection = self.new_collection(fieldset, crate::collection::FIELDSET_ELEMENTS, None);
        association.collection.set(self.gc, Some(collection));
        collection
    }

    /// `legend.form`: its parent's form owner if a `<fieldset>`, else none.
    pub fn legend_form(&self, legend: NodeRef<'gc>) -> Option<NodeRef<'gc>> {
        let parent = self.parent(legend)?;
        if self.qualified_name(parent) != Some(QualifiedName::html(names::local::FIELDSET)) {
            return None;
        }
        self.form_owner(parent)
    }

    /// A `<label>`'s labeled control (HTML's; `control`): with a `for`
    /// attribute, the first element in its tree with that id if labelable;
    /// without, its first labelable descendant.
    pub fn label_control(&self, label: NodeRef<'gc>) -> Option<NodeRef<'gc>> {
        if let Some(id) = self.attribute_value_by_id(label, names::local::FOR) {
            let id = id.to_owned();
            let root = self.root_node(label, false);
            return self
                .element_by_id(root, &id.to_str())
                .filter(|found| self.is_labelable(*found));
        }
        let mut at = self.following_within(label, label);
        while let Some(node) = at {
            if self.is_labelable(node) {
                return Some(node);
            }
            at = self.following_within(node, label);
        }
        None
    }

    /// `label.form`: its labeled control's form owner (a listed one's; a
    /// labelable element that isn't listed has none).
    pub fn label_form(&self, label: NodeRef<'gc>) -> Option<NodeRef<'gc>> {
        self.form_owner(self.label_control(label)?)
    }

    /// A labelable element's `labels`, the same each time; none for one
    /// that isn't labelable now (a hidden `<input>`), the list kept for when
    /// it is again.
    pub fn labels(&self, element: NodeRef<'gc>) -> Option<Gc<'gc, NodeList>> {
        if !self.is_labelable(element) {
            return None;
        }
        let association = self.association_or_new(element);
        if let Some(list) = association.labels.get(self.gc) {
            return Some(list);
        }
        let list = self.new_labels_list(element);
        association.labels.set(self.gc, Some(list));
        Some(list)
    }

    /// The `<label>`s in `element`'s tree whose labeled control it is, in
    /// tree order, each given to `f` until it says stop.
    pub(crate) fn for_each_label(
        &self,
        element: NodeRef<'gc>,
        mut f: impl FnMut(NodeRef<'gc>) -> bool,
    ) {
        let root = self.root_node(element, false);
        let label = QualifiedName::html(names::local::LABEL);
        let mut at = Some(root);
        while let Some(node) = at {
            if self.qualified_name(node) == Some(label)
                && self
                    .label_control(node)
                    .is_some_and(|control| Gc::ptr_eq(control, element))
                && !f(node)
            {
                return;
            }
            at = self.following_within(node, root);
        }
    }

    /// HTML's reset of `form`'s controls, after its `reset` event: each of
    /// its listed elements' reset algorithm (an input's, a textarea's, a
    /// select's, an output's).
    pub fn reset_form_controls(&self, form: NodeRef<'gc>) {
        let elements = self.form_listed_elements(form);
        let elements: Vec<NodeRef<'gc>> = (0..elements.len())
            .filter_map(|index| elements.get(self.gc, index))
            .collect();
        for element in elements {
            self.input_reset(element);
            self.textarea_reset(element);
            self.select_reset(element);
            self.output_reset(element);
        }
    }

    /// Whether `form` is locked for reset (`reset()` is resetting it); and
    /// locks or unlocks it.
    pub fn form_locked_for_reset(&self, form: NodeRef<'gc>) -> bool {
        self.association(form)
            .is_some_and(|association| association.locked_for_reset.get())
    }

    pub fn set_form_locked_for_reset(&self, form: NodeRef<'gc>, locked: bool) {
        self.association_or_new(form).locked_for_reset.set(locked);
    }

    /// `RadioNodeList`'s `value`: the first of its radio buttons that is
    /// checked's `value`, or the empty string.
    pub fn radio_list_value(&self, list: Gc<'gc, NodeList>) -> DomString {
        let mut found = None;
        if let Some(radio) = self.node_list_radio(list) {
            self.for_each_radio_list_node(radio, |node| {
                if self.input_kind(node) == Some(InputKind::Radio) && self.checkedness(node) {
                    found = Some(node);
                    return false;
                }
                true
            });
        }
        found.map_or_else(|| DomString::new(""), |radio| self.input_value(radio))
    }

    /// Setting `RadioNodeList`'s `value`: the first of its radio buttons
    /// whose `value` attribute is the value (or, for `on`, that has none)
    /// is checked.
    pub fn set_radio_list_value(&self, list: Gc<'gc, NodeList>, value: &str) {
        let mut found = None;
        if let Some(radio) = self.node_list_radio(list) {
            self.for_each_radio_list_node(radio, |node| {
                let matches = self.input_kind(node) == Some(InputKind::Radio)
                    && match self.attribute_value_by_id(node, names::local::VALUE) {
                        Some(attribute) => *attribute.borrow() == value,
                        None => value == "on",
                    };
                if matches {
                    found = Some(node);
                }
                !matches
            });
        }
        if let Some(radio) = found {
            self.set_checked(radio, true);
        }
    }

    /// Whether `element` is a disabled form control or fieldset (HTML's: a
    /// `button`, `fieldset`, `input`, `select` or `textarea` with a
    /// `disabled` attribute, or a descendant of a fieldset with one but not
    /// of its first `<legend>` child; Blink's `IsActuallyDisabled`).
    pub fn is_disabled_form_control(&self, element: NodeRef<'gc>) -> bool {
        self.form_kind(element) & DISABLABLE != 0
            && (self
                .attribute_value_by_id(element, names::local::DISABLED)
                .is_some()
                || self.ancestor_disabled(element))
    }

    /// Whether a fieldset ancestor of `element` disables it: as kept, else
    /// found by a walk up and kept. None before a fieldset is made.
    fn ancestor_disabled(&self, element: NodeRef<'gc>) -> bool {
        if !self.has_seen(FIELDSETS_SEEN) {
            return false;
        }
        let association = self.association_or_new(element);
        match association.ancestor_disabled.get() {
            ENABLED => return false,
            DISABLED => return true,
            _ => {}
        }
        DISABLED_WALKS.set(DISABLED_WALKS.get().wrapping_add(1));
        let fieldset = QualifiedName::html(names::local::FIELDSET);
        let mut child = element;
        let mut disabled = false;
        while let Some(parent) = self.parent(child) {
            if self.qualified_name(parent) == Some(fieldset)
                && self
                    .attribute_value_by_id(parent, names::local::DISABLED)
                    .is_some()
                && self
                    .first_legend_child(parent)
                    .is_none_or(|legend| !Gc::ptr_eq(legend, child))
            {
                disabled = true;
                break;
            }
            child = parent;
        }
        association
            .ancestor_disabled
            .set(if disabled { DISABLED } else { ENABLED });
        disabled
    }

    fn first_legend_child(&self, fieldset: NodeRef<'gc>) -> Option<NodeRef<'gc>> {
        let legend = QualifiedName::html(names::local::LEGEND);
        let mut at = self.first_child(fieldset);
        while let Some(child) = at {
            if self.qualified_name(child) == Some(legend) {
                return Some(child);
            }
            at = self.next_sibling(child);
        }
        None
    }

    /// What `element`'s ancestors say of its being disabled is found again
    /// when next asked for.
    fn ancestor_disabled_changed(&self, element: NodeRef<'gc>) {
        if let Some(association) = self.association(element) {
            association.ancestor_disabled.set(UNKNOWN);
        }
    }

    /// A fieldset's `disabled` attribute changed: its listed descendants'
    /// are found again (Blink's `HTMLFieldSetElement::DisabledAttributeChanged`).
    fn fieldset_disabled_changed(&self, fieldset: NodeRef<'gc>) {
        self.for_each_inclusive_descendant(fieldset, |at| {
            if !Gc::ptr_eq(at, fieldset) && self.is_listed(at) {
                self.ancestor_disabled_changed(at);
                self.state_changed(at, crate::AttributeChanges::DISABLED);
            } else if self.is_option(at) || self.is_optgroup(at) {
                // Disabled with their select.
                self.state_changed(at, crate::AttributeChanges::DISABLED);
            }
        });
    }

    /// `node` came into or left `parent`'s children: a `<legend>` in or out
    /// of a fieldset's may make another its first, the one whose
    /// descendants it doesn't disable (Blink's
    /// `HTMLFieldSetElement::ChildrenChanged`).
    fn fieldset_children_changed(&self, parent: NodeRef<'gc>, node: NodeRef<'gc>) {
        if self.qualified_name(parent) == Some(QualifiedName::html(names::local::FIELDSET))
            && self.qualified_name(node) == Some(QualifiedName::html(names::local::LEGEND))
        {
            self.fieldset_disabled_changed(parent);
        }
    }
}
