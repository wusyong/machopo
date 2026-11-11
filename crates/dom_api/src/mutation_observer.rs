//! `MutationObserver` and `MutationRecord`
//! (https://dom.spec.whatwg.org/#mutation-observers): the observers'
//! members, the options' checks, and the delivery (DOM's "notify mutation
//! observers"), which the engine's microtask runs. The registrations and
//! the records are `dom`'s.

use dom::{
    DomString, MutationObserver, MutationRecord, NodeList, NodeRef, ObserveOptions, TargetRef,
    TargetRoot, names,
};
use elise::{Gc, Persistent};

use crate::interfaces::MutationObserverInit;
use crate::typed::{Callback, DomStr, NullableStr, OpFront, ScriptHost};
use crate::{ApiCx, ApiError};

/// `new MutationObserver(callback)`.
pub(crate) fn construct<'gc, O: OpFront>(
    op: &mut O,
    cx: &ApiCx<'_, 'gc>,
    callback: Callback<O::Callback>,
) -> Result<Gc<'gc, MutationObserver>, ApiError> {
    let observer = cx.create_mutation_observer();
    op.store_callback(cx, cx.observer_callback_edge(observer), callback);
    Ok(observer)
}

/// `observe(target, options)`: the options checked and completed (an old
/// value or a filter asked for implies what it is of), as DOM's steps 1
/// to 6.
pub(crate) fn observe<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, MutationObserver>,
    target: NodeRef<'gc>,
    options: &MutationObserverInit,
) -> Result<(), ApiError> {
    let attributes = options
        .attributes
        .unwrap_or(options.attributeOldValue.is_some() || options.attributeFilter.is_some());
    let character_data = options
        .characterData
        .unwrap_or(options.characterDataOldValue.is_some());
    if !options.childList && !attributes && !character_data {
        return Err(ApiError::type_error(
            "The options object must set at least one of 'attributes', 'characterData', or 'childList' to true.",
        ));
    }
    if options.attributeOldValue == Some(true) && !attributes {
        return Err(ApiError::type_error(
            "The options object may only set 'attributeOldValue' to true when 'attributes' is true or not present.",
        ));
    }
    if options.attributeFilter.is_some() && !attributes {
        return Err(ApiError::type_error(
            "The options object may only set 'attributeFilter' when 'attributes' is true or not present.",
        ));
    }
    if options.characterDataOldValue == Some(true) && !character_data {
        return Err(ApiError::type_error(
            "The options object may only set 'characterDataOldValue' to true when 'characterData' is true or not present.",
        ));
    }
    let filter: Option<Vec<&str>> = options
        .attributeFilter
        .as_ref()
        .map(|names| names.iter().map(String::as_str).collect());
    cx.observe(
        this,
        target,
        ObserveOptions {
            child_list: options.childList,
            attributes,
            character_data,
            subtree: options.subtree,
            attribute_old_value: options.attributeOldValue == Some(true),
            character_data_old_value: options.characterDataOldValue == Some(true),
        },
        filter.as_deref(),
    );
    Ok(())
}

pub(crate) fn disconnect<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, MutationObserver>,
) -> Result<(), ApiError> {
    cx.disconnect(this);
    Ok(())
}

pub(crate) fn take_records<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, MutationObserver>,
) -> Result<Vec<Gc<'gc, MutationRecord>>, ApiError> {
    Ok(cx.take_records(this))
}

pub(crate) fn record_type<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, MutationRecord>,
) -> Result<DomStr<'gc>, ApiError> {
    Ok(DomStr::Owned(DomString::new(cx.record_kind(this).name())))
}

pub(crate) fn target<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, MutationRecord>,
) -> Result<NodeRef<'gc>, ApiError> {
    Ok(cx.record_target(this))
}

pub(crate) fn added_nodes<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, MutationRecord>,
) -> Result<Gc<'gc, NodeList>, ApiError> {
    Ok(cx.record_added(this))
}

pub(crate) fn removed_nodes<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, MutationRecord>,
) -> Result<Gc<'gc, NodeList>, ApiError> {
    Ok(cx.record_removed(this))
}

pub(crate) fn previous_sibling<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, MutationRecord>,
) -> Result<Option<NodeRef<'gc>>, ApiError> {
    Ok(cx.record_previous_sibling(this))
}

pub(crate) fn next_sibling<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, MutationRecord>,
) -> Result<Option<NodeRef<'gc>>, ApiError> {
    Ok(cx.record_next_sibling(this))
}

pub(crate) fn attribute_name<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, MutationRecord>,
) -> Result<NullableStr<'gc>, ApiError> {
    Ok(NullableStr(cx.record_attribute(this).0.map(DomStr::Name)))
}

pub(crate) fn attribute_namespace<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, MutationRecord>,
) -> Result<NullableStr<'gc>, ApiError> {
    Ok(NullableStr(cx.record_attribute(this).1.map(DomStr::Name)))
}

pub(crate) fn old_value<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, MutationRecord>,
) -> Result<NullableStr<'gc>, ApiError> {
    Ok(NullableStr::from(cx.record_old_value(this)))
}

/// DOM's "notify mutation observers" (the microtask's): each observer
/// that had records or transient registrations, in order, its transient
/// registrations gone and its records taken, its callback called with
/// them if there are any (each in an operation of its own: the callback
/// runs script); then a `slotchange` (bubbling) at each signal slot, the
/// slots taken with the observers.
pub fn deliver_mutations<H: ScriptHost>(host: &mut H) {
    let (observers, slots): (Vec<Persistent<MutationObserver>>, Vec<TargetRoot>) =
        host.mutate(|cx| {
            let observers = cx
                .take_pending_observers()
                .into_iter()
                .map(|observer| Persistent::new(cx.gc, observer))
                .collect();
            let slots = cx
                .take_signal_slots()
                .into_iter()
                .map(|slot| cx.root_target(TargetRef::Node(slot)))
                .collect();
            (observers, slots)
        });
    for observer in &observers {
        let records: Vec<Persistent<MutationRecord>> = host.mutate(|cx| {
            let this = observer.get(cx.gc);
            cx.remove_transient_registrations(this);
            cx.take_records(this)
                .into_iter()
                .map(|record| Persistent::new(cx.gc, record))
                .collect()
        });
        if !records.is_empty() {
            host.call_mutation_callback(observer, &records);
        }
    }
    for slot in &slots {
        crate::event_target::fire_event(host, slot, names::event::SLOTCHANGE, true, false);
    }
}
