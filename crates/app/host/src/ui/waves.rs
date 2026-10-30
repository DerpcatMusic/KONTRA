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

/// What a display reads of a zone: its peaks and its length.
type Read = (Arc<[[i8; 2]]>, i64);

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
        for wave in tree.widgets.iter_mut().filter_map(|w| w.waveform.as_mut()) {
            let Ok(zone) = u32::try_from(wave.zone) else {
                continue;
            };
            let zones = &self.zones;
            let read = self.read.entry(zone).or_insert_with(|| {
                let (_, sample) = zones.iter().find(|(id, _)| *id == zone)?;
                peaks(sample)
            });
            if let Some((peaks, length)) = read {
                if *wave.peaks != **peaks {
                    wave.peaks = peaks.to_vec();
                }
                wave.length_us = *length;
            }
        }
    }
}

/// `sample` as [`COLUMNS`] columns of its channels' lowest and highest
/// level, and its length in microseconds.
fn peaks(sample: &ir::Sample) -> Option<Read> {
    let (rate, frames) = kontra_stream::decode(sample, MAX_FRAMES).ok()?;
    let per = frames.len().div_ceil(COLUMNS).max(1);
    let level = |x: f32| (x.clamp(-1., 1.) * 127.).round() as i8;
    let columns = frames
        .chunks(per)
        .map(|c| {
            let all = c.iter().flatten().copied();
            let (low, high) = all.fold((0f32, 0f32), |(l, h), x| (l.min(x), h.max(x)));
            [level(low), level(high)]
        })
        .collect();
    let length = frames.len() as f64 * 1e6 / f64::from(rate.max(1));
    Some((columns, length as i64))
}
