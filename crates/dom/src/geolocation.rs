//! A navigator's `geolocation` (Blink's `core/geolocation/geolocation.h`):
//! the same object each time, which the navigator keeps. Its requests
//! (Blink's `GeoNotifier`s: `getCurrentPosition`'s and `watchPosition`'s,
//! told apart by the watch id, both of Blink's lists in one) wait in the
//! order made, each with its two callbacks, the JS engine's; one the page
//! is done with (timed out, its watch cleared) is taken out. A
//! `GeolocationPositionError` is an error's code. When the requests are
//! answered is `dom_api`'s.

use std::cell::Cell;

use elise::{Gc, GcObject, HeapVector, JsEdge, Member};

use crate::{Cx, Navigator, Window, sites};

#[derive(GcObject)]
pub struct Geolocation {
    window: Member<Window>,
    requests: Member<HeapVector<GeoRequest>>,
    /// The id the next request gets (what its timer's task finds it by),
    /// and the next watch's (`watchPosition`'s, from 1).
    next_id: Cell<u32>,
    next_watch_id: Cell<i32>,
    /// Whether the permission was asked for and not answered yet (Blink's
    /// `permission_request_in_progress_`).
    asking: Cell<bool>,
    wrapper: JsEdge,
}

/// One `getCurrentPosition` or `watchPosition` request.
#[derive(GcObject)]
pub struct GeoRequest {
    id: Cell<u32>,
    /// A watch's id; 0 for a one-shot request.
    watch_id: Cell<i32>,
    /// Its options' `timeout`, in ms.
    timeout: Cell<u32>,
    /// Whether it was started (Blink's `Geolocation::StartRequest`): not
    /// while the page was hidden, until it shows.
    started: Cell<bool>,
    success: JsEdge,
    error: JsEdge,
}

/// A task of a geolocation's (in its window's element task queue).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GeolocationStep {
    /// The timer of the request the number names fired (Blink's
    /// `GeoNotifier::TimerFired`).
    Timer(u32),
    /// The permission was asked for (on its way to the browser, in
    /// Chrome), or `answered`.
    Permission { answered: bool },
}

/// What an error callback gets: its code (`GeolocationPositionError`'s
/// constants), which its message goes by.
#[derive(GcObject)]
pub struct GeolocationPositionError {
    code: Cell<u16>,
    wrapper: JsEdge,
}

