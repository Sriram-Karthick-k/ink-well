//! Taps with several fingers at once, a way to undo, redo and so on without reaching for a button.
//!
//! A tap is every finger going down and coming up again within a moment, none of them having moved far. The
//! number of fingers is the most that were down together. Two fingers that pinch, pan or rest are not a tap.

use std::collections::HashMap;

use eframe::egui::{Event, Pos2, TouchPhase};

/// All fingers must be up again within this long of the first going down, in seconds.
const MAX_TAP_SECONDS: f64 = 0.4;
/// A finger that goes further than this from where it came down, in points, is moving and not tapping.
const MAX_TRAVEL: f32 = 24.0;

/// Follows the touches of the screen and says when a tap with more than one finger has happened.
#[derive(Debug, Default)]
pub struct TapTracker {
    /// Where each finger that is down (by its touch number) came down.
    fingers: HashMap<u64, Pos2>,
    /// The most fingers that have been down together since the first one.
    most: usize,
    /// When the first finger came down.
    since: f64,
    /// Nothing so far rules this out as a tap.
    steady: bool,
}

impl TapTracker {
    /// Takes the touch events of one frame (`now` is the time of the frame) and returns how many fingers made a
    /// tap that was completed in them, if one was.
    pub fn update(&mut self, events: &[Event], now: f64) -> Option<usize> {
        let mut tapped = None;
        for event in events {
            let Event::Touch { id, phase, pos, .. } = event else {
                continue;
            };
            match phase {
                TouchPhase::Start => {
                    if self.fingers.is_empty() {
                        self.most = 0;
                        self.since = now;
                        self.steady = true;
                    }
                    self.fingers.insert(id.0, *pos);
                    self.most = self.most.max(self.fingers.len());
                }
                TouchPhase::Move => {
                    if self
                        .fingers
                        .get(&id.0)
                        .is_some_and(|start| start.distance(*pos) > MAX_TRAVEL)
                    {
                        self.steady = false;
                    }
                }
                TouchPhase::End => {
                    self.fingers.remove(&id.0);
                    if self.fingers.is_empty() {
                        if self.steady && self.most >= 2 && now - self.since <= MAX_TAP_SECONDS {
                            tapped = Some(self.most);
                        }
                        self.most = 0;
                    }
                }
                TouchPhase::Cancel => {
                    self.fingers.clear();
                    self.most = 0;
                    self.steady = false;
                }
            }
        }
        if !self.fingers.is_empty() && now - self.since > MAX_TAP_SECONDS {
            self.steady = false;
        }
        tapped
    }
}

#[cfg(test)]
mod tests {
    use eframe::egui::{TouchDeviceId, TouchId, pos2};

    use super::*;

    fn touch(id: u64, phase: TouchPhase, x: f32, y: f32) -> Event {
        Event::Touch {
            device_id: TouchDeviceId(0),
            id: TouchId(id),
            phase,
            pos: pos2(x, y),
            force: None,
        }
    }

    #[test]
    fn two_fingers_down_and_up_together_are_a_two_finger_tap() {
        let mut taps = TapTracker::default();
        assert_eq!(
            taps.update(
                &[
                    touch(1, TouchPhase::Start, 100.0, 100.0),
                    touch(2, TouchPhase::Start, 160.0, 100.0)
                ],
                1.0
            ),
            None
        );
        assert_eq!(
            taps.update(
                &[
                    touch(1, TouchPhase::End, 100.0, 100.0),
                    touch(2, TouchPhase::End, 160.0, 100.0)
                ],
                1.1
            ),
            Some(2)
        );
    }

    #[test]
    fn three_and_four_fingers_are_counted_and_a_stagger_in_the_lifting_does_not_matter() {
        let mut taps = TapTracker::default();
        let down: Vec<Event> = (1..=3)
            .map(|i| touch(i, TouchPhase::Start, i as f32 * 50.0, 80.0))
            .collect();
        taps.update(&down, 2.0);
        taps.update(&[touch(1, TouchPhase::End, 50.0, 80.0)], 2.05);
        taps.update(&[touch(2, TouchPhase::End, 100.0, 80.0)], 2.1);
        assert_eq!(
            taps.update(&[touch(3, TouchPhase::End, 150.0, 80.0)], 2.15),
            Some(3)
        );
        let down: Vec<Event> = (1..=4)
            .map(|i| touch(i, TouchPhase::Start, i as f32 * 40.0, 80.0))
            .collect();
        taps.update(&down, 3.0);
        let up: Vec<Event> = (1..=4)
            .map(|i| touch(i, TouchPhase::End, i as f32 * 40.0, 80.0))
            .collect();
        assert_eq!(taps.update(&up, 3.1), Some(4));
    }

    #[test]
    fn one_finger_is_drawing_and_not_a_tap() {
        let mut taps = TapTracker::default();
        taps.update(&[touch(1, TouchPhase::Start, 10.0, 10.0)], 1.0);
        assert_eq!(
            taps.update(&[touch(1, TouchPhase::End, 10.0, 10.0)], 1.05),
            None
        );
    }

    #[test]
    fn fingers_that_move_or_rest_are_not_a_tap() {
        let mut taps = TapTracker::default();
        let both = |phase, dx: f32| {
            vec![
                touch(1, phase, 100.0 + dx, 100.0),
                touch(2, phase, 200.0 - dx, 100.0),
            ]
        };
        // A pinch: the fingers come together.
        taps.update(&both(TouchPhase::Start, 0.0), 1.0);
        taps.update(&[touch(1, TouchPhase::Move, 140.0, 100.0)], 1.05);
        assert_eq!(taps.update(&both(TouchPhase::End, 40.0), 1.1), None);
        // Resting: down for too long.
        taps.update(&both(TouchPhase::Start, 0.0), 5.0);
        taps.update(&[], 5.3);
        taps.update(&[], 5.6);
        assert_eq!(taps.update(&both(TouchPhase::End, 0.0), 5.7), None);
    }

    #[test]
    fn a_touch_that_is_cancelled_is_not_a_tap_and_the_next_one_starts_fresh() {
        let mut taps = TapTracker::default();
        taps.update(
            &[
                touch(1, TouchPhase::Start, 0.0, 0.0),
                touch(2, TouchPhase::Start, 50.0, 0.0),
            ],
            1.0,
        );
        assert_eq!(
            taps.update(
                &[
                    touch(1, TouchPhase::Cancel, 0.0, 0.0),
                    touch(2, TouchPhase::End, 50.0, 0.0)
                ],
                1.1
            ),
            None
        );
        taps.update(
            &[
                touch(7, TouchPhase::Start, 0.0, 0.0),
                touch(8, TouchPhase::Start, 50.0, 0.0),
            ],
            9.0,
        );
        assert_eq!(
            taps.update(
                &[
                    touch(7, TouchPhase::End, 0.0, 0.0),
                    touch(8, TouchPhase::End, 50.0, 0.0)
                ],
                9.1
            ),
            Some(2)
        );
    }

    #[test]
    fn events_that_are_not_touches_are_ignored() {
        let mut taps = TapTracker::default();
        assert_eq!(taps.update(&[Event::Copy, Event::PointerGone], 1.0), None);
    }
}
