//! The RIFF `sfbk` reader (SoundFont 2.01 §4–7; 2.04; SF3): the `INFO`
//! name and version, where `sdta`'s `smpl` and `sm24` data lie, and every
//! `pdta` record. Every length and index is checked against the file; a
//! file that breaks its own bounds is an [`Error`], never a panic. Only
//! `INFO` and `pdta` are read into memory; sample data stays on disk.

use std::io::{self, Read, Seek, SeekFrom};
use std::path::Path;

use crate::Error;

/// Sample type flags (§7.10 `sfSampleLink`; SF3 adds the Ogg flag).
pub(crate) const RIGHT: u16 = 0x0002;
pub(crate) const LEFT: u16 = 0x0004;
pub(crate) const OGG: u16 = 0x0010;
pub(crate) const ROM: u16 = 0x8000;

/// A parsed bank. Every record list keeps its terminal record (`EOP`,
/// `EOI`, `EOS` and the closing bag, modulator and generator), which bounds
/// the last real record's list.
#[derive(Debug)]
pub(crate) struct SoundFont {
    #[expect(dead_code, reason = "INAM and ifil, for a bank list that shows them")]
    pub name: String,
    #[expect(dead_code, reason = "INAM and ifil, for a bank list that shows them")]
    pub version: (u16, u16),
    /// Bytes of 16-bit sample words in the file; `None` without `smpl`.
    pub smpl: Option<Span>,
    pub sm24: Option<Span>,
    pub presets: Vec<PresetHeader>,
    pub pbag: Vec<Bag>,
    pub pmod: Vec<Modulator>,
    pub pgen: Vec<Generator>,
    pub inst: Vec<InstrumentHeader>,
    pub ibag: Vec<Bag>,
    pub imod: Vec<Modulator>,
    pub igen: Vec<Generator>,
    pub samples: Vec<SampleHeader>,
}

/// A byte range of the file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Span {
    pub offset: u64,
    pub len: u64,
}

/// `phdr`, 38 bytes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PresetHeader {
    pub name: String,
    pub program: u16,
    pub bank: u16,
    pub bag: u16,
}

/// `pbag`/`ibag`, 4 bytes: where a zone's generators and modulators start.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Bag {
    pub generator: u16,
    pub modulator: u16,
}

/// `pmod`/`imod`, 10 bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Modulator {
    pub source: u16,
    pub destination: u16,
    pub amount: i16,
    pub amount_source: u16,
    pub transform: u16,
}

/// `pgen`/`igen`, 4 bytes: the amount as stored (a signed word, an unsigned
/// word or a low/high byte range, by generator).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Generator {
    pub oper: u16,
    pub amount: u16,
}

/// `inst`, 22 bytes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct InstrumentHeader {
    pub name: String,
    pub bag: u16,
}

/// `shdr`, 46 bytes. Positions are sample words from the start of `smpl`;
/// `end` and `loop_end` are the first words after the sample and the loop.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SampleHeader {
    pub name: String,
    pub start: u32,
    pub end: u32,
    pub loop_start: u32,
    pub loop_end: u32,
    pub rate: u32,
    pub original_pitch: u8,
    pub pitch_correction: i8,
    pub link: u16,
    pub kind: u16,
}

/// One zone's generator and modulator records.
pub(crate) struct ZoneRecords<'a> {
    pub generators: &'a [Generator],
    pub modulators: &'a [Modulator],
    /// Its modulator indices broke the list's bounds (read as none).
    pub modulators_broken: bool,
}