impl<'gc> Cx<'_, 'gc> {
    /// `navigator`'s `geolocation`, the same each time.
    pub fn navigator_geolocation(&self, navigator: Gc<'gc, Navigator>) -> Gc<'gc, Geolocation> {
        if let Some(geolocation) = navigator.geolocation.get(self.gc) {
            return geolocation;
        }
        let geolocation = self.gc.alloc_in(
            &sites::OTHER,
            GeolocationInit {
                window: Some(self.navigator_window(navigator)),
                requests: None,
                next_id: Cell::new(1),
                next_watch_id: Cell::new(1),
                asking: Cell::new(false),
                wrapper: (),
            },
        );
        navigator.geolocation.set(self.gc, Some(geolocation));
        geolocation
    }

    /// `window`'s `navigator.geolocation`, if script made it.
    pub fn window_geolocation(&self, window: Gc<'gc, Window>) -> Option<Gc<'gc, Geolocation>> {
        window.navigator.get(self.gc)?.geolocation.get(self.gc)
    }

    pub fn geolocation_window(&self, geolocation: Gc<'gc, Geolocation>) -> Gc<'gc, Window> {
        geolocation
            .window
            .get(self.gc)
            .expect("a geolocation's window")
    }

    /// A new request of `geolocation`'s, a watch if `watch`, with its
    /// options' `timeout` (ms), started if `started`; its callbacks are
    /// the engine's to set. Its id, and its watch id (0 for a one-shot
    /// one).
    pub fn add_geolocation_request(
        &self,
        geolocation: Gc<'gc, Geolocation>,
        watch: bool,
        timeout: u32,
        started: bool,
    ) -> Gc<'gc, GeoRequest> {
        let id = geolocation.next_id.get();
        geolocation.next_id.set(id.wrapping_add(1));
        let watch_id = if watch {
            let watch_id = geolocation.next_watch_id.get();
            // Blink's ids are `int`s too; one page won't watch 2^31 times.
            geolocation.next_watch_id.set(watch_id.saturating_add(1));
            watch_id
        } else {
            0
        };
        let request = self.gc.alloc_in(
            &sites::OTHER,
            GeoRequestInit {
                id: Cell::new(id),
                watch_id: Cell::new(watch_id),
                timeout: Cell::new(timeout),
                started: Cell::new(started),
                success: (),
                error: (),
            },
        );
        let requests = match geolocation.requests.get(self.gc) {
            Some(requests) => requests,
            None => {
                let requests = HeapVector::new(self.gc, 1);
                geolocation.requests.set(self.gc, Some(requests));
                requests
            }
        };
        requests.push(self.gc, request);
        request
    }

    /// `request`'s id, watch id (0 for a one-shot one) and `timeout`.
    pub fn geolocation_request(&self, request: Gc<'gc, GeoRequest>) -> (u32, i32, u32) {
        (
            request.id.get(),
            request.watch_id.get(),
            request.timeout.get(),
        )
    }

    /// `request`'s references to its success and error callbacks, which
    /// only the JS engine reads and writes (the error's empty without
    /// one).
    pub fn geolocation_request_edges(
        &self,
        request: Gc<'gc, GeoRequest>,
    ) -> (&'gc JsEdge, &'gc JsEdge) {
        let request = request.into_ref();
        (&request.success, &request.error)
    }

    /// Request `id`'s timer fired (Blink's `GeoNotifier::TimerFired`): the
    /// request, if it still waits; a one-shot one is done
    /// (`Geolocation::RequestTimedOut`), taken out.
    pub fn geolocation_timed_out(
        &self,
        geolocation: Gc<'gc, Geolocation>,
        id: u32,
    ) -> Option<Gc<'gc, GeoRequest>> {
        let requests = geolocation.requests.get(self.gc)?;
        let index = (0..requests.len()).find(|&index| {
            requests
                .get(self.gc, index)
                .is_some_and(|r| r.id.get() == id)
        })?;
        let request = requests.get(self.gc, index)?;
        if request.watch_id.get() == 0 {
            requests.remove(self.gc, index);
        }
        Some(request)
    }

    /// `clearWatch(watch_id)`: that watch, if any, taken out.
    pub fn geolocation_clear_watch(&self, geolocation: Gc<'gc, Geolocation>, watch_id: i32) {
        let Some(requests) = geolocation.requests.get(self.gc) else {
            return;
        };
        if watch_id <= 0 {
            return;
        }
        if let Some(index) = (0..requests.len()).find(|&index| {
            requests
                .get(self.gc, index)
                .is_some_and(|r| r.watch_id.get() == watch_id)
        }) {
            requests.remove(self.gc, index);
        }
    }

    /// The requests that weren't started (made while the page was
    /// hidden), started now, in the order made.
    pub fn geolocation_start_waiting(
        &self,
        geolocation: Gc<'gc, Geolocation>,
    ) -> Vec<Gc<'gc, GeoRequest>> {
        let Some(requests) = geolocation.requests.get(self.gc) else {
            return Vec::new();
        };
        (0..requests.len())
            .filter_map(|index| requests.get(self.gc, index))
            .filter(|request| !request.started.replace(true))
            .collect()
    }

    /// Asks for the permission (Blink's `UpdateGeolocationState`) unless
    /// it is being asked for: whether it is asked now.
    pub fn geolocation_ask_permission(&self, geolocation: Gc<'gc, Geolocation>) -> bool {
        !geolocation.asking.replace(true)
    }

    /// A fatal error (Blink's `Geolocation::HandleError`): the requests
    /// waiting, the one-shot ones first, each kind in the order made,
    /// taken out; the permission isn't being asked for any more.
    pub fn geolocation_fail_all(
        &self,
        geolocation: Gc<'gc, Geolocation>,
    ) -> Vec<Gc<'gc, GeoRequest>> {
        geolocation.asking.set(false);
        let Some(requests) = geolocation.requests.get(self.gc) else {
            return Vec::new();
        };
        let all: Vec<_> = (0..requests.len())
            .filter_map(|index| requests.get(self.gc, index))
            .collect();
        requests.clear(self.gc);
        let (mut one_shots, watchers): (Vec<_>, Vec<_>) =
            all.into_iter().partition(|r| r.watch_id.get() == 0);
        one_shots.extend(watchers);
        one_shots
    }

    /// A task of `geolocation`'s queued in its window.
    pub fn queue_geolocation_task(&self, geolocation: Gc<'gc, Geolocation>, step: GeolocationStep) {
        let window = self.geolocation_window(geolocation);
        let data = match step {
            GeolocationStep::Timer(id) => u64::from(id).wrapping_shl(2),
            GeolocationStep::Permission { answered } => 1 | (u64::from(answered) << 1),
        };
        let document = self.window_document(window);
        self.push_element_task(
            window,
            document,
            crate::names::NONE,
            crate::window::GEOLOCATION,
            data,
        );
    }

    /// A new `GeolocationPositionError` of `code`.
    pub fn create_geolocation_position_error(
        &self,
        code: u16,
    ) -> Gc<'gc, GeolocationPositionError> {
        self.gc.alloc_in(
            &sites::OTHER,
            GeolocationPositionErrorInit {
                code: Cell::new(code),
                wrapper: (),
            },
        )
    }

    pub fn geolocation_position_error_code(&self, error: Gc<'gc, GeolocationPositionError>) -> u16 {
        error.code.get()
    }

    pub(crate) fn geolocation_wrapper_edge(
        &self,
        geolocation: Gc<'gc, Geolocation>,
    ) -> &'gc JsEdge {
        &geolocation.into_ref().wrapper
    }

    pub(crate) fn geolocation_position_error_wrapper_edge(
        &self,
        error: Gc<'gc, GeolocationPositionError>,
    ) -> &'gc JsEdge {
        &error.into_ref().wrapper
    }
}
