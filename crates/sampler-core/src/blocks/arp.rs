//! Arpeggiator: turns the held notes into a stepped note stream. Control-rate,
//! fixed capacity, no allocation. Events are delivered through a callback with
//! the frame offset inside the advanced span. Not Kontakt's "Arp" (0x09): that
//! runtime object, `BArp32Step`, is a note-blind 32-step table modulator and
//! lives in [`crate::blocks::step_table`]. This block serves formats with a
//! real arpeggiator (UVI).

pub const MAX_HELD: usize = 16;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArpOrder {
    Up,
    Down,
    UpDown,
    DownUp,
    AsPlayed,
    Random,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Arp {
    pub order: ArpOrder,
    /// Step length in quarter-note beats.
    pub step_beats: f64,
    /// Sounding fraction of a step (0..=1).
    pub gate: f64,
    /// Octave repeats of the held set, at least 1.
    pub octaves: u8,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ArpEvent {
    On { key: u8, at: u32 },
    Off { key: u8, at: u32 },
}

#[derive(Clone, Copy, Debug, Default)]
pub struct ArpState {
    played: [u8; MAX_HELD],
    count: usize,
    step: u64,
    /// Frames until the next step starts (relative to the span start).
    next_on: f64,
    off: Option<(u8, f64)>,
    seed: u64,
}

impl ArpState {
    pub fn note_on(&mut self, key: u8) {
        if self.count < MAX_HELD && !self.played[..self.count].contains(&key) {
            if self.count == 0 {
                self.step = 0;
                self.next_on = 0.0;
            }
            self.played[self.count] = key;
            self.count += 1;
        }
    }

    pub fn note_off(&mut self, key: u8) {
        if let Some(i) = self.played[..self.count].iter().position(|&k| k == key) {
            self.played.copy_within(i + 1..self.count, i);
            self.count -= 1;
        }
    }

    pub fn reset(&mut self, seed: u64) {
        *self = Self { seed, ..Self::default() };
    }

    /// The key at step `n` of the pattern (`None` when nothing is held).
    fn key(&self, arp: &Arp, n: u64) -> Option<u8> {
        if self.count == 0 {
            return None;
        }
        let octaves = usize::from(arp.octaves.max(1));
        let len = self.count * octaves;
        let n = n as usize;
        let period = (2 * len).saturating_sub(2).max(1);
        let q = n % period;
        let bounce = if q < len { q } else { period - q };
        let p = match arp.order {
            ArpOrder::Up | ArpOrder::AsPlayed => n % len,
            ArpOrder::Down => len - 1 - n % len,
            ArpOrder::UpDown => bounce,
            ArpOrder::DownUp => len - 1 - bounce,
            ArpOrder::Random => {
                (crate::blocks::lfo::hash(self.seed, 0, n as u64) % len as u64) as usize
            }
        };
        let (slot, octave) = (p % self.count, p / self.count);
        let held = &self.played[..self.count];
        let key = if arp.order == ArpOrder::AsPlayed {
            held[slot]
        } else {
            // The slot-th smallest held key, without allocating.
            *held
                .iter()
                .find(|&&k| held.iter().filter(|&&j| j < k).count() == slot)?
        };
        u8::try_from(u32::from(key) + 12 * octave as u32)
            .ok()
            .filter(|&k| k < 128)
    }

    /// Advance `frames` at `tempo` BPM and `rate` Hz, reporting events in order.
    pub fn advance(
        &mut self,
        arp: &Arp,
        frames: u32,
        tempo: f64,
        rate: f64,
        mut emit: impl FnMut(ArpEvent),
    ) {
        let span = f64::from(frames);
        let step = (arp.step_beats * 60.0 / tempo * rate).max(1.0);
        if self.count == 0 {
            if let Some((key, _)) = self.off.take() {
                emit(ArpEvent::Off { key, at: 0 });
            }
            return;
        }
        loop {
            let on = self.next_on;
            let off = self.off.map(|o| o.1);
            if off.is_some_and(|o| o < span && o <= on) {
                let (key, at) = self.off.take().unwrap();
                emit(ArpEvent::Off { key, at: at.max(0.0) as u32 });
            } else if on < span {
                if let Some((key, _)) = self.off.take() {
                    emit(ArpEvent::Off { key, at: on.max(0.0) as u32 });
                }
                if let Some(key) = self.key(arp, self.step) {
                    emit(ArpEvent::On { key, at: on.max(0.0) as u32 });
                    self.off = Some((key, on + step * arp.gate.clamp(0.0, 1.0)));
                }
                self.step += 1;
                self.next_on = on + step;
            } else {
                break;
            }
        }
        self.next_on -= span;
        if let Some(o) = &mut self.off {
            o.1 -= span;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(arp: Arp, keys: &[u8], frames: u32) -> Vec<ArpEvent> {
        let mut s = ArpState::default();
        s.reset(5);
        for &k in keys {
            s.note_on(k);
        }
        let mut out = Vec::new();
        // 120 BPM, 1000 Hz: a quarter-note step is 500 frames.
        s.advance(&arp, frames, 120.0, 1000.0, |e| out.push(e));
        out
    }

    fn ons(events: &[ArpEvent]) -> Vec<u8> {
        events
            .iter()
            .filter_map(|e| match e {
                ArpEvent::On { key, .. } => Some(*key),
                _ => None,
            })
            .collect()
    }

    const UP: Arp = Arp { order: ArpOrder::Up, step_beats: 1.0, gate: 0.5, octaves: 1 };

    #[test]
    fn orders_walk_the_held_notes() {
        let e = |order| ons(&run(Arp { order, ..UP }, &[64, 60, 67], 3000));
        assert_eq!(e(ArpOrder::Up), [60, 64, 67, 60, 64, 67]);
        assert_eq!(e(ArpOrder::Down), [67, 64, 60, 67, 64, 60]);
        assert_eq!(e(ArpOrder::UpDown), [60, 64, 67, 64, 60, 64]);
        assert_eq!(e(ArpOrder::DownUp), [67, 64, 60, 64, 67, 64]);
        assert_eq!(e(ArpOrder::AsPlayed), [64, 60, 67, 64, 60, 67]);
        assert_eq!(ons(&run(Arp { octaves: 2, ..UP }, &[60, 64], 2000)), [60, 64, 72, 76]);
    }

    #[test]
    fn gate_timing_and_split_spans_agree() {
        let whole = run(UP, &[60], 1000);
        assert_eq!(
            whole,
            [
                ArpEvent::On { key: 60, at: 0 },
                ArpEvent::Off { key: 60, at: 250 },
                ArpEvent::On { key: 60, at: 500 },
                ArpEvent::Off { key: 60, at: 750 },
            ]
        );
        // The same stream when advanced in uneven spans (offsets rebased).
        let mut s = ArpState::default();
        s.note_on(60);
        let (mut base, mut got) = (0u32, Vec::new());
        for span in [100, 150, 300, 1, 99, 200, 150] {
            s.advance(&UP, span, 120.0, 1000.0, |e| {
                got.push(match e {
                    ArpEvent::On { key, at } => ArpEvent::On { key, at: at + base },
                    ArpEvent::Off { key, at } => ArpEvent::Off { key, at: at + base },
                })
            });
            base += span;
        }
        assert_eq!(got, whole);
    }

    #[test]
    fn releasing_every_note_stops_the_sounding_one() {
        let mut s = ArpState::default();
        s.note_on(60);
        let mut out = Vec::new();
        s.advance(&UP, 100, 120.0, 1000.0, |e| out.push(e));
        s.note_off(60);
        s.advance(&UP, 100, 120.0, 1000.0, |e| out.push(e));
        assert_eq!(out, [ArpEvent::On { key: 60, at: 0 }, ArpEvent::Off { key: 60, at: 0 }]);
    }
}
