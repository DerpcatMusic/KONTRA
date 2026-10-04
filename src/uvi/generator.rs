//! Native Program oscillators, with per-note phase state and resident resources.
//!
//! Parameter names/units: https://lua.uvi.net/_elements.html (Analog/Wavetable/FM).
//! Single-voice sine amplitude, polarity, phase and tuning were compared with
//! authored fixtures in official UVI Workstation 4.0.9, without license changes.
//! The discontinuous-wave anti-aliasing below is an original polynomial law,
//! not a reconstruction of UVI's minimum-phase BLEP implementation.

use super::{dsp::Frame, program::ProgramNode, sample::Sample, storage::Storage};
use anyhow::{Context, Result, ensure};
use std::{
    collections::hash_map::DefaultHasher,
    f64::consts::TAU,
    hash::{Hash, Hasher},
    sync::Arc,
};

pub const FIDELITY_DIAGNOSTIC: &str = "Native authored fixtures cover Analog sine/PWM, Analog/Wavetable deterministic unison phase/detune/gain/stereo, filename/channel-based wavetable slices and index spread, bounded PNG image-wavetable conversion, wavetable phase-distortion modes 0/3 and tracked sine-FM ratio modes 0/1/2, and tracked four-operator FM topologies 0/5/6/7/10 with D feedback; numerical parity remains unverified for polynomial anti-aliasing, hard-sync edges, random phase/noise sequences, linear wavetable readout and FM clock precision";

pub(crate) const IMAGE_BYTES_LIMIT: usize = 8 << 20;
const IMAGE_FRAMES: usize = 2048;

pub(crate) fn image_signature(bytes: &[u8]) -> bool {
    bytes.starts_with(b"\x89PNG\r\n\x1a\n") || bytes.starts_with(&[0xff, 0xd8])
}

fn image_dimensions(width: usize, height: usize) -> Result<()> {
    ensure!(width != 0 && height != 0, "Empty image wavetable");
    // Official 4.0.9 importer 0x141528b50 scales the whole image to
    // 2048 × min(height, 128), with resampling quality 1. The existing
    // authored oracle covers horizontal upsampling only; retain both gates.
    ensure!(
        width <= IMAGE_FRAMES,
        "Image wavetable requires native resampling to 2048 columns; downsampling fidelity is unverified"
    );
    ensure!(
        height <= 128,
        "Image wavetable requires native resampling to 128 rows; vertical resampling fidelity is unverified"
    );
    Ok(())
}

/// Decode only measured image color models, with limits before pixel allocation.
/// This resource is a table bank, never audio for a SamplePlayer or Lua readAudio.
pub(crate) fn image_wavetable(bytes: &[u8]) -> Result<Sample> {
    ensure!(
        bytes.len() <= IMAGE_BYTES_LIMIT,
        "Image wavetable exceeds 8 MiB source limit"
    );
    let (width, height, components, pixels) = if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        let decoder = png::Decoder::new_with_limits(
            std::io::Cursor::new(bytes),
            png::Limits {
                bytes: IMAGE_BYTES_LIMIT,
            },
        );
        let mut reader = decoder
            .read_info()
            .context("Invalid image-wavetable PNG header")?;
        let info = reader.info();
        let (width, height) = (info.width as usize, info.height as usize);
        image_dimensions(width, height)?;
        ensure!(
            info.bit_depth == png::BitDepth::Eight && info.animation_control.is_none(),
            "Only still 8-bit image-wavetable PNGs are measured"
        );
        let components = match info.color_type {
            png::ColorType::Grayscale => 1,
            png::ColorType::GrayscaleAlpha => 2,
            png::ColorType::Rgb => 3,
            png::ColorType::Rgba => 4,
            _ => anyhow::bail!("Indexed image-wavetable PNG color conversion is unverified"),
        };
        ensure!(
            info.trns.is_none(),
            "Image-wavetable PNG transparency is unverified"
        );
        let size = reader
            .output_buffer_size()
            .context("Image-wavetable PNG size overflow")?;
        ensure!(
            size == width * height * components,
            "Unexpected image-wavetable PNG layout"
        );
        let mut pixels = vec![0; size];
        let output = reader
            .next_frame(&mut pixels)
            .context("Invalid image-wavetable PNG pixels")?;
        ensure!(
            output.buffer_size() == size,
            "Incomplete image-wavetable PNG pixels"
        );
        (width, height, components, pixels)
    } else {
        ensure!(
            bytes.starts_with(&[0xff, 0xd8]),
            "Expected image-wavetable PNG or JPEG signature"
        );
        anyhow::bail!(
            "JPEG image-wavetable decoding is not implemented with verified native fidelity"
        )
    };
    image_pixels(width, height, components, &pixels)
}

fn image_pixels(width: usize, height: usize, components: usize, pixels: &[u8]) -> Result<Sample> {
    image_dimensions(width, height)?;
    ensure!(
        (1..=4).contains(&components) && pixels.len() == width * height * components,
        "Invalid image-wavetable pixel layout"
    );
    if components == 2 || components == 4 {
        ensure!(
            pixels
                .chunks_exact(components)
                .all(|pixel| pixel[components - 1] == 255),
            "Nonopaque image-wavetable alpha conversion is unverified"
        );
    }
    let mut brightness = vec![0u8; IMAGE_FRAMES * height];
    let colors = if components <= 2 { 1 } else { 3 };
    for x in 0..IMAGE_FRAMES {
        let position = (x as f64 + 0.5) * width as f64 / IMAGE_FRAMES as f64 - 0.5;
        let base = position.floor();
        let left = (base as isize).clamp(0, width as isize - 1) as usize;
        let right = (base as isize + 1).clamp(0, width as isize - 1) as usize;
        let fraction = ((position - base) * 256.).round() / 256.;
        for row in 0..height {
            let mut value = 0;
            for color in 0..colors {
                let a = pixels[(row * width + left) * components + color] as f64;
                let b = pixels[(row * width + right) * components + color] as f64;
                let normalized = ((a * (1. - fraction) + b * fraction) / 255.) as f32;
                // Authored native PNGs establish 8-bit subpixel interpolation
                // and truncation to ten fraction bits before byte rounding.
                // This is a measured conversion law, not vendor implementation code.
                let truncated = f32::from_bits(normalized.to_bits() & !0x1fff);
                value = value.max((truncated * 255.).round() as u8);
            }
            brightness[x * height + height - row - 1] = value;
        }
    }
    let flat = brightness.iter().all(|&value| value == brightness[0]);
    let mut values: Vec<f32> = brightness
        .into_iter()
        .map(|value| value as f32 * (1f32 / 255f32) * 255f32)
        .collect();
    // Fresh authored 96/128-row native PNGs establish this reciprocal
    // roundtrip and sequential f32 sum in reversed row/planar order.
    // Changing precision or traversal changes normalization at large totals.
    let mut mean = 0f32;
    for row in 0..height {
        for frame in 0..IMAGE_FRAMES {
            mean += values[frame * height + row];
        }
    }
    mean /= values.len() as f32;
    let peak = values.iter().copied().fold(f32::NEG_INFINITY, f32::max) - mean;
    for value in &mut values {
        *value = if flat { 0. } else { (*value - mean) / peak };
    }
    Ok(Sample {
        rate: 48_000,
        channels: height,
        frames: IMAGE_FRAMES,
        interleaved: Storage::from_f32(values)?,
        loops: Vec::new(),
        unity_note: None,
        wavetable_cycle_frames: Some(IMAGE_FRAMES as u32),
        wavetable_image: true,
        riff_metadata: Vec::new(),
    })
}

pub fn supports(kind: &str) -> bool {
    matches!(
        kind,
        "MinBlepGenerator" | "WaveTableOscillator" | "FmOscillator"
    )
}

fn number(node: &ProgramNode, name: &str, default: f64) -> Result<f64> {
    let value = node
        .attributes
        .get(name)
        .map(|s| s.parse::<f64>())
        .transpose()
        .with_context(|| format!("Invalid oscillator {name}"))?
        .unwrap_or(default);
    ensure!(value.is_finite(), "Nonfinite oscillator {name}");
    Ok(value)
}

pub fn validate(node: &ProgramNode) -> Result<()> {
    ensure!(supports(&node.kind), "Unsupported UVI generator");
    let n = |name: &str, default: f64| number(node, name, default);
    if node.kind == "FmOscillator" {
        fm_settings(&n)?;
        return Ok(());
    }
    let voices = if node.kind == "MinBlepGenerator" {
        "NumOscillators"
    } else {
        "NumOscs"
    };
    let count = n(voices, 1.)?;
    ensure!(
        count.fract() == 0. && (1. ..=8.).contains(&count),
        "Invalid oscillator unison count"
    );
    ensure!(
        (0. ..=1.).contains(&n("StartPhase", 0.)?),
        "Invalid oscillator start phase"
    );
    ensure!(
        [0., 1.].contains(&n("Stereo", 0.)?),
        "Invalid generator Stereo flag"
    );
    ensure!(
        [0., 1., 2.].contains(&n("PhaseSpread", 1.)?),
        "Invalid oscillator phase spread"
    );
    ensure!(
        (0. ..=1.).contains(&n(
            if node.kind == "MinBlepGenerator" {
                "MultiOscSpread"
            } else {
                "Spread"
            },
            0.1
        )?),
        "Invalid generator detune spread"
    );
    ensure!(
        [0., 1.].contains(&n("DetuneMode", 0.)?),
        "Invalid generator detune mode"
    );
    ensure!(
        (0. ..=1.).contains(&n("StereoSpread", 0.1)?),
        "Invalid generator stereo spread"
    );
    ensure!(
        [0., 1.].contains(&n(
            "StereoSpreadMode",
            if node.kind == "MinBlepGenerator" {
                0.
            } else {
                1.
            }
        )?),
        "Invalid generator stereo spread mode"
    );
    if node.kind == "MinBlepGenerator" {
        let wave = n("Waveform", 4.)?;
        ensure!(
            wave.fract() == 0. && (1. ..=5.).contains(&wave),
            "Unsupported Analog waveform (pulse-train law is unverified)"
        );
        ensure!(
            (0.01..=0.99).contains(&n("Pwm", 0.5)?),
            "Invalid Analog pulse width"
        );
        ensure!(
            [0., 1.].contains(&n("Polarity", 0.)?),
            "Invalid Analog polarity"
        );
        ensure!(
            [0., 1.].contains(&n("HardSync", 0.)?),
            "Invalid Analog hard sync"
        );
        ensure!(
            (0. ..=36.).contains(&n("HardSyncShift", 0.)?),
            "Invalid Analog hard-sync shift"
        );
    } else {
        ensure!(
            node.attributes
                .get("WavetablePath")
                .is_some_and(|s| !s.is_empty()),
            "A resolved external wavetable is required; factory tables are not bundled"
        );
        table_phase(
            0.,
            n("PhaseDistortionMode", 3.)?,
            n("PhaseDistortionAmount", 0.)?,
        )?;
        let fm = n("EnableFM", 0.)?;
        ensure!([0., 1.].contains(&fm), "Invalid wavetable FM flag");
        if fm != 0. {
            table_fm(&n)?;
        }
        ensure!(
            (0. ..=1.).contains(&n("WaveIndexSpread", 0.)?),
            "Invalid wavetable unison index spread"
        );
    }
    Ok(())
}

