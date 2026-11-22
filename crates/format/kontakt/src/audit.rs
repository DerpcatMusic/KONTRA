//! Load-side fundamentals audit: every saved zone value against what the
//! translator makes of it and against the sample file's own header. Reads
//! only; prints numbers. `KONTRA_AUDIT=<dir> cargo test -p
//! kontra-format-kontakt --release audit -- --ignored --nocapture`

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::io;
use std::path::{Path, PathBuf};

use ni_file::kontakt::objects::{LoopArray, ZoneList};

use crate::container;

/// Finding -> (instruments, zones, an example).
type Tally = BTreeMap<&'static str, (BTreeSet<PathBuf>, u64, String)>;
/// Sample `(file, offset)` -> `(channels, frames, sample_rate)`.
type Headers = HashMap<(PathBuf, u64), Option<(u8, u64, u32)>>;

fn walk(dir: &Path, found: &mut Vec<PathBuf>) -> io::Result<()> {
    let at = |path: &Path, err: io::Error| {
        io::Error::new(err.kind(), format!("{}: {err}", path.display()))
    };
    for entry in std::fs::read_dir(dir).map_err(|err| at(dir, err))? {
        let entry = entry.map_err(|err| at(dir, err))?;
        let path = entry.path();
        let kind = entry.file_type().map_err(|err| at(&path, err))?;
        if kind.is_dir() {
            walk(&path, found)?;
        } else if kind.is_file()
            && path
                .extension()
                .is_some_and(|x| x.eq_ignore_ascii_case("nki") || x.eq_ignore_ascii_case("nkm"))
        {
            found.push(path);
        }
    }
    Ok(())
}

#[test]
#[ignore = "reads a library corpus: KONTRA_AUDIT=<dir>"]
fn audit() -> io::Result<()> {
    let Some(dir) = std::env::var_os("KONTRA_AUDIT") else {
        return Ok(());
    };
    let mut files = Vec::new();
    walk(Path::new(&dir), &mut files)?;
    files.sort();
    let mut tally = Tally::new();
    let mut headers = Headers::new();
    let mut zones_seen = 0u64;
    for (n, path) in files.iter().enumerate() {
        zones_seen += instrument(path, &mut tally, &mut headers);
        eprintln!("{n}/{} {}", files.len(), path.display());
    }
    println!("{} instruments, {zones_seen} zones", files.len());
    for (finding, (instruments, zones, example)) in &tally {
        println!("{finding}\t{}\t{zones}\t{example}", instruments.len());
    }
    Ok(())
}

