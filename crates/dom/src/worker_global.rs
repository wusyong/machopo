//! A worker's global (HTML's `WorkerGlobalScope`, Blink's
//! `WorkerGlobalScope`), a realm's global object as a window is, in a DOM
//! of its own (the worker's thread's): an event target (its listeners its
//! own, an event's path only itself), with an execution context of its own
//! (its timers, tasks, messages), and no document. What a window answers
//! from its document (its URL, its origin) it has itself, given as the
//! worker's script is fetched (HTML's "run a worker").

use std::cell::Cell;

use elise::{Gc, GcObject, HeapVector, JsEdge, LocalExternal, Member};

use url::Url;

use crate::{Cx, DomString, ExecutionContext, Listener, sites};

#[derive(GcObject)]
pub struct WorkerGlobalScope {
    /// Its execution context (Blink's `WorkerGlobalScope` is one, inheriting
    /// `WorkerOrWorkletGlobalScope`).
    pub(crate) context: Member<ExecutionContext>,
    /// Its URL (HTML's worker global scope's url): its script's response's
    /// URL, its API base URL.
    url: LocalExternal<Url>,
    /// Its origin (HTML's worker's settings object's): an opaque one for a
    /// script of a `data:` URL, else the origin of the realm that made it
    /// (its URL's if that has none).
    origin: LocalExternal<url::Origin>,
    /// Its name (`new Worker(url, { name })`, `self.name`).
    name: LocalExternal<DomString>,
    /// A module worker's (HTML's worker global scope's type `module`).
    module: Cell<bool>,
    /// Its `location` and `navigator`, once asked for.
    pub(crate) location: Member<crate::Location>,
    pub(crate) navigator: Member<crate::Navigator>,
    pub(crate) listeners: Member<HeapVector<Listener>>,
    wrapper: JsEdge,
}

impl<'gc> Cx<'_, 'gc> {
    /// A new worker's global, its URL `url`, its origin `origin` and its
    /// name `name`, with an execution context of its own, its time origin
    /// now.
    pub fn create_worker_global(
        &self,
        url: Url,
        origin: url::Origin,
        name: DomString,
    ) -> Gc<'gc, WorkerGlobalScope> {
        let context = self.create_execution_context();
        let global = self.gc.alloc_in(
            &sites::OTHER,
            WorkerGlobalScopeInit {
                context: Some(context),
                url: LocalExternal::new(url),
                origin: LocalExternal::new(origin),
                name: LocalExternal::new(name),
                module: Cell::new(false),
                location: None,
                navigator: None,
                listeners: None,
                wrapper: (),
            },
        );
        self.set_context_worker_global(context, global);
        global
    }

    /// `global`'s execution context.
    pub fn worker_global_context(
        &self,
        global: Gc<'gc, WorkerGlobalScope>,
    ) -> Gc<'gc, ExecutionContext> {
        global
            .context
            .get(self.gc)
            .expect("a worker's global has its execution context")
    }

    /// `global`'s URL.
    pub fn worker_global_url(&self, global: Gc<'gc, WorkerGlobalScope>) -> &'gc Url {
        global.into_ref().url.get()
    }

    /// `global`'s origin.
    pub fn worker_global_origin(&self, global: Gc<'gc, WorkerGlobalScope>) -> &'gc url::Origin {
        global.into_ref().origin.get()
    }

    /// `global`'s name.
    pub fn worker_global_name(&self, global: Gc<'gc, WorkerGlobalScope>) -> &'gc DomString {
        global.into_ref().name.get()
    }

    /// `global` is a module worker's (its type `module`), as it is made.
    pub fn set_worker_global_module(&self, global: Gc<'gc, WorkerGlobalScope>) {
        global.module.set(true);
    }

    /// Whether `global` is a module worker's.
    pub fn worker_global_is_module(&self, global: Gc<'gc, WorkerGlobalScope>) -> bool {
        global.module.get()
    }

    pub(crate) fn worker_global_wrapper_edge(
        &self,
        global: Gc<'gc, WorkerGlobalScope>,
    ) -> &'gc JsEdge {
        &global.into_ref().wrapper
    }
}

/// A realm's global, of either kind: what runs a realm's tasks takes it
/// (`dom_api`'s timers, messages, reports), and what is a window's alone
/// (an idle callback's timeout, a `hashchange`) is matched on it, as no
/// function gives an execution context's window (Blink's
/// `DynamicTo<LocalDOMWindow>` of an `ExecutionContext`).
#[derive(Clone, Copy)]
pub enum GlobalRef<'gc> {
    Window(Gc<'gc, crate::Window>),
    WorkerGlobal(Gc<'gc, WorkerGlobalScope>),
}

impl<'gc> GlobalRef<'gc> {
    /// As an event target (an `error` event's, a message's).
    pub fn target(self) -> crate::TargetRef<'gc> {
        match self {
            Self::Window(window) => crate::TargetRef::Window(window),
            Self::WorkerGlobal(global) => crate::TargetRef::WorkerGlobal(global),
        }
    }
}

/// A root of a `GlobalRef` (`Cx::root_global`).
#[derive(Debug, Clone)]
pub enum GlobalRoot {
    Window(elise::Persistent<crate::Window>),
    WorkerGlobal(elise::Persistent<WorkerGlobalScope>),
}

impl GlobalRoot {
    pub fn get<'gc>(&self, cx: &elise::Mutation<'gc>) -> GlobalRef<'gc> {
        match self {
            Self::Window(root) => GlobalRef::Window(root.get(cx)),
            Self::WorkerGlobal(root) => GlobalRef::WorkerGlobal(root.get(cx)),
        }
    }
}

impl<'gc> Cx<'_, 'gc> {
    /// `global`'s execution context.
    pub fn global_context(&self, global: GlobalRef<'gc>) -> Gc<'gc, ExecutionContext> {
        match global {
            GlobalRef::Window(window) => self.window_context(window),
            GlobalRef::WorkerGlobal(global) => self.worker_global_context(global),
        }
    }

    /// A root of `global`.
    pub fn root_global(&self, global: GlobalRef<'gc>) -> GlobalRoot {
        match global {
            GlobalRef::Window(window) => {
                GlobalRoot::Window(elise::Persistent::new(self.gc, window))
            }
            GlobalRef::WorkerGlobal(global) => {
                GlobalRoot::WorkerGlobal(elise::Persistent::new(self.gc, global))
            }
        }
    }
}
