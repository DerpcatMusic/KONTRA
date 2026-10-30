//! What a level meter shows over time. Kontakt 8.13's meters (decoded from
//! its binary, scratch/laneJ/meter/FACTS.md) light `1 + log2(peak) / 9` of
//! their length, rise at once, fall 0.4 of the way to the level each GUI
//! tick, hold the peak line 10 ticks and the overload light 25. The 40 ms
//! tick is inferred: 25 ticks of overload make one second, and the value
//! edit's measured arrow repeat waits the same 10 ticks (0.4 s).

/// Kontakt's GUI tick, in seconds.
pub const TICK: f64 = 0.04;
const FALL: f64 = 0.4;
const PEAK_HOLD: u32 = 10;
const OVER_HOLD: u32 = 25;
/// Ticks a frame catches up at most: enough to settle any meter.
const CATCH_UP: u32 = 64;

/// How much of a meter a peak lights: linear in dB from −54.2 dB (nine
/// octaves down) to 0 dB.
pub fn fraction(peak: f64) -> f64 {
    (1. + peak.clamp(1e-9, 1.).log2() / 9.).clamp(0., 1.)
}

/// One channel of a meter as it shows: the lit fraction, the peak line's,
/// and whether the overload light is on.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Shown {
    pub level: f64,
    pub peak: f64,
    pub over: bool,
}

#[derive(Clone, Copy, Debug, Default)]
struct Channel {
    shown: Shown,
    held: u32,
    over: u32,
    /// The highest peak since the last tick.
    pending: f64,
}

impl Channel {
    fn tick(&mut self) {
        let target = fraction(self.pending);
        let level = self.shown.level;
        self.shown.level = if target >= level {
            target
        } else {
            let fallen = level + (target - level) * FALL;
            if fallen - target < 0.001 {
                target
            } else {
                fallen
            }
        };
        if self.shown.level > self.shown.peak || self.held == 0 {
            self.shown.peak = self.shown.level;
            self.held = PEAK_HOLD;
        } else {
            self.held -= 1;
        }
        self.over = if self.pending >= 1. {
            OVER_HOLD
        } else {
            self.over.saturating_sub(1)
        };
        self.shown.over = self.over > 0;
        self.pending = 0.;
    }

    fn settled(&self) -> bool {
        self.shown == Shown::default()
    }
}

/// A face's meters: one per source (the part's output, then each bus) and
/// channel.
#[derive(Debug, Default)]
pub struct Meters {
    channels: Vec<[Channel; 2]>,
    shown: Vec<[Shown; 2]>,
    /// Frame time of the last tick.
    tick: f64,
}

impl Meters {
    /// Folds this frame's `peaks` in at frame time `now`: what the meters
    /// show, and whether one still moves (a frame is owed a tick later).
    pub fn update(&mut self, peaks: &[[f64; 2]], now: f64) -> (&[[Shown; 2]], bool) {
        self.channels.resize(peaks.len(), Default::default());
        for (c, p) in self.channels.iter_mut().zip(peaks) {
            for (c, p) in c.iter_mut().zip(p) {
                c.pending = c.pending.max(*p);
            }
        }
        if now < self.tick || now - self.tick > f64::from(CATCH_UP) * TICK {
            self.tick = now - f64::from(CATCH_UP) * TICK;
        }
        while self.tick + TICK <= now {
            self.tick += TICK;
            self.channels.iter_mut().flatten().for_each(Channel::tick);
        }
        self.shown.clear();
        let shown = self.channels.iter().map(|c| c.map(|c| c.shown));
        self.shown.extend(shown);
        let moving = !self.channels.iter().flatten().all(Channel::settled);
        (&self.shown, moving)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_meter_maps_falls_and_holds_as_kontakts_do() {
        // Linear in dB over nine octaves: 0 dB full, -27 dB half, -54 dB dark.
        let at = [1., 2_f64.powf(-4.5), 2_f64.powi(-9), 2.];
        assert_eq!(at.map(fraction), [1., 0.5, 0., 1.]);
        let mut m = Meters::default();
        let mut t = 10.;
        let mut frame = |peak: f64, m: &mut Meters| {
            t += TICK;
            m.update(&[[peak, 0.]], t).0[0][0]
        };
        frame(0., &mut m);
        // Rises at once; a clip lights overload.
        let full = frame(1., &mut m);
        assert_eq!((full.level, full.peak, full.over), (1., 1., true));
        // Falls 0.4 of the gap a tick; the peak line holds.
        let fell = frame(0., &mut m);
        assert!((fell.level - 0.6).abs() < 1e-9 && fell.peak == 1.);
        for _ in 0..9 {
            frame(0., &mut m);
        }
        let held = frame(0., &mut m);
        assert!(held.peak < 1., "the peak line drops after 10 ticks");
        for _ in 0..13 {
            frame(0., &mut m);
        }
        assert!(!frame(0., &mut m).over, "overload holds 25 ticks");
        let (_, moving) = m.update(&[[0., 0.]], t + 10.);
        assert!(!moving, "silence settles");
    }
}
