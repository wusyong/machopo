//! An animation's playback (web-animations 1, "Animations"; Blink's
//! `Animation`'s time and play state): its start time, hold time and
//! playback rate, the pending play and pause tasks, and the procedures
//! that change them (play, pause, finish, cancel, seeking, a new playback
//! rate). Times are on the document timeline, in milliseconds; the
//! timeline's time is `None` when it has none (no document shows it).
//!
//! What a change does to the animation's promises and its `finish` and
//! `cancel` events comes out as [`Notice`]s: the DOM keeps the promises and
//! the objects the events go to.

/// A task waiting for the frame that shows its change (web-animations 1's
/// pending play and pause tasks): done at that frame's ready time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PendingTask {
    Play,
    Pause,
}

/// web-animations 1's play state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayState {
    Idle,
    Running,
    Paused,
    Finished,
}

impl PlayState {
    /// The `AnimationPlayState` enumeration's value.
    pub fn name(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Running => "running",
            Self::Paused => "paused",
            Self::Finished => "finished",
        }
    }
}

/// What a playback step did to the animation's promises and events.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Notice {
    /// The current ready promise is a new, pending one.
    NewReady,
    /// The current ready promise is resolved with the animation.
    ReadyResolved,
    /// The current ready promise is rejected with an `AbortError`, and a
    /// new, resolved one is the current one.
    ReadyAborted,
    /// The current finished promise is resolved with the animation, and a
    /// `finish` event is queued (the finish notification steps).
    Finished {
        current_time: Option<f64>,
        timeline_time: Option<f64>,
    },
    /// The current finished promise is a new, pending one.
    NewFinished,
    /// The current finished promise is rejected with an `AbortError`, a
    /// new, pending one is the current one, and a `cancel` event is
    /// queued.
    Canceled { timeline_time: Option<f64> },
    /// It was replaced (web-animations 1's "remove replaced animations"):
    /// a `remove` event is queued; its promises stay as they are.
    Removed {
        current_time: Option<f64>,
        timeline_time: Option<f64>,
    },
}

/// The script error a playback procedure throws.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaybackError {
    /// `InvalidStateError`: an infinite effect played backwards, finished
    /// or reversed without a timeline.
    InvalidState,
}

/// An animation's times and pending tasks.
#[derive(Debug, Clone, PartialEq)]
pub struct Playback {
    pub start_time: Option<f64>,
    pub hold_time: Option<f64>,
    pub playback_rate: f64,
    pub pending_playback_rate: Option<f64>,
    pub pending: Option<PendingTask>,
    /// The current time when the finished state was last updated.
    previous_current_time: Option<f64>,
    /// The current finished promise is resolved (it is pending
    /// otherwise; a new animation's ready promise is resolved).
    finished_resolved: bool,
    /// The current ready promise is pending.
    ready_pending: bool,
    /// The finish notification steps are queued, to run at the next
    /// update of animations if it is still finished then.
    finish_queued: bool,
}

impl Default for Playback {
    /// A new animation's: idle, at rate 1, its ready promise resolved
    /// and its finished promise pending.
    fn default() -> Self {
        Self {
            start_time: None,
            hold_time: None,
            playback_rate: 1.0,
            pending_playback_rate: None,
            pending: None,
            previous_current_time: None,
            finished_resolved: false,
            ready_pending: false,
            finish_queued: false,
        }
    }
}

impl Playback {
    /// Its current time at the timeline's time `timeline`.
    pub fn current_time(&self, timeline: Option<f64>) -> Option<f64> {
        if let Some(hold) = self.hold_time {
            return Some(hold);
        }
        Some(from_start(timeline?, self.start_time?, self.playback_rate))
    }

    /// The playback rate it will have once its pending tasks are done.
    pub fn effective_playback_rate(&self) -> f64 {
        self.pending_playback_rate.unwrap_or(self.playback_rate)
    }

    pub fn play_state(&self, timeline: Option<f64>, end: f64) -> PlayState {
        let current = self.current_time(timeline);
        if current.is_none() && self.start_time.is_none() && self.pending.is_none() {
            return PlayState::Idle;
        }
        if self.pending == Some(PendingTask::Pause)
            || (self.start_time.is_none() && self.pending != Some(PendingTask::Play))
        {
            return PlayState::Paused;
        }
        let rate = self.effective_playback_rate();
        if let Some(current) = current
            && ((rate > 0.0 && reached(current, end)) || (rate < 0.0 && reached(0.0, current)))
        {
            return PlayState::Finished;
        }
        PlayState::Running
    }

    /// Whether a play or pause task waits.
    pub fn is_pending(&self) -> bool {
        self.pending.is_some()
    }

    /// Whether its current ready promise waits (else it is resolved).
    pub fn is_ready_pending(&self) -> bool {
        self.ready_pending
    }

