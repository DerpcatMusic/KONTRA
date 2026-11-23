//! The sources a modulation route reads, and the laws it moves by.

/// A sum of live sources, as a macro knob and what moves it:
/// `base + Σ amount × input × Π factors`. [`Source::Blend`] reads it.
#[derive(Clone, Debug, PartialEq)]
pub struct Blend {
    pub base: f32,
    pub terms: Vec<Term>,
}

/// One input of a [`Blend`]: its source through a table, scaled.
#[derive(Clone, Debug, PartialEq)]
pub struct Term {
    pub input: Transfer,
    pub amount: f32,
    pub factors: Vec<Factor>,
}

/// A live multiplier: `1 − depth + depth × t` (`1 + depth × t` for a
/// negative depth), `t` its source through its table.
#[derive(Clone, Debug, PartialEq)]
pub struct Factor {
    pub input: Transfer,
    pub depth: f32,
}

impl Factor {
    /// The multiplier for the table's output `t`.
    #[must_use]
    pub fn at(&self, t: f32) -> f32 {
        let d = self.depth;
        if d < 0.0 {
            1.0 + d * t
        } else {
            1.0 - d + d * t
        }
    }
}

/// A source through a transfer table: `table` holds the outputs for evenly
/// spaced inputs 0..=1, read with linear interpolation (or held, `stepped`);
/// an LFO enters as
/// `(x + 1)/2`. Its source is never itself shaped.
///
/// The output is used as is, in the domain the route that reads it consumes
/// (a pitch or sample-start route reads -1..=1, a gain 0..=1): the engine
/// converts no polarity, so a translator emits each table in its route's domain.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Transfer {
    pub source: Source,
    pub table: Vec<f32>,
    /// Seconds of a one-pole after the table (0: none): the output reaches
    /// `1 − e^−1` of a step in this long. Read for an LFO source, from the
    /// LFO's own past, so it needs no state and follows the tempo.
    pub smooth_s: f32,
    /// Each point holds until the next: `x` reads point `⌊(len − 1)·x⌋`,
    /// never between two, so a 14-bit source steps where a 7-bit one would.
    pub stepped: bool,
}

/// Taps of [`Transfer::smoothed`] over five time constants.
const SMOOTH_TAPS: usize = 20;

impl Transfer {
    /// The output smoothed by [`Self::smooth_s`]: `at(back)` is the table's
    /// output `back` seconds ago, averaged under `e^(−back/τ)` (a one-pole's
    /// impulse response) over five time constants.
    pub fn smoothed(&self, mut at: impl FnMut(f32) -> Option<f32>) -> Option<f32> {
        if self.smooth_s <= 0.0 {
            return at(0.0);
        }
        let (mut sum, mut norm) = (0.0, 0.0);
        for j in 0..SMOOTH_TAPS {
            let back = 5.0 * self.smooth_s * j as f32 / (SMOOTH_TAPS - 1) as f32;
            let end = if j == 0 || j == SMOOTH_TAPS - 1 {
                0.5
            } else {
                1.0
            };
            let w = end * (-back / self.smooth_s).exp();
            sum += w * at(back)?;
            norm += w;
        }
        Some(sum / norm)
    }
    /// The table's output for `x`, clamped to 0..=1; an empty table passes
    /// `x` through.
    #[must_use]
    pub fn apply(&self, x: f32) -> f32 {
        let Some(last) = self.table.len().checked_sub(1) else {
            return x;
        };
        let at = x.clamp(0.0, 1.0) * last as f32;
        let i = (at as usize).min(last);
        if self.stepped {
            return self.table.get(i).copied().unwrap_or(x);
        }
        let (a, b) = (
            self.table.get(i).copied().unwrap_or(x),
            self.table
                .get(i + 1)
                .or(self.table.get(i))
                .copied()
                .unwrap_or(x),
        );
        a + (b - a) * (at - i as f32)
    }
}

/// How a modulation route scales an envelope stage's duration `t` (ms) by
/// `m = 1 − (1 − x)·|intensity|` (`x` on 0..=1, inverted as `1 − x` when
/// `intensity < 0`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TimeScale {
    /// Linear duration scaling: `t · m^power`.
    Linear { power: u8 },
    /// Logarithmic knob scaling in `ln(t + floor_ms)`:
    /// `floor_ms · ((t + floor_ms) / floor_ms)^(m^power) − floor_ms`.
    LogKnob { floor_ms: f32, power: u8 },
}

/// How a volume modulation turns its source into gain.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Curve {
    /// Linear in dB: `intensity ×` this many dB down at source 0.
    Decibels(f32),
    /// `(1 − (1 − x') × |intensity|)^power`, where `x' = 1 − x` when
    /// `intensity < 0`: the source pulls the gain down from unity by its
    /// depth, never boosting above unity; power 1 is linear in amplitude.
    Depth { power: u8 },
    /// [`Self::Depth`] at power 1 on `x²`; a negative intensity reads `1 − x²`.
    Squared,
    /// `1 − x × intensity`: the gain falls from unity as the source rises.
    Falling,
    /// [`Self::Depth`] at power 1 for an intensity of 0 or more, and
    /// `1 + x × intensity` below: the sign of the depth picks whether the
    /// gain falls as the source falls or as it rises.
    Signed,
}