enum Source {
    Analog,
    Fm,
    Table { sample: Arc<Sample>, cycle: usize },
}

pub struct Generator {
    source: Source,
    rate: f64,
    phase: [f64; 8],
    master_phase: [f64; 8],
    oscillators: usize,
    noise: u32,
    stereo: bool,
    fm: bool,
    phase_spread: f64,
    fm_feedback: f64,
}

impl Generator {
    pub fn new(node: &ProgramNode, rate: f64, table: Option<Arc<Sample>>) -> Result<Self> {
        Self::new_seeded(node, rate, table, 0)
    }

    /// The renderer supplies a local launch identity, never a process-wide RNG.
    /// Native random phase varies per launch; its RNG sequence is unverified.
    pub fn new_seeded(
        node: &ProgramNode,
        rate: f64,
        table: Option<Arc<Sample>>,
        seed: u64,
    ) -> Result<Self> {
        validate(node)?;
        ensure!(
            rate.is_finite() && (8_000. ..=192_000.).contains(&rate),
            "Invalid oscillator sample rate"
        );
        if node.kind == "FmOscillator" {
            return Ok(Self {
                source: Source::Fm,
                rate,
                phase: [0.; 8],
                master_phase: [0.; 8],
                oscillators: 1,
                noise: 0,
                stereo: false,
                fm: false,
                phase_spread: 0.,
                fm_feedback: 0.,
            });
        }
        let source = if node.kind == "MinBlepGenerator" {
            Source::Analog
        } else {
            let table = table.context("Missing resolved external wavetable")?;
            ensure!(
                !table.wavetable_image
                    || (table.frames == IMAGE_FRAMES
                        && table.wavetable_cycle_frames == Some(IMAGE_FRAMES as u32)),
                "Invalid imported image-wavetable geometry"
            );
            ensure!(
                (1..=256).contains(&table.channels)
                    && table.frames >= 4
                    && table.frames <= 1 << 20
                    && table.interleaved.len() == table.frames * table.channels
                    && table.interleaved.len() <= 1 << 20
                    && table.interleaved.iter().all(|n| n.is_finite()),
                "Only bounded mono or channel-sliced external wavetables are implemented"
            );
            let filename_cycle = (!table.wavetable_image)
                .then(|| {
                    node.attributes
                        .get("WavetablePath")
                        .and_then(|p| p.rsplit(['/', '\\']).next())
                        .and_then(|p| p.rsplit_once('.').map(|(stem, _)| stem))
                        .and_then(|p| p.rsplit_once('_').map(|(_, size)| size))
                        .and_then(|size| size.parse::<usize>().ok())
                })
                .flatten();
            ensure!(
                filename_cycle.is_some()
                    || table
                        .wavetable_cycle_frames
                        .is_none_or(|hint| hint as usize == table.frames),
                "Ambiguous multi-cycle wavetable hint; native bank cycle geometry is unverified"
            );
            let cycle = filename_cycle.unwrap_or(table.frames);
            ensure!(
                cycle >= 4 && cycle <= table.frames && table.frames % cycle == 0,
                "Invalid external wavetable cycle geometry"
            );
            ensure!(
                table.channels == 1 || filename_cycle.is_none(),
                "Combining channel slices with filename cycle hints is unverified"
            );
            // Native external import recognizes a numeric filename suffix.
            // Retained RIFF clm metadata did not change authored native import;
            // bank-resource-specific clm handling remains unverified.
            Source::Table {
                sample: table,
                cycle,
            }
        };
        let oscillators = number(
            node,
            if node.kind == "MinBlepGenerator" {
                "NumOscillators"
            } else {
                "NumOscs"
            },
            1.,
        )? as usize;
        let start = number(node, "StartPhase", 0.)?;
        let phase_spread = number(node, "PhaseSpread", 1.)?;
        let mut noise = 0x91e1_0da5;
        if seed != 0 {
            let mut hash = DefaultHasher::new();
            seed.hash(&mut hash);
            noise = (hash.finish() as u32).max(1);
        }
        let phase = std::array::from_fn(|i| {
            (start
                + match phase_spread {
                    0. => i as f64 / (2 * oscillators) as f64,
                    1. => 2f64.powf(i as f64 / oscillators as f64) - 1.,
                    _ => f64::from(next_random(&mut noise)) / 4294967296.,
                })
            .rem_euclid(1.)
        });
        Ok(Self {
            source,
            rate,
            phase,
            master_phase: [0.; 8],
            oscillators,
            noise,
            stereo: number(node, "Stereo", 0.)? != 0.,
            fm: node.kind == "WaveTableOscillator" && number(node, "EnableFM", 0.)? != 0.,
            phase_spread,
            fm_feedback: 0.,
        })
    }

    pub fn channels(&self) -> usize {
        if self.stereo { 2 } else { 1 }
    }

    /// Frequency already includes note tracking and tuning; playback applies Gain.
    /// Parameter lookup must use the current voice's effective modulation values.
    pub fn next(
        &mut self,
        numeric: impl Fn(&str, f64) -> Result<f64>,
        frequency_hz: f64,
    ) -> Result<Frame> {
        ensure!(
            frequency_hz.is_finite() && frequency_hz >= 0.,
            "Invalid oscillator frequency"
        );
        if matches!(self.source, Source::Fm) {
            let settings = fm_settings(&numeric)?;
            let phase =
                std::array::from_fn(|i| TAU * (self.phase[i] + settings.phase[i] * settings.scale));
            let value = fm_value(phase, &settings, &mut self.fm_feedback);
            for i in 0..4 {
                self.phase[i] = (self.phase[i]
                    + frequency_hz / self.rate * settings.ratio[i] * settings.scale)
                    .rem_euclid(1.);
            }
            let mut frame = [0.; super::dsp::MAX_CHANNELS];
            frame[0] = value as f32;
            return Ok(frame);
        }
        let voices = if matches!(self.source, Source::Analog) {
            "NumOscillators"
        } else {
            "NumOscs"
        };
        ensure!(
            numeric(voices, 1.)? == self.oscillators as f64,
            "Generator unison count changed; note retrigger required"
        );
        ensure!(
            numeric("Stereo", 0.)? == f64::from(u8::from(self.stereo)),
            "Generator Stereo changes bus width; rebuild graph required"
        );
        ensure!(
            numeric("PhaseSpread", 1.)? == self.phase_spread,
            "Generator phase spread changed; note retrigger required"
        );
        let spread = numeric(
            if matches!(self.source, Source::Analog) {
                "MultiOscSpread"
            } else {
                "Spread"
            },
            0.1,
        )?;
        let detune_mode = numeric("DetuneMode", 0.)?;
        let stereo_spread = numeric("StereoSpread", 0.1)?;
        let stereo_mode = numeric(
            "StereoSpreadMode",
            if matches!(self.source, Source::Analog) {
                0.
            } else {
                1.
            },
        )?;
        ensure!(
            (0. ..=1.).contains(&spread) && [0., 1.].contains(&detune_mode),
            "Invalid generator detune"
        );
        ensure!(
            (0. ..=1.).contains(&stereo_spread) && [0., 1.].contains(&stereo_mode),
            "Invalid generator stereo spread"
        );
        let mut frame = [0.; super::dsp::MAX_CHANNELS];
        for oscillator in 0..self.oscillators {
            let position = unison_position(oscillator, self.oscillators);
            let detune = if detune_mode == 0. {
                50. * spread * position
            } else {
                100. * spread.powi(3) * position * (self.oscillators / 2) as f64
            };
            let step = frequency_hz * 2f64.powf(detune / 1200.) / self.rate;
            let value = match &self.source {
                Source::Fm => unreachable!("FM is rendered before unison"),
                Source::Table {
                    sample: table,
                    cycle,
                } => {
                    let mode = numeric("PhaseDistortionMode", 3.)?;
                    let amount = numeric("PhaseDistortionAmount", 0.)?;
                    ensure!(
                        numeric("EnableFM", 0.)? == f64::from(u8::from(self.fm)),
                        "Wavetable FM enable changed; note retrigger required"
                    );
                    let phase = if self.fm {
                        let (depth, ratio) = table_fm(&numeric)?;
                        let phase = (self.phase[oscillator]
                            + depth * 0.5 * (TAU * self.master_phase[oscillator]).sin())
                        .rem_euclid(1.);
                        self.master_phase[oscillator] =
                            (self.master_phase[oscillator] + step * ratio).rem_euclid(1.);
                        phase
                    } else {
                        self.phase[oscillator]
                    };
                    let phase = table_phase(phase, mode, amount)?;
                    ensure!(
                        (0. ..=1.).contains(&numeric("WaveIndexSpread", 0.)?),
                        "Invalid wavetable unison index spread"
                    );
                    let index = numeric("WaveIndex", 0.)?;
                    let fade = numeric("FadeWaveIndex", 1.)?;
                    ensure!(
                        (0. ..=1.).contains(&index) && [0., 1.].contains(&fade),
                        "Invalid wavetable wave index"
                    );
                    // Native table spread moves successive oscillators toward
                    // higher indices and clamps there, unlike centered detune.
                    let index = (index
                        + numeric("WaveIndexSpread", 0.)? * oscillator as f64
                            / self.oscillators as f64)
                        .min(1.);
                    let slices = if table.channels == 1 {
                        table.frames / cycle
                    } else {
                        table.channels
                    };
                    let index = index * (slices - 1) as f64;
                    let low = if fade == 0. {
                        index.round() as usize
                    } else {
                        index as usize
                    };
                    let high = (low + 1).min(slices - 1);
                    let read = |slice: usize| {
                        let at = phase * *cycle as f64;
                        let lo = at as usize % cycle;
                        let hi = (lo + 1) % cycle;
                        let address = |frame| {
                            if table.channels == 1 {
                                slice * cycle + frame
                            } else {
                                frame * table.channels + slice
                            }
                        };
                        let a = table
                            .interleaved
                            .value(address(lo))
                            .expect("validated wavetable bounds");
                        let b = table
                            .interleaved
                            .value(address(hi))
                            .expect("validated wavetable bounds");
                        f64::from(a) + f64::from(b - a) * at.fract()
                    };
                    let a = read(low);
                    if fade != 0. {
                        a + (read(high) - a) * index.fract()
                    } else {
                        a
                    }
                }
                Source::Analog => {
                    let wave = numeric("Waveform", 4.)?;
                    ensure!(
                        wave.fract() == 0. && (1. ..=5.).contains(&wave),
                        "Unsupported Analog waveform"
                    );
                    let sync = numeric("HardSync", 0.)?;
                    ensure!([0., 1.].contains(&sync), "Invalid Analog hard sync");
                    let shift = numeric("HardSyncShift", 0.)?;
                    ensure!(
                        (0. ..=36.).contains(&shift),
                        "Invalid Analog hard-sync shift"
                    );
                    let ratio = if sync != 0. {
                        2f64.powf(shift / 12.)
                    } else {
                        1.
                    };
                    let increment = step * ratio;
                    let edge_step = increment.min(0.5);
                    let pwm = numeric("Pwm", 0.5)?;
                    ensure!((0.01..=0.99).contains(&pwm), "Invalid Analog pulse width");
                    let polarity = numeric("Polarity", 0.)?;
                    ensure!([0., 1.].contains(&polarity), "Invalid Analog polarity");
                    let value = match wave as u8 {
                        1 => {
                            let p = self.phase[oscillator];
                            (if p < 1. - pwm {
                                p / (2. * (1. - pwm))
                            } else {
                                (p - 1.) / (2. * pwm)
                            }) - 0.5 * blep((p + pwm).rem_euclid(1.), edge_step)
                        }
                        2 => {
                            let p = self.phase[oscillator];
                            (if p < pwm { 0.5 } else { -0.5 }) + 0.5 * blep(p, edge_step)
                                - 0.5 * blep((p - pwm).rem_euclid(1.), edge_step)
                        }
                        3 => {
                            let p = self.phase[oscillator];
                            if p < pwm * 0.5 {
                                p / pwm
                            } else if p < 1. - pwm * 0.5 {
                                0.5 - (p - pwm * 0.5) / (1. - pwm)
                            } else {
                                (p - 1.) / pwm
                            }
                        }
                        4 => -0.5 * (TAU * sine_phase(self.phase[oscillator], pwm)).sin(),
                        _ => f64::from(next_random(&mut self.noise)) / f64::from(u32::MAX) - 0.5,
                    };
                    self.phase[oscillator] = (self.phase[oscillator] + increment).rem_euclid(1.);
                    self.master_phase[oscillator] += step;
                    if self.master_phase[oscillator] >= 1. {
                        self.master_phase[oscillator] =
                            self.master_phase[oscillator].rem_euclid(1.);
                        if sync != 0. {
                            // ponytail: direct phase reset; add a measured sync-edge
                            // BLEP when native hard-sync residual comparisons exist.
                            self.phase[oscillator] =
                                (self.master_phase[oscillator] * ratio).rem_euclid(1.);
                        }
                    }
                    if polarity != 0. { -value } else { value }
                }
            };
            if matches!(self.source, Source::Table { .. }) {
                self.phase[oscillator] = (self.phase[oscillator] + step).rem_euclid(1.);
            }
            let value = value / (self.oscillators as f64).sqrt();
            if self.stereo {
                let pan = unison_pan(oscillator, self.oscillators, stereo_mode);
                let angle = (pan * stereo_spread + 1.) * TAU / 8.;
                frame[0] += (value * angle.cos().powi(2)) as f32;
                frame[1] += (value * angle.sin().powi(2)) as f32;
            } else {
                frame[0] += value as f32;
            }
        }
        Ok(frame)
    }
}

