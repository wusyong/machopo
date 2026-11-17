//! A worker's global (HTML's `WorkerGlobalScope`, Blink's
//! `WorkerGlobalScope`), a realm's global object as a window is, in a DOM
//! of its own (the worker's thread's): an event target (its listeners its
//! own, an event's path only itself), with an execution context of its own
//! (its timers, tasks, messages), and no document. What a window answers
//! from its document (its URL, its origin) it has itself, given as the
//! worker's script is fetched (HTML's "run a worker").

use elise::{Gc, GcObject, HeapVector, JsEdge, LocalExternal, Member};

use url::Url;

use crate::{Cx, ExecutionContext, Listener, sites};

#[derive(GcObject)]
pub struct WorkerGlobalScope {
    /// Its execution context (Blink's `WorkerGlobalScope` is one, inheriting
    /// `WorkerOrWorkletGlobalScope`).
    pub(crate) context: Member<ExecutionContext>,
    /// Its URL (HTML's worker global scope's url): its script's response's
    /// URL, its API base URL.
    url: LocalExternal<Url>,
    /// Its origin (HTML's worker's settings object's): its owner's for a
    /// script of a `data:` URL, an opaque one, else its URL's.
    origin: LocalExternal<url::Origin>,
    pub(crate) listeners: Member<HeapVector<Listener>>,
    wrapper: JsEdge,
}

impl<'gc> Cx<'_, 'gc> {
    /// A new worker's global, its URL `url` and its origin `origin`, with an
    /// execution context of its own, its time origin now.
    pub fn create_worker_global(
        &self,
        url: Url,
        origin: url::Origin,
    ) -> Gc<'gc, WorkerGlobalScope> {
        let context = self.create_execution_context();
        let global = self.gc.alloc_in(
            &sites::OTHER,
            WorkerGlobalScopeInit {
                context: Some(context),
                url: LocalExternal::new(url),
                origin: LocalExternal::new(origin),
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

    pub(crate) fn worker_global_wrapper_edge(
        &self,
        global: Gc<'gc, WorkerGlobalScope>,
    ) -> &'gc JsEdge {
        &global.into_ref().wrapper
    }
}
