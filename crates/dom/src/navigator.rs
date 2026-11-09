//! A window's `Navigator` (HTML's, Blink's `Navigator`) and its
//! `UserActivation` (HTML's `NavigatorUserActivation`): the same object
//! each time, each knowing its window; what they answer is the window's
//! (its last activation) or the host's. It keeps the objects of the specs
//! that hang off it as they are first asked for: `geolocation`
//! (`geolocation`), `wakeLock` (`wake_lock`), `scheduling` (Blink's
//! `Scheduling`, whose `isInputPending()` asks the host) and `connection`
//! (Network Information's, an event target of its own,
//! `EventTargetImpl`'s `connection`).

use elise::{Gc, GcObject, JsEdge, Member};

use crate::{Cx, EventTargetImpl, Window, sites};

#[derive(GcObject)]
pub struct Navigator {
    window: Member<Window>,
    /// Its `userActivation`, `geolocation`, `wakeLock`, `scheduling` and
    /// `connection`, once asked for.
    user_activation: Member<UserActivation>,
    pub(crate) geolocation: Member<crate::Geolocation>,
    pub(crate) wake_lock: Member<crate::WakeLock>,
    scheduling: Member<Scheduling>,
    connection: Member<EventTargetImpl>,
    wrapper: JsEdge,
}

#[derive(GcObject)]
pub struct UserActivation {
    window: Member<Window>,
    wrapper: JsEdge,
}

#[derive(GcObject)]
pub struct Scheduling {
    window: Member<Window>,
    wrapper: JsEdge,
}

impl crate::Dom {
    /// The browser's user agent string (which its requests send as
    /// `User-Agent`), and whether automation controls it (WebDriver's
    /// webdriver-active flag): the host's, which every frame's
    /// `navigator` answers (Blink's `LocalFrameClient::UserAgent`,
    /// `--enable-automation`).
    pub fn set_agent(&mut self, user_agent: &str, webdriver: bool) {
        user_agent.clone_into(&mut self.changes.user_agent.borrow_mut());
        self.changes.webdriver.set(webdriver);
    }
}

impl<'gc> Cx<'_, 'gc> {
    /// The browser's user agent (`Dom::set_agent`); empty if none was set.
    pub fn user_agent(&self) -> String {
        self.changes.user_agent.borrow().clone()
    }

    /// Whether automation controls the browser (`Dom::set_agent`).
    pub fn webdriver(&self) -> bool {
        self.changes.webdriver.get()
    }

    /// `window`'s navigator, the same each time.
    pub fn window_navigator(&self, window: Gc<'gc, Window>) -> Gc<'gc, Navigator> {
        if let Some(navigator) = window.navigator.get(self.gc) {
            return navigator;
        }
        let navigator = self.gc.alloc_in(
            &sites::OTHER,
            NavigatorInit {
                window: Some(window),
                user_activation: None,
                geolocation: None,
                wake_lock: None,
                scheduling: None,
                connection: None,
                wrapper: (),
            },
        );
        window.navigator.set(self.gc, Some(navigator));
        navigator
    }

    pub fn navigator_window(&self, navigator: Gc<'gc, Navigator>) -> Gc<'gc, Window> {
        navigator.window.get(self.gc).expect("a navigator's window")
    }

    /// `navigator`'s `userActivation`, the same each time.
    pub fn navigator_user_activation(
        &self,
        navigator: Gc<'gc, Navigator>,
    ) -> Gc<'gc, UserActivation> {
        if let Some(activation) = navigator.user_activation.get(self.gc) {
            return activation;
        }
        let activation = self.gc.alloc_in(
            &sites::OTHER,
            UserActivationInit {
                window: navigator.window.get(self.gc),
                wrapper: (),
            },
        );
        navigator.user_activation.set(self.gc, Some(activation));
        activation
    }

    pub fn user_activation_window(&self, activation: Gc<'gc, UserActivation>) -> Gc<'gc, Window> {
        activation
            .window
            .get(self.gc)
            .expect("a user activation's window")
    }

    /// `navigator`'s `scheduling`, the same each time.
    pub fn navigator_scheduling(&self, navigator: Gc<'gc, Navigator>) -> Gc<'gc, Scheduling> {
        if let Some(scheduling) = navigator.scheduling.get(self.gc) {
            return scheduling;
        }
        let scheduling = self.gc.alloc_in(
            &sites::OTHER,
            SchedulingInit {
                window: navigator.window.get(self.gc),
                wrapper: (),
            },
        );
        navigator.scheduling.set(self.gc, Some(scheduling));
        scheduling
    }

    pub fn scheduling_window(&self, scheduling: Gc<'gc, Scheduling>) -> Gc<'gc, Window> {
        scheduling
            .window
            .get(self.gc)
            .expect("a scheduling's window")
    }

    /// `navigator`'s `connection`, the same each time: an event target of
    /// its window's.
    pub fn navigator_connection(&self, navigator: Gc<'gc, Navigator>) -> Gc<'gc, EventTargetImpl> {
        if let Some(connection) = navigator.connection.get(self.gc) {
            return connection;
        }
        let connection = self.create_event_target();
        connection
            .connection
            .set(self.gc, navigator.window.get(self.gc));
        navigator.connection.set(self.gc, Some(connection));
        connection
    }

    pub fn is_network_information(&self, target: Gc<'gc, EventTargetImpl>) -> bool {
        target.connection.get(self.gc).is_some()
    }

    /// The window whose navigator's `connection` `target` is; none for
    /// another target.
    pub fn network_information_window(
        &self,
        target: Gc<'gc, EventTargetImpl>,
    ) -> Option<Gc<'gc, Window>> {
        target.connection.get(self.gc)
    }

    pub(crate) fn navigator_wrapper_edge(&self, navigator: Gc<'gc, Navigator>) -> &'gc JsEdge {
        &navigator.into_ref().wrapper
    }

    pub(crate) fn scheduling_wrapper_edge(&self, scheduling: Gc<'gc, Scheduling>) -> &'gc JsEdge {
        &scheduling.into_ref().wrapper
    }

    pub(crate) fn user_activation_wrapper_edge(
        &self,
        activation: Gc<'gc, UserActivation>,
    ) -> &'gc JsEdge {
        &activation.into_ref().wrapper
    }
}