struct FmSettings {
    level: [f64; 4],
    ratio: [f64; 4],
    phase: [f64; 4],
    topology: u8,
    scale: f64,
    feedback: f64,
}

fn fm_settings(n: &impl Fn(&str, f64) -> Result<f64>) -> Result<FmSettings> {
    let feedback = n("Feedback", 0.)?;
    ensure!((0. ..=1.).contains(&feedback), "Invalid FM feedback");
    let topology = n("Topology", 0.)?;
    ensure!(
        [0., 5., 6., 7., 10.].contains(&topology),
        "Unsupported FM operator topology"
    );
    let version = n("FmOscillatorVersion", 0.)?;
    ensure!(
        [0., 1.].contains(&version),
        "Unsupported FM oscillator version"
    );
    let mut level = [0.; 4];
    let mut ratio = [0.; 4];
    let mut phase = [0.; 4];
    for (i, names) in [
        [
            "LevelA",
            "RatioA",
            "RatioFineA",
            "SnapRatioA",
            "PhaseA",
            "FixedFreqA",
        ],
        [
            "LevelB",
            "RatioB",
            "RatioFineB",
            "SnapRatioB",
            "PhaseB",
            "FixedFreqB",
        ],
        [
            "LevelC",
            "RatioC",
            "RatioFineC",
            "SnapRatioC",
            "PhaseC",
            "FixedFreqC",
        ],
        [
            "LevelD",
            "RatioD",
            "RatioFineD",
            "SnapRatioD",
            "PhaseD",
            "FixedFreqD",
        ],
    ]
    .iter()
    .enumerate()
    {
        level[i] = n(names[0], 1.)?;
        let coarse = n(names[1], 1.)?;
        let fine = n(names[2], 0.)?;
        let snap = n(names[3], 0.)?;
        phase[i] = n(names[4], 0.)?;
        // Native tracked captures are unchanged by Freq/FreqMultiplier fields.
        // Their fixed-frequency consumer remains deliberately unsupported.
        ensure!(
            n(names[5], 0.)? == 0.,
            "Fixed-frequency FM operators are not implemented"
        );
        ensure!((0. ..=20.).contains(&level[i]), "Invalid FM operator level");
        ensure!(
            (1. ..=40.).contains(&coarse)
                && (-1200. ..=1200.).contains(&fine)
                && [0., 1.].contains(&snap),
            "Invalid FM operator ratio"
        );
        ensure!((0. ..=1.).contains(&phase[i]), "Invalid FM operator phase");
        ratio[i] = if snap == 0. {
            coarse
        } else {
            (coarse + 0.5).floor()
        } * 2f64.powf(fine / 1200.);
    }
    Ok(FmSettings {
        level,
        ratio,
        phase,
        topology: topology as u8,
        scale: if version == 0. { 0.5 } else { 1. },
        feedback,
    })
}

/// Native authored operator isolation plus independent phase/fine/ratio fixtures.
/// Phase modulation uses the modulator's radian-valued output, unlike WT FM.
fn fm_value(phase: [f64; 4], settings: &FmSettings, previous_d: &mut f64) -> f64 {
    let level = settings.level;
    // Native feedback uses the preceding unscaled D sine, independent of LevelD.
    *previous_d = (phase[3] + settings.feedback * *previous_d).sin();
    let raw = [
        level[0] * phase[0].sin(),
        level[1] * phase[1].sin(),
        level[2] * phase[2].sin(),
        level[3] * *previous_d,
    ];
    match settings.topology {
        0 | 5 => {
            let c = level[2] * (phase[2] + raw[3]).sin();
            let b = level[1] * (phase[1] + c).sin();
            if settings.topology == 0 {
                level[0] * (phase[0] + b).sin()
            } else {
                raw[0] + b
            }
        }
        6 => level[0] * (phase[0] + raw[1] + raw[2] + raw[3]).sin(),
        7 => level[0] * (phase[0] + raw[1]).sin() + level[2] * (phase[2] + raw[3]).sin(),
        10 => raw.iter().sum(),
        _ => unreachable!("validated FM topology"),
    }
}

fn next_random(state: &mut u32) -> u32 {
    *state ^= *state << 13;
    *state ^= *state >> 17;
    *state ^= *state << 5;
    *state
}

fn blep(phase: f64, step: f64) -> f64 {
    if step == 0. {
        return 0.;
    }
    if phase < step {
        let t = phase / step;
        2. * t - t * t - 1.
    } else if phase > 1. - step {
        let t = (phase - 1.) / step;
        t * t + 2. * t + 1.
    } else {
        0.
    }
}

/// Odd counts start with the center oscillator, then negative/positive pairs;
/// even counts contain pairs only. Pair magnitudes increase toward both edges.
fn unison_position(index: usize, count: usize) -> f64 {
    if count == 1 || (count % 2 != 0 && index == 0) {
        return 0.;
    }
    let pair = if count % 2 == 0 {
        index / 2 + 1
    } else {
        index.div_ceil(2)
    };
    let negative = if count % 2 == 0 {
        index % 2 == 0
    } else {
        index % 2 != 0
    };
    pair as f64 / (count / 2) as f64 * if negative { -1. } else { 1. }
}

/// Alternate stereo uses the trailing terms of the eight-term Thue-Morse
/// sequence; odd counts retain a centered first oscillator. Native authored
/// sine fixtures distinguish this from simply alternating left/right pairs.
fn unison_pan(index: usize, count: usize, mode: f64) -> f64 {
    if mode == 0. || count == 1 || (count % 2 != 0 && index == 0) {
        unison_position(index, count)
    } else if (8 - count + index).count_ones() % 2 == 0 {
        -1.
    } else {
        1.
    }
}

/// Native PWM keeps sine zero crossings fixed while moving its two peaks.
/// The same three segments give the Analog triangle's asymmetric slopes.
fn sine_phase(phase: f64, pwm: f64) -> f64 {
    if phase < pwm * 0.5 {
        phase / (2. * pwm)
    } else if phase < 1. - pwm * 0.5 {
        0.25 + (phase - pwm * 0.5) / (2. * (1. - pwm))
    } else {
        0.75 + (phase - 1. + pwm * 0.5) / (2. * pwm)
    }
}