impl Curve {
    /// Linear gain for source value `x` (0..=1) at `intensity`.
    #[must_use]
    pub fn gain(self, intensity: f32, x: f32) -> f32 {
        let x = x.clamp(0.0, 1.0);
        match self {
            Self::Decibels(range) => 10f32.powf(-range * intensity * (1.0 - x) / 20.0),
            Self::Depth { power } => {
                let x = if intensity < 0.0 { 1.0 - x } else { x };
                (1.0 - (1.0 - x) * intensity.abs())
                    .max(0.0)
                    .powi(i32::from(power))
            }
            Self::Squared => {
                let x = if intensity < 0.0 { 1.0 - x * x } else { x * x };
                (1.0 - (1.0 - x) * intensity.abs()).max(0.0)
            }
            Self::Falling => (1.0 - x * intensity).max(0.0),
            Self::Signed if intensity < 0.0 => (1.0 + x * intensity).max(0.0),
            Self::Signed => (1.0 - (1.0 - x) * intensity).max(0.0),
        }
    }
}

/// A power of two as a sampler computes it: exactly, or as
/// `2^n·(1 + f − c·(f − f²))` for `x = n + f` (`f` in 0..1), a quadratic
/// exact at whole octaves and up to 4.6 cents flat between them for
/// `c = 0.33971`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Exp2 {
    Exact,
    Quadratic { c: f32 },
}

impl Exp2 {
    #[must_use]
    pub fn of(self, x: f64) -> f64 {
        match self {
            Self::Exact => x.exp2(),
            Self::Quadratic { c } => {
                let n = x.floor();
                let f = x - n;
                n.exp2() * (1.0 + f - f64::from(c) * (f - f * f))
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Wave {
    Sine,
    Triangle,
    /// Falling: `1 − 2p`.
    Saw,
    /// Rising: `2p − 1`.
    Ramp,
    /// High for `width` of the cycle.
    Square {
        width: f32,
    },
    /// A new random level each half cycle.
    Random,
    /// `−w0·sine + w1·saw(2p) + w2·triangle − w3·pulse(width) + w4·random
    /// + w5·ramp`, the saw rising twice a cycle, the triangle from −1 and
    /// the ramp falling once (`1 − 2p`); divided by `max(1, Σ|w|)` when
    /// `normalize`.
    Multi {
        weights: [f32; 6],
        width: f32,
        normalize: bool,
    },
    /// A chaotic `attractor` from (1, 1, 1), one Euler step a span of
    /// `step × rate` time units a second; its x times `scale`.
    Chaos {
        attractor: Attractor,
        step: f32,
        scale: f32,
    },
}

/// A chaotic system's equations.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Attractor {
    /// `x' = σ(y − x)`, `y' = x(ρ − z) − y`, `z' = xy − βz`.
    Lorenz { sigma: f32, rho: f32, beta: f32 },
    /// `x' = −y − z`, `y' = x + a·y`, `z' = b + z(x − c)`.
    Rossler { a: f32, b: f32, c: f32 },
}

/// Per-note expression dimension (CLAP note expression / MPE / VST3).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum NoteExpr {
    /// Per-note pressure (falls back to channel aftertouch), `0..=1`.
    Pressure,
    /// Per-note brightness / MPE timbre (falls back to CC 74), `0..=1`.
    Brightness,
    /// Per-note vibrato depth (falls back to CC 1), `0..=1`.
    Vibrato,
    /// Per-note expression (falls back to CC 11), `0..=1`.
    Expression,
    /// Per-note pitch expression, normalized `semitones / 12.0` clamped to `-1..=1`.
    Tune,
    /// Per-note gain multiplier (`0..=4`, unity at 1.0).
    Gain,
    /// Per-note stereo pan (`-1..=1`).
    Pan,
}

/// A per-note modulation source, 0..=1 unless noted.
#[derive(Default, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Source {
    Velocity,
    /// Note-off release velocity, 0..=1 (0.5 when a note-off carried none).
    ReleaseVelocity,
    Controller(u8),
    /// Controller `cc`, reading `start` (0..=127) until its first message
    /// on the channel arrives.
    ControllerFrom {
        cc: u8,
        start: u8,
    },
    /// The channel's pitch bend, −1..=1.
    PitchBend,
    /// The note's pressure: its own (a per-note expression, or polyphonic
    /// aftertouch on its key) once it has one, else its channel's (an MPE
    /// member channel's is its one note's).
    Pressure,
    /// A per-note expression dimension (CLAP/VST3/MPE), falling back to the
    /// matching channel controller when no per-note value was sent.
    NoteExpr(NoteExpr),
    /// A MIDI 2.0 per-note controller (RPNC when `registered`, assignable APNC
    /// otherwise), normalized `0..=1`; reads `0.0` until sent for the note.
    PerNoteCc {
        registered: bool,
        index: u8,
    },
    /// The note's key over 127.
    Key,
    /// The group's [`Group::envelopes`](crate::Group::envelopes) entry.
    Envelope(u8),
    /// The group's [`Group::shapers`](crate::Group::shapers) entry: its source through its table.
    Shaped(u8),
    /// The group's [`Group::lfos`](crate::Group::lfos) entry, −1..=1.
    Lfo(u8),
    /// The group's [`Group::blends`](crate::Group::blends) entry.
    Blend(u8),
    /// Always 1.
    #[default]
    Constant,
    /// The value a script sent this note for "from script" modulator `id`,
    /// −1..=1; 0 until sent.
    Script(u16),
    /// Macro `n` of a scripted program, −1..=1 as its script sets it: the
    /// script writes it on script channel [`Source::MACRO_CHANNELS`]` + n`.
    Macro(u16),
    /// 1 at note-on, falling linearly to 0 after `span_ms` held; frozen at
    /// key-up, so a release sample is quieter the longer its key was held.
    HeldTime {
        span_ms: u32,
    },
    /// A value drawn once per note and held: 0..=1, or −1..=1 when `bipolar`.
    Random {
        bipolar: bool,
    },
    /// The playing zone's [`Zone::envelopes`](crate::Zone::envelopes) entry
    /// matching this slot, bipolar −1..=1; zones without one skip the route.
    ZoneEnvelope(u8),
}

impl Source {
    /// The first script channel a [`Source::Macro`] is sent on, above every
    /// "from script" modulator id.
    pub const MACRO_CHANNELS: u16 = 0x8000;