/// Reads the bank from `r`; `path` names it in errors.
pub(crate) fn parse<R: Read + Seek>(r: &mut R, path: &Path) -> Result<SoundFont, Error> {
    let io = |source: io::Error| Error::Io {
        path: path.to_path_buf(),
        source,
    };
    let size = r.seek(SeekFrom::End(0)).map_err(io)?;
    if size < 12 {
        return Err(Error::NotSoundFont("shorter than a RIFF header"));
    }
    let head = read_at(r, 0, 12).map_err(io)?;
    if head.get(..4) != Some(b"RIFF".as_slice()) || head.get(8..12) != Some(b"sfbk".as_slice()) {
        return Err(Error::NotSoundFont("no RIFF sfbk header"));
    }
    let end = 8 + u64::from(u32_at(&head, 4));
    if end > size {
        return Err(malformed(
            "RIFF",
            format!("declares {end} bytes; the file has {size}"),
        ));
    }
    let (mut info, mut sdta, mut pdta) = (None, None, None);
    let mut pos = 12;
    while pos + 8 <= end {
        let header = read_at(r, pos, 8).map_err(io)?;
        let len = u64::from(u32_at(&header, 4));
        let body = pos + 8;
        let next = body + len;
        if next > end {
            return Err(malformed(
                "RIFF",
                format!("chunk {} at byte {pos} runs past the end", fourcc(&header)),
            ));
        }
        if header.get(..4) == Some(b"LIST".as_slice()) && len >= 4 {
            let kind = read_at(r, body, 4).map_err(io)?;
            match kind.as_slice() {
                b"INFO" => info = Some(read_at(r, body + 4, len - 4).map_err(io)?),
                b"sdta" => sdta = Some(sample_data(r, body + 4, next, path)?),
                b"pdta" => pdta = Some(read_at(r, body + 4, len - 4).map_err(io)?),
                _ => {}
            }
        }
        pos = next + (len & 1);
    }
    let (name, version) = info.as_deref().map_or_else(
        || Ok((String::new(), (2, 1))),
        |info| -> Result<_, Error> { read_info(info) },
    )?;
    let (smpl, sm24) = sdta.unwrap_or((None, None));
    let pdta = pdta.ok_or_else(|| malformed("RIFF", "no pdta list"))?;
    let chunks = sub_chunks(&pdta, "pdta")?;
    let find = |name: &'static str| {
        (chunks.iter())
            .find(|(id, _)| id.as_slice() == name.as_bytes())
            .map(|(_, data)| *data)
            .ok_or_else(|| malformed("pdta", format!("no {name} chunk")))
    };
    Ok(SoundFont {
        name,
        version,
        smpl,
        sm24,
        presets: records(find("phdr")?, 38, "phdr", |b| PresetHeader {
            name: name_of(b),
            program: u16_at(b, 20),
            bank: u16_at(b, 22),
            bag: u16_at(b, 24),
        })?,
        pbag: records(find("pbag")?, 4, "pbag", bag)?,
        pmod: records(find("pmod")?, 10, "pmod", modulator)?,
        pgen: records(find("pgen")?, 4, "pgen", generator)?,
        inst: records(find("inst")?, 22, "inst", |b| InstrumentHeader {
            name: name_of(b),
            bag: u16_at(b, 20),
        })?,
        ibag: records(find("ibag")?, 4, "ibag", bag)?,
        imod: records(find("imod")?, 10, "imod", modulator)?,
        igen: records(find("igen")?, 4, "igen", generator)?,
        samples: records(find("shdr")?, 46, "shdr", |b| SampleHeader {
            name: name_of(b),
            start: u32_at(b, 20),
            end: u32_at(b, 24),
            loop_start: u32_at(b, 28),
            loop_end: u32_at(b, 32),
            rate: u32_at(b, 36),
            original_pitch: b.get(40).copied().unwrap_or(60),
            pitch_correction: i8::from_le_bytes([b.get(41).copied().unwrap_or(0)]),
            link: u16_at(b, 42),
            kind: u16_at(b, 44),
        })?,
    })
}

/// The zones of a preset or instrument whose bags run `first..last`: each
/// bag's generators and modulators run to the next bag's. `what` names the
/// bag list in errors.
pub(crate) fn zones<'a>(
    (first, last): (u16, u16),
    bags: &[Bag],
    generators: &'a [Generator],
    modulators: &'a [Modulator],
    what: &'static str,
) -> Result<Vec<ZoneRecords<'a>>, Error> {
    let (first, last) = (usize::from(first), usize::from(last));
    if first > last {
        return Err(malformed(
            what,
            format!("bag indices fall: {first} > {last}"),
        ));
    }
    if last >= bags.len() {
        return Err(malformed(
            what,
            format!("bag range {first}..{last} outside {} bags", bags.len()),
        ));
    }
    (first..last)
        .map(|j| {
            let (Some(this), Some(next)) = (bags.get(j), bags.get(j + 1)) else {
                return Err(malformed(
                    what,
                    format!("bag {j} past the end of {} bags", bags.len()),
                ));
            };
            let span = |a: u16, b: u16| usize::from(a)..usize::from(b);
            let gens = span(this.generator, next.generator);
            let generators = generators.get(gens.clone()).ok_or_else(|| {
                malformed(
                    what,
                    format!("bag {j} generators {gens:?} outside {}", generators.len()),
                )
            })?;
            let found = modulators.get(span(this.modulator, next.modulator));
            Ok(ZoneRecords {
                generators,
                modulators: found.unwrap_or_default(),
                modulators_broken: found.is_none(),
            })
        })
        .collect()
}

