//! WebIDL's named properties (`dataset.fooBar`; Blink's generated
//! `NamedPropertyGetterCallback` and the rest): interceptors on an
//! interface's instances that call its anonymous special operations'
//! typed paths (`#[named(..)]`) through the front end every member runs
//! on (`info_front::InfoFront`), each callback V8's own, given the
//! callback info alone.
//!
//! - Get: the getter (a string, or an object: `form[name]`); its none is
//!   no property (V8 looks on).
//! - Query: the getter too, asked only whether it gives the name (nothing
//!   of V8's is made for what it gives); a named property is
//!   configurable, writable with a setter, enumerable with its names
//!   listed (else as with `[LegacyUnenumerableNamedProperties]`).
//! - Set: the setter; one that gives `false` didn't take the name, an
//!   ordinary property's (`style.notAProperty = 1`).
//! - Define: a value goes through the setter; a getter or setter is
//!   refused (WebIDL's [[DefineOwnProperty]]).
//! - Delete: a named property through the deleter; another name is an
//!   ordinary property's.
//! - Enumerate: the supported names, own keys.
//!
//! An array index is a name like any other (`dataset[9]`), as in Blink's
//! bindings: its digits.
//!
//! An interface's indexed setter (`select[0] = option`) is called from
//! its indexed interceptor (`IndexedSetter`): the index and the value its
//! arguments, the setter always taking the index.

use dom_api::{Named, TypedMember};

use crate::info_front::{
    Has, IndexedSet, InfoFront, NamedDelete, NamedGet, NamedHas, NamedSet, Names,
};
use crate::state::State;

type NameInfo<'s> = bindings::StateNameInfo<'s, State>;
type IndexInfo<'s> = bindings::StateIndexInfo<'s, State>;

/// A special operation's typed path through the front end that is the
/// interceptor's info: the getter; whether it gives the name (the info
/// back, for a deleter that goes on); the setter; whether the deleter
/// threw; and a call of no name's (the supported names, an indexed
/// setter).
type GetCall = for<'s> fn(NameInfo<'s>) -> v8::Intercepted;
type HasCall = for<'s> fn(NameInfo<'s>) -> (NameInfo<'s>, Has);
type AssignCall = for<'s> fn(NameInfo<'s>) -> bindings::NamedAssigned;
type DeleteCall = for<'s> fn(NameInfo<'s>) -> bool;
type IndexCall = for<'s> fn(IndexInfo<'s>);

/// An interface's named properties' special operations, found as its
/// template is made.
#[derive(Clone, Copy, Default)]
pub(crate) struct NamedCalls {
    get: Option<GetCall>,
    has: Option<HasCall>,
    assign: Option<AssignCall>,
    delete: Option<DeleteCall>,
    names: Option<IndexCall>,
    /// The indexed setter (`#[indexed(setter)]`), which the indexed
    /// interceptor calls.
    pub(crate) indexed_set: Option<IndexCall>,
    /// The getter's and the setter's own callbacks: what a string name's
    /// get and set call (`NamedGetter`, `NamedSetter`).
    get_info: Option<v8::NamedPropertyGetterCallback>,
    set_info: Option<v8::NamedPropertySetterCallback>,
}

impl NamedCalls {
    pub(crate) fn add<M: TypedMember>(&mut self, named: Named) {
        match named {
            Named::Getter => {
                self.get_info = Some(bindings::info_named_getter::<State, NamedGetter<M>>());
                self.get = Some(get_name::<M>);
                self.has = Some(has_name::<M>);
            }
            Named::Setter => {
                self.set_info = Some(bindings::info_named_setter::<State, NamedSetter<M>>());
                self.assign = Some(assign_name::<M>);
            }
            Named::Deleter => self.delete = Some(delete_name::<M>),
            Named::Names => self.names = Some(names::<M>),
        }
    }

    pub(crate) fn add_indexed_setter<M: TypedMember>(&mut self) {
        self.indexed_set = Some(indexed_set::<M>);
    }

    /// The interceptors' options, if the interface has named properties
    /// (a getter): for one whose named properties shadow its own
    /// (`masking`) or not, and with indexed properties of its own or not.
    pub(crate) fn options(&self, masking: bool, indexed: bool) -> Option<bindings::NamedOptions> {
        Some(bindings::NamedOptions {
            masking,
            deleter: self.delete.is_some(),
            enumerator: self.names.is_some(),
            indices: !indexed,
            getter_info: self.get_info?,
            setter_info: self.set_info,
        })
    }
}

/// The named getter `M`'s call: a result that is null is no property of
/// the name, and V8 looks on (the object's own, its prototypes').
fn get_name<M: TypedMember>(info: NameInfo<'_>) -> v8::Intercepted {
    let mut front = InfoFront::<NamedGet>::new(info);
    crate::templates::census_label::<M>();
    M::call(&mut front);
    if front.noted().0 {
        v8::Intercepted::kNo
    } else {
        v8::Intercepted::kYes
    }
}

/// Whether the named getter `M` gives `info`'s name: its call with
/// nothing of V8's made for what it gives (`info_front::NamedHas`).
fn has_name<M: TypedMember>(info: NameInfo<'_>) -> (NameInfo<'_>, Has) {
    let mut front = InfoFront::<NamedHas>::new(info);
    crate::templates::census_label::<M>();
    M::call(&mut front);
    let has = *front.noted();
    (front.into_info(), has)
}

