//! Test-only source-bound numeric kernel for configured UVI Lorenz and User LFOs.
//!
//! This is a raw control-point building block. It neither assigns source owners
//! nor implements note, transport, shared-state or parameter-graph scheduling.
//! Its caller owns the state and borrowed immutable table, supplies the proved
//! update cadence, and keeps them alive through every consumer. In particular,
//! a Program/Layer source must not become one independent instance per voice.
//!
//! Original instruction/state oracles are recorded in
//! `docs/architecture-v2/NATIVE_UVI_OFFLINE_EVIDENCE.md`. This kernel does not
//! admit additional importer features or implement a native object ABI.

/// A refusal outside the configured, source-reviewed kernel domain.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DomainError {
    SampleRate,
    Frequency,
    Coordinates,
    Phase,
    TableNonFinite,
    FrameCount,
    Amplitude,
    NonFiniteResult,
}

/// A checked borrowed table with an explicit final interpolation guard cell.
///
/// Construction scans 257 values on the control side. The handle contains only
/// an immutable borrow; generation never clones an allocation or reference count.
#[derive(Clone, Copy, Debug)]
struct Table<'a>(&'a [f32; 257]);

impl<'a> Table<'a> {
    fn checked(values: &'a [f32; 257]) -> Result<Self, DomainError> {
        if values.iter().any(|v| !v.is_finite()) {
            return Err(DomainError::TableNonFinite);
        }
        Ok(Self(values))
    }
}

#[derive(Clone, Copy, Debug)]
enum Waveform<'a> {
    Lorenz,
    User(Table<'a>),
}

/// Explicit numeric inputs, with a defined finite native phase conversion.
/// Exact original vectors currently cover 44.1/48/96 kHz; no host admission is implied.
#[derive(Clone, Copy, Debug)]
struct Parameters {
    rate: f32,
    frequency: f32,
}

impl Parameters {
    fn checked(rate: f32, frequency: f32) -> Result<Self, DomainError> {
        if !rate.is_finite() || rate <= 0.0 {
            return Err(DomainError::SampleRate);
        }
        if !frequency.is_finite() || !(0.0..=20.0).contains(&frequency) {
            return Err(DomainError::Frequency);
        }
        let increment = (16_777_216.0 / rate) * frequency * 256.0;
        // CVTTSS2SI's finite signed-64 domain, before taking its low 32 bits.
        if !increment.is_finite() || increment >= 9_223_372_036_854_775_808.0 {
            return Err(DomainError::SampleRate);
        }
        Ok(Self { rate, frequency })
    }
}

/// Only future waveform state; no native pointers, callbacks or factory handles.
#[derive(Clone, Copy, Debug, PartialEq)]
struct State {
    coordinates: [f32; 3],
    phase: u32,
}

impl State {
    /// Start from caller-supplied scalar state, without implying a native trigger.
    fn explicit(coordinates: [f32; 3], phase: u32) -> Result<Self, DomainError> {
        if coordinates.iter().any(|v| !v.is_finite()) {
            return Err(DomainError::Coordinates);
        }
        Ok(Self { coordinates, phase })
    }

    fn coordinates(&self) -> [f32; 3] {
        self.coordinates
    }
    fn phase(&self) -> u32 {
        self.phase
    }

    /// Reseed raw coordinates and phase using the original finite CRT polynomial.
    ///
    /// This is not an engine/event reset: delay, rise, smoothing, note ownership
    /// and callback state are deliberately outside this building block.
    fn reseed(&mut self, phase: f32) -> Result<(), DomainError> {
        if !phase.is_finite() || !(0.0..=1.0).contains(&phase) {
            return Err(DomainError::Phase);
        }
        let angle = phase * f32::from_bits(0x40c9_0fdb) + f32::from_bits(0x3f49_0fdb);
        let sine = finite_sine(angle);
        let cosine = finite_sine(angle.abs() + f32::from_bits(0x3fc9_0fdb));
        self.coordinates = [
            cosine * f32::from_bits(0x3fb5_04f3),
            sine * f32::from_bits(0x3fb5_04f3),
            1.0,
        ];
        // Original CVTTSS2SI is a 64-bit conversion followed by its low32 bits.
        // In particular phase1 wraps User phase to0, but must not be normalized
        // before computing the chaos coordinates above.
        self.phase = ((phase * 4_294_967_296.0) as i64) as u32;
        Ok(())
    }