/// Authored native ramp and sine tables distinguish the two segment bend (0)
/// from the symmetric peak stretch (3). Mode 1 has only been verified at zero.
fn table_phase(phase: f64, mode: f64, amount: f64) -> Result<f64> {
    ensure!(
        (0. ..=1.).contains(&amount),
        "Invalid wavetable phase distortion amount"
    );
    match mode {
        0. => Ok(if phase < amount {
            phase / (2. * amount)
        } else {
            0.5 + (phase - amount) / (2. * (1. - amount))
        }),
        3. => Ok(sine_phase(phase, (1. + amount) * 0.5)),
        1. if amount == 0. => Ok(phase),
        _ => anyhow::bail!("Unsupported wavetable phase distortion mode/amount"),
    }
}

/// Key-tracked FM: continuous ratios (0), harmonic/reciprocal-harmonic rounding
/// (1), or chromatic semitone rounding (2), followed by independent fine cents.
fn table_fm(n: &impl Fn(&str, f64) -> Result<f64>) -> Result<(f64, f64)> {
    ensure!(
        n("FMFixedFreq", 0.)? == 0.,
        "Fixed-frequency wavetable FM is not implemented"
    );
    let depth = n("FMDepth", 0.1)?;
    let coarse = n("FMRatio", 0.)?;
    let fine = n("FMRatioFine", 0.)?;
    let mode = n("FMRatioMode", 1.)?;
    ensure!((0. ..=1.).contains(&depth), "Invalid wavetable FM depth");
    ensure!(
        (-48. ..=48.).contains(&coarse) && (-1200. ..=1200.).contains(&fine),
        "Invalid wavetable FM ratio"
    );
    let ratio = match mode {
        0. => 2f64.powf(coarse / 12.),
        1. => {
            let harmonic = 2f64.powf(coarse.abs() / 12.).round();
            if coarse < 0. {
                harmonic.recip()
            } else {
                harmonic
            }
        }
        // Native half-semitone ties round upward, including negative values.
        2. => 2f64.powf((coarse + 0.5).floor() / 12.),
        _ => anyhow::bail!("Unsupported wavetable FM ratio mode"),
    };
    Ok((depth, ratio * 2f64.powf(fine / 1200.)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::uvi::program::parse_program;

    #[test]
    fn authored_native_image_conversion_and_bounds() {
        // Independently authored gray pairs were measured at exact StartPhase
        // positions in official Workstation4.0.9. Values below are imported
        // bytes inferred from constant black/white calibration rows.
        let pairs = [
            (128, 134),
            (140, 146),
            (146, 152),
            (0, 255),
            (255, 0),
            (128, 0),
            (0, 128),
            (128, 255),
            (255, 128),
            (0, 6),
            (6, 12),
            (12, 18),
            (0, 0),
            (255, 255),
        ];
        let pixels: Vec<u8> = pairs
            .iter()
            .flat_map(|&(a, b)| [a].into_iter().chain(std::iter::repeat_n(b, 127)))
            .collect();
        let table = image_pixels(128, pairs.len(), 1, &pixels).unwrap();
        assert!(table.wavetable_image);
        assert_eq!(
            (table.frames, table.channels, table.wavetable_cycle_frames),
            (2048, 14, Some(2048))
        );
        let at = |row: usize, x: usize| table.interleaved.value(x * 14 + 13 - row).unwrap() as f64;
        let black = at(12, 0);
        let white = at(13, 0);
        let expected = [
            [128, 128, 128, 129, 130, 131, 134],
            [140, 140, 140, 141, 142, 144, 146],
            [146, 146, 147, 147, 148, 150, 152],
            [0, 0, 24, 40, 72, 151, 255],
            [255, 255, 231, 215, 183, 104, 0],
            [128, 128, 116, 108, 92, 52, 0],
            [0, 0, 12, 20, 36, 76, 128],
            [128, 128, 140, 148, 164, 203, 255],
            [255, 255, 243, 235, 219, 180, 128],
            [0, 0, 1, 1, 2, 4, 6],
            [6, 6, 7, 7, 8, 10, 12],
            [12, 12, 13, 13, 14, 16, 18],
        ];
        for (row, values) in expected.iter().enumerate() {
            for (x, pixel) in [0, 1, 9, 10, 12, 17, 25].into_iter().zip(values) {
                assert_eq!(
                    ((at(row, x) - black) / (white - black) * 255.).round() as u8,
                    *pixel
                );
            }
        }
        let rgb = [[255, 255, 255], [255, 255, 0], [255, 0, 0], [128, 128, 128]];
        let rgba: Vec<u8> = rgb
            .iter()
            .flat_map(|c| c.iter().copied().chain([255]))
            .collect();
        let colors = image_pixels(1, 4, 4, &rgba).unwrap();
        assert_eq!(colors.interleaved.to_vec().unwrap()[..4], [-3., 1., 1., 1.]);
        let program = parse_program(r#"<Program><Layers><Layer><Keygroups><Keygroup><Oscillators><WaveTableOscillator WavetablePath="authored_128.wav"/></Oscillators></Keygroup></Keygroups></Layer></Layers></Program>"#).unwrap();
        let node = program
            .nodes
            .iter()
            .find(|n| n.kind == "WaveTableOscillator")
            .unwrap();
        // Signature-imported image metadata defeats an unrelated filename hint.
        let mut generator = Generator::new(node, 48000., Some(Arc::new(colors))).unwrap();
        assert_eq!(generator.channels(), 1);
        assert_eq!(
            generator
                .next(|n, d| number(node, n, d), 261.625565)
                .unwrap()[..2],
            [-3., 0.]
        );
        assert!(image_pixels(0, 1, 3, &[]).is_err());
        assert!(image_pixels(2049, 1, 3, &[]).is_err());
        // Larger images resize to 128 slices rather than cropping; exact
        // vertical interpolation remains unverified.
        assert!(image_pixels(1, 129, 3, &vec![0; 129 * 3]).is_err());
        assert!(image_pixels(1, 257, 3, &[]).is_err());
        assert!(image_pixels(1, 1, 4, &[0, 0, 0, 128]).is_err());
        assert!(image_pixels(1, 1, 3, &[0, 0]).is_err());
        assert!(image_signature(b"\x89PNG\r\n\x1a\n"));
        assert!(image_signature(&[255, 216]));
        assert!(!image_signature(b"RIFF"));
        assert_eq!(
            image_wavetable(&[255, 216]).unwrap_err().to_string(),
            "JPEG image-wavetable decoding is not implemented with verified native fidelity"
        );
        assert!(image_wavetable(b"RIFF").is_err());
        assert!(image_wavetable(&vec![0; IMAGE_BYTES_LIMIT + 1]).is_err());
        for (width, height, bit_depth, color, data, accepted) in [
            (
                128,
                14,
                png::BitDepth::Eight,
                png::ColorType::Grayscale,
                pixels,
                true,
            ),
            (1, 4, png::BitDepth::Eight, png::ColorType::Rgba, rgba, true),
            (
                1,
                128,
                png::BitDepth::Eight,
                png::ColorType::Grayscale,
                vec![0; 128],
                true,
            ),
            (
                1,
                129,
                png::BitDepth::Eight,
                png::ColorType::Grayscale,
                vec![0; 129],
                false,
            ),
            (
                1,
                1,
                png::BitDepth::Eight,
                png::ColorType::Rgba,
                vec![0, 0, 0, 128],
                false,
            ),
            (
                2049,
                1,
                png::BitDepth::Eight,
                png::ColorType::Grayscale,
                vec![0; 2049],
                false,
            ),
            (
                1,
                257,
                png::BitDepth::Eight,
                png::ColorType::Grayscale,
                vec![0; 257],
                false,
            ),
            (
                1,
                1,
                png::BitDepth::Sixteen,
                png::ColorType::Grayscale,
                vec![0; 2],
                false,
            ),
        ] {
            let mut encoded = Vec::new();
            let mut encoder = png::Encoder::new(&mut encoded, width, height);
            encoder.set_color(color);
            encoder.set_depth(bit_depth);
            encoder
                .write_header()
                .unwrap()
                .write_image_data(&data)
                .unwrap();
            let decoded = image_wavetable(&encoded);
            if accepted {
                let decoded = decoded.unwrap();
                let original = image_pixels(
                    width as usize,
                    height as usize,
                    if color == png::ColorType::Rgba { 4 } else { 1 },
                    &data,
                )
                .unwrap();
                assert_eq!(
                    decoded.interleaved.to_vec().unwrap(),
                    original.interleaved.to_vec().unwrap()
                );
            } else {
                assert!(decoded.is_err());
            }
        }
    }

    #[test]
    fn image_resampling_boundary_is_explicit_before_pixel_decode() {
        for (width, height, expected) in [
            (2048, 129, "128 rows"),
            (2048, 256, "128 rows"),
            (2049, 128, "2048 columns"),
        ] {
            let mut encoded = Vec::new();
            // The valid stream contains no pixels: geometry must fail before
            // attempting to decode those missing pixels or allocate storage.
            let mut writer = png::Encoder::new(&mut encoded, width, height)
                .write_header()
                .unwrap();
            writer
                .write_chunk(png::chunk::IDAT, &[0x78, 0x9c, 3, 0, 0, 0, 0, 1])
                .unwrap();
            writer.finish().unwrap();
            let error = image_wavetable(&encoded).unwrap_err().to_string();
            assert!(error.contains(expected), "{error}");
            assert!(error.contains("fidelity is unverified"), "{error}");
        }
        assert!(image_dimensions(2048, 128).is_ok());
        assert!(image_dimensions(2048, 0).is_err());
        assert!(image_dimensions(0, 128).is_err());
    }

    #[test]
    fn authored_native_image_normalization_order() {
        // Individually authored grayscale row barcodes, captured in official
        // Workstation 4.0.9 with one oscillator and FadeWaveIndex=0. The mono
        // keygroup center contributes a factor of 0.5 to these native samples.
        let indices = [0f64, 0.1, 0.25, 0.5, 0.75, 0.9, 0.99, 1.];
        for (height, stride, offset, native) in [
            (
                128usize,
                17usize,
                11usize,
                [
                    -0.0024106507f32,
                    0.20330076,
                    -0.12900233,
                    -0.25559399,
                    -0.38218564,
                    0.36549637,
                    -0.37427366,
                    -0.44152549,
                ],
            ),
            (
                96,
                47,
                23,
                [
                    0.04353172,
                    0.39375308,
                    -0.36571562,
                    0.23241511,
                    -0.17683224,
                    0.25602555,
                    -0.40113127,
                    -0.40113127,
                ],
            ),
        ] {
            let pixels: Vec<u8> = (0..height)
                .map(|row| ((row * stride + offset) % 256) as u8)
                .collect();
            let table = image_pixels(1, height, 1, &pixels).unwrap();
            for (index, expected) in indices.into_iter().zip(native) {
                let row = ((index * height as f64) as usize).min(height - 1);
                let actual = table.interleaved.value(row).unwrap() * 0.5;
                assert!(
                    (actual - expected).abs() < 1e-7,
                    "{height} rows, index {index}"
                );
                assert_eq!(
                    table.interleaved.value(row),
                    table.interleaved.value(2047 * height + row)
                );
            }
            let flat = image_pixels(1, height, 1, &vec![13; height]).unwrap();
            assert!(flat.interleaved.iter().all(|value| value == 0.));
        }
    }

    #[test]
    fn authored_native_external_table_slices() {
        let p = parse_program(r#"<Program><Layers><Layer><Keygroups><Keygroup><Oscillators><WaveTableOscillator WavetablePath="authored_128.wav" PhaseDistortionMode="3" PhaseDistortionAmount="0"/></Oscillators></Keygroup></Keygroups></Layer></Layers></Program>"#).unwrap();
        let node = p
            .nodes
            .iter()
            .find(|n| n.kind == "WaveTableOscillator")
            .unwrap();
        let table = Arc::new(Sample {
            rate: 44_100,
            channels: 1,
            frames: 256,
            interleaved: super::super::storage::Storage::from_f32(
                [vec![0.125; 128], vec![0.375; 128]].concat(),
            )
            .unwrap(),
            loops: Vec::new(),
            unity_note: None,
            riff_metadata: Vec::new(),
            wavetable_cycle_frames: Some(128),
            wavetable_image: false,
        });
        let plain = parse_program(r#"<Program><Layers><Layer><Keygroups><Keygroup><Oscillators><WaveTableOscillator WavetablePath="authored.wav"/></Oscillators></Keygroup></Keygroups></Layer></Layers></Program>"#).unwrap();
        let plain = plain
            .nodes
            .iter()
            .find(|n| n.kind == "WaveTableOscillator")
            .unwrap();
        assert!(Generator::new(plain, 48_000., Some(table.clone())).is_err());
        let mut g = Generator::new(node, 48_000., Some(table)).unwrap();
        // Native independently authored two-cycle constant WAVs establish both
        // the linear slice morph and nonsmoothed nearest-slice boundary at .5.
        for (index, fade, native) in [
            (0., 1., 0.0625),
            (0.5, 1., 0.125),
            (1., 1., 0.1875),
            (0.49, 0., 0.0625),
            (0.5, 0., 0.1875),
        ] {
            let params = |name: &str, default| match name {
                "WaveIndex" => Ok(index),
                "FadeWaveIndex" => Ok(fade),
                _ => number(node, name, default),
            };
            assert_eq!(g.next(params, 261.6255653005986).unwrap()[0] * 0.5, native);
        }
        assert!(
            g.next(
                |name, default| if name == "WaveIndex" {
                    Ok(1.1)
                } else {
                    number(node, name, default)
                },
                440.
            )
            .is_err()
        );
    }

    #[test]
    fn authored_native_fixed_phase_unison() {
        for (count, native) in [
            (2, [-0.17677669, -0.18272589, -0.18846081]),
            (3, [-0.24999997, -0.25479552, -0.25929227]),
            (4, [-0.30177668, -0.30587977, -0.30962408]),
            (8, [-0.44435823, -0.44712409, -0.44936562]),
        ] {
            let xml = format!(
                r#"<Program><Layers><Layer><Keygroups><Keygroup><Oscillators><MinBlepGenerator Waveform="4" NumOscillators="{count}" MultiOscSpread="0" PhaseSpread="0"/></Oscillators></Keygroup></Keygroups></Layer></Layers></Program>"#
            );
            let p = parse_program(&xml).unwrap();
            let node = p
                .nodes
                .iter()
                .find(|n| n.kind == "MinBlepGenerator")
                .unwrap();
            let mut g = Generator::new(node, 48_000., None).unwrap();
            for reference in native {
                let actual = g
                    .next(
                        |name, default| number(node, name, default),
                        261.6255653005986,
                    )
                    .unwrap()[0]
                    * 0.5;
                assert!((actual - reference).abs() < 0.000002);
            }
        }
        for (count, spread, native) in [
            (2, 0.5, [-0.05177670, -0.30177670]),
            (3, 1., [-0.25000003, -0.24999993]),
            (4, 1., [-0.27588835, -0.32766503]),
        ] {
            let xml = format!(
                r#"<Program><Layers><Layer><Keygroups><Keygroup><Oscillators><MinBlepGenerator Waveform="4" NumOscillators="{count}" MultiOscSpread="1" DetuneMode="0" PhaseSpread="0" Stereo="1" StereoSpread="{spread}" StereoSpreadMode="0"/></Oscillators></Keygroup></Keygroups></Layer></Layers></Program>"#
            );
            let p = parse_program(&xml).unwrap();
            let node = p
                .nodes
                .iter()
                .find(|n| n.kind == "MinBlepGenerator")
                .unwrap();
            let mut g = Generator::new(node, 48_000., None).unwrap();
            let actual = g
                .next(
                    |name, default| number(node, name, default),
                    261.6255653005986,
                )
                .unwrap();
            assert_eq!(g.channels(), 2);
            for channel in 0..2 {
                assert!((actual[channel] - native[channel]).abs() < 0.000002);
            }
        }
        let p = parse_program(r#"<Program><Layers><Layer><Keygroups><Keygroup><Oscillators><MinBlepGenerator Waveform="4" NumOscillators="2" MultiOscSpread="0.5" DetuneMode="1" PhaseSpread="0" Stereo="1" StereoSpread="1"/></Oscillators></Keygroup></Keygroups></Layer></Layers></Program>"#).unwrap();
        let node = p
            .nodes
            .iter()
            .find(|n| n.kind == "MinBlepGenerator")
            .unwrap();
        let mut g = Generator::new(node, 48_000., None).unwrap();
        let params = |name: &str, default| number(node, name, default);
        g.next(params, 261.6255653005986).unwrap();
        assert!((g.next(params, 261.6255653005986).unwrap()[0] + 0.012018654).abs() < 0.000002);
        let p = parse_program(r#"<Program><Layers><Layer><Keygroups><Keygroup><Oscillators><MinBlepGenerator Waveform="4" NumOscillators="4" MultiOscSpread="1" DetuneMode="1" PhaseSpread="0" Stereo="1" StereoSpread="1"/></Oscillators></Keygroup></Keygroups></Layer></Layers></Program>"#).unwrap();
        let node = p
            .nodes
            .iter()
            .find(|n| n.kind == "MinBlepGenerator")
            .unwrap();
        let mut g = Generator::new(node, 48_000., None).unwrap();
        let params = |name: &str, default| number(node, name, default);
        g.next(params, 261.6255653005986).unwrap();
        let actual = g.next(params, 261.6255653005986).unwrap();
        for (actual, native) in actual[..2].iter().zip([-0.28359058, -0.32729816]) {
            assert!((actual - native).abs() < 0.000002);
        }
    }

    #[test]
    fn authored_native_sine_observations_and_phase_lifetime() {
        let p = parse_program(r#"<Program><Layers><Layer><Keygroups><Keygroup><Oscillators><MinBlepGenerator Waveform="4" PhaseSpread="0" StartPhase="0"/></Oscillators></Keygroup></Keygroups></Layer></Layers></Program>"#).unwrap();
        let node = p
            .nodes
            .iter()
            .find(|n| n.kind == "MinBlepGenerator")
            .unwrap();
        let mut g = Generator::new(node, 48_000., None).unwrap();
        // Original authored Workstation 4.0.9 note60/Gain1 capture. Native mono
        // KG center contributes .5; omit its two leading callback latency frames.
        let native = [
            -0.000000010927847,
            -0.008560038,
            -0.017110027,
            -0.025639949,
            -0.034139805,
        ];
        let params = |name: &str, default| number(node, name, default);
        for reference in native {
            let actual = g.next(params, 261.6255653005986).unwrap()[0] * 0.5;
            assert!((actual - reference).abs() < 0.000002);
        }
        let before = g.phase[0];
        g.next(params, 523.2511306011972).unwrap();
        assert!((g.phase[0] - before - 523.2511306011972 / 48_000.).abs() < 1e-12);
        let mut fresh = Generator::new(node, 48_000., None).unwrap();
        assert!(fresh.next(params, 261.6255653005986).unwrap()[0].abs() < 1e-7);
        assert_eq!(g.channels(), 1);
        assert!(Generator::new(node, f64::NAN, None).is_err());
        assert!(g.next(params, f64::INFINITY).is_err());

        for (wave, native) in [(1., 0.0027252734), (3., 0.0054505467)] {
            let mut g = Generator::new(node, 48_000., None).unwrap();
            let params = |name: &str, default| {
                if name == "Waveform" {
                    Ok(wave)
                } else {
                    number(node, name, default)
                }
            };
            g.next(params, 261.6255653005986).unwrap();
            assert!(
                (g.next(params, 261.6255653005986).unwrap()[0] * 0.5 - native).abs() < 0.000002
            );
        }
        for (wave, native) in [(1., 0.0018168489), (3., 0.010901064), (4., -0.017109968)] {
            let mut g = Generator::new(node, 48_000., None).unwrap();
            let params = |name: &str, default| match name {
                "Waveform" => Ok(wave),
                "Pwm" => Ok(0.25),
                _ => number(node, name, default),
            };
            g.next(params, 261.6255653005986).unwrap();
            assert!(
                (g.next(params, 261.6255653005986).unwrap()[0] * 0.5 - native).abs() < 0.000002
            );
        }
        for (wave, expected) in [
            (3., [0.23159003, 0.12257910, 0.01356816, -0.09544277]),
            (4., [-0.24832933, -0.17406739, -0.02128702, 0.14109556]),
        ] {
            let mut g = Generator::new(node, 48_000., None).unwrap();
            let params = |name: &str, default| match name {
                "Waveform" => Ok(wave),
                "Pwm" => Ok(0.25),
                _ => number(node, name, default),
            };
            for i in 0..=118 {
                let actual = g.next(params, 261.6255653005986).unwrap()[0] * 0.5;
                if let Some(at) = [28, 58, 88, 118].iter().position(|&n| n == i) {
                    assert!((actual - expected[at]).abs() < 0.000005);
                }
            }
        }
        assert!(
            g.next(
                |name, default| if name == "NumOscillators" {
                    Ok(2.)
                } else {
                    Ok(default)
                },
                440.
            )
            .is_err()
        );

        let p = parse_program(r#"<Program><Layers><Layer><Keygroups><Keygroup><Oscillators><WaveTableOscillator WavetablePath="authored_128.wav" PhaseDistortionMode="3" PhaseDistortionAmount="0"/></Oscillators></Keygroup></Keygroups></Layer></Layers></Program>"#).unwrap();
        let node = p
            .nodes
            .iter()
            .find(|n| n.kind == "WaveTableOscillator")
            .unwrap();
        let table = Arc::new(Sample {
            rate: 44_100,
            channels: 1,
            frames: 128,
            interleaved: super::super::storage::Storage::from_f32(
                (0..128)
                    .map(|i| (16384. * (TAU * i as f64 / 128.).sin()).round() as f32 / 32768.)
                    .collect(),
            )
            .unwrap(),
            loops: Vec::new(),
            unity_note: None,
            riff_metadata: Vec::new(),
            wavetable_cycle_frames: None,
            wavetable_image: false,
        });
        let mut g = Generator::new(node, 48_000., Some(table)).unwrap();
        // Native authored PCM16 external sine table at neutral phase mode3.
        // A bounded tolerance exposes the stated linear-vs-native readout gap.
        for native in [0., 0.008561726, 0.017110242, 0.025641577, 0.034140345] {
            let actual = g
                .next(
                    |name, default| number(node, name, default),
                    261.6255653005986,
                )
                .unwrap()[0]
                * 0.5;
            assert!((actual - native).abs() < 0.00004);
        }
        for _ in 0..48_000 {
            assert!(
                g.next(
                    |name, default| number(node, name, default),
                    261.6255653005986
                )
                .unwrap()[0]
                    .is_finite()
            );
        }
        assert!(Generator::new(node, 48_000., None).is_err());
        let constant = Arc::new(Sample {
            rate: 44_100,
            channels: 1,
            frames: 128,
            interleaved: super::super::storage::Storage::from_f32(vec![0.125; 128]).unwrap(),
            loops: Vec::new(),
            unity_note: None,
            riff_metadata: Vec::new(),
            wavetable_cycle_frames: None,
            wavetable_image: false,
        });
        let mut g = Generator::new(node, 48_000., Some(constant)).unwrap();
        // A non-sine authored table distinguishes native resource loading from
        // the oscillator's default sine fallback: native L/R are exactly .0625.
        for _ in 0..200 {
            assert_eq!(
                g.next(
                    |name, default| number(node, name, default),
                    261.6255653005986
                )
                .unwrap()[0]
                    * 0.5,
                0.0625
            );
        }
    }
    #[test]
    fn authored_native_wavetable_phase_distortion_and_fm() {
        let table = Arc::new(Sample {
            rate: 44_100,
            channels: 1,
            frames: 2048,
            interleaved: super::super::storage::Storage::from_f32(
                (0..2048)
                    .map(|i| (16384. * (TAU * i as f64 / 2048.).sin()).round() as f32 / 32768.)
                    .collect(),
            )
            .unwrap(),
            loops: Vec::new(),
            unity_note: None,
            riff_metadata: Vec::new(),
            wavetable_cycle_frames: None,
            wavetable_image: false,
        });
        // Independently authored PCM16 sine table rendered in Workstation4.0.9.
        // These cover both phase bends, three ratio quantizers, reciprocal
        // harmonics, fine cents, independent FM start phase and FM-before-bend.
        for (attributes, native) in [
            (
                r#"NumOscs="1" PhaseDistortionMode="0" PhaseDistortionAmount="0.25" EnableFM="0""#,
                [0.221286297, -0.079275049, -0.211388722, -0.248151630],
            ),
            (
                r#"NumOscs="1" PhaseDistortionMode="0" PhaseDistortionAmount="0.75" EnableFM="0""#,
                [0.158156887, 0.244968951, 0.221286267, 0.097780176],
            ),
            (
                r#"NumOscs="1" PhaseDistortionMode="3" PhaseDistortionAmount="0.25" EnableFM="0""#,
                [0.183114290, 0.248151645, 0.019777611, -0.240257353],
            ),
            (
                r#"NumOscs="1" PhaseDistortionMode="3" PhaseDistortionAmount="0.5" EnableFM="0""#,
                [0.158156887, 0.244968951, 0.029626202, -0.248151645],
            ),
            (
                r#"NumOscs="1" PhaseDistortionMode="3" PhaseDistortionAmount="1" EnableFM="0""#,
                [0.122851476, 0.213990197, 0.249889717, -0.221286267],
            ),
            (
                r#"NumOscs="1" PhaseDistortionMode="1" PhaseDistortionAmount="0" EnableFM="0""#,
                [0.213990197, 0.221286267, 0.014840190, -0.205939636],
            ),
            (
                r#"NumOscs="1" PhaseDistortionMode="3" PhaseDistortionAmount="0" EnableFM="1" FMDepth="0" FMRatio="0" FMRatioMode="1" FMFixedFreq="0""#,
                [0.213990197, 0.221286267, 0.014840190, -0.205939636],
            ),
            (
                r#"NumOscs="1" PhaseDistortionMode="3" PhaseDistortionAmount="0" EnableFM="1" FMDepth="0.1" FMRatio="0" FMRatioMode="1" FMFixedFreq="0""#,
                [0.240642637, 0.180850729, 0.010183915, -0.162811130],
            ),
            (
                r#"NumOscs="1" PhaseDistortionMode="3" PhaseDistortionAmount="0" EnableFM="1" FMDepth="0.25" FMRatio="0" FMRatioMode="1" FMFixedFreq="0""#,
                [0.247928917, 0.095416874, 0.003193203, -0.078886494],
            ),
            (
                r#"NumOscs="1" PhaseDistortionMode="3" PhaseDistortionAmount="0" EnableFM="1" FMDepth="0.5" FMRatio="0" FMRatioMode="1" FMFixedFreq="0""#,
                [0.173972219, -0.074734695, -0.008460922, 0.080051571],
            ),
            (
                r#"NumOscs="1" PhaseDistortionMode="3" PhaseDistortionAmount="0" EnableFM="1" FMDepth="0.25" FMRatio="12" FMRatioMode="1" FMFixedFreq="0""#,
                [0.247125953, 0.246687114, 0.037967816, -0.247865841],
            ),
            (
                r#"NumOscs="1" PhaseDistortionMode="3" PhaseDistortionAmount="0" EnableFM="1" FMDepth="0.25" FMRatio="-12" FMRatioMode="1" FMFixedFreq="0""#,
                [0.246907800, 0.100685626, -0.165908024, -0.248935327],
            ),
            (
                r#"NumOscs="1" PhaseDistortionMode="3" PhaseDistortionAmount="0" EnableFM="1" FMDepth=".25" FMRatio="3.84" FMRatioMode="1" FMFixedFreq="0""#,
                [0.247928917, 0.095416874, 0.003193203, -0.078886494],
            ),
            (
                r#"NumOscs="1" PhaseDistortionMode="3" PhaseDistortionAmount="0" EnableFM="1" FMDepth=".25" FMRatio="3.84" FMRatioMode="1" FMFixedFreq="0" FMRatioFine="25""#,
                [0.247731641, 0.098005563, 0.011985413, -0.072825484],
            ),
            (
                r#"NumOscs="1" PhaseDistortionMode="3" PhaseDistortionAmount="0" EnableFM="1" FMDepth=".25" FMRatio="0" FMRatioMode="1" FMFixedFreq="0" StartPhase=".25""#,
                [-0.032129847, -0.231074795, -0.249979585, -0.237226292],
            ),
            (
                r#"NumOscs="1" PhaseDistortionMode="0" PhaseDistortionAmount=".25" EnableFM="1" FMDepth=".25" FMRatio="0" FMRatioMode="1" FMFixedFreq="0""#,
                [-0.021454122, -0.176905826, -0.215435237, -0.238115102],
            ),
            (
                r#"NumOscs="1" PhaseDistortionMode="3" PhaseDistortionAmount=".5" EnableFM="1" FMDepth=".25" FMRatio="0" FMRatioMode="1" FMFixedFreq="0""#,
                [0.226434514, 0.176389500, 0.006385846, -0.149714127],
            ),
            (
                r#"NumOscs="1" PhaseDistortionMode="3" PhaseDistortionAmount="0" EnableFM="1" FMDepth=".25" FMRatio="3.84" FMRatioMode="0" FMFixedFreq="0""#,
                [0.244527623, 0.153016835, 0.134689257, -0.061890118],
            ),
            (
                r#"NumOscs="1" PhaseDistortionMode="3" PhaseDistortionAmount="0" EnableFM="1" FMDepth=".25" FMRatio="7" FMRatioMode="1" FMFixedFreq="0""#,
                [0.247928917, 0.095416874, 0.003193203, -0.078886494],
            ),
            (
                r#"NumOscs="1" PhaseDistortionMode="3" PhaseDistortionAmount="0" EnableFM="1" FMDepth=".25" FMRatio="7.1" FMRatioMode="1" FMFixedFreq="0""#,
                [0.247125953, 0.246687114, 0.037967816, -0.247865841],
            ),
            (
                r#"NumOscs="1" PhaseDistortionMode="3" PhaseDistortionAmount="0" EnableFM="1" FMDepth=".25" FMRatio="-7" FMRatioMode="1" FMFixedFreq="0""#,
                [0.247928917, 0.095416874, 0.003193203, -0.078886494],
            ),
            (
                r#"NumOscs="1" PhaseDistortionMode="3" PhaseDistortionAmount="0" EnableFM="1" FMDepth=".25" FMRatio="-7.1" FMRatioMode="1" FMFixedFreq="0""#,
                [0.246907800, 0.100685626, -0.165908024, -0.248935327],
            ),
            (
                r#"NumOscs="1" PhaseDistortionMode="3" PhaseDistortionAmount="0" EnableFM="1" FMDepth=".25" FMRatio="18" FMRatioMode="1" FMFixedFreq="0""#,
                [0.219782606, 0.231138200, -0.019930065, -0.176386967],
            ),
            (
                r#"NumOscs="1" PhaseDistortionMode="3" PhaseDistortionAmount="0" EnableFM="1" FMDepth=".25" FMRatio="3.84" FMRatioMode="2" FMFixedFreq="0""#,
                [0.244391486, 0.156121314, 0.139085442, -0.065752700],
            ),
            (
                r#"NumOscs="1" PhaseDistortionMode="0" PhaseDistortionAmount="0" EnableFM="0" FMDepth=".25" FMRatio="0" FMRatioMode="1" FMFixedFreq="0""#,
                [-0.122851372, -0.213990137, -0.249889717, -0.221286312],
            ),
            (
                r#"NumOscs="1" PhaseDistortionMode="0" PhaseDistortionAmount="1" EnableFM="0" FMDepth=".25" FMRatio="0" FMRatioMode="1" FMFixedFreq="0""#,
                [0.122851461, 0.213990197, 0.249889717, 0.221286297],
            ),
            (
                r#"NumOscs="1" PhaseDistortionMode="3" PhaseDistortionAmount="0" EnableFM="1" FMDepth=".25" FMRatio="-3.5" FMRatioMode="2" FMFixedFreq="0""#,
                [0.249638259, 0.076517284, -0.085954845, -0.165842399],
            ),
            (
                r#"NumOscs="1" PhaseDistortionMode="3" PhaseDistortionAmount="0" EnableFM="1" FMDepth=".25" FMRatio="3.5" FMRatioMode="2" FMFixedFreq="0""#,
                [0.244391486, 0.156121314, 0.139085442, -0.065752700],
            ),
        ] {
            let xml = format!(
                r#"<Program><Layers><Layer><Keygroups><Keygroup><Oscillators><WaveTableOscillator WavetablePath="authored_sine_2048.wav" {attributes}/></Oscillators></Keygroup></Keygroups></Layer></Layers></Program>"#
            );
            let p = parse_program(&xml).unwrap();
            let node = p
                .nodes
                .iter()
                .find(|n| n.kind == "WaveTableOscillator")
                .unwrap();
            let mut g = Generator::new(node, 48_000., Some(table.clone())).unwrap();
            for i in 0..=120 {
                let actual = g
                    .next(
                        |name, default| number(node, name, default),
                        261.6255653005986,
                    )
                    .unwrap()[0]
                    * 0.5;
                if let Some(at) = [30, 60, 90, 120].iter().position(|&n| n == i) {
                    assert!(
                        (actual - native[at]).abs() < 0.000035,
                        "{attributes} frame{i}: {actual} vs {}",
                        native[at]
                    );
                }
            }
        }
        // Native unison fixtures cover exponential initial phases, Analog and
        // WT detune, alternate stereo, and independent FM phases per oscillator.
        for (kind, attributes, offset, native) in [
            (
                "MinBlepGenerator",
                r#"PhaseSpread="1" Stereo="1" StereoSpread="1" StereoSpreadMode="0" DetuneMode="0" Waveform="4" NumOscillators="3" MultiOscSpread="1""#,
                2,
                [-0.275590807, -0.014004918, 0.266967982, 0.304236174],
            ),
            (
                "MinBlepGenerator",
                r#"PhaseSpread="1" Stereo="1" StereoSpread="1" StereoSpreadMode="0" DetuneMode="0" Waveform="4" NumOscillators="4" MultiOscSpread="1""#,
                2,
                [-0.107728302, 0.050638020, 0.167280227, 0.129072845],
            ),
            (
                "MinBlepGenerator",
                r#"PhaseSpread="1" Stereo="1" StereoSpread="1" StereoSpreadMode="0" DetuneMode="0" Waveform="4" NumOscillators="8" MultiOscSpread="1""#,
                2,
                [-0.062036742, 0.061183780, 0.130527824, 0.073105335],
            ),
            (
                "WaveTableOscillator",
                r#"PhaseSpread="1" Stereo="1" StereoSpread="1" StereoSpreadMode="0" DetuneMode="0" NumOscs="3" Spread="1" PhaseDistortionMode="3" PhaseDistortionAmount="0" EnableFM="0""#,
                0,
                [0.264549941, -0.007548321, -0.279071480, -0.295308977],
            ),
            (
                "WaveTableOscillator",
                r#"PhaseSpread="1" Stereo="1" StereoSpread="1" StereoSpreadMode="0" DetuneMode="0" NumOscs="4" Spread="1" PhaseDistortionMode="3" PhaseDistortionAmount="0" EnableFM="0""#,
                0,
                [0.099129915, -0.061487794, -0.170098007, -0.120848477],
            ),
            (
                "WaveTableOscillator",
                r#"PhaseSpread="1" Stereo="1" StereoSpread="1" StereoSpreadMode="0" DetuneMode="0" NumOscs="8" Spread="1" PhaseDistortionMode="3" PhaseDistortionAmount="0" EnableFM="0""#,
                0,
                [0.054703251, -0.068812735, -0.130791947, -0.065306954],
            ),
            (
                "MinBlepGenerator",
                r#"PhaseSpread="1" Stereo="1" StereoSpread="1" StereoSpreadMode="1" DetuneMode="0" Waveform="4" NumOscillators="5" MultiOscSpread="1""#,
                2,
                [0.064328887, 0.270805240, 0.221229121, -0.046154205],
            ),
            (
                "MinBlepGenerator",
                r#"PhaseSpread="1" Stereo="1" StereoSpread="1" StereoSpreadMode="1" DetuneMode="0" Waveform="4" NumOscillators="7" MultiOscSpread="1""#,
                2,
                [0.117592521, 0.195809245, 0.081077613, -0.121896811],
            ),
            (
                "WaveTableOscillator",
                r#"PhaseSpread="1" Stereo="1" StereoSpread="1" StereoSpreadMode="1" DetuneMode="0" NumOscs="5" Spread="1" PhaseDistortionMode="3" PhaseDistortionAmount="0" EnableFM="0""#,
                0,
                [-0.082784764, -0.276580662, -0.208153874, 0.066034853],
            ),
            (
                "WaveTableOscillator",
                r#"PhaseSpread="1" Stereo="1" StereoSpread="1" StereoSpreadMode="1" DetuneMode="0" NumOscs="7" Spread="1" PhaseDistortionMode="3" PhaseDistortionAmount="0" EnableFM="0""#,
                0,
                [-0.128001288, -0.194017753, -0.068180121, 0.133526132],
            ),
            (
                "WaveTableOscillator",
                r#"NumOscs="3" Spread="0.5" PhaseSpread="0" Stereo="1" StereoSpread="1" StereoSpreadMode="0" DetuneMode="1" PhaseDistortionMode="3" PhaseDistortionAmount="0" EnableFM="0""#,
                0,
                [0.377380818, 0.143450543, -0.228840873, -0.383089066],
            ),
            (
                "WaveTableOscillator",
                r#"NumOscs="4" Spread="1" PhaseSpread="0" Stereo="1" StereoSpread="1" StereoSpreadMode="0" DetuneMode="1" PhaseDistortionMode="3" PhaseDistortionAmount="0" EnableFM="0""#,
                0,
                [0.363319933, 0.141347200, -0.210431963, -0.393792987],
            ),
            (
                "WaveTableOscillator",
                r#"NumOscs="3" Spread="1" PhaseSpread="1" Stereo="1" StereoSpread="1" StereoSpreadMode="0" DetuneMode="0" PhaseDistortionMode="3" PhaseDistortionAmount="0" EnableFM="1" FMDepth=".25" FMRatio="0" FMRatioMode="1" FMFixedFreq="0""#,
                0,
                [0.100060381, -0.214301318, -0.286698788, -0.319311619],
            ),
            (
                "WaveTableOscillator",
                r#"NumOscs="4" Spread="1" PhaseSpread="0" Stereo="1" StereoSpread="1" StereoSpreadMode="0" DetuneMode="0" PhaseDistortionMode="3" PhaseDistortionAmount="0" EnableFM="1" FMDepth=".25" FMRatio="12" FMRatioMode="1" FMFixedFreq="0""#,
                0,
                [0.197078899, 0.281218469, -0.201622218, -0.219292223],
            ),
        ] {
            let xml = format!(
                r#"<Program><Layers><Layer><Keygroups><Keygroup><Oscillators><{kind} WavetablePath="authored_sine_2048.wav" {attributes}/></Oscillators></Keygroup></Keygroups></Layer></Layers></Program>"#
            );
            let p = parse_program(&xml).unwrap();
            let node = p.nodes.iter().find(|n| n.kind == kind).unwrap();
            let mut g = Generator::new(
                node,
                48_000.,
                if kind == "WaveTableOscillator" {
                    Some(table.clone())
                } else {
                    None
                },
            )
            .unwrap();
            for i in 0..=120 - offset {
                let actual = g
                    .next(
                        |name, default| number(node, name, default),
                        261.6255653005986,
                    )
                    .unwrap()[0];
                if let Some(at) = [30, 60, 90, 120].iter().position(|&n| n == i + offset) {
                    // Native table interpolation differs from the original
                    // linear PCM readout, particularly when summing voices.
                    let tolerance = if kind == "WaveTableOscillator" {
                        0.000015
                    } else {
                        0.000008
                    };
                    assert!(
                        (actual - native[at]).abs() < tolerance,
                        "{kind} {attributes} frame{i}: {actual} vs {}",
                        native[at]
                    );
                }
            }
        }
        assert!(table_phase(0.2, 2., 0.).is_err());
        assert!(table_phase(0.2, 3., 1.1).is_err());
        assert!(
            table_fm(&|name, default| Ok(if name == "FMFixedFreq" { 1. } else { default }))
                .is_err()
        );
        assert!(
            table_fm(&|name, default| Ok(if name == "FMRatioMode" { 3. } else { default }))
                .is_err()
        );
        // Native authored 46-note sequences establish phase changes per launch
        // without gain changes; original RNG sequence parity is not claimed.
        // Local launch seeds must reproduce a render and differ between voices.
        for kind in ["MinBlepGenerator", "WaveTableOscillator"] {
            let xml = format!(
                r#"<Program><Layers><Layer><Keygroups><Keygroup><Oscillators><{kind} WavetablePath="authored_sine_2048.wav" PhaseSpread="2" NumOscs="3" NumOscillators="3"/></Oscillators></Keygroup></Keygroups></Layer></Layers></Program>"#
            );
            let p = parse_program(&xml).unwrap();
            let node = p.nodes.iter().find(|n| n.kind == kind).unwrap();
            let a = Generator::new_seeded(node, 48_000., Some(table.clone()), 41).unwrap();
            let b = Generator::new_seeded(node, 48_000., Some(table.clone()), 41).unwrap();
            let c = Generator::new_seeded(node, 48_000., Some(table.clone()), 42).unwrap();
            assert_eq!(a.phase, b.phase);
            assert_ne!(a.phase, c.phase);
            assert_ne!(a.phase[0], a.phase[1]);
            assert!(a.phase.iter().all(|p| (0. ..1.).contains(p)));
        }
    }
    #[test]
    fn authored_native_table_channels_and_index_spread() {
        let p = parse_program(r#"<Program><Layers><Layer><Keygroups><Keygroup><Oscillators><WaveTableOscillator WavetablePath="authored.wav"/></Oscillators></Keygroup></Keygroups></Layer></Layers></Program>"#).unwrap();
        let node = p
            .nodes
            .iter()
            .find(|n| n.kind == "WaveTableOscillator")
            .unwrap();
        // Independently authored 2/3/4/65-channel PCM16 WAVs at8000Hz establish
        // that channel data are waves, rather than simultaneous output buses.
        for channels in [2, 3, 4, 65] {
            let table = Arc::new(Sample {
                rate: 8000,
                channels,
                frames: 2048,
                interleaved: super::super::storage::Storage::from_f32(
                    (0..2048 * channels)
                        .map(|i| {
                            (4096. + 8192. * (i % channels) as f64 / (channels - 1) as f64).round()
                                as f32
                                / 32768.
                        })
                        .collect(),
                )
                .unwrap(),
                loops: Vec::new(),
                unity_note: None,
                riff_metadata: Vec::new(),
                wavetable_cycle_frames: None,
                wavetable_image: false,
            });
            let mut g = Generator::new(node, 48000., Some(table)).unwrap();
            for (index, native) in [(0., 0.0625), (0.5, 0.125), (1., 0.1875)] {
                let actual = g
                    .next(
                        |name, default| {
                            if name == "WaveIndex" {
                                Ok(index)
                            } else {
                                number(node, name, default)
                            }
                        },
                        261.6255653005986,
                    )
                    .unwrap()[0]
                    * 0.5;
                assert!((actual - native).abs() < 1e-7);
            }
            assert_eq!(g.channels(), 1);
        }
        let table = Arc::new(Sample {
            rate: 44100,
            channels: 1,
            frames: 4096,
            interleaved: super::super::storage::Storage::from_f32(
                [vec![0.125; 2048], vec![0.375; 2048]].concat(),
            )
            .unwrap(),
            loops: Vec::new(),
            unity_note: None,
            riff_metadata: Vec::new(),
            wavetable_cycle_frames: None,
            wavetable_image: false,
        });
        // Native L/R observations distinguish positive i/N index spread from
        // centered detune positions and establish clamping at the top wave.
        for (count, index, spread, native) in [
            (2., 0.5, 0.0, [0.176776692, 0.176776692]),
            (2., 0.5, 0.25, [0.176776692, 0.198873773]),
            (2., 0.5, 0.5, [0.176776692, 0.220970869]),
            (2., 0.5, 1.0, [0.176776692, 0.265165031]),
            (2., 0.0, 0.5, [0.088388346, 0.132582515]),
            (2., 0.2, 0.5, [0.123743691, 0.167937845]),
            (2., 0.8, 0.5, [0.229809716, 0.265165031]),
            (2., 1.0, 0.5, [0.265165031, 0.265165031]),
            (3., 0.2, 0.6, [0.180421963, 0.209289476]),
            (4., 0.2, 0.6, [0.215245888, 0.247254133]),
            (8., 0.0, 1.0, [0.314807981, 0.348104656]),
        ] {
            let xml = format!(
                r#"<Program><Layers><Layer><Keygroups><Keygroup><Oscillators><WaveTableOscillator WavetablePath="authored_2048.wav" NumOscs="{count}" WaveIndex="{index}" WaveIndexSpread="{spread}" PhaseSpread="0" Stereo="1" StereoSpread="1" StereoSpreadMode="0"/></Oscillators></Keygroup></Keygroups></Layer></Layers></Program>"#
            );
            let p = parse_program(&xml).unwrap();
            let node = p
                .nodes
                .iter()
                .find(|n| n.kind == "WaveTableOscillator")
                .unwrap();
            let mut g = Generator::new(node, 48000., Some(table.clone())).unwrap();
            let actual = g
                .next(
                    |name, default| number(node, name, default),
                    261.6255653005986,
                )
                .unwrap();
            for channel in 0..2 {
                assert!((actual[channel] - native[channel]).abs() < 1e-7);
            }
        }
    }
    #[test]
    fn authored_native_four_operator_fm() {
        // Independently authored native fixtures, not values generated by this DSP.
        // Mixed levels, phases, ratios, fine tuning and harmonic snapping exercise
        // every admitted topology, both versions and nonzero D feedback.
        for (topology, version, feedback, native) in [
            (
                0.,
                1.,
                0.,
                [
                    0.3496526777744293,
                    0.21780210733413696,
                    -0.3497423827648163,
                    0.34804442524909973,
                    0.34429341554641724,
                ],
            ),
            (
                5.,
                1.,
                0.,
                [
                    0.6546869277954102,
                    0.03177136182785034,
                    -0.4014642536640167,
                    0.6363136768341064,
                    -0.28029942512512207,
                ],
            ),
            (
                6.,
                1.,
                0.,
                [
                    0.30837351083755493,
                    0.2014549970626831,
                    -0.34748515486717224,
                    0.23946194350719452,
                    0.3497641682624817,
                ],
            ),
            (
                10.,
                1.,
                0.,
                [
                    0.9233050346374512,
                    0.06094476580619812,
                    -0.3223050832748413,
                    0.28053271770477295,
                    -0.3523567318916321,
                ],
            ),
            (
                0.,
                0.,
                1.,
                [
                    0.3005619943141937,
                    0.28907546401023865,
                    0.14037208259105682,
                    0.1383959800004959,
                    -0.2191067487001419,
                ],
            ),
            (
                6.,
                1.,
                0.37,
                [
                    0.30837351083755493,
                    0.2004684954881668,
                    -0.34795334935188293,
                    0.21789395809173584,
                    0.34804585576057434,
                ],
            ),
            (
                7.,
                1.,
                0.6,
                [
                    0.47180497646331787,
                    0.1619706153869629,
                    -0.21064996719360352,
                    0.19575756788253784,
                    0.47868961095809937,
                ],
            ),
            (
                5.,
                0.,
                0.37,
                [
                    0.46748751401901245,
                    0.7478116750717163,
                    0.43330276012420654,
                    -0.4486665427684784,
                    -0.6644572019577026,
                ],
            ),
            (
                10.,
                0.,
                0.37,
                [
                    0.7317759990692139,
                    0.7381051778793335,
                    0.21440300345420837,
                    -0.4636915624141693,
                    -0.8253520727157593,
                ],
            ),
            (
                6.,
                0.,
                0.224,
                [
                    0.3499845266,
                    0.2983056307,
                    0.2498360872,
                    0.1441069394,
                    -0.1261043698,
                ],
            ),
        ] {
            let mut p = parse_program(r#"<Program><Layers><Layer><Keygroups><Keygroup><Oscillators><FmOscillator/></Oscillators></Keygroup></Keygroups></Layer></Layers></Program>"#).unwrap();
            let node = p
                .nodes
                .iter_mut()
                .find(|n| n.kind == "FmOscillator")
                .unwrap();
            for (name, value) in [
                ("Topology", topology),
                ("FmOscillatorVersion", version),
                ("Feedback", feedback),
                ("LevelA", 0.7),
                ("LevelB", 0.9),
                ("LevelC", 0.3),
                ("LevelD", 0.5),
                ("RatioA", 1.6),
                ("RatioB", 2.3),
                ("RatioC", 3.1),
                ("RatioD", 4.7),
                ("RatioFineA", 17.),
                ("RatioFineB", -22.),
                ("RatioFineC", 30.),
                ("SnapRatioA", 1.),
                ("SnapRatioD", 1.),
                ("PhaseA", 0.1),
                ("PhaseB", 0.2),
                ("PhaseC", 0.3),
                ("PhaseD", 0.4),
            ] {
                node.attributes.insert(name.into(), value.to_string());
            }
            // Native full captures with these changed fixed-frequency controls
            // were byte-identical while all four FixedFreq flags stayed false.
            if (topology == 0. && version == 1. && feedback == 0.)
                || (topology == 6. && version == 0. && feedback == 0.224)
            {
                for (name, value) in [
                    ("FreqA", 0.13),
                    ("FreqB", 0.28),
                    ("FreqC", 1.1),
                    ("FreqD", 1.8),
                    ("FreqMultiplierA", 0.),
                    ("FreqMultiplierB", 1.),
                    ("FreqMultiplierC", 2.),
                    ("FreqMultiplierD", 4.),
                ] {
                    node.attributes.insert(name.into(), value.to_string());
                }
            }
            let mut g = Generator::new(node, 48_000., None).unwrap();
            assert_eq!(g.channels(), 1);
            for frame in 0..=120 {
                let actual = g
                    .next(
                        |name, default| number(node, name, default),
                        261.6255653005986,
                    )
                    .unwrap()[0]
                    * 0.5;
                if let Some(i) = [0, 30, 60, 90, 120].iter().position(|&at| at == frame) {
                    assert!(
                        (actual - native[i]).abs() < 3e-6,
                        "topology {topology}, version {version}, feedback {feedback}, frame {frame}: {actual} vs {}",
                        native[i]
                    );
                }
            }
            if version == 0. {
                node.attributes.remove("FmOscillatorVersion");
                let mut g = Generator::new(node, 48_000., None).unwrap();
                let actual = g
                    .next(
                        |name, default| number(node, name, default),
                        261.6255653005986,
                    )
                    .unwrap()[0]
                    * 0.5;
                assert!((actual - native[0]).abs() < 3e-6);
            }
            for (name, bad) in [
                ("Topology", "1"),
                ("FmOscillatorVersion", "2"),
                ("FixedFreqA", "1"),
                ("Feedback", "1.1"),
            ] {
                let previous = node.attributes.insert(name.into(), bad.into());
                assert!(Generator::new(node, 48_000., None).is_err());
                if let Some(value) = previous {
                    node.attributes.insert(name.into(), value);
                } else {
                    node.attributes.remove(name);
                }
            }
        }
    }
}
