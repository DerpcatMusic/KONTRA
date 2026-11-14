//! The samples behind a script's waveform displays (`attach_zone`): each
//! zone's sample, read once into columns of peaks when a display first
//! shows it.

use std::collections::HashMap;
use std::sync::Arc;

use kontra_engine::ir;
use kontra_ui_model as model;

/// Columns a sample is read into; a display draws them across its width.
const COLUMNS: usize = 512;

/// The longest sample read, in frames (ten minutes at 48 kHz).
const MAX_FRAMES: u64 = 48_000 * 600;

/// A wavetable's frame, in samples (Kontakt's wavetable mode).
const FRAME: usize = 2048;

/// What a display reads of a zone: its peaks, its length, how its samples
/// spread down each column, and its wavetable frames.
#[derive(Clone, Debug)]
struct Read {
    peaks: Arc<[[i8; 2]]>,
    length_us: i64,
    density: Arc<[u16]>,
    frames: Arc<[Vec<i8>]>,
}

/// The instrument's zone samples by zone number, and those read so far.
#[derive(Clone, Debug, Default)]
pub struct Waves {
    zones: Arc<[(u32, ir::Sample)]>,
    read: HashMap<u32, Option<Read>>,
}

impl Waves {
    /// The zones of `instrument` a display may attach.
    #[must_use]
    pub fn of(instrument: &ir::Instrument) -> Self {
        let zones = (instrument.groups.iter())
            .flat_map(|g| &g.zones)
            .filter_map(|z| Some((z.id, instrument.samples.get(z.sample)?.clone())))
            .collect();
        Self {
            zones,
            read: HashMap::new(),
        }
    }

    /// Gives each waveform display in `tree` its zone's peaks.
    ///
    /// ponytail: a zone's sample is read on the control thread the first
    /// time it shows (tens of ms for a long one); read it off-thread if a
    /// display that swaps zones per note stutters.
    pub fn fill(&mut self, tree: &mut model::Interface) {
        for w in &mut tree.widgets {
            let table = matches!(w.kind, model::Kind::Wavetable { .. });
            let Some(wave) = w.waveform.as_mut() else {
                continue;
            };
            let Ok(zone) = u32::try_from(wave.zone) else {
                continue;
            };
            let zones = &self.zones;
            let read = self.read.entry(zone).or_insert_with(|| {
                let (_, sample) = zones.iter().find(|(id, _)| *id == zone)?;
                peaks(sample)
            });
            if let Some(read) = read {
                if *wave.peaks != *read.peaks {
                    wave.peaks = read.peaks.to_vec();
                }
                if wave.xray && *wave.density != *read.density {
                    wave.density = read.density.to_vec();
                }
                if table && *wave.frames != *read.frames {
                    wave.frames = read.frames.to_vec();
                }
                wave.length_us = read.length_us;
            }
        }
    }
}

/// `sample` as [`COLUMNS`] columns of its channels' lowest and highest
/// level and of how its (channel-mixed) samples spread down
/// [`model::WAVE_BINS`] bands, its length in microseconds, and its
/// [`FRAME`]-sample frames at [`model::FRAME_POINTS`] points each.
fn peaks(sample: &ir::Sample) -> Option<Read> {
    let (rate, frames) = kontra_stream::decode(sample, MAX_FRAMES).ok()?;
    let per = frames.len().div_ceil(COLUMNS).max(1);
    let level = |x: f32| (x.clamp(-1., 1.) * 127.).round() as i8;
    let mono = |f: &[f32; 2]| f.iter().sum::<f32>() / 2.;
    let bins = model::WAVE_BINS;
    let mut density = Vec::with_capacity(COLUMNS * bins);
    let peaks = frames
        .chunks(per)
        .map(|c| {
            let all = c.iter().flatten().copied();
            let (low, high) = all.fold((0f32, 0f32), |(l, h), x| (l.min(x), h.max(x)));
            let mut column = vec![0u16; bins];
            for f in c {
                let band = ((1. - mono(f).clamp(-1., 1.)) / 2. * bins as f32) as usize;
                if let Some(n) = column.get_mut(band.min(bins - 1)) {
                    *n = n.saturating_add(1);
                }
            }
            density.extend(column);
            [level(low), level(high)]
        })
        .collect();
    let points = model::FRAME_POINTS;
    // Whole frames only: a sample shorter than one has none.
    let cycles = (frames.as_chunks::<FRAME>().0.iter())
        .map(|cycle| {
            (0..points)
                .map(|i| cycle.get(i * FRAME / points).map_or(0, |f| level(mono(f))))
                .collect()
        })
        .collect();
    let length = frames.len() as f64 * 1e6 / f64::from(rate.max(1));
    Some(Read {
        peaks,
        length_us: length as i64,
        density: density.into(),
        frames: cycles,
    })
}