    /// Whether its current finished promise is resolved (else it waits).
    pub fn is_finished_resolved(&self) -> bool {
        self.finished_resolved
    }

    fn apply_pending_playback_rate(&mut self) {
        if let Some(rate) = self.pending_playback_rate.take() {
            self.playback_rate = rate;
        }
    }

    fn resolve_ready(&mut self, notices: &mut Vec<Notice>) {
        if self.ready_pending {
            self.ready_pending = false;
            notices.push(Notice::ReadyResolved);
        }
    }

    fn new_ready(&mut self, notices: &mut Vec<Notice>) {
        if !self.ready_pending {
            self.ready_pending = true;
            notices.push(Notice::NewReady);
        }
    }

    /// "Silently set the current time".
    fn silently_set_current_time(&mut self, seek: f64, timeline: Option<f64>) {
        if self.hold_time.is_some()
            || self.start_time.is_none()
            || timeline.is_none()
            || self.playback_rate == 0.0
        {
            self.hold_time = Some(seek);
        } else if let Some(timeline) = timeline {
            self.start_time = Some(timeline - seek / self.playback_rate);
        }
        if timeline.is_none() {
            self.start_time = None;
        }
        self.previous_current_time = None;
    }

    /// Setting `currentTime` ("set the current time").
    pub fn set_current_time(
        &mut self,
        seek: f64,
        timeline: Option<f64>,
        end: f64,
        notices: &mut Vec<Notice>,
    ) {
        self.silently_set_current_time(seek, timeline);
        if self.pending == Some(PendingTask::Pause) {
            self.hold_time = Some(seek);
            self.apply_pending_playback_rate();
            self.start_time = None;
            self.pending = None;
            self.resolve_ready(notices);
        }
        self.update_finished_state(true, false, timeline, end, notices);
    }

    /// Setting `startTime` ("set the start time").
    pub fn set_start_time(
        &mut self,
        start: Option<f64>,
        timeline: Option<f64>,
        end: f64,
        notices: &mut Vec<Notice>,
    ) {
        if timeline.is_none() && start.is_some() {
            self.hold_time = None;
        }
        let previous = self.current_time(timeline);
        self.apply_pending_playback_rate();
        self.start_time = start;
        if start.is_some() {
            if self.playback_rate != 0.0 {
                self.hold_time = None;
            }
        } else {
            self.hold_time = previous;
        }
        if self.pending.take().is_some() {
            self.resolve_ready(notices);
        }
        self.update_finished_state(true, false, timeline, end, notices);
    }

    /// "Play an animation", rewinding to its start (or end, backwards) if
    /// `auto_rewind` and it is outside its interval.
    pub fn play(
        &mut self,
        auto_rewind: bool,
        timeline: Option<f64>,
        end: f64,
        notices: &mut Vec<Notice>,
    ) -> Result<(), PlaybackError> {
        let aborted_pause = self.pending == Some(PendingTask::Pause);
        let rate = self.effective_playback_rate();
        let current = self.current_time(timeline);
        let mut seek = None;
        if rate > 0.0 && auto_rewind && current.is_none_or(|c| c < 0.0 || c >= end) {
            seek = Some(0.0);
        } else if rate < 0.0 && auto_rewind && current.is_none_or(|c| c <= 0.0 || c > end) {
            if end == f64::INFINITY {
                return Err(PlaybackError::InvalidState);
            }
            seek = Some(end);
        } else if rate == 0.0 && current.is_none() {
            seek = Some(0.0);
        }
        if let Some(seek) = seek {
            self.hold_time = Some(seek);
        }
        if self.hold_time.is_some() {
            self.start_time = None;
        }
        let had_pending = self.pending.take().is_some();
        if self.hold_time.is_none()
            && seek.is_none()
            && !aborted_pause
            && self.pending_playback_rate.is_none()
        {
            return Ok(());
        }
        if !had_pending {
            self.new_ready(notices);
        }
        self.pending = Some(PendingTask::Play);
        self.update_finished_state(false, false, timeline, end, notices);
        Ok(())
    }

    /// "Pause an animation".
    pub fn pause(
        &mut self,
        timeline: Option<f64>,
        end: f64,
        notices: &mut Vec<Notice>,
    ) -> Result<(), PlaybackError> {
        if self.pending == Some(PendingTask::Pause) {
            return Ok(());
        }
        if self.current_time(timeline).is_none() {
            let seek = if self.effective_playback_rate() >= 0.0 {
                0.0
            } else {
                if end == f64::INFINITY {
                    return Err(PlaybackError::InvalidState);
                }
                end
            };
            self.hold_time = Some(seek);
        }
        let had_pending_play = self.pending.take() == Some(PendingTask::Play);
        if !had_pending_play {
            self.new_ready(notices);
        }
        self.pending = Some(PendingTask::Pause);
        self.update_finished_state(false, false, timeline, end, notices);
        Ok(())
    }