    /// Generate raw points for an explicit actual call of at most64 frames.
    ///
    /// The extra boundary point does not commit its lookahead state. Native
    /// Euler integration follows the actual call partitions; this does not make
    /// arbitrary host partitions equivalent. The caller must provide the proper
    /// source/control-point cache and update cadence. Errors leave state intact.
    fn generate(
        &mut self,
        waveform: Waveform<'_>,
        parameters: Parameters,
        frames: u32,
    ) -> Result<Points, DomainError> {
        if frames > 64 {
            return Err(DomainError::FrameCount);
        }
        let mut next = *self;
        let mut points = Points::empty();
        if frames == 0 {
            return Ok(points);
        }
        match waveform {
            Waveform::Lorenz => {
                let dt = parameters.frequency / parameters.rate;
                let mut remaining = frames;
                while remaining != 0 {
                    let count = remaining.min(32);
                    remaining -= count;
                    let derivative = next.derivatives();
                    let step = count as f32 * dt;
                    for (coordinate, derivative) in next.coordinates.iter_mut().zip(derivative) {
                        *coordinate = derivative * step + *coordinate;
                    }
                    points
                        .push((next.coordinates[0] * f32::from_bits(0x3d51_2ec7)).clamp(-1.0, 1.0));
                }
                let derivative = next.derivatives();
                let pad = frames.wrapping_neg() & 31;
                points.push(
                    ((pad as f32 * dt * derivative[0] + next.coordinates[0])
                        * f32::from_bits(0x3d51_2ec7))
                    .clamp(-1.0, 1.0),
                );
            }
            Waveform::User(table) => {
                let increment =
                    ((16_777_216.0 / parameters.rate) * parameters.frequency * 256.0) as i64 as u32;
                for offset in (0..frames).step_by(32) {
                    points.push(lookup(
                        table,
                        next.phase.wrapping_add(offset.wrapping_mul(increment)),
                    ));
                }
                let boundary = frames.div_ceil(32) * 32;
                points.push(lookup(
                    table,
                    next.phase.wrapping_add(boundary.wrapping_mul(increment)),
                ));
                next.phase = next.phase.wrapping_add(frames.wrapping_mul(increment));
            }
        }
        if next.coordinates.iter().any(|v| !v.is_finite())
            || points.as_slice().iter().any(|v| !v.is_finite())
        {
            return Err(DomainError::NonFiniteResult);
        }
        *self = next;
        Ok(points)
    }

    fn derivatives(&self) -> [f32; 3] {
        let [x, y, z] = self.coordinates;
        [
            (y - x) * 10.0,
            (28.0 - z) * x - y,
            x * y - z * f32::from_bits(0x402a_9fbe),
        ]
    }
}

/// At most ceil(64/32)+1 points; no allocation, destruction or shared ownership.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Points {
    values: [f32; 3],
    len: u8,
}

impl Points {
    fn empty() -> Self {
        Self {
            values: [0.0; 3],
            len: 0,
        }
    }
    fn push(&mut self, value: f32) {
        self.values[self.len as usize] = value;
        self.len += 1;
    }
    fn as_slice(&self) -> &[f32] {
        &self.values[..usize::from(self.len)]
    }

    /// Apply the original output stage to already configured raw points.
    ///
    /// Depth0 is intentionally outside this initial oracle domain: the native
    /// optimized zero-fill branch has not been witnessed. This is not a
    /// replacement for delay, rise, smoothing or live parameter updates.
    fn apply_amplitude(&mut self, depth: f32, bipolar: bool) -> Result<(), DomainError> {
        if !depth.is_finite() || !(0.0 < depth && depth <= 1.0) {
            return Err(DomainError::Amplitude);
        }
        for value in &mut self.values[..usize::from(self.len)] {
            *value = if bipolar {
                *value * depth
            } else {
                (*value + 1.0) * (depth * 0.5)
            };
        }
        Ok(())
    }
}

fn lookup(table: Table<'_>, phase: u32) -> f32 {
    let i = (phase >> 24) as usize;
    let fraction = (phase & 0x00ff_ffff) as f32 * (1.0 / 16_777_216.0);
    (1.0 - fraction) * table.0[i] + fraction * table.0[i + 1]
}

// Exact normal finite source branch. Inputs from the checked phase domain are
// below9; exceptional-range helpers and platform libm are not involved.
fn finite_sine(value: f32) -> f32 {
    let sign = value.to_bits() & 0x8000_0000;
    let x = f32::from_bits(value.to_bits() & 0x7fff_ffff);
    let magic = x * f32::from_bits(0x3ea2_f983) + 12_582_912.0;
    let n = magic - 12_582_912.0;
    let quadrant = magic.to_bits().wrapping_shl(31);
    let mut reduced = x - f32::from_bits(0x4049_0000) * n;
    reduced -= f32::from_bits(0x3a7d_a000) * n;
    reduced -= f32::from_bits(0x3422_2000) * n;
    reduced -= f32::from_bits(0x2cb4_611a) * n;
    let square = reduced * reduced;
    reduced = f32::from_bits(reduced.to_bits() ^ quadrant);
    let mut poly = f32::from_bits(0x362e_def8) * square;
    poly += f32::from_bits(0xb94f_b7ff);
    poly *= square;
    poly += f32::from_bits(0x3c08_8766);
    poly *= square;
    poly += f32::from_bits(0xbe2a_aaa6);
    let result = reduced + (square * poly) * reduced;
    f32::from_bits(result.to_bits() ^ sign)
}

#[path = "uvi_lfo/native_vectors.rs"]
mod native_vectors;