/// The `smpl` and `sm24` sub-chunks of the `sdta` list at `pos..end`.
fn sample_data<R: Read + Seek>(
    r: &mut R,
    mut pos: u64,
    end: u64,
    path: &Path,
) -> Result<(Option<Span>, Option<Span>), Error> {
    let (mut smpl, mut sm24) = (None, None);
    while pos + 8 <= end {
        let header = read_at(r, pos, 8).map_err(|source| Error::Io {
            path: path.to_path_buf(),
            source,
        })?;
        let len = u64::from(u32_at(&header, 4));
        let offset = pos + 8;
        if len > end - offset {
            return Err(malformed(
                "sdta",
                format!("chunk {} at byte {pos} runs past the end", fourcc(&header)),
            ));
        }
        let span = Span { offset, len };
        match header.get(..4) {
            Some(b"smpl") => smpl = Some(span),
            Some(b"sm24") => sm24 = Some(span),
            _ => {}
        }
        pos += 8 + len + (len & 1);
    }
    Ok((smpl, sm24))
}

/// The bank name (`INAM`) and version (`ifil`), each optional.
fn read_info(info: &[u8]) -> Result<(String, (u16, u16)), Error> {
    let mut name = String::new();
    let mut version = (2, 1);
    for (id, data) in sub_chunks(info, "INFO")? {
        match &id {
            b"INAM" => name = text(data),
            b"ifil" if data.len() >= 4 => version = (u16_at(data, 0), u16_at(data, 2)),
            _ => {}
        }
    }
    Ok((name, version))
}

/// The sub-chunks of a list body, each `(id, data)`.
type SubChunk<'a> = ([u8; 4], &'a [u8]);

fn sub_chunks<'a>(body: &'a [u8], list: &'static str) -> Result<Vec<SubChunk<'a>>, Error> {
    let mut out = Vec::new();
    let mut pos = 0usize;
    while pos.saturating_add(8) <= body.len() {
        let id: [u8; 4] = at_n(body, pos);
        let len = usize::try_from(u32_at(body, pos + 4)).unwrap_or(usize::MAX);
        let start = pos + 8;
        let data = start
            .checked_add(len)
            .and_then(|end| body.get(start..end))
            .ok_or_else(|| {
                malformed(
                    list,
                    format!("{} at byte {pos} runs past the end", fourcc(&id)),
                )
            })?;
        out.push((id, data));
        pos = start + len + (len & 1);
    }
    Ok(out)
}

/// Fixed-size records, the terminal one included (so at least one).
fn records<T>(
    data: &[u8],
    size: usize,
    chunk: &'static str,
    read: impl Fn(&[u8]) -> T,
) -> Result<Vec<T>, Error> {
    if !data.len().is_multiple_of(size) {
        return Err(malformed(
            chunk,
            format!(
                "{} bytes is not a whole number of {size}-byte records",
                data.len()
            ),
        ));
    }
    if data.is_empty() {
        return Err(malformed(chunk, "no terminal record"));
    }
    Ok(data.chunks_exact(size).map(read).collect())
}

fn bag(b: &[u8]) -> Bag {
    Bag {
        generator: u16_at(b, 0),
        modulator: u16_at(b, 2),
    }
}

fn modulator(b: &[u8]) -> Modulator {
    Modulator {
        source: u16_at(b, 0),
        destination: u16_at(b, 2),
        amount: i16::from_le_bytes(at_n(b, 4)),
        amount_source: u16_at(b, 6),
        transform: u16_at(b, 8),
    }
}

fn generator(b: &[u8]) -> Generator {
    Generator {
        oper: u16_at(b, 0),
        amount: u16_at(b, 2),
    }
}

/// A record's 20-byte name, up to its first NUL.
fn name_of(b: &[u8]) -> String {
    text(b.get(..20).unwrap_or_default())
}

/// NUL-terminated text, trimmed.
fn text(b: &[u8]) -> String {
    let end = b.iter().position(|&c| c == 0).unwrap_or(b.len());
    String::from_utf8_lossy(b.get(..end).unwrap_or_default())
        .trim()
        .to_owned()
}

fn read_at<R: Read + Seek>(r: &mut R, pos: u64, len: u64) -> io::Result<Vec<u8>> {
    let len = usize::try_from(len)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "chunk too large"))?;
    r.seek(SeekFrom::Start(pos))?;
    let mut out = vec![0; len];
    r.read_exact(&mut out)?;
    Ok(out)
}

/// `N` bytes of `b` from `at`, zero where `b` is short.
fn at_n<const N: usize>(b: &[u8], at: usize) -> [u8; N] {
    at.checked_add(N)
        .and_then(|end| b.get(at..end))
        .and_then(|s| s.try_into().ok())
        .unwrap_or([0; N])
}

fn u16_at(b: &[u8], at: usize) -> u16 {
    u16::from_le_bytes(at_n(b, at))
}

fn u32_at(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(at_n(b, at))
}

fn fourcc(b: &[u8]) -> String {
    String::from_utf8_lossy(b.get(..4).unwrap_or_default()).into_owned()
}

pub(crate) fn malformed(chunk: &'static str, detail: impl ToString) -> Error {
    Error::Malformed {
        chunk,
        detail: detail.to_string(),
    }
}
