//! Auto-align measurement on the engine: a loaded patch played as a
//! [`kontra_align::Player`], offline, with each read waiting for the disk.

use std::sync::Arc;
use std::time::Duration;

use kontra_engine::{Engine, Event, HostNote, HostPattern, Plan, SampleTime, Timed};
use kontra_stream::StreamClient;

use crate::rack::Hold;

/// A patch the measurement plays. Each reset starts a fresh engine, so its
/// scripts begin from `on init` every time.
pub struct Measured {
    plan: Arc<Plan>,
    stream: StreamClient,
    rate: u32,
    /// The key that picks each articulation.
    switches: Vec<u8>,
    engine: Engine,
    events: Vec<Timed<Event>>,
}

impl Measured {
    #[must_use]
    pub fn new(plan: Arc<Plan>, stream: StreamClient, rate: u32, switches: Vec<u8>) -> Self {
        let engine = Engine::new(Arc::clone(&plan), stream.clone(), rate);
        Self {
            plan,
            stream,
            rate,
            switches,
            engine,
            events: Vec::new(),
        }
    }

    fn send(&mut self, key: u8, velocity: u8, on: bool) {
        let note = HostNote {
            port: 0,
            channel: 0,
            key,
            id: -1,
            clap: true,
        };
        let body = if on {
            Event::NoteOn {
                note,
                velocity: (f32::from(velocity) / 127.0).into(),
                tune: 0.0,
            }
        } else {
            let pattern = HostPattern {
                port: 0,
                channel: 0,
                key: i32::from(key),
                id: -1,
                clap: true,
            };
            Event::NoteOff(pattern)
        };
        self.events.push(Timed {
            at: SampleTime(0),
            body,
        });
    }
}

impl kontra_align::Player for Measured {
    fn rate(&self) -> f64 {
        f64::from(self.rate)
    }

    fn reset(&mut self) {
        self.engine = Engine::new(Arc::clone(&self.plan), self.stream.clone(), self.rate);
        self.events.clear();
        // Dynamics and expression as a player leaves them, mostly up, and a
        // quarter second for scripts that wait while they load.
        for (cc, value) in [(1u32, 100u32), (11, 127)] {
            let word = 0x20B0_0000 | cc << 8 | value;
            self.events.push(Timed {
                at: SampleTime(0),
                body: Event::Ump([word, 0]),
            });
        }
        let mut sink = Vec::new();
        kontra_align::Player::render(self, self.rate as usize / 4, &mut sink);
    }

    fn articulation(&mut self, row: usize) {
        if let Some(&key) = self.switches.get(row) {
            self.send(key, 100, true);
            let mut sink = Vec::new();
            kontra_align::Player::render(self, 256, &mut sink);
            self.send(key, 0, false);
        }
    }

    fn note(&mut self, key: u8, velocity: u8, on: bool) {
        self.send(key, velocity, on);
    }

    fn render(&mut self, frames: usize, out: &mut Vec<f32>) {
        let mut block = [[0f32; 2]; 64];
        let mut left = frames;
        while left > 0 {
            let n = left.min(block.len());
            let Some(part) = block.get_mut(..n) else {
                break;
            };
            self.stream.settle(Duration::from_secs(5));
            self.engine.process(part, &self.events);
            self.events.clear();
            out.extend(part.iter().map(|[l, r]| l + r));
            left -= n;
        }
    }
}

/// What each note of a part with `timing` waits against the `reported`
/// latency (ms), for [`crate::rack::Rack::set_hold`].
#[must_use]
pub fn hold(timing: &kontra_align::Timing, reported: f32, rate: f64) -> Hold {
    let holds = kontra_align::Holds::new(timing, reported, rate);
    let mut hold = Hold {
        mono: holds.mono(None),
        ..Hold::flat(0)
    };
    // ponytail: past HOLD_STEPS ranges the last takes the rest.
    let steps = holds.steps(None);
    for (n, slot) in hold.steps.iter_mut().enumerate() {
        if let Some(&step) = steps.get(n).or(steps.last()) {
            *slot = step;
        }
    }
    hold
}
