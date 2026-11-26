//! Shared hotkey gesture state machine, independent of the OS capture layer.
//!
//! Hosts feed raw key edges (`Down`/`Up`) of the bound activation key and
//! receive [`HotkeyEvent`]s. Three gesture profiles fall out of two knobs:
//!
//! | profile | construction | behavior |
//! |---|---|---|
//! | **smart**  | `smart = true`, default threshold 350ms | hold → PTT (stop on release); quick tap → latch until next press |
//! | **hold**   | `smart = false` | recording runs only while the key is held (Up always stops) |
//! | **toggle** | `hold_threshold = Duration::MAX` | every tap latches; next press stops |
//!
//! Chord protection: a second key pressed while the activation key is down and
//! the gesture is not yet classified cancels the take and is passed through to
//! the OS (this is how Fn+Space style combos keep working). Once a take has
//! outlived the chord window, further keys don't cut it.

use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Edge {
    Down,
    Up,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HotkeyEvent {
    /// A dictation session should start now.
    Start,
    /// The current dictation session should stop (and be sent).
    Stop,
    /// The activation key press was part of a chord; host must not swallow it.
    PassThrough,
    /// Escape was pressed while a host session was active.
    Cancel,
}

#[derive(Debug, Clone)]
enum State {
    Idle,
    /// Activation key is down and a take is in progress, not yet classified.
    Pressed {
        since: Instant,
    },
    /// Take is latched (key released after a quick tap); next press stops it.
    Latched,
}

#[derive(Debug)]
pub struct GestureDecoder {
    hold_threshold: Duration,
    smart: bool,
    state: State,
}

impl Default for GestureDecoder {
    fn default() -> Self {
        Self::new(Duration::from_millis(350), true)
    }
}

impl GestureDecoder {
    pub fn new(hold_threshold: Duration, smart: bool) -> Self {
        Self {
            hold_threshold,
            smart,
            state: State::Idle,
        }
    }

    /// Toggle profile: every tap latches, next press stops.
    pub fn toggle() -> Self {
        Self::new(Duration::MAX, true)
    }

    /// A non-activation key edge occurred while the activation key was down.
    /// Before classification this cancels the take and passes the key through.
    pub fn note_other_key(&mut self) -> Option<HotkeyEvent> {
        match self.state {
            State::Pressed { .. } => {
                self.state = State::Idle;
                Some(HotkeyEvent::PassThrough)
            }
            State::Latched | State::Idle => None,
        }
    }

    pub fn on_edge(&mut self, edge: Edge, now: Instant) -> Option<HotkeyEvent> {
        match (edge, &self.state) {
            (Edge::Down, State::Idle) => {
                self.state = State::Pressed { since: now };
                Some(HotkeyEvent::Start)
            }
            (Edge::Down, State::Pressed { .. }) => None, // key auto-repeat
            (Edge::Down, State::Latched) => {
                self.state = State::Idle;
                Some(HotkeyEvent::Stop)
            }
            (Edge::Up, State::Pressed { since }) => {
                let held = now.saturating_duration_since(*since);
                if !self.smart || held >= self.hold_threshold {
                    self.state = State::Idle;
                    Some(HotkeyEvent::Stop)
                } else {
                    // quick tap in smart mode: keep recording, latched
                    self.state = State::Latched;
                    Some(HotkeyEvent::Start)
                }
            }
            (Edge::Up, _) => None,
        }
    }

    pub fn reset(&mut self) {
        self.state = State::Idle;
    }

    pub fn is_idle(&self) -> bool {
        matches!(self.state, State::Idle)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(ms: u64) -> Instant {
        Instant::now() + Duration::from_millis(ms)
    }

    #[test]
    fn tap_latches_and_next_tap_stops() {
        let mut d = GestureDecoder::default();
        let t0 = Instant::now();
        assert_eq!(d.on_edge(Edge::Down, t0), Some(HotkeyEvent::Start));
        assert_eq!(
            d.on_edge(Edge::Up, t0 + Duration::from_millis(100)),
            Some(HotkeyEvent::Start)
        );
        assert_eq!(d.on_edge(Edge::Down, t(10)), Some(HotkeyEvent::Stop));
        assert_eq!(d.on_edge(Edge::Up, t(30)), None);
        assert!(d.is_idle());
    }

    #[test]
    fn long_hold_is_ptt() {
        let mut d = GestureDecoder::default();
        let t0 = Instant::now();
        assert_eq!(d.on_edge(Edge::Down, t0), Some(HotkeyEvent::Start));
        assert_eq!(
            d.on_edge(Edge::Up, t0 + Duration::from_millis(500)),
            Some(HotkeyEvent::Stop)
        );
        assert!(d.is_idle());
    }

    #[test]
    fn hold_profile_stops_on_release_even_when_quick() {
        let mut d = GestureDecoder::new(Duration::from_millis(350), false);
        let t0 = Instant::now();
        assert_eq!(d.on_edge(Edge::Down, t0), Some(HotkeyEvent::Start));
        assert_eq!(
            d.on_edge(Edge::Up, t0 + Duration::from_millis(50)),
            Some(HotkeyEvent::Stop)
        );
    }

    #[test]
    fn toggle_profile_latches_any_tap() {
        let mut d = GestureDecoder::toggle();
        let t0 = Instant::now();
        assert_eq!(d.on_edge(Edge::Down, t0), Some(HotkeyEvent::Start));
        assert_eq!(
            d.on_edge(Edge::Up, t0 + Duration::from_millis(50)),
            Some(HotkeyEvent::Start)
        );
        assert_eq!(d.on_edge(Edge::Down, t(100)), Some(HotkeyEvent::Stop));
    }

    #[test]
    fn chord_passes_through_and_cancels() {
        let mut d = GestureDecoder::default();
        let t0 = Instant::now();
        assert_eq!(d.on_edge(Edge::Down, t0), Some(HotkeyEvent::Start));
        assert_eq!(d.note_other_key(), Some(HotkeyEvent::PassThrough));
        assert_eq!(d.on_edge(Edge::Up, t0 + Duration::from_millis(20)), None);
        assert!(d.is_idle());
    }
}