    /// The pending task done at `ready_time`, the time of the frame whose
    /// rendering first shows it (Blink's `PendingAnimations::Update`);
    /// `true` if there was one.
    pub fn commit_pending(&mut self, ready_time: f64, end: f64, notices: &mut Vec<Notice>) -> bool {
        match self.pending.take() {
            Some(PendingTask::Play) => {
                if let Some(hold) = self.hold_time {
                    self.apply_pending_playback_rate();
                    let rate = self.playback_rate;
                    self.start_time = Some(if rate == 0.0 {
                        ready_time
                    } else {
                        ready_time - hold / rate
                    });
                    if rate != 0.0 {
                        self.hold_time = None;
                    }
                } else if let (Some(start), Some(_)) = (self.start_time, self.pending_playback_rate)
                {
                    let current = from_start(ready_time, start, self.playback_rate);
                    self.apply_pending_playback_rate();
                    if self.playback_rate == 0.0 {
                        self.hold_time = Some(current);
                    } else {
                        self.start_time = Some(ready_time - current / self.playback_rate);
                    }
                }
            }
            Some(PendingTask::Pause) => {
                // In whole microseconds, as the current time read until
                // now was: a paused animation holds where it was drawn.
                if let (Some(start), None) = (self.start_time, self.hold_time) {
                    self.hold_time = Some(from_start(ready_time, start, self.playback_rate));
                }
                self.apply_pending_playback_rate();
                self.start_time = None;
            }
            None => return false,
        }
        self.resolve_ready(notices);
        self.update_finished_state(false, false, Some(ready_time), end, notices);
        true
    }

    /// `finish()` ("finish an animation").
    pub fn finish(
        &mut self,
        timeline: Option<f64>,
        end: f64,
        notices: &mut Vec<Notice>,
    ) -> Result<(), PlaybackError> {
        let rate = self.effective_playback_rate();
        if rate == 0.0 || (rate > 0.0 && end == f64::INFINITY) {
            return Err(PlaybackError::InvalidState);
        }
        self.apply_pending_playback_rate();
        let limit = if self.playback_rate > 0.0 { end } else { 0.0 };
        self.silently_set_current_time(limit, timeline);
        if self.start_time.is_none()
            && let Some(timeline) = timeline
        {
            self.start_time = Some(timeline - limit / self.playback_rate);
        }
        if self.pending == Some(PendingTask::Pause) && self.start_time.is_some() {
            self.hold_time = None;
            self.pending = None;
            self.resolve_ready(notices);
        }
        if self.pending == Some(PendingTask::Play) && self.start_time.is_some() {
            self.pending = None;
            self.resolve_ready(notices);
        }
        self.update_finished_state(true, true, timeline, end, notices);
        Ok(())
    }

    /// `cancel()` ("cancel an animation"): idle, its promises aborted and
    /// a `cancel` event, unless it was idle.
    pub fn cancel(&mut self, timeline: Option<f64>, end: f64, notices: &mut Vec<Notice>) {
        if self.play_state(timeline, end) != PlayState::Idle {
            if self.pending.take().is_some() {
                self.apply_pending_playback_rate();
                if self.ready_pending {
                    self.ready_pending = false;
                    notices.push(Notice::ReadyAborted);
                }
            }
            self.finished_resolved = false;
            self.finish_queued = false;
            notices.push(Notice::Canceled {
                timeline_time: timeline,
            });
        }
        self.start_time = None;
        self.hold_time = None;
    }

    /// `reverse()`.
    pub fn reverse(
        &mut self,
        timeline: Option<f64>,
        end: f64,
        notices: &mut Vec<Notice>,
    ) -> Result<(), PlaybackError> {
        if timeline.is_none() {
            return Err(PlaybackError::InvalidState);
        }
        let original = self.pending_playback_rate;
        // A rate of 0 stays 0, not -0.
        let rate = self.effective_playback_rate();
        self.pending_playback_rate = Some(if rate == 0.0 { 0.0 } else { -rate });
        let mut sent = Vec::new();
        if let Err(error) = self.play(true, timeline, end, &mut sent) {
            self.pending_playback_rate = original;
            return Err(error);
        }
        notices.extend(sent);
        Ok(())
    }

    /// Setting `playbackRate`: its current time stays where it is.
    pub fn set_playback_rate(
        &mut self,
        rate: f64,
        timeline: Option<f64>,
        end: f64,
        notices: &mut Vec<Notice>,
    ) {
        self.pending_playback_rate = None;
        let previous = self.current_time(timeline);
        self.playback_rate = rate;
        if let Some(previous) = previous {
            self.set_current_time(previous, timeline, end, notices);
        }
    }

