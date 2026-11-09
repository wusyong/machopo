//! An animation's play state and times (`Playback`).

use animation::{Notice, Playback};

/// A reversed animation sought to its start is at `0`, not `-0`, which a
/// time in whole microseconds (Blink's) can't be: script reads it as
/// Chrome does (WPT's `updating-the-finished-state.html`).
#[test]
fn a_reversed_animation_at_its_start_is_at_zero() {
    let end = 100_000.0;
    let mut notices: Vec<Notice> = Vec::new();
    let mut playback = Playback::default();
    playback.set_playback_rate(-1.0, Some(0.0), end, &mut notices);
    playback.play(true, Some(0.0), end, &mut notices).unwrap();
    playback.commit_pending(10.0, end, &mut notices);
    playback.set_current_time(0.0, Some(20.0), end, &mut notices);
    for timeline in [20.0, 36.0] {
        let current = playback.current_time(Some(timeline)).unwrap();
        assert_eq!(current, 0.0);
        assert!(current.is_sign_positive(), "{current} at {timeline}");
    }
}

/// A pause holds the time the animation was at when it was paused, in
/// whole microseconds as its current time reads before (Blink's): the
/// frame that shows the pause and every one after it sample the same time.
#[test]
fn a_pause_holds_the_time_it_read() {
    let end = 100_000.0;
    let mut notices: Vec<Notice> = Vec::new();
    let mut playback = Playback::default();
    playback.play(true, Some(0.0), end, &mut notices).unwrap();
    playback.commit_pending(242.225_542, end, &mut notices);
    let ready = 322.271_167_000_000_05;
    playback.pause(Some(ready), end, &mut notices).unwrap();
    let before = playback.current_time(Some(ready)).unwrap();
    playback.commit_pending(ready, end, &mut notices);
    assert_eq!(playback.current_time(Some(ready + 300.0)), Some(before));
    assert_eq!(before, 80.046);
}
