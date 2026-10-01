//! What a `ui_waveform` shows of its attached zone: the zone's sample read
//! once, off the UI thread, into the low and high of each of a fixed number
//! of columns, and kept.

use crate::audio::{Frame, Sources};
use crate::import::Zone;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, LazyLock, Mutex, PoisonError};

/// Columns kept per wave: more than a waveform is pixels wide at 2x.
const COLUMNS: usize = 1024;

/// A zone's wave from its start to its end.
#[derive(Debug, PartialEq)]
pub struct Peaks {
    /// Low and high of each column, -1 to 1.
    pub columns: Vec<[f32; 2]>,
    /// Frames the columns cover, and their rate: where a play position falls.
    pub frames: u64,
    pub rate: u32,
}

impl Peaks {
    /// Where `micros` into the zone falls across the wave, 0 to 1.
    pub fn at(&self, micros: i64) -> f64 {
        let frames = micros as f64 * f64::from(self.rate) / 1e6;
        (frames / self.frames.max(1) as f64).clamp(0., 1.)
    }
}

type Key = (PathBuf, usize, i32);

/// Each wave read or being read; `None` while it is read or when it can't be.
static WAVES: LazyLock<Mutex<HashMap<Key, Option<Arc<Peaks>>>>> = LazyLock::new(Mutex::default);

fn key(zone: &Zone) -> Key {
    (zone.sample.clone(), zone.start, zone.end)
}

fn waves() -> std::sync::MutexGuard<'static, HashMap<Key, Option<Arc<Peaks>>>> {
    WAVES.lock().unwrap_or_else(PoisonError::into_inner)
}

/// `zone`'s wave if it has been read; else it is read on a thread of its
/// own, for a later frame.
pub fn ask(zone: &Zone) -> Option<Arc<Peaks>> {
    let k = key(zone);
    if let Some(w) = waves().get(&k) {
        return w.clone();
    }
    waves().insert(k.clone(), None);
    let zone = zone.clone();
    let _ = std::thread::Builder::new().name("kontakto-wave".into()).spawn(move || {
        let w = read(&zone).map(Arc::new);
        waves().insert(k, w);
    });
    None
}

/// `zone`'s wave, read now if it has not been (the audit, a screenshot).
pub fn now(zone: &Zone) -> Option<Arc<Peaks>> {
    let k = key(zone);
    if let Some(Some(w)) = waves().get(&k) {
        return Some(w.clone());
    }
    let w = read(zone).map(Arc::new);
    waves().insert(k, w.clone());
    w
}

/// Read `zone`'s sample a block at a time into its peaks.
fn read(zone: &Zone) -> Option<Peaks> {
    let mut reader = Sources::default().source(&zone.sample).ok()?.open().ok()?;
    let start = (zone.start as u64).min(reader.frames);
    let end = u64::try_from(zone.end).ok().filter(|&e| e > start).map_or(reader.frames, |e| e.min(reader.frames));
    let frames = end - start;
    if frames == 0 {
        return None;
    }
    let mut columns = vec![[0f32; 2]; COLUMNS];
    let mut block = vec![[0.0; 2]; 1 << 16];
    let mut at = 0u64;
    while at < frames {
        let n = (frames - at).min(block.len() as u64) as usize;
        reader.read(start + at, &mut block[..n]).ok()?;
        fold(&mut columns, &block[..n], at, frames);
        at += n as u64;
    }
    Some(Peaks { columns, frames, rate: reader.rate })
}

/// Widen each column by the frames of `block`, the `at`th on of `total`.
fn fold(columns: &mut [[f32; 2]], block: &[Frame], at: u64, total: u64) {
    let n = columns.len() as u64;
    for (i, f) in block.iter().enumerate() {
        let col = &mut columns[((at + i as u64) * n / total) as usize];
        for v in f {
            col[0] = col[0].min(v.clamp(-1., 1.));
            col[1] = col[1].max(v.clamp(-1., 1.));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn columns_keep_each_stretch_low_and_high() {
        let mut columns = vec![[0f32; 2]; 4];
        let block: Vec<Frame> = (0..8).map(|i| [i as f32 / 8., -(i as f32) / 16.]).collect();
        fold(&mut columns, &block[..4], 0, 8);
        fold(&mut columns, &block[4..], 4, 8);
        assert_eq!(columns, [[-0.0625, 0.125], [-0.1875, 0.375], [-0.3125, 0.625], [-0.4375, 0.875]]);
        let p = Peaks { columns, frames: 48_000, rate: 48_000 };
        assert_eq!((p.at(500_000), p.at(-1), p.at(9_000_000)), (0.5, 0., 1.));
    }
}
