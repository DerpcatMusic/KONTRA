use std::io::Cursor;

use crate::{
    kontakt::{Chunk, KontaktError, StructuredObject},
    read_bytes::ReadBytesExt,
    Error,
};

const VERSION: u16 = 0x10;

/// Type:           Chunk (structured), child of a zone (BZone)
/// SerType:        0x35
/// KontaktIO:      not named; observed in Kontakt 8 files
///
/// Slice markers of a zone with a grid (Beat Machine slices). The public payload is:
///
/// - `u8` reserved (0), `i32` selected (-1), `u32` count
/// - if `count > 0`: `u32` grid length in sample frames (24000 = four 6000-frame slices)
/// - `count` records of `u32` frame, `f32` frame (same value up to rounding, meaning
///   open), `u32` extra (8 on the first record, 0 on the others, meaning open)
///
/// Observed: a grid-fixed zone with four 1/16 slices at 120 BPM at 48 kHz has count 5 and
/// boundaries 0, 6000, 12000, 18000, 24000, so four slices are consecutive frame pairs. A
/// plain zone has count 0 and no length field. The length field is inferred from these two
/// layouts; a grid with other counts is not in the corpus yet.
#[derive(Debug, Clone, PartialEq)]
pub struct ZoneSlices {
    pub selected: i32,
    /// Grid length in sample frames; `None` when the zone has no markers.
    pub length: Option<u32>,
    pub markers: Vec<SliceMarker>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SliceMarker {
    /// Position in sample frames from the sample start.
    pub frame: u32,
    /// Stored float copy of `frame`; not used for playback yet.
    pub frame_f32: f32,
    pub extra: u32,
}

impl ZoneSlices {
    pub fn read(public: &[u8]) -> Result<Self, Error> {
        let mut reader = Cursor::new(public);
        let _reserved = reader.read_u8()?;
        let selected = reader.read_i32_le()?;
        let count = reader.read_u32_le()?;
        let length = if count > 0 {
            Some(reader.read_u32_le()?)
        } else {
            None
        };
        let expected = 1 + 4 + 4 + if count > 0 { 4 } else { 0 } + 12 * count as usize;
        if public.len() != expected {
            return Err(Error::Generic(format!(
                "zone slices: {} public bytes for {count} markers, expected {expected}",
                public.len()
            )));
        }
        let mut markers = Vec::with_capacity(count as usize);
        for _ in 0..count {
            let frame = reader.read_u32_le()?;
            let frame_f32 = reader.read_f32_le()?;
            let extra = reader.read_u32_le()?;
            markers.push(SliceMarker {
                frame,
                frame_f32,
                extra,
            });
        }
        if markers
            .windows(2)
            .any(|pair| pair[1].frame <= pair[0].frame)
        {
            return Err(Error::Static("zone slice markers are not increasing"));
        }
        Ok(Self {
            selected,
            length,
            markers,
        })
    }
}

impl std::convert::TryFrom<&Chunk> for ZoneSlices {
    type Error = Error;

    fn try_from(chunk: &Chunk) -> Result<Self, Self::Error> {
        if chunk.id != 0x35 {
            return Err(KontaktError::IncorrectID {
                expected: 0x35,
                got: chunk.id,
            }
            .into());
        }
        let object = StructuredObject::try_from(chunk)?;
        if object.version != VERSION {
            return Err(Error::Generic(format!(
                "Unsupported zone slices version {:x}",
                object.version
            )));
        }
        Self::read(&object.public_data)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Public payload of the zone child 0x35 in the Beat Machine capture (grid fixed, 120 BPM,
    /// four 1/16 slices at 48 kHz). Taken from the saved instrument, not synthesised.
    const FIXED_GRID: [u8; 73] = [
        0x00, 0xff, 0xff, 0xff, 0xff, 0x05, 0x00, 0x00, 0x00, 0xc0, 0x5d, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x08, 0x00, 0x00, 0x00, 0x70, 0x17, 0x00, 0x00, 0x00,
        0x80, 0xbb, 0x45, 0x00, 0x00, 0x00, 0x00, 0xe0, 0x2e, 0x00, 0x00, 0x00, 0x80, 0x3b, 0x46,
        0x00, 0x00, 0x00, 0x00, 0x50, 0x46, 0x00, 0x00, 0x00, 0xa0, 0x8c, 0x46, 0x00, 0x00, 0x00,
        0x00, 0xc0, 0x5d, 0x00, 0x00, 0x00, 0x80, 0xbb, 0x46, 0x00, 0x00, 0x00, 0x00,
    ];

    #[test]
    fn captured_fixed_grid_reads_five_boundaries_for_four_slices() {
        let slices = ZoneSlices::read(&FIXED_GRID).unwrap();
        assert_eq!(slices.selected, -1);
        assert_eq!(slices.length, Some(24000));
        let frames: Vec<u32> = slices.markers.iter().map(|m| m.frame).collect();
        assert_eq!(frames, [0, 6000, 12000, 18000, 24000]);
    }

    #[test]
    fn plain_zone_has_no_markers_and_no_length_field() {
        let slices =
            ZoneSlices::read(&[0x00, 0xff, 0xff, 0xff, 0xff, 0x00, 0x00, 0x00, 0x00]).unwrap();
        assert_eq!(slices.length, None);
        assert!(slices.markers.is_empty());
    }

    #[test]
    fn wrong_length_and_unsorted_markers_are_errors() {
        assert!(ZoneSlices::read(&FIXED_GRID[..72]).is_err());
        let mut unsorted = FIXED_GRID;
        unsorted[25..29].copy_from_slice(&0u32.to_le_bytes());
        assert!(ZoneSlices::read(&unsorted).is_err());
    }
}