/// The named setter `M`'s call: one that gives `false` didn't take the
/// name, an ordinary property's.
fn assign_name<M: TypedMember>(info: NameInfo<'_>) -> bindings::NamedAssigned {
    let mut front = InfoFront::<NamedSet>::new(info);
    crate::templates::census_label::<M>();
    M::call(&mut front);
    front.report();
    let noted = front.noted();
    if noted.threw {
        bindings::NamedAssigned::Threw
    } else if noted.declined {
        bindings::NamedAssigned::Declined
    } else {
        bindings::NamedAssigned::Took
    }
}

/// The named deleter `M`'s call; whether it threw.
fn delete_name<M: TypedMember>(info: NameInfo<'_>) -> bool {
    let mut front = InfoFront::<NamedDelete>::new(info);
    crate::templates::census_label::<M>();
    M::call(&mut front);
    front.report();
    front.noted().0
}

/// The supported names' member `M`'s call: their array the result.
fn names<M: TypedMember>(info: IndexInfo<'_>) {
    let mut front = InfoFront::<Names>::new(info);
    crate::templates::census_label::<M>();
    M::call(&mut front);
}

/// The indexed setter `M`'s call.
fn indexed_set<M: TypedMember>(info: IndexInfo<'_>) {
    let mut front = InfoFront::<IndexedSet>::new(info);
    crate::templates::census_label::<M>();
    M::call(&mut front);
    front.report();
}

/// `object.name` of the named getter `M`: a string name's own callback.
struct NamedGetter<M>(std::marker::PhantomData<M>);

impl<M: TypedMember> bindings::StateInfoNamedGetter<State> for NamedGetter<M> {
    fn get(info: NameInfo<'_>) -> v8::Intercepted {
        get_name::<M>(info)
    }
}

/// `object.name = value` of the named setter `M`: a string name's own
/// callback.
struct NamedSetter<M>(std::marker::PhantomData<M>);

impl<M: TypedMember> bindings::StateInfoNamedSetter<State> for NamedSetter<M> {
    fn set(info: NameInfo<'_>) -> v8::Intercepted {
        match assign_name::<M>(info) {
            bindings::NamedAssigned::Declined => v8::Intercepted::kNo,
            bindings::NamedAssigned::Took | bindings::NamedAssigned::Threw => v8::Intercepted::kYes,
        }
    }
}

/// The special operations of the interceptor's interface (its index the
/// callbacks' data).
fn calls_of<'s>(
    info: &mut impl bindings::InfoState<'s, State>,
    data: v8::Local<'s, v8::Value>,
) -> NamedCalls {
    let interface = v8::machopo::as_smi(data)
        .and_then(|interface| usize::try_from(interface).ok())
        .expect("named interceptors carry their interface's index");
    info.state().0.named[interface]
}

/// The interceptor's callbacks that aren't an interface's own: found by
/// the interface's index in their data.
pub(crate) struct NamedProperties;

impl bindings::StateInfoNamed<State> for NamedProperties {
    fn get(mut info: NameInfo<'_>) -> v8::Intercepted {
        let data = info.data();
        let calls = calls_of(&mut info, data);
        calls.get.expect("a named getter")(info)
    }

    fn assign(mut info: NameInfo<'_>) -> bindings::NamedAssigned {
        let data = info.data();
        let calls = calls_of(&mut info, data);
        calls.assign.expect("a named setter")(info)
    }

    /// The getter asked whether it gives the name; a named property is
    /// configurable, writable with a setter, enumerable with its names
    /// listed.
    fn query(mut info: NameInfo<'_>) -> bindings::NamedQueried {
        let data = info.data();
        let calls = calls_of(&mut info, data);
        let (_, has) = calls.has.expect("a named getter")(info);
        match has {
            Has::No => bindings::NamedQueried::None,
            Has::Threw => bindings::NamedQueried::Threw,
            Has::Yes => {
                let mut attributes = v8::PropertyAttribute::NONE;
                if calls.assign.is_none() {
                    attributes = attributes | v8::PropertyAttribute::READ_ONLY;
                }
                if calls.names.is_none() {
                    attributes = attributes | v8::PropertyAttribute::DONT_ENUM;
                }
                bindings::NamedQueried::Has(attributes)
            }
        }
    }

    /// A named property through the deleter; another name is an ordinary
    /// property's.
    fn delete(mut info: NameInfo<'_>) -> bindings::NamedDeleted {
        let data = info.data();
        let calls = calls_of(&mut info, data);
        let (info, has) = calls.has.expect("a named getter")(info);
        match has {
            Has::No => return bindings::NamedDeleted::None,
            Has::Threw => return bindings::NamedDeleted::Threw,
            Has::Yes => {}
        }
        if calls.delete.expect("a named deleter")(info) {
            bindings::NamedDeleted::Threw
        } else {
            bindings::NamedDeleted::Deleted
        }
    }

    fn enumerate(mut info: IndexInfo<'_>) {
        let data = info.data();
        let calls = calls_of(&mut info, data);
        calls.names.expect("named properties' names")(info);
    }
}

/// The indexed setter of the interceptor's interface (by its index in the
/// callbacks' data), given the index and the value: it takes every index.
pub(crate) struct IndexedSetter;

impl bindings::StateInfoIndexedSetter<State> for IndexedSetter {
    fn set(mut info: IndexInfo<'_>) {
        let data = info.data();
        let calls = calls_of(&mut info, data);
        calls.indexed_set.expect("an indexed setter")(info);
    }
}
