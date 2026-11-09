//! HTML's event handlers on V8: a content attribute's source compiled the
//! first time it is needed (`ScriptHost::compile_event_handler`, Blink's
//! `JSEventHandlerForContentAttribute::GetCompiledHandler`), and what an
//! event handler's call does besides a listener's (`call_listener`: its
//! arguments, its return value).

use dom::{NameId, ObjectRef, TargetRef, TargetRoot, names};

use crate::listeners::{ReportAt, have, report_caught, wrap};
use crate::state::Scope;
use crate::templates;

/// HTML's "getting the current value of the event handler" of `target`'s
/// handler of `event_type`, if it is uncompiled: V8's `CompileFunction`
/// of its source at its line, an element's with its document and itself
/// as context extensions (the scopes HTML puts around it; no form owner
/// yet), a window's with none, its parameters `event` (a window's
/// `onerror`: `event`, `source`, `lineno`, `colno`, `error`), in the
/// window's realm. Its function becomes the handler's callback; a source
/// that doesn't compile is reported at the window, and the handler is
/// deactivated (HTML leaves it in place with a null value: set again, it
/// comes last here).
pub(crate) fn compile_event_handler(
    scope: &mut Scope<'_, '_, '_>,
    target: &TargetRoot,
    event_type: NameId,
) {
    let found = {
        let (state, mut no_gc) = scope.state();
        // Allocates nothing: a read.
        let found = state.dom.read(|cx| {
            let listener = cx.event_handler(target.get(cx.gc), event_type)?;
            let (source, line, element) = cx.uncompiled_handler(listener)?;
            let (scopes, document) = match target.get(cx.gc) {
                TargetRef::Window(window) => (None, cx.node_id(cx.window_document(window))),
                _ => (
                    Some((
                        have(&mut no_gc, cx, ObjectRef::Node(cx.document_of(element))),
                        have(&mut no_gc, cx, ObjectRef::Node(element)),
                    )),
                    cx.node_id(cx.document_of(element)),
                ),
            };
            Some((source, line, scopes, document))
        });
        // HTML's: in the element's (or the window's) realm, the frame's
        // showing its document; the current one's for a document no frame
        // shows.
        found.map(|(source, line, scopes, document)| {
            let realm = state
                .frames
                .of_document(document)
                .map(|frame| frame.context.clone());
            (source, line, scopes, realm)
        })
    };
    let Some((source, line, scopes, realm)) = found else {
        return;
    };
    let extensions: Vec<v8::Local<'_, v8::Object>> = match scopes {
        Some((document, element)) => vec![wrap(scope, document), wrap(scope, element)],
        None => Vec::new(),
    };
    let on_error = matches!(target, TargetRoot::Window(_)) && event_type == names::event::ERROR;
    let params: &[&str] = if on_error {
        &["event", "source", "lineno", "colno", "error"]
    } else {
        &["event"]
    };
    let function = {
        let v8_scope = scope.scope();
        let context = match &realm {
            Some(realm) => v8::Local::new(v8_scope, realm),
            None => v8_scope.get_current_context(),
        };
        let v8_scope = &mut v8::ContextScope::new(v8_scope, context);
        let window = context.global(v8_scope);
        v8::tc_scope!(let tc, v8_scope);
        let code = templates::string(tc, &source.to_str());
        let name = v8::String::empty(tc);
        let line_offset = i32::try_from(line.saturating_sub(1)).unwrap_or(i32::MAX);
        // Shared cross-origin: machopo mutes no script's errors (Blink's
        // `SanitizeScriptErrors::kDoNotSanitize`).
        let origin = v8::ScriptOrigin::new(
            tc,
            name.into(),
            line_offset,
            0,
            true,
            0,
            None,
            false,
            false,
            false,
            None,
        );
        let mut compiled_source = v8::script_compiler::Source::new(code, Some(&origin));
        let params: Vec<_> = params
            .iter()
            .map(|param| templates::string(tc, param))
            .collect();
        let function = v8::script_compiler::compile_function(
            tc,
            &mut compiled_source,
            &params,
            &extensions,
            v8::script_compiler::CompileOptions::NoCompileOptions,
            v8::script_compiler::NoCacheReason::NoReason,
        );
        if function.is_none() {
            report_caught(tc, ReportAt::Global(window));
        }
        function
    };
    let (state, mut no_gc) = scope.state_mut();
    state.dom.mutate(|cx| match function {
        Some(function) => {
            let listener = cx.set_compiled_handler(target.get(cx.gc), event_type);
            no_gc.set_strong(cx.callback_edge(listener), function.into());
        }
        None => cx.remove_event_handler(target.get(cx.gc), event_type),
    });
}