    /// The controller it reads, if any: a note expression's fallback CC too.
    #[must_use]
    pub fn controller(self) -> Option<u8> {
        match self {
            Self::Controller(cc) | Self::ControllerFrom { cc, .. } => Some(cc),
            Self::NoteExpr(NoteExpr::Brightness) => Some(74),
            Self::NoteExpr(NoteExpr::Vibrato) => Some(1),
            Self::NoteExpr(NoteExpr::Expression) => Some(11),
            _ => None,
        }
    }

    /// Whether it reads the same for the whole of a note (what the note
    /// started with), so a route can read it once at note-on.
    #[must_use]
    pub fn fixed_for_note(self) -> bool {
        matches!(
            self,
            Self::Velocity
                | Self::ReleaseVelocity
                | Self::Key
                | Self::Constant
                | Self::Random { .. }
        )
    }

    /// The script channel it reads (a script modulator or a macro), if any.
    #[must_use]
    pub fn script_channel(self) -> Option<u16> {
        match self {
            Self::Script(id) => Some(id),
            Self::Macro(n) => Some(Self::MACRO_CHANNELS.saturating_add(n)),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Exp2;

    #[test]
    fn the_quadratic_power_of_two_plays_a_two_semitone_bend_flat() {
        let law = Exp2::Quadratic { c: 0.33971 };
        let semitones = |st: f64| 12.0 * law.of(st / 12.0).log2();
        // Kontakt on a ±2 st route: full bend up and down, a quarter each way.
        for (st, heard) in [(2.0, 1.954), (-2.0, -1.958), (0.5, 0.480), (-0.5, -0.485)] {
            assert!(
                (semitones(st) - heard).abs() < 0.001,
                "{st}: {}",
                semitones(st)
            );
        }
        assert_eq!(law.of(1.0), 2.0);
        assert_eq!(Exp2::Exact.of(-1.0), 0.5);
    }

    #[test]
    fn curve_depth_negative_intensity_inverts_source_without_boosting() {
        use super::Curve;
        for power in [1u8, 3] {
            let c = Curve::Depth { power };
            // Positive depth: x=1 -> 1.0, x=0 -> (1 - d)^power.
            assert!((c.gain(0.5, 1.0) - 1.0).abs() < 1e-6);
            assert!((c.gain(0.5, 0.0) - 0.5f32.powi(i32::from(power))).abs() < 1e-6);
            // Negative depth inverts x -> 1 - x and scales by |d|:
            // x=0 -> 1.0, x=1 -> (1 - |d|)^power, never > 1.0.
            assert!((c.gain(-0.5, 0.0) - 1.0).abs() < 1e-6);
            assert!((c.gain(-0.5, 1.0) - 0.5f32.powi(i32::from(power))).abs() < 1e-6);
            assert!((c.gain(-1.0, 1.0) - 0.0).abs() < 1e-6);
            assert!((c.gain(-1.0, 0.0) - 1.0).abs() < 1e-6);
            for k in 0..=10 {
                let x = k as f32 / 10.0;
                assert!(c.gain(-1.0, x) <= 1.0 + 1e-6);
            }
        }
    }
}
