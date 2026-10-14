//! Bounded file playback metadata for immutable sample revisions. Byte layouts
//! follow RIFF smpl/inst, Apple AIFF 1.3 INST/MARK and FLAC foreign APPLICATION
//! chunks; UVI semantics are documented at lua.uvi.net/_sample_metadata.html.
//! File roots describe sampleInfo; loadSample retains the oscillator BaseNote.
use sampler_core::{Loop, LoopMode, LoopShape, Playback};
use std::collections::BTreeMap;

const MAX_CHUNKS: usize = 65536;
const MAX_METADATA: usize = 1 << 20;
const MAX_MARKERS: usize = 4096;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Format {
    Wav,
    Aiff,
    Flac,
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct SampleMetadata {
    pub format: Format,
    pub base_note: u8,
    pub fine_tune: f64,
    pub frames: usize,
    pub looping: Option<Loop>,
}
impl SampleMetadata {
    pub fn playback(self) -> Playback {
        Playback {
            end: Some(self.frames),
            loop_range: self.looping,
            ..Default::default()
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MetadataError {
    Format,
    Truncated,
    Limit,
    Invalid,
    UnsupportedVendorSamplerData,
}
type Result<T> = std::result::Result<T, MetadataError>;
fn le32(b: &[u8], at: usize) -> Result<u32> {
    Ok(u32::from_le_bytes(
        b.get(at..at + 4)
            .ok_or(MetadataError::Truncated)?
            .try_into()
            .unwrap(),
    ))
}
fn be32(b: &[u8], at: usize) -> Result<u32> {
    Ok(u32::from_be_bytes(
        b.get(at..at + 4)
            .ok_or(MetadataError::Truncated)?
            .try_into()
            .unwrap(),
    ))
}
fn be16(b: &[u8], at: usize) -> Result<u16> {
    Ok(u16::from_be_bytes(
        b.get(at..at + 2)
            .ok_or(MetadataError::Truncated)?
            .try_into()
            .unwrap(),
    ))
}

#[derive(Default)]
struct State {
    chunks: usize,
    metadata: usize,
    smpl_root: Option<(u8, f64)>,
    inst_root: Option<(u8, f64)>,
    looping: Option<(usize, usize, LoopShape)>,
    markers: BTreeMap<u16, usize>,
    sustain: Option<(u16, u16, u16)>,
}
impl State {
    fn chunk(&mut self, id: &[u8], body: &[u8], aiff: bool) -> Result<()> {
        self.chunks += 1;
        if self.chunks > MAX_CHUNKS {
            return Err(MetadataError::Limit);
        }
        if matches!(id, b"smpl" | b"inst" | b"INST" | b"MARK") {
            self.metadata = self
                .metadata
                .checked_add(body.len())
                .ok_or(MetadataError::Limit)?;
            if self.metadata > MAX_METADATA {
                return Err(MetadataError::Limit);
            }
        }
        match id {
            b"smpl" if !aiff => {
                if body.len() < 36 {
                    return Err(MetadataError::Truncated);
                }
                let root = le32(body, 12)?;
                if root > 127 {
                    return Err(MetadataError::Invalid);
                }
                self.smpl_root =
                    Some((root as u8, f64::from(le32(body, 16)?) / 4294967296. * 100.));
                let loops = le32(body, 28)? as usize;
                let vendor = le32(body, 32)? as usize;
                if loops > MAX_MARKERS {
                    return Err(MetadataError::Limit);
                }
                let end = 36usize
                    .checked_add(loops.checked_mul(24).ok_or(MetadataError::Limit)?)
                    .and_then(|v| v.checked_add(vendor))
                    .ok_or(MetadataError::Limit)?;
                if end > body.len() {
                    return Err(MetadataError::Truncated);
                }
                if vendor != 0 {
                    return Err(MetadataError::UnsupportedVendorSamplerData);
                }
                if loops > 0 {
                    let kind = le32(body, 40)?;
                    if kind <= 2 {
                        let start = le32(body, 44)? as usize;
                        // RIFF dwEnd includes its named frame; core ranges do not.
                        let end = (le32(body, 48)? as usize)
                            .checked_add(1)
                            .ok_or(MetadataError::Invalid)?;
                        self.looping = Some((
                            start,
                            end,
                            if kind == 1 {
                                LoopShape::PingPong
                            } else {
                                LoopShape::Wrap
                            },
                        ));
                    }
                }
            }
            b"inst" if !aiff => {
                if body.len() != 7 || body[0] > 127 {
                    return Err(MetadataError::Invalid);
                }
                self.inst_root = Some((body[0], f64::from(body[1] as i8)));
            }
            b"INST" if aiff => {
                if body.len() != 20 || body[0] > 127 {
                    return Err(MetadataError::Invalid);
                }
                self.inst_root = Some((body[0], f64::from(body[1] as i8)));
                self.sustain = Some((be16(body, 8)?, be16(body, 10)?, be16(body, 12)?));
            }
            b"MARK" if aiff => {
                let count = be16(body, 0)? as usize;
                if count > MAX_MARKERS {
                    return Err(MetadataError::Limit);
                }
                let mut at = 2;
                for _ in 0..count {
                    let id = be16(body, at)?;
                    let position = be32(body, at + 2)? as usize;
                    let n = *body.get(at + 6).ok_or(MetadataError::Truncated)? as usize;
                    let width = 7usize.checked_add(n).ok_or(MetadataError::Limit)?;
                    at = at
                        .checked_add(width + (width & 1))
                        .ok_or(MetadataError::Limit)?;
                    if at > body.len() {
                        return Err(MetadataError::Truncated);
                    }
                    if id == 0 || self.markers.insert(id, position).is_some() {
                        return Err(MetadataError::Invalid);
                    }
                }
                if at != body.len() {
                    return Err(MetadataError::Invalid);
                }
            }
            _ => {}
        }
        Ok(())
    }
    fn chunks(&mut self, mut bytes: &[u8], aiff: bool) -> Result<()> {
        while !bytes.is_empty() {
            if bytes.len() < 8 {
                return Err(MetadataError::Truncated);
            }
            let n = if aiff {
                be32(bytes, 4)?
            } else {
                le32(bytes, 4)?
            } as usize;
            let end = 8usize.checked_add(n).ok_or(MetadataError::Limit)?;
            let body = bytes.get(8..end).ok_or(MetadataError::Truncated)?;
            self.chunk(&bytes[..4], body, aiff)?;
            let padded = end.checked_add(n & 1).ok_or(MetadataError::Limit)?;
            bytes = bytes.get(padded..).ok_or(MetadataError::Truncated)?;
        }
        Ok(())
    }
    fn foreign(&mut self, bytes: &[u8], aiff: bool) -> Result<()> {
        if bytes.len() < 8 {
            return Err(MetadataError::Truncated);
        }
        if &bytes[..4] == if aiff { b"FORM" } else { b"RIFF" } {
            // Xiph keeps FORM/RIFF as a separate twelve-byte APPLICATION block.
            if bytes.len() != 12
                || if aiff {
                    !matches!(&bytes[8..12], b"AIFF" | b"AIFC")
                } else {
                    &bytes[8..12] != b"WAVE"
                }
            {
                return Err(MetadataError::Invalid);
            }
            return Ok(());
        }
        let id = &bytes[..4];
        // Xiph retains only data/SSND headers, with their original larger length.
        if matches!(id, b"data" | b"SSND") {
            return Ok(());
        }
        self.chunks(bytes, aiff)
    }
}

pub(crate) fn parse(bytes: &[u8], frames: usize) -> Result<SampleMetadata> {
    if frames == 0 {
        return Err(MetadataError::Invalid);
    }
    let mut state = State::default();
    let format = if bytes.starts_with(b"RIFF") {
        if bytes.get(8..12) != Some(b"WAVE") {
            return Err(MetadataError::Format);
        }
        state.chunks(&bytes[12..], false)?;
        Format::Wav
    } else if bytes.starts_with(b"FORM") {
        if !matches!(bytes.get(8..12), Some(b"AIFF" | b"AIFC")) {
            return Err(MetadataError::Format);
        }
        state.chunks(&bytes[12..], true)?;
        Format::Aiff
    } else if bytes.starts_with(b"fLaC") {
        let mut rest = &bytes[4..];
        let mut blocks = 0;
        loop {
            let header = rest.get(..4).ok_or(MetadataError::Truncated)?;
            blocks += 1;
            if blocks > MAX_CHUNKS {
                return Err(MetadataError::Limit);
            }
            let n = (usize::from(header[1]) << 16)
                | (usize::from(header[2]) << 8)
                | usize::from(header[3]);
            let body = rest.get(4..4 + n).ok_or(MetadataError::Truncated)?;
            if header[0] & 0x7f == 2 {
                if body.len() < 4 {
                    return Err(MetadataError::Truncated);
                }
                match &body[..4] {
                    b"riff" => state.foreign(&body[4..], false)?,
                    b"aiff" => state.foreign(&body[4..], true)?,
                    _ => {}
                }
            }
            rest = &rest[4 + n..];
            if header[0] & 0x80 != 0 {
                break;
            }
        }
        Format::Flac
    } else {
        return Err(MetadataError::Format);
    };
    if let Some((mode, begin, end)) = state.sustain {
        if matches!(mode, 1 | 2) {
            // AIFF markers fall between frames, so their end is already exclusive.
            let start = *state.markers.get(&begin).ok_or(MetadataError::Invalid)?;
            let end = *state.markers.get(&end).ok_or(MetadataError::Invalid)?;
            if start < end {
                state.looping = Some((
                    start,
                    end,
                    if mode == 2 {
                        LoopShape::PingPong
                    } else {
                        LoopShape::Wrap
                    },
                ));
            }
        }
    }
    let looping = state
        .looping
        .map(|(start, end, shape)| {
            if start >= end || end > frames {
                return Err(MetadataError::Invalid);
            }
            Ok(Loop {
                start,
                end,
                shape,
                mode: LoopMode::Continuous,
                passes: None,
            })
        })
        .transpose()?;
    let (base_note, fine_tune) = state.inst_root.or(state.smpl_root).unwrap_or((60, 0.));
    Ok(SampleMetadata {
        format,
        base_note,
        fine_tune,
        frames,
        looping,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn chunk(id: &[u8; 4], body: &[u8], aiff: bool) -> Vec<u8> {
        let mut bytes = id.to_vec();
        bytes.extend_from_slice(&if aiff {
            (body.len() as u32).to_be_bytes()
        } else {
            (body.len() as u32).to_le_bytes()
        });
        bytes.extend_from_slice(body);
        if body.len() % 2 != 0 {
            bytes.push(0);
        }
        bytes
    }
    fn wav(chunks: &[Vec<u8>]) -> Vec<u8> {
        let mut bytes = b"RIFF\0\0\0\0WAVE".to_vec();
        for c in chunks {
            bytes.extend_from_slice(c);
        }
        let size = (bytes.len() - 8) as u32;
        bytes[4..8].copy_from_slice(&size.to_le_bytes());
        bytes
    }
    fn sampler(kind: u32, start: u32, end: u32) -> Vec<u8> {
        let mut bytes = vec![0; 60];
        bytes[12..16].copy_from_slice(&61u32.to_le_bytes());
        bytes[16..20].copy_from_slice(&0x80000000u32.to_le_bytes());
        bytes[28..32].copy_from_slice(&1u32.to_le_bytes());
        bytes[40..44].copy_from_slice(&kind.to_le_bytes());
        bytes[44..48].copy_from_slice(&start.to_le_bytes());
        bytes[48..52].copy_from_slice(&end.to_le_bytes());
        bytes[56..60].copy_from_slice(&3u32.to_le_bytes());
        bytes
    }
    fn flac(application: &[Vec<u8>]) -> Vec<u8> {
        let mut bytes = b"fLaC".to_vec();
        for (i, body) in application.iter().enumerate() {
            let n = body.len();
            bytes.extend_from_slice(&[
                2 | if i + 1 == application.len() { 128 } else { 0 },
                (n >> 16) as u8,
                (n >> 8) as u8,
                n as u8,
            ]);
            bytes.extend_from_slice(body);
        }
        bytes
    }
    #[test]
    fn wav_inclusive_end_inst_precedence_and_native_loop_modes() {
        for (kind, shape) in [
            (0, LoopShape::Wrap),
            (1, LoopShape::PingPong),
            (2, LoopShape::Wrap),
        ] {
            let smpl = chunk(b"smpl", &sampler(kind, 4, 9), false);
            let inst = chunk(b"inst", &[72, (-7i8) as u8, 0, 0, 127, 0, 127], false);
            for chunks in [
                vec![smpl.clone(), inst.clone()],
                vec![inst.clone(), smpl.clone()],
            ] {
                let m = parse(&wav(&chunks), 16).unwrap();
                assert_eq!((m.base_note, m.fine_tune), (72, -7.));
                assert_eq!(
                    m.looping,
                    Some(Loop {
                        start: 4,
                        end: 10,
                        shape,
                        mode: LoopMode::Continuous,
                        passes: None
                    })
                );
                assert_eq!(m.playback().end, Some(16));
            }
        }
        let m = parse(&wav(&[chunk(b"smpl", &sampler(3, 4, 9), false)]), 16).unwrap();
        assert!(m.looping.is_none());
        assert_eq!((m.base_note, m.fine_tune), (61, 50.));
        assert!(parse(&wav(&[chunk(b"smpl", &sampler(0, 4, 99), false)]), 16).is_err());
    }
    #[test]
    fn aiff_markers_are_exclusive_and_foreign_flac_metadata_matches() {
        let mut inst = vec![0; 20];
        inst[0] = 68;
        inst[1] = (-12i8) as u8;
        inst[8..10].copy_from_slice(&2u16.to_be_bytes());
        inst[10..12].copy_from_slice(&1u16.to_be_bytes());
        inst[12..14].copy_from_slice(&2u16.to_be_bytes());
        let mut marks = 2u16.to_be_bytes().to_vec();
        for (id, pos, name) in [
            (1u16, 3u32, b"start".as_slice()),
            (2, 11, b"end".as_slice()),
        ] {
            marks.extend_from_slice(&id.to_be_bytes());
            marks.extend_from_slice(&pos.to_be_bytes());
            marks.push(name.len() as u8);
            marks.extend_from_slice(name);
            if (name.len() + 1) % 2 != 0 {
                marks.push(0);
            }
        }
        let inst = chunk(b"INST", &inst, true);
        let marks = chunk(b"MARK", &marks, true);
        let mut aiff = b"FORM\0\0\0\0AIFF".to_vec();
        aiff.extend_from_slice(&inst);
        aiff.extend_from_slice(&marks);
        let size = (aiff.len() - 8) as u32;
        aiff[4..8].copy_from_slice(&size.to_be_bytes());
        let m = parse(&aiff, 16).unwrap();
        assert_eq!((m.base_note, m.fine_tune), (68, -12.));
        assert_eq!(m.looping.unwrap().end, 11);
        let applications = [aiff[..12].to_vec(), inst, marks]
            .into_iter()
            .map(|b| [b"aiff".as_slice(), &b].concat())
            .collect::<Vec<_>>();
        let f = parse(&flac(&applications), 16).unwrap();
        assert_eq!(
            (f.base_note, f.fine_tune, f.looping),
            (m.base_note, m.fine_tune, m.looping)
        );
        assert_eq!(f.format, Format::Flac);
        let riff = [
            b"riff".as_slice(),
            chunk(b"smpl", &sampler(1, 2, 7), false).as_slice(),
        ]
        .concat();
        let f = parse(&flac(&[riff]), 16).unwrap();
        assert_eq!((f.looping.unwrap().start, f.looping.unwrap().end), (2, 8));
    }
    #[test]
    fn malformed_and_unknown_vendor_metadata_fail_before_revision_publication() {
        let mut unknown = sampler(0, 2, 7);
        unknown[32..36].copy_from_slice(&4u32.to_le_bytes());
        unknown.extend_from_slice(&[1, 2, 3, 4]);
        assert_eq!(
            parse(&wav(&[chunk(b"smpl", &unknown, false)]), 16),
            Err(MetadataError::UnsupportedVendorSamplerData)
        );
        for n in 0..36 {
            assert!(parse(&wav(&[chunk(b"smpl", &vec![0; n], false)]), 16).is_err());
        }
        let encoded = wav(&[chunk(b"smpl", &sampler(0, 2, 7), false)]);
        for n in (0..12).chain(13..encoded.len()) {
            assert!(parse(&encoded[..n], 16).is_err());
        }
        assert_eq!(parse(b"not an audio file", 16), Err(MetadataError::Format));
        assert_eq!(parse(&encoded, 0), Err(MetadataError::Invalid));
        let mut bad = encoded.clone();
        bad[16..20].copy_from_slice(&u32::MAX.to_le_bytes());
        assert_eq!(parse(&bad, 16), Err(MetadataError::Truncated));
    }
}
