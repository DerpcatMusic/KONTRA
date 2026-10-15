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
    pub geometry_error: Option<MetadataError>,
    pub channels: Option<u16>,
    pub bits: Option<u16>,
    pub tempo: f64,
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
    UnsupportedFractionalLoop,
    UnsupportedConflictingLoops,
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
    allow_unknown_geometry: bool,
    geometry_error: Option<MetadataError>,
    acid_loop: Option<bool>,
    acid_root: Option<u8>,
    channels: Option<u16>,
    bits: Option<u16>,
    tempo: f64,
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
        if matches!(
            id,
            b"smpl" | b"inst" | b"INST" | b"MARK" | b"acid" | b"fmt " | b"COMM"
        ) {
            self.metadata = self
                .metadata
                .checked_add(body.len())
                .ok_or(MetadataError::Limit)?;
            if self.metadata > MAX_METADATA {
                return Err(MetadataError::Limit);
            }
        }
        match id {
            b"fmt " if !aiff => {
                if body.len() < 16 {
                    return Err(MetadataError::Truncated);
                }
                self.channels = Some(u16::from_le_bytes(body[2..4].try_into().unwrap()));
                self.bits = Some(u16::from_le_bytes(body[14..16].try_into().unwrap()));
            }
            b"COMM" if aiff => {
                if body.len() < 18 {
                    return Err(MetadataError::Truncated);
                }
                self.channels = Some(be16(body, 0)?);
                self.bits = Some(be16(body, 6)?);
            }
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
                    if !self.allow_unknown_geometry {
                        return Err(MetadataError::UnsupportedVendorSamplerData);
                    }
                    self.geometry_error = Some(MetadataError::UnsupportedVendorSamplerData);
                }
                if loops > 0 {
                    let kind = le32(body, 40)?;
                    if le32(body, 52)? != 0 {
                        if !self.allow_unknown_geometry {
                            return Err(MetadataError::UnsupportedFractionalLoop);
                        }
                        self.geometry_error = Some(MetadataError::UnsupportedFractionalLoop);
                    }
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
            b"acid" if !aiff => {
                if body.len() < 24 {
                    return Err(MetadataError::Truncated);
                }
                let flags = le32(body, 0)?;
                self.acid_loop = Some(flags & 1 == 0);
                let tempo = f32::from_le_bytes(body[20..24].try_into().unwrap());
                if !tempo.is_finite() || tempo < 0. {
                    return Err(MetadataError::Invalid);
                }
                self.tempo = f64::from(tempo);
                if flags & 2 != 0 {
                    let root = u16::from_le_bytes(body[4..6].try_into().unwrap());
                    if root > 127 {
                        return Err(MetadataError::Invalid);
                    }
                    self.acid_root = Some(root as u8);
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
        let end = (le32(bytes, 4)? as usize)
            .checked_add(8)
            .ok_or(MetadataError::Invalid)?;
        if end < 12 {
            return Err(MetadataError::Invalid);
        }
        let scope = bytes.get(12..end).ok_or(MetadataError::Truncated)?;
        state.chunks(scope, false)?;
        Format::Wav
    } else if bytes.starts_with(b"FORM") {
        if !matches!(bytes.get(8..12), Some(b"AIFF" | b"AIFC")) {
            return Err(MetadataError::Format);
        }
        let end = (be32(bytes, 4)? as usize)
            .checked_add(8)
            .ok_or(MetadataError::Invalid)?;
        if end < 12 {
            return Err(MetadataError::Invalid);
        }
        let scope = bytes.get(12..end).ok_or(MetadataError::Truncated)?;
        state.chunks(scope, true)?;
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
            if header[0] & 0x7f == 0 {
                if body.len() != 34 {
                    return Err(MetadataError::Invalid);
                }
                let packed = u64::from_be_bytes(body[10..18].try_into().unwrap());
                state.channels = Some(((packed >> 41) & 7) as u16 + 1);
                state.bits = Some(((packed >> 36) & 31) as u16 + 1);
            }
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
    finish(state, format, frames)
}

fn finish(mut state: State, format: Format, frames: usize) -> Result<SampleMetadata> {
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
    if format == Format::Flac && state.acid_loop == Some(true) {
        if state
            .looping
            .is_some_and(|l| l != (0, frames, LoopShape::Wrap))
        {
            if !state.allow_unknown_geometry {
                return Err(MetadataError::UnsupportedConflictingLoops);
            }
            state.geometry_error = Some(MetadataError::UnsupportedConflictingLoops);
        } else {
            state.looping = Some((0, frames, LoopShape::Wrap));
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
    let (base_note, fine_tune) = state
        .inst_root
        .or(state.smpl_root)
        .or(state.acid_root.map(|r| (r, 0.)))
        .unwrap_or((60, 0.));
    Ok(SampleMetadata {
        format,
        base_note,
        fine_tune,
        frames,
        looping,
        geometry_error: state.geometry_error,
        channels: state.channels,
        bits: state.bits,
        tempo: state.tempo,
    })
}

/// Read only bounded metadata and chunk headers; sample payloads are skipped.
/// The reader is the same origin-aware (possibly decrypted) view used by audio.
pub(crate) fn parse_reader(
    reader: &mut (impl std::io::Read + std::io::Seek),
    frames: usize,
    geometry_override: bool,
) -> Result<SampleMetadata> {
    use std::io::SeekFrom;
    if frames == 0 {
        return Err(MetadataError::Invalid);
    }
    let physical = reader
        .seek(SeekFrom::End(0))
        .map_err(|_| MetadataError::Truncated)?;
    reader
        .seek(SeekFrom::Start(0))
        .map_err(|_| MetadataError::Truncated)?;
    let mut header = [0u8; 12];
    reader
        .read_exact(&mut header)
        .map_err(|_| MetadataError::Truncated)?;
    let mut state = State {
        allow_unknown_geometry: geometry_override,
        ..Default::default()
    };
    let format;
    if header.starts_with(b"RIFF") || header.starts_with(b"FORM") {
        let aiff = header.starts_with(b"FORM");
        if if aiff {
            !matches!(&header[8..12], b"AIFF" | b"AIFC")
        } else {
            &header[8..12] != b"WAVE"
        } {
            return Err(MetadataError::Format);
        }
        let end = u64::from(if aiff {
            be32(&header, 4)?
        } else {
            le32(&header, 4)?
        }) + 8;
        if end < 12 || end > physical {
            return Err(MetadataError::Truncated);
        }
        let mut at = 12u64;
        while at < end {
            if end - at < 8 {
                return Err(MetadataError::Truncated);
            }
            reader
                .seek(SeekFrom::Start(at))
                .map_err(|_| MetadataError::Truncated)?;
            let mut h = [0u8; 8];
            reader
                .read_exact(&mut h)
                .map_err(|_| MetadataError::Truncated)?;
            let n = if aiff { be32(&h, 4)? } else { le32(&h, 4)? } as usize;
            let next = at
                .checked_add(8 + n as u64 + (n & 1) as u64)
                .ok_or(MetadataError::Limit)?;
            if next > end {
                return Err(MetadataError::Truncated);
            }
            if matches!(
                &h[..4],
                b"smpl" | b"inst" | b"INST" | b"MARK" | b"acid" | b"fmt " | b"COMM"
            ) {
                if n > MAX_METADATA.saturating_sub(state.metadata) {
                    return Err(MetadataError::Limit);
                }
                let mut body = vec![0; n];
                reader
                    .read_exact(&mut body)
                    .map_err(|_| MetadataError::Truncated)?;
                state.chunk(&h[..4], &body, aiff)?;
            } else {
                state.chunk(&h[..4], &[], aiff)?;
            }
            at = next;
        }
        format = if aiff { Format::Aiff } else { Format::Wav };
    } else if header.starts_with(b"fLaC") {
        let mut at = 4u64;
        for index in 0..MAX_CHUNKS {
            reader
                .seek(SeekFrom::Start(at))
                .map_err(|_| MetadataError::Truncated)?;
            let mut h = [0u8; 4];
            reader
                .read_exact(&mut h)
                .map_err(|_| MetadataError::Truncated)?;
            let n = (usize::from(h[1]) << 16) | (usize::from(h[2]) << 8) | usize::from(h[3]);
            let next = at + 4 + n as u64;
            if next > physical {
                return Err(MetadataError::Truncated);
            }
            if h[0] & 0x7f == 0 {
                if n != 34 {
                    return Err(MetadataError::Invalid);
                }
                let mut body = [0u8; 34];
                reader
                    .read_exact(&mut body)
                    .map_err(|_| MetadataError::Truncated)?;
                let packed = u64::from_be_bytes(body[10..18].try_into().unwrap());
                state.channels = Some(((packed >> 41) & 7) as u16 + 1);
                state.bits = Some(((packed >> 36) & 31) as u16 + 1);
            }
            if h[0] & 0x7f == 2 {
                if n < 4 {
                    return Err(MetadataError::Truncated);
                }
                let mut id = [0u8; 4];
                reader
                    .read_exact(&mut id)
                    .map_err(|_| MetadataError::Truncated)?;
                if matches!(&id, b"riff" | b"aiff") {
                    if n - 4 > MAX_METADATA.saturating_sub(state.metadata) {
                        return Err(MetadataError::Limit);
                    }
                    let mut body = vec![0; n - 4];
                    reader
                        .read_exact(&mut body)
                        .map_err(|_| MetadataError::Truncated)?;
                    state.foreign(&body, &id == b"aiff")?;
                }
            }
            at = next;
            if h[0] & 0x80 != 0 {
                break;
            }
            if index + 1 == MAX_CHUNKS {
                return Err(MetadataError::Limit);
            }
        }
        format = Format::Flac;
    } else {
        return Err(MetadataError::Format);
    }
    finish(state, format, frames)
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
    #[test]
    fn uvi_first_loop_and_ignored_play_count_do_not_invent_generic_wav_behavior() {
        let mut two = sampler(2, 2, 7);
        two[28..32].copy_from_slice(&2u32.to_le_bytes());
        two.extend_from_slice(&sampler(1, 30, 40)[36..60]);
        let metadata = parse(&wav(&[chunk(b"smpl", &two, false)]), 16).unwrap();
        assert_eq!(
            metadata.looping,
            Some(Loop {
                start: 2,
                end: 8,
                shape: LoopShape::Wrap,
                mode: LoopMode::Continuous,
                passes: None
            }),
            "UVI intentionally plays type2 forward and ignores later loops/play count"
        );
        two[52..56].copy_from_slice(&1u32.to_le_bytes());
        assert_eq!(
            parse(&wav(&[chunk(b"smpl", &two, false)]), 16),
            Err(MetadataError::UnsupportedFractionalLoop)
        );
    }
    #[test]
    fn declared_container_scope_excludes_appended_metadata_and_rejects_truncated_chunks() {
        let mut bytes = wav(&[]);
        bytes.extend_from_slice(&chunk(b"smpl", &sampler(0, 2, 7), false));
        assert!(
            parse(&bytes, 16).unwrap().looping.is_none(),
            "outside-RIFF bytes cannot supply a loop"
        );
        let mut valid = wav(&[chunk(b"smpl", &sampler(0, 2, 7), false)]);
        valid[4..8].copy_from_slice(&u32::MAX.to_le_bytes());
        assert_eq!(parse(&valid, 16), Err(MetadataError::Truncated));
        valid[4..8].copy_from_slice(&13u32.to_le_bytes());
        assert_eq!(parse(&valid, 16), Err(MetadataError::Truncated));
    }
    #[test]
    fn streamed_metadata_reader_skips_payload_and_matches_complete_input() {
        struct Reader {
            bytes: std::io::Cursor<Vec<u8>>,
            read: usize,
        }
        impl std::io::Read for Reader {
            fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
                let n = self.bytes.read(out)?;
                self.read += n;
                Ok(n)
            }
        }
        impl std::io::Seek for Reader {
            fn seek(&mut self, at: std::io::SeekFrom) -> std::io::Result<u64> {
                self.bytes.seek(at)
            }
        }
        let mut fmt = vec![0; 16];
        fmt[..2].copy_from_slice(&1u16.to_le_bytes());
        fmt[2..4].copy_from_slice(&2u16.to_le_bytes());
        fmt[14..16].copy_from_slice(&24u16.to_le_bytes());
        let bytes = wav(&[
            chunk(b"fmt ", &fmt, false),
            chunk(b"data", &vec![0; 4 << 20], false),
            chunk(b"smpl", &sampler(1, 4, 9), false),
        ]);
        let expected = parse(&bytes, 16).unwrap();
        let mut reader = Reader {
            bytes: std::io::Cursor::new(bytes),
            read: 0,
        };
        assert_eq!(parse_reader(&mut reader, 16, false).unwrap(), expected);
        assert!(
            reader.read < 256,
            "metadata must not read the audio payload: {}",
            reader.read
        );
        assert_eq!((expected.channels, expected.bits), (Some(2), Some(24)));
    }
    #[test]
    fn explicit_geometry_can_override_unknown_vendor_payload_without_guessing_it() {
        let mut smpl = sampler(0, 2, 7);
        smpl[32..36].copy_from_slice(&4u32.to_le_bytes());
        smpl.extend_from_slice(&[1, 2, 3, 4]);
        let bytes = wav(&[chunk(b"smpl", &smpl, false)]);
        assert_eq!(
            parse_reader(&mut std::io::Cursor::new(&bytes), 16, false),
            Err(MetadataError::UnsupportedVendorSamplerData)
        );
        let metadata = parse_reader(&mut std::io::Cursor::new(&bytes), 16, true).unwrap();
        assert_eq!(
            metadata.geometry_error,
            Some(MetadataError::UnsupportedVendorSamplerData)
        );
        assert_eq!((metadata.base_note, metadata.fine_tune), (61, 50.));
    }
    #[test]
    fn acid_whole_file_loop_is_flac_only_and_conflicts_are_explicit() {
        let mut acid = vec![0; 24];
        acid[..4].copy_from_slice(&2u32.to_le_bytes());
        acid[4..6].copy_from_slice(&69u16.to_le_bytes());
        acid[20..24].copy_from_slice(&120f32.to_le_bytes());
        let encoded = chunk(b"acid", &acid, false);
        let wav_meta = parse(&wav(&[encoded.clone()]), 16).unwrap();
        assert!(wav_meta.looping.is_none());
        let foreign = [b"riff".as_slice(), &encoded].concat();
        let meta = parse(&flac(&[foreign.clone()]), 16).unwrap();
        assert_eq!(
            meta.looping,
            Some(Loop {
                start: 0,
                end: 16,
                shape: LoopShape::Wrap,
                mode: LoopMode::Continuous,
                passes: None
            })
        );
        assert_eq!((meta.base_note, meta.tempo), (69, 120.));
        let looped = [
            b"riff".as_slice(),
            &chunk(b"smpl", &sampler(0, 2, 7), false),
        ]
        .concat();
        assert_eq!(
            parse(&flac(&[foreign, looped]), 16),
            Err(MetadataError::UnsupportedConflictingLoops)
        );
        acid[0] = 3;
        let oneshot = [b"riff".as_slice(), &chunk(b"acid", &acid, false)].concat();
        assert!(parse(&flac(&[oneshot]), 16).unwrap().looping.is_none());
    }
}
