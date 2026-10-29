//! Auto-align: measured delays, reported latency and holds.
#![cfg(test)]

use kontra_align::{Delay, Holds, Player, Step, Timing, measure, onset_ms, reported_ms};

const RATE: f64 = 48_000.0;

fn hz(key: u8) -> f64 {
    440.0 * 2f64.powf((f64::from(key) - 69.0) / 12.0)
}

/// A patch whose first note sounds `first` ms after its note-on and whose
/// legato note `legato` ms after its, as a sine at the key's pitch.
struct Fake {
    first: f64,
    /// At or below this velocity, first notes sound 200 ms late.
    soft: u8,
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
    fn note(&mut self, key: u8, velocity: u8, on: bool) {
        if on {
            let ms = if self.on.is_empty() {
                if velocity <= self.soft {
                    200.0
                } else {
                    self.first
                }
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
        soft: 0,
        legato: 200.0,
        on: Vec::new(),
        now: 0,
    };
    let timing = measure(&mut fake, 1, 60);
    let [step] = timing.loaded.steps[..] else {
        panic!("one range: {:?}", timing.loaded);
    };
    let (first, legato) = (step.first.unwrap(), step.legato.unwrap());
    assert!((first - 80.0).abs() < 4.0, "first {first}");
    assert!((legato - 200.0).abs() < 25.0, "legato {legato}");
    assert!(timing.loaded.mono());
    assert_eq!(timing.articulations.len(), 1);
    assert!((timing.latest() - legato).abs() < 0.01);
    assert_eq!(onset_ms(&[0.0; 4800], RATE, -20.0), None);
}

#[test]
fn a_softer_layer_that_sounds_later_gets_its_own_velocity_range() {
    let mut fake = Fake {
        first: 80.0,
        soft: 35,
        legato: 120.0,
        on: Vec::new(),
        now: 0,
    };
    let steps = measure(&mut fake, 0, 60).loaded.steps;
    let tops: Vec<u8> = steps.iter().map(|s| s.top).collect();
    assert_eq!(tops, [35, 127], "{steps:?}");
    assert!((steps[0].first.unwrap() - 200.0).abs() < 4.0, "{steps:?}");
    assert!((steps[1].first.unwrap() - 80.0).abs() < 4.0, "{steps:?}");
}

fn timing(first: f32, legato: f32) -> Timing {
    Timing {
        loaded: Delay {
            steps: vec![Step {
                top: 127,
                first: Some(first),
                legato: Some(legato),
            }],
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
fn a_velocity_takes_its_range_and_a_missing_kind_takes_the_other() {
    let d = Delay {
        steps: vec![
            Step {
                top: 40,
                first: Some(200.0),
                legato: None,
            },
            Step {
                top: 100,
                first: Some(30.0),
                legato: Some(90.0),
            },
        ],
    };
    assert_eq!(d.ms(true, 20), Some(200.0));
    assert_eq!(d.ms(false, 41), Some(30.0));
    assert_eq!(d.ms(true, 127), Some(90.0));
    assert_eq!(Delay::default().ms(true, 1), None);
    let holds = Holds::new(
        &Timing {
            loaded: d,
            ..Timing::default()
        },
        200.0,
        RATE,
    );
    assert_eq!(
        holds.steps(None),
        [(40, 0, 0), (100, 8160, 5280), (127, 8160, 5280)]
    );
}