fn instrument(path: &Path, tally: &mut Tally, headers: &mut Headers) -> u64 {
    let mut hit = |finding: &'static str, example: String| {
        let t = tally
            .entry(finding)
            .or_insert_with(|| (BTreeSet::new(), 0, example));
        t.0.insert(path.to_path_buf());
        t.1 += 1;
    };
    let (Ok(ir), Ok((chunks, _, _))) = (crate::load(path), container::opened(path)) else {
        hit("load failed", String::new());
        return 0;
    };
    let Some(Ok(program)) = chunks.program() else {
        return 0;
    };
    let Some(Ok(list)) = program.0.find_first(0x34).map(ZoneList::try_from) else {
        return 0;
    };
    let by_id: HashMap<u32, (&kontra_ir::Group, &kontra_ir::Zone)> = ir
        .groups
        .iter()
        .flat_map(|g| g.zones.iter().map(move |z| (z.id, (g, z))))
        .collect();
    let mut n = 0;
    for (id, zone) in (0u32..).zip(list.zones()) {
        let at = format!("{} zone {id}", path.display());
        let Some(&(group, out)) = by_id.get(&id) else {
            hit("zone not translated", at);
            continue;
        };
        let Ok(z) = zone.params() else { continue };
        n += 1;
        let sample = &ir.samples[out.sample];
        let header = *headers
            .entry((sample.file.clone(), sample.offset))
            .or_insert_with(|| kontra_stream::header(sample).ok());
        let Some((channels, frames, rate)) = header else {
            hit("header unreadable", format!("{at}: {}", sample.name));
            continue;
        };
        if i64::from(z.num_frames) != i64::try_from(frames).unwrap_or(-1) {
            hit(
                "saved frames != header",
                format!("{at}: {} vs {frames}", z.num_frames),
            );
        }
        if sample.frames != frames {
            hit(
                "loaded frames != header",
                format!("{at}: {} vs {frames}", sample.frames),
            );
        }
        if u32::try_from(z.sample_rate).ok() != Some(rate) {
            hit(
                "saved rate != header",
                format!("{at}: {} vs {rate}", z.sample_rate),
            );
        }
        if sample.rate != rate {
            hit(
                "loaded rate != header",
                format!("{at}: {} vs {rate}", sample.rate),
            );
        }
        if z.num_channels != channels {
            hit(
                "saved channels != header",
                format!("{at}: {} vs {channels}", z.num_channels),
            );
        }
        if sample.channels != channels {
            hit(
                "loaded channels != header",
                format!("{at}: {} vs {channels}", sample.channels),
            );
        }
        if channels == 1 && ir.samples.iter().any(|s| s.channels == 2) {
            hit("mono sample beside stereo", String::new());
        }
        if out.end > frames {
            hit("end past the file", format!("{at}: {} > {frames}", out.end));
        }
        if z.sample_end > 0 {
            hit("saved end positive", format!("{at}: {}", z.sample_end));
        }
        if z.sample_start < 0 {
            hit("saved start negative", format!("{at}: {}", z.sample_start));
        }
        if z.sample_start_mod_range < 0 {
            hit(
                "saved S.Mod negative",
                format!("{at}: {}", z.sample_start_mod_range),
            );
        }
        if out.start + out.start_mod > out.end {
            hit(
                "S.Mod past the end",
                format!("{at}: {}+{} > {}", out.start, out.start_mod, out.end),
            );
        }
        if !(0..=127).contains(&z.root_key) {
            hit("root clamped", format!("{at}: {}", z.root_key));
        }
        if z.root_key != i16::try_from(z.root_note).unwrap_or(-1) {
            hit(
                "root key != sample root",
                format!("{at}: {} vs {}", z.root_key, z.root_note),
            );
        }
        if z.tuning != 0.0 {
            hit("sample tuning set", format!("{at}: {}", z.tuning));
        }
        if !(-1.0..=1.0).contains(&z.zone_pan) {
            hit("pan clamped", format!("{at}: {}", z.zone_pan));
        }
        if !z.zone_tune.is_finite() || z.zone_tune <= 0.0 {
            hit("tune ratio invalid", format!("{at}: {}", z.zone_tune));
        }
        if z.low_velocity < 1 {
            hit("low velocity 0 raised", format!("{at}: {}", z.low_velocity));
        }
        if z.high_key > 127 || z.low_key < 0 || z.high_velocity > 127 {
            hit(
                "range clamped",
                format!(
                    "{at}: {}..{} {}..{}",
                    z.low_key, z.high_key, z.low_velocity, z.high_velocity
                ),
            );
        }
        if group.stream.is_some() {
            hit("zone streamed (DFD)", String::new());
        } else {
            hit("zone in memory (sampler)", String::new());
        }
        let loops = zone.0.find_first(0x39).map(LoopArray::try_from);
        for l in loops.into_iter().flatten().flat_map(|l| l.items) {
            match l.mode {
                0 => continue,
                1 | 2 => {}
                m => {
                    hit("loop mode unknown, dropped", format!("{at}: mode {m}"));
                    continue;
                }
            }
            let (Ok(from), Ok(len)) = (u64::try_from(l.loop_start), u64::try_from(l.loop_length))
            else {
                hit(
                    "loop dropped (length/start)",
                    format!("{at}: {}+{}", l.loop_start, l.loop_length),
                );
                continue;
            };
            if len == 0 {
                hit(
                    "loop dropped (length/start)",
                    format!("{at}: {}+{}", l.loop_start, l.loop_length),
                );
                continue;
            }
            let to = from + len;
            if to > out.end {
                hit(
                    "loop end past zone end",
                    format!("{at}: {to} > {}", out.end),
                );
            }
            if to > frames {
                hit("loop end past the file", format!("{at}: {to} > {frames}"));
            }
            if from < out.start {
                hit(
                    "loop start before zone start",
                    format!("{at}: {from} < {}", out.start),
                );
            }
            if i64::from(l.x_fade_length) > i64::from(l.loop_length) {
                hit(
                    "crossfade longer than loop",
                    format!("{at}: {} > {}", l.x_fade_length, l.loop_length),
                );
            }
            if u64::try_from(l.x_fade_length).unwrap_or(0) > from {
                hit(
                    "crossfade longer than loop start",
                    format!("{at}: {} > {from}", l.x_fade_length),
                );
            }
            if l.x_fade_length < 0 {
                hit("crossfade negative", format!("{at}: {}", l.x_fade_length));
            }
            if l.loop_tuning != 0.0 {
                hit("loop tuning dropped", format!("{at}: {}", l.loop_tuning));
            }
            if l.loop_count < 0 {
                hit("loop count negative", format!("{at}: {}", l.loop_count));
            }
        }
    }
    n
}