    /// `updatePlaybackRate()`: a new rate, without a jump, once it is
    /// ready ("seamlessly update the playback rate").
    pub fn update_playback_rate(
        &mut self,
        rate: f64,
        timeline: Option<f64>,
        end: f64,
        notices: &mut Vec<Notice>,
    ) {
        let previous_state = self.play_state(timeline, end);
        self.pending_playback_rate = Some(rate);
        if self.pending.is_some() {
            return;
        }
        // Without a current time it is the rate at once.
        if self.current_time(timeline).is_none() {
            self.apply_pending_playback_rate();
            return;
        }
        match previous_state {
            PlayState::Idle | PlayState::Paused => self.apply_pending_playback_rate(),
            PlayState::Finished => {
                let unconstrained = timeline
                    .zip(self.start_time)
                    .map(|(t, s)| (t - s) * self.playback_rate);
                if let Some(timeline) = timeline {
                    self.start_time = Some(match unconstrained {
                        Some(current) if rate != 0.0 => timeline - current / rate,
                        _ => timeline,
                    });
                }
                self.apply_pending_playback_rate();
                self.update_finished_state(false, false, timeline, end, notices);
            }
            PlayState::Running => {
                // Never fails: it doesn't rewind.
                let _ = self.play(false, timeline, end, notices);
            }
        }
    }

    /// "Update an animation's finished state": held at its end once there
    /// (or at its start, backwards), and the finish notification queued
    /// (or run, if `notify_now`) once finished; a new finished promise
    /// once it isn't.
    pub fn update_finished_state(
        &mut self,
        did_seek: bool,
        notify_now: bool,
        timeline: Option<f64>,
        end: f64,
        notices: &mut Vec<Notice>,
    ) {
        let unconstrained = if did_seek {
            self.current_time(timeline)
        } else {
            timeline
                .zip(self.start_time)
                .map(|(t, s)| from_start(t, s, self.playback_rate))
        };
        if let (Some(_), None, Some(unconstrained)) = (self.start_time, self.pending, unconstrained)
        {
            let rate = self.playback_rate;
            if rate > 0.0 && reached(unconstrained, end) {
                self.hold_time = Some(if did_seek {
                    unconstrained
                } else {
                    self.previous_current_time.map_or(end, |p| p.max(end))
                });
            } else if rate < 0.0 && reached(0.0, unconstrained) {
                self.hold_time = Some(if did_seek {
                    unconstrained
                } else {
                    self.previous_current_time.map_or(0.0, |p| p.min(0.0))
                });
            } else if rate != 0.0
                && let Some(timeline) = timeline
                && let Some(hold) = self.hold_time
            {
                if did_seek {
                    self.start_time = Some(timeline - hold / rate);
                }
                self.hold_time = None;
            }
        }
        self.previous_current_time = self.current_time(timeline);
        let finished = self.play_state(timeline, end) == PlayState::Finished;
        if finished && !self.finished_resolved {
            if notify_now {
                self.finish_queued = false;
                self.notify_finished(timeline, notices);
            } else {
                self.finish_queued = true;
            }
        } else if !finished {
            self.finish_queued = false;
            if self.finished_resolved {
                self.finished_resolved = false;
                notices.push(Notice::NewFinished);
            }
        }
    }

    fn notify_finished(&mut self, timeline: Option<f64>, notices: &mut Vec<Notice>) {
        self.finished_resolved = true;
        notices.push(Notice::Finished {
            current_time: self.current_time(timeline),
            timeline_time: timeline,
        });
    }

    /// The queued finish notification steps, at the update of animations
    /// after they were queued: run if it is still finished.
    pub fn run_finish_notification(
        &mut self,
        timeline: Option<f64>,
        end: f64,
        notices: &mut Vec<Notice>,
    ) {
        if std::mem::take(&mut self.finish_queued)
            && !self.finished_resolved
            && self.play_state(timeline, end) == PlayState::Finished
        {
            self.notify_finished(timeline, notices);
        }
    }
}

/// The current time `timeline` gives from `start` at `rate`, in whole
/// microseconds, as Blink's times are: a start time put `limit` before now
/// (`finish()`, a seek) gives back `limit`, not a hair less; and no `-0`,
/// which a whole number of microseconds can't be (backwards, at the start
/// time, is `0`, as script reads it in Chrome).
fn from_start(timeline: f64, start: f64, rate: f64) -> f64 {
    ((timeline - start) * rate * 1000.0).round() / 1000.0 + 0.0
}

/// Whether `time` has reached `limit`: within a microsecond it has (Blink's
/// `TimingCalculations::IsWithinAnimationTimeTolerance`), which an end
/// time not in whole microseconds (`53.43000000000001`) needs.
fn reached(time: f64, limit: f64) -> bool {
    time >= limit - 0.001
}
