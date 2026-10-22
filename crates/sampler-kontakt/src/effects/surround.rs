//! Kontakt Surround Panner record (`BParFXSurroundPanner`, module 0x1d, version 0x70): reader `0x140d02f60`.
//!
//! Reader order, little-endian: `i32 n`, then `n` x {`f32 x`, `f32 y`, `u8 disabled`} (speakers);
//! `i32 m`, then `m` x the same triple (sources); then `f32` at `+0x194`, `+0x198` (LFE volume, dB),
//! `+0x19c` (size), `+0x1a0` (divergence), `+0x1bc`, `+0x1c0`; `i32` gain mode (`+0x1c4`, 0..=2);
//! `i32` layout (`+0x1c8`, 0..=6); `u8` x3 (`+0x1a4..+0x1a6`); `f32` at `+0x1a8` (distance), `+0x1ac`
//! (azimuth), `+0x1b0`/`+0x1b4` (offset x/y); `i32` at `+0x1cc`. Field names come from
//! `docs/re/kontakt-bindings/fx_other.tsv`. Unnamed fields are read and dropped.
//!
//! The apply that copies these onto the Panner object is not identified, so nothing lowers from the
//! record yet: the module stays NotModeled (see `docs/re/engine/k-spatial.md`).
use super::Reader;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Speaker {
    pub x: f32,
    pub y: f32,
    pub disabled: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Record {
    pub speakers: Vec<Speaker>,
    pub sources: Vec<Speaker>,
    /// `$ENGINE_PAR_SP_LFE_VOLUME`, dB.
    pub lfe_volume: f32,
    /// `$ENGINE_PAR_SP_SIZE`.
    pub size: f32,
    /// `$ENGINE_PAR_SP_DIVERGENCE`.
    pub divergence: f32,
    /// `$ENGINE_PAR_SP_OFFSET_DISTANCE`.
    pub distance: f32,
    /// `$ENGINE_PAR_SP_OFFSET_AZIMUTH`.
    pub azimuth: f32,
    /// `$ENGINE_PAR_SP_OFFSET_X`, `$ENGINE_PAR_SP_OFFSET_Y`.
    pub offset: [f32; 2],
    /// Gain-mode selector at `+0x1c4` (0..=2).
    pub mode: i32,
    /// Layout selector at `+0x1c8` (0..=6).
    pub layout: i32,
}

fn speaker_list(r: &mut Reader) -> Option<Vec<Speaker>> {
    let n = usize::try_from(r.i32()?).ok()?;
    // Each entry is 9 bytes; a count the record cannot hold is rejected before allocating.
    (n <= r.0.len() / 9).then_some(())?;
    (0..n).map(|_| Some(Speaker { x: r.f32()?, y: r.f32()?, disabled: r.flag()? })).collect()
}

/// Reads one record. The whole payload must be consumed: trailing bytes mean this is not the 0x70 layout.
pub(super) fn read(r: &mut Reader) -> Option<Record> {
    let speakers = speaker_list(r)?;
    let sources = speaker_list(r)?;
    let _ = r.f32()?;
    let lfe_volume = r.f32()?;
    let size = r.f32()?;
    let divergence = r.f32()?;
    let _ = (r.f32()?, r.f32()?);
    let mode = r.i32()?;
    let layout = r.i32()?;
    let _ = (r.flag()?, r.flag()?, r.flag()?);
    let distance = r.f32()?;
    let azimuth = r.f32()?;
    let offset = [r.f32()?, r.f32()?];
    let _ = r.i32()?;
    r.0.is_empty().then_some(())?;
    Some(Record { speakers, sources, lfe_volume, size, divergence, distance, azimuth, offset, mode, layout })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Reader order is the byte order of a saved record: speakers, sources, floats, ints, flags, floats.
    fn record_bytes() -> Vec<u8> {
        let mut b = Vec::new();
        b.extend(2i32.to_le_bytes());
        for (x, y, d) in [(0.1f32, 0.9f32, 0u8), (0.9, 0.9, 1)] {
            b.extend(x.to_le_bytes());
            b.extend(y.to_le_bytes());
            b.push(d);
        }
        b.extend(1i32.to_le_bytes());
        b.extend(0.5f32.to_le_bytes());
        b.extend(0.5f32.to_le_bytes());
        b.push(0);
        // 0x194, LFE, size, divergence, 0x1bc, 0x1c0
        for v in [7.0f32, -6.0, 0.3, 0.25, 8.0, 9.0] {
            b.extend(v.to_le_bytes());
        }
        b.extend(1i32.to_le_bytes()); // mode
        b.extend(3i32.to_le_bytes()); // layout
        b.extend([1u8, 0, 1]);
        for v in [0.4f32, -0.2, 0.6, -0.8] {
            b.extend(v.to_le_bytes());
        }
        b.extend(5i32.to_le_bytes());
        b
    }

    #[test]
    fn saved_record_reads_in_reader_order() {
        let bytes = record_bytes();
        let rec = read(&mut Reader(&bytes)).expect("record parses");
        assert_eq!(rec.speakers, vec![Speaker { x: 0.1, y: 0.9, disabled: false }, Speaker { x: 0.9, y: 0.9, disabled: true }]);
        assert_eq!(rec.sources, vec![Speaker { x: 0.5, y: 0.5, disabled: false }]);
        assert_eq!((rec.lfe_volume, rec.size, rec.divergence), (-6.0, 0.3, 0.25));
        assert_eq!((rec.distance, rec.azimuth, rec.offset), (0.4, -0.2, [0.6, -0.8]));
        assert_eq!((rec.mode, rec.layout), (1, 3));
    }

    #[test]
    fn trailing_or_short_payload_is_rejected() {
        let mut bytes = record_bytes();
        bytes.push(0);
        assert!(read(&mut Reader(&bytes)).is_none());
        let short = record_bytes();
        assert!(read(&mut Reader(&short[..short.len() - 1])).is_none());
    }

    #[test]
    fn impossible_speaker_count_is_rejected_before_allocating() {
        let bytes = i32::MAX.to_le_bytes();
        assert!(read(&mut Reader(&bytes)).is_none());
    }

    /// A saved record reaches the chain as a parsed Surround Panner: named NotModeled, no DSP invented.
    #[test]
    fn saved_record_is_named_not_modelled() {
        use crate::effects::{Scope, chain, test_support::slot};
        let mut fx = slot(0x1d, record_bytes(), 1.0);
        fx.version = 0x70;
        let built = chain(&[fx], Scope::Bus);
        assert!(built.processors.is_empty(), "{:?}", built.processors);
        assert!(built.notes.iter().any(|(_, _, t, r)| t.contains("Surround Panner") && *r == sampler_ir::Reason::NotModeled), "{:?}", built.notes);
    }
}
