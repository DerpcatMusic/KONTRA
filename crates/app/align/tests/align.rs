//! Auto-align: measured delays, reported latency, holds and the hold queue.
#![cfg(test)]

use kontra_align::{Delay, Holder, Holds, Player, Timing, measure, onset_ms, reported_ms};

const RATE: f64 = 48_000.0;

fn hz(key: u8) -> f64 {
    440.0 * 2f64.powf((f64::from(key) - 69.0) / 12.0)
}

/// A patch whose first note sounds `first` ms after its note-on and whose
/// legato note `legato` ms after its, as a sine at the key's pitch.
struct Fake {
    first: f64,
    legato: f64,
    /// Keys sounding: (key, frames from its note-on to its sound).
    on: Vec<(u8, usize, usize)>,
    now: usize,
}

impl Player for Fake {
    fn rate(&self) -> f64 {
        RATE
    }
    fn reset(&mut self) {
        self.on.clear();
        self.now = 0;
    }
    fn articulation(&mut self, _: usize) {}
    /// Releases ring on: a hard cut would click across every partial.
    fn note(&mut self, key: u8, _: u8, on: bool) {
        if on {
            let ms = if self.on.is_empty() {
                self.first
            } else {
                self.legato
            };
            self.on.push((key, self.now, (ms * RATE / 1000.0) as usize));
        }
    }
    fn render(&mut self, frames: usize, out: &mut Vec<f32>) {
        for _ in 0..frames {
            let t = self.now;
            let s: f64 = (self.on.iter())
                .filter(|(_, at, wait)| t >= at + wait)
                .map(|&(key, at, _)| {
                    (std::f64::consts::TAU * hz(key) * (t - at) as f64 / RATE).sin()
                })
                .sum();
            out.push(s as f32 * 0.3);
            self.now += 1;
        }
    }
}

#[test]
fn a_patch_is_measured_where_its_attack_and_legato_sound() {
    let mut fake = Fake {
        first: 80.0,
        legato: 200.0,
        on: Vec::new(),
        now: 0,
    };
    let timing = measure(&mut fake, 1, 60);
    let first = timing.loaded.first[1].unwrap();
    let legato = timing.loaded.legato[1].unwrap();
    assert!((first - 80.0).abs() < 4.0, "first {first}");
    assert!((legato - 200.0).abs() < 25.0, "legato {legato}");
    assert!(timing.loaded.mono());
    assert_eq!(timing.articulations.len(), 1);
    assert!((timing.latest() - legato).abs() < 0.01);
    assert_eq!(onset_ms(&[0.0; 4800], RATE, -20.0), None);
}

fn timing(first: f32, legato: f32) -> Timing {
    Timing {
        loaded: Delay {
            first: [Some(first); 3],
            legato: [Some(legato); 3],
        },
        ..Timing::default()
    }
}

#[test]
fn the_slowest_patch_sets_the_latency_and_the_others_wait_the_difference() {
    let slow = timing(120.0, 120.0);
    let fast = timing(20.0, 20.0);
    let reported = reported_ms([slow.latest(), fast.latest()]);
    assert_eq!(reported, 120.0);
    let (slow, fast) = (
        Holds::new(&slow, reported, RATE),
        Holds::new(&fast, reported, RATE),
    );
    assert_eq!(slow.frames(None, false, 100), 0);
    assert_eq!(fast.frames(None, false, 100), 4800);
    // A single patch reports its own delay and waits nothing.
    let alone = timing(80.0, 80.0);
    assert_eq!(
        Holds::new(&alone, reported_ms([alone.latest()]), RATE).frames(None, false, 30),
        0
    );
    // Excluded: plays as late as it is and sets no latency; never a negative hold.
    let excluded = Timing {
        exclude: true,
        ..timing(300.0, 300.0)
    };
    assert_eq!(excluded.latest(), 0.0);
    assert_eq!(
        Holds::new(&excluded, 120.0, RATE).frames(None, false, 100),
        5760
    );
    assert_eq!(
        Holds::new(&timing(400.0, 400.0), 120.0, RATE).frames(None, false, 100),
        0
    );
}

#[test]
fn a_missing_bucket_takes_the_nearest_then_the_other_kind() {
    let d = Delay {
        first: [None, None, Some(30.0)],
        legato: [Some(90.0), None, None],
    };
    assert_eq!(d.ms(false, 40), Some(30.0));
    assert_eq!(d.ms(true, 120), Some(90.0));
    assert_eq!(Delay::default().ms(true, 1), None);
}

#[test]
fn held_events_play_in_order_and_a_legato_never_overtakes() {
    let mut h = Holder::new(4);
    // Nothing to wait for: played at once.
    assert_eq!(h.on(0, 0, 60, 0, false, "now"), Err("now"));
    // A first note waits 100; a legato after it waits 0 but may not overtake.
    h.on(10, 0, 60, 100, true, "a on").unwrap();
    h.on(20, 0, 64, 0, true, "b on").unwrap();
    h.follow(30, 0, 60, "a off").unwrap();
    assert_eq!(h.pop_due(109), None);
    assert_eq!(h.pop_due(110), Some((110, "a on")));
    assert_eq!(h.pop_due(110), Some((110, "b on")));
    // The release waits as long as its note-on: 30 + 100.
    assert_eq!(h.next_due(), Some(130));
    assert_eq!(h.pop_due(200), Some((130, "a off")));
    // Full: plays at once, counted.
    for n in 0..4 {
        h.hold(0, 10, false, ["0", "1", "2", "3"][n]).unwrap();
    }
    assert_eq!(h.hold(0, 10, false, "9"), Err("9"));
    assert_eq!(h.overflows, 1);
    h.clear();
    assert_eq!(h.next_due(), None);
}
