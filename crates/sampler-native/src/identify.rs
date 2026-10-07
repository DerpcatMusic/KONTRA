//! `identify-kontakt`: which sample of a Kontakt instrument sounded for each note
//! of a rendered probe grid. Candidate samples are read from the library in
//! memory only (never written out), pitched to the played key and matched by
//! normalised cross-correlation of their first 100 ms against each render,
//! greedily so that mic layers summed in one render are found one by one.
//!
//! `identify-kontakt INPUT.nki GRID.json OUT.json NAME=RENDER.wav[@LEAD_S] ...`
//! NAME=RENDER.wav*SCALE@LEAD: SCALE compresses the grid's timeline (default 1), LEAD defaults to the first sound.
//! GRID.json is `{"notes":[{"key":60,"vel":100,"t":2.0},...]}` (note-on seconds in the MIDI file).

use realfft::{RealFftPlanner, RealToComplex, ComplexToReal, num_complex::Complex};
use sampler_ir as ir;
use serde_json::{Value, json};
use std::{collections::HashMap, io, path::Path, sync::Arc};

const RATE: f64 = 24_000.0; // matching rate (renders are decimated 2:1 from 48 kHz)
const WIN: usize = 2_400; // 100 ms onset window
const SEG: usize = 12_000; // 0.5 s of render searched per note
const FFT: usize = 16_384;
const BEFORE: f64 = 0.15; // search starts this long before the expected onset
const MIN_NCC: f32 = 0.2;
const PICKS: usize = 4;

fn fail<T>(m: impl ToString) -> io::Result<T> {
    Err(io::Error::other(m.to_string()))
}

struct Candidate {
    asset: usize,
    window: Vec<f32>,
    norm: f32,
    spectrum: Vec<Complex<f32>>,
    window_at: usize, // where in the resampled sample the onset window starts
}

fn cents(p: ir::Pitch) -> f64 {
    match p {
        ir::Pitch::Cents(c) => c,
        ir::Pitch::Semitones(s) => s * 100.0,
        ir::Pitch::Ratio(r) => 1200.0 * r.log2(),
    }
}

/// Semitones a zone shifts its sample to sound at `key`.
fn shift(zone: &ir::Zone, group_tune: ir::Pitch, key: u8) -> f64 {
    let tracked = match zone.pitch {
        ir::KeyTracking::Tracked { root } => f64::from(key) - f64::from(root),
        ir::KeyTracking::Scaled { root, cents_per_key } => {
            (f64::from(key) - f64::from(root)) * f64::from(cents_per_key) / 100.0
        }
        ir::KeyTracking::Fixed => 0.0,
    };
    tracked + (cents(zone.tune) + cents(group_tune)) / 100.0
}

/// Mono, `RATE`, pitched by `semitones`, from a sample's first frames.
fn resample(frames: &[[f32; 2]], rate: u32, semitones: f64, out_len: usize) -> Vec<f32> {
    let step = 2f64.powf(semitones / 12.0) * f64::from(rate) / RATE;
    (0..out_len)
        .map(|n| {
            let p = n as f64 * step;
            let i = p as usize;
            if i + 1 >= frames.len() {
                return 0.0;
            }
            let f = (p - i as f64) as f32;
            let a = (frames[i][0] + frames[i][1]) * 0.5;
            let b = (frames[i + 1][0] + frames[i + 1][1]) * 0.5;
            a + (b - a) * f
        })
        .collect()
}

struct Fft {
    fwd: Arc<dyn RealToComplex<f32>>,
    inv: Arc<dyn ComplexToReal<f32>>,
}

impl Fft {
    fn spectrum(&self, x: &[f32]) -> Vec<Complex<f32>> {
        let mut input = vec![0f32; FFT];
        input[..x.len()].copy_from_slice(x);
        let mut out = self.fwd.make_output_vec();
        self.fwd.process(&mut input, &mut out).expect("fft");
        out
    }
}

/// First moment (seconds) the stereo render rises above its pre-note level and -80 dBFS, searching from just before `note_on`.
fn resid_onset(mono: &[f32], note_on: f64) -> Option<f64> {
    let a = ((note_on - 0.1) * RATE).max(0.0) as usize;
    let pre = mono.get(a..a + 1_200)?.iter().fold(0f32, |m, x| m.max(x.abs()));
    let thr = (pre * 2.0).max(3e-4);
    let end = (a + 12_000).min(mono.len());
    (a + 1_200..end).find(|&i| mono[i].abs() > thr).map(|i| i as f64 / RATE)
}

/// Best normalised correlation of `window` (norm `wn`) inside `hay`, and its start position.
fn best_position(window: &[f32], wn: f32, hay: &[f32]) -> Option<(f32, usize)> {
    let n = (hay.len() + window.len()).next_power_of_two();
    let mut planner = RealFftPlanner::<f32>::new();
    let (fwd, inv) = (planner.plan_fft_forward(n), planner.plan_fft_inverse(n));
    let (mut a, mut b) = (vec![0f32; n], vec![0f32; n]);
    a[..hay.len()].copy_from_slice(hay);
    b[..window.len()].copy_from_slice(window);
    let (mut fa, mut fb) = (fwd.make_output_vec(), fwd.make_output_vec());
    fwd.process(&mut a, &mut fa).ok()?;
    fwd.process(&mut b, &mut fb).ok()?;
    for (x, y) in fa.iter_mut().zip(&fb) {
        *x *= y.conj();
    }
    fa[0].im = 0.0;
    let last = fa.len() - 1;
    fa[last].im = 0.0;
    let mut t = inv.make_output_vec();
    inv.process(&mut fa, &mut t).ok()?;
    let mut prefix = vec![0f64; hay.len() + 1];
    for (i, x) in hay.iter().enumerate() {
        prefix[i + 1] = prefix[i] + f64::from(x * x);
    }
    (0..=hay.len().saturating_sub(window.len()))
        .filter_map(|p| {
            let e = (prefix[p + window.len()] - prefix[p]).sqrt() as f32;
            (e > 1e-5).then(|| (t[p] / n as f32 / (e * wn), p))
        })
        .max_by(|x, y| x.0.total_cmp(&y.0))
}

/// Pre-emphasis by repeated first differences (+6 dB/octave each): sustained tones of one pitch correlate at any phase
/// through their harmonics, the noise and upper partials are what tell recordings apart.
fn emph(x: &mut [f32]) {
    let order = std::env::var("KONTRA_ID_EMPH").ok().and_then(|v| v.parse().ok()).unwrap_or(2);
    for _ in 0..order {
        for i in (1..x.len()).rev() {
            x[i] -= x[i - 1];
        }
        if let Some(f) = x.first_mut() {
            *f = 0.0;
        }
    }
}

fn db(x: f64) -> f64 {
    20.0 * x.max(1e-12).log10()
}

struct Render {
    name: String,
    lead: f64,
    scale: f64, // render timeline = first + (t - first) * scale (v1 caps a render at 60 s)
    mono: Vec<f32>,   // at RATE
    stereo: Vec<[f32; 2]>, // at 48 kHz
}

/// A 48 kHz stereo WAV (float32, PCM16 or PCM24, also WAVE_FORMAT_EXTENSIBLE) as frames.
fn read_wav(path: &Path) -> io::Result<Vec<[f32; 2]>> {
    let b = std::fs::read(path)?;
    let le16 = |i: usize| u16::from_le_bytes([b[i], b[i + 1]]);
    let le32 = |i: usize| u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]]);
    if b.len() < 12 || &b[..4] != b"RIFF" {
        return fail(format!("{}: not a WAV", path.display()));
    }
    let (mut at, mut fmt, mut data) = (12, None, None);
    while at + 8 <= b.len() {
        let (id, size) = (&b[at..at + 4], le32(at + 4) as usize);
        let body = at + 8;
        let end = if size == 0xffff_ffff || body + size > b.len() { b.len() } else { body + size };
        match id {
            b"fmt " => {
                let tag = if le16(body) == 0xfffe && size >= 26 { le16(body + 24) } else { le16(body) };
                fmt = Some((tag, le16(body + 2), le32(body + 4), le16(body + 14)))
            }
            b"data" => { data = Some(&b[body..end]); break }
            _ => {}
        }
        at = end + (size & 1);
    }
    let (Some((tag, channels, rate, bits)), Some(data)) = (fmt, data) else { return fail("WAV without fmt or data") };
    if rate != 48_000 || channels != 2 {
        return fail(format!("{}: expected 48 kHz stereo, got {rate} Hz x {channels}", path.display()));
    }
    let width = usize::from(bits / 8);
    let sample = |i: usize| -> f32 {
        let s = &data[i * width..];
        match (tag, bits) {
            (3, 32) => f32::from_le_bytes([s[0], s[1], s[2], s[3]]),
            (1, 16) => f32::from(i16::from_le_bytes([s[0], s[1]])) / 32768.0,
            (1, 24) => (i32::from_le_bytes([0, s[0], s[1], s[2]]) >> 8) as f32 / 8_388_608.0,
            _ => 0.0,
        }
    };
    if !matches!((tag, bits), (3, 32) | (1, 16) | (1, 24)) {
        return fail(format!("{}: unsupported WAV format {tag}/{bits}", path.display()));
    }
    Ok((0..data.len() / (2 * width)).map(|n| [sample(2 * n), sample(2 * n + 1)]).collect())
}

fn load_render(spec: &str) -> io::Result<Render> {
    let (name, rest) = spec.split_once('=').ok_or_else(|| io::Error::other("NAME=WAV[@LEAD]"))?;
    let (rest, scale) = match rest.rsplit_once('*') {
        Some((p, k)) => (p, k.parse::<f64>().map_err(io::Error::other)?),
        None => (rest, 1.0),
    };
    // No @LEAD: aligned from the first sound (above -60 dBFS), see `run`.
    let (path, lead) = match rest.rsplit_once('@') {
        Some((p, l)) => (p, l.parse::<f64>().map_err(io::Error::other)?),
        None => (rest, f64::NAN),
    };
    let stereo = read_wav(Path::new(path))?;
    let mono = stereo
        .chunks_exact(2)
        .map(|p| (p[0][0] + p[0][1] + p[1][0] + p[1][1]) * 0.25)
        .collect();
    Ok(Render { name: name.into(), lead, scale, mono, stereo })
}

pub fn run(instrument: &Path, grid: &Path, output: &Path, renders: &[String]) -> io::Result<()> {
    let grid: Value = serde_json::from_slice(&std::fs::read(grid)?)?;
    let notes: Vec<(u8, u8, f64)> = grid["notes"]
        .as_array()
        .ok_or_else(|| io::Error::other("grid.notes"))?
        .iter()
        .map(|n| (n["key"].as_u64().unwrap_or(0) as u8, n["vel"].as_u64().unwrap_or(0) as u8, n["t"].as_f64().unwrap_or(0.0)))
        .collect();
    let mut renders = renders.iter().map(|r| load_render(r)).collect::<io::Result<Vec<_>>>()?;
    let first = notes.iter().map(|n| n.2).fold(f64::MAX, f64::min);
    for r in &mut renders {
        if r.lead.is_nan() {
            let at = r.mono.iter().position(|x| x.abs() > 0.001).unwrap_or(0);
            r.lead = at as f64 / RATE - first;
            eprintln!("{}: first sound at {:.3} s, lead {:.3} s", r.name, at as f64 / RATE, r.lead);
        }
    }
    let mut keys: Vec<u8> = notes.iter().map(|n| n.0).collect();
    keys.sort_unstable();
    keys.dedup();

    let mut kontakt = sampler_kontakt::read(instrument).map_err(|e| io::Error::other(e.to_string()))?;
    let kept = kontakt
        .instrument
        .retain_zones(|z| keys.iter().any(|&k| z.keys.low <= k && k <= z.keys.high));
    let ins = &kontakt.instrument;
    let label = |asset: usize| -> String {
        let p = &kontakt.locations[kept[asset]];
        p.to_string_lossy().rsplit(['/', '\\']).next().unwrap_or("").to_string()
    };
    eprintln!("{}: {} zones over {} samples at keys {keys:?}", ins.name, ins.zones.len(), kept.len());

    // First 1.5 s of every kept asset, in memory only.
    let mut heads: Vec<(u32, Vec<[f32; 2]>)> = Vec::with_capacity(kept.len());
    let mut tails: Vec<Vec<[f32; 2]>> = Vec::with_capacity(kept.len());
    for &orig in &kept {
        let source = kontakt.samples.source(&kontakt.locations[orig]).map_err(|e| io::Error::other(e.to_string()))?;
        let mut reader = sampler_kontakt::SampleReader::open(&source)?;
        let rate = reader.rate();
        let want = (reader.frames()).min((rate as usize) * 4);
        let mut buf = vec![[0f32; 2]; want];
        reader.read(0, &mut buf)?;
        let total = reader.frames();
        let from = total.saturating_sub((rate as usize) * 4);
        let mut tail = vec![[0f32; 2]; total - from];
        reader.read(from, &mut tail)?;
        tail.reverse();
        tails.push(tail);
        heads.push((rate, buf));
    }

    let mut planner = RealFftPlanner::<f32>::new();
    let fft = Fft { fwd: planner.plan_fft_forward(FFT), inv: planner.plan_fft_inverse(FFT) };

    // Candidates per key: one per distinct (asset, pitch shift).
    let mut per_key: HashMap<u8, Vec<Candidate>> = HashMap::new();
    for &key in &keys {
        let mut seen: HashMap<(usize, i64), ()> = HashMap::new();
        let mut list = Vec::new();
        for zone in &ins.zones {
            if !(zone.keys.low <= key && key <= zone.keys.high) {
                continue;
            }
            let group_tune = zone.group.map_or(ir::Pitch::NONE, |g| ins.groups[g.0].tune);
            let semis = shift(zone, group_tune, key);
            let id = (zone.asset.0, (semis * 1000.0).round() as i64);
            if seen.insert(id, ()).is_some() {
                continue;
            }
            let (rate, frames) = &heads[zone.asset.0];
            let long = resample(frames, *rate, semis, (2.0 * RATE) as usize);
            let peak = long.iter().take(RATE as usize).fold(0f32, |m, x| m.max(x.abs()));
            let Some(on) = long.iter().position(|x| x.abs() > peak * 0.003 && peak > 0.0) else { continue };
            let on = on.saturating_sub(24);
            let mut long = long;
            emph(&mut long);
            let mut window: Vec<f32> = long[on..].iter().copied().take(WIN).collect();
            window.resize(WIN, 0.0);
            let norm = window.iter().map(|x| x * x).sum::<f32>().sqrt();
            if norm < 1e-6 {
                continue;
            }
            let spectrum = fft.spectrum(&window);
            list.push(Candidate { asset: zone.asset.0, window, norm, spectrum, window_at: on });
        }
        per_key.insert(key, list);
    }

    let mut out_notes = Vec::new();
    for (index, &(key, vel, t)) in notes.iter().enumerate() {
        let candidates = &per_key[&key];
        let eligible: Vec<String> = {
            let mut v: Vec<String> = ins
                .zones
                .iter()
                .filter(|z| z.keys.low <= key && key <= z.keys.high && z.velocities.low <= vel && vel <= z.velocities.high)
                .map(|z| label(z.asset.0))
                .collect();
            v.sort();
            v.dedup();
            v
        };
        let zone_info = |asset: usize| -> Vec<Value> {
            ins.zones
                .iter()
                .filter(|z| z.asset.0 == asset && z.keys.low <= key && key <= z.keys.high)
                .map(|z| {
                    let take = z.selection.as_ref().map(|s| match s.take {
                        ir::Take::Index(i) => json!(i),
                        ir::Take::Probability { low, high } => json!([low, high]),
                    });
                    json!({
                        "group": z.group.map(|g| ins.groups[g.0].name.clone()),
                        "keys": [z.keys.low, z.keys.high],
                        "vel": [z.velocities.low, z.velocities.high],
                        "artic": z.articulation.map(|a| ins.articulations[a.0].name.clone()),
                        "take": take,
                        "trigger": format!("{:?}", z.trigger),
                        "start": format!("{:?}", z.playback.start),
                        "start_range": format!("{:?}", z.playback.start_range),
                        "reverse": z.playback.reverse,
                        "looping": format!("{:?}", z.playback.looping),
                    })
                })
                .collect()
        };
        let mut engines = serde_json::Map::new();
        for r in &renders {
            let start = ((first + (t - first) * r.scale + r.lead - BEFORE) * RATE).max(0.0) as usize;
            let mut resid: Vec<f32> = r.mono.get(start.min(r.mono.len())..).unwrap_or(&[]).iter().copied().take(SEG).collect();
            resid.resize(SEG, 0.0);
            emph(&mut resid);
            // Level: onset (first frame within 30 dB of the segment peak) plus 0.4 s, stereo power.
            let s48 = start * 2;
            let seg = r.stereo.get(s48.min(r.stereo.len())..(s48 + SEG * 2).min(r.stereo.len())).unwrap_or(&[]);
            let peak = seg.iter().flatten().fold(0f32, |m, x| m.max(x.abs()));
            let on = seg.iter().position(|f| f[0].abs().max(f[1].abs()) > peak * 0.03).unwrap_or(0);
            let body = &seg[on..(on + 19_200).min(seg.len())];
            let power = body.iter().map(|f| f64::from(f[0] * f[0] + f[1] * f[1])).sum::<f64>() / body.len().max(1) as f64;
            let mut matches = Vec::new();
            for _ in 0..PICKS {
                // Local energy of the residual per window position.
                let mut prefix = vec![0f64; SEG + 1];
                for (i, x) in resid.iter().enumerate() {
                    prefix[i + 1] = prefix[i] + f64::from(x * x);
                }
                let xs = fft.spectrum(&resid);
                let mut best: Option<(f32, usize, usize, f32)> = None; // ncc, candidate, lag, dot
                let mut scratch = fft.inv.make_input_vec();
                let mut time = fft.inv.make_output_vec();
                for (ci, c) in candidates.iter().enumerate() {
                    for ((s, a), b) in scratch.iter_mut().zip(&xs).zip(&c.spectrum) {
                        *s = a * b.conj();
                    }
                    scratch[0].im = 0.0;
                    let last = scratch.len() - 1;
                    scratch[last].im = 0.0;
                    fft.inv.process(&mut scratch, &mut time).expect("ifft");
                    for lag in 0..=(SEG - WIN) {
                        let dot = time[lag] / FFT as f32;
                        let e = (prefix[lag + WIN] - prefix[lag]).sqrt() as f32;
                        if e < 1e-5 {
                            continue;
                        }
                        let ncc = dot / (e * c.norm);
                        if best.is_none_or(|b| ncc > b.0) {
                            best = Some((ncc, ci, lag, dot));
                        }
                    }
                }
                let Some((ncc, ci, lag, dot)) = best else { break };
                if ncc < MIN_NCC {
                    break;
                }
                let c = &candidates[ci];
                let gain = dot / (c.norm * c.norm);
                for (i, w) in c.window.iter().enumerate() {
                    resid[lag + i] -= gain * w;
                }
                matches.push(json!({
                    "asset": label(c.asset),
                    "asset_index": c.asset,
                    "ncc": (ncc * 1000.0).round() / 1000.0,
                    "gain_db": (db(f64::from(gain)) * 10.0).round() / 10.0,
                    "lag_ms": ((lag as f64 / RATE - BEFORE) * 1000.0).round(),
                    "zones": zone_info(c.asset),
                }));
            }
            // Where in the sample, and which way: the render's 100 ms at the top match's lag (or, with no match, at the render's
            // own onset) against the whole first/last 4 s of the candidate sample, forward and reversed.
            let note_on = first + (t - first) * r.scale + r.lead;
            let mono_at = |from_s: f64| -> Vec<f32> {
                let a = (from_s * RATE).max(0.0) as usize;
                let mut v: Vec<f32> = r.mono.get(a.min(r.mono.len())..).unwrap_or(&[]).iter().copied().take(WIN).collect();
                v.resize(WIN, 0.0);
                emph(&mut v);
                v
            };
            let mut playback = Value::Null;
            let top = matches.first().and_then(|m| m["asset_index"].as_u64()).map(|a| a as usize);
            let (probe_assets, probe_window, base_lag): (Vec<usize>, Vec<f32>, Option<usize>) = match (top, matches.first()) {
                (Some(a), Some(m)) => {
                    let lag = ((m["lag_ms"].as_f64().unwrap_or(0.0) / 1000.0 + BEFORE) * RATE) as usize;
                    (vec![a], mono_at(note_on - BEFORE + lag as f64 / RATE), Some(lag))
                }
                _ => {
                    // No onset-window match: sample start moved, reversed, or another sample. Search velocity-eligible samples.
                    let on = resid_onset(&r.mono, note_on);
                    let mut assets: Vec<usize> = ins.zones.iter()
                        .filter(|z| z.keys.low <= key && key <= z.keys.high && z.velocities.low <= vel && vel <= z.velocities.high)
                        .map(|z| z.asset.0).collect();
                    assets.sort_unstable();
                    assets.dedup();
                    assets.truncate(300);
                    (assets, mono_at(on.map_or(note_on, |o| o - 24.0 / RATE)), None)
                }
            };
            let wn = probe_window.iter().map(|x| x * x).sum::<f32>().sqrt();
            if wn > 1e-6 {
                let mut best: Option<(f32, usize, bool, f64, f64)> = None; // ncc, asset, reversed, position, step
                for &asset in &probe_assets {
                    let zone = ins.zones.iter().find(|z| z.asset.0 == asset && z.keys.low <= key && key <= z.keys.high);
                    let Some(zone) = zone else { continue };
                    let group_tune = zone.group.map_or(ir::Pitch::NONE, |g| ins.groups[g.0].tune);
                    let semis = shift(zone, group_tune, key);
                    let (rate, head) = &heads[asset];
                    let step = 2f64.powf(semis / 12.0) * f64::from(*rate) / RATE;
                    for (reversed, frames) in [(false, head), (true, &tails[asset])] {
                        let mut hay = resample(frames, *rate, semis, (4.0 * RATE) as usize);
                        emph(&mut hay);
                        if let Some((ncc, pos)) = best_position(&probe_window, wn, &hay) {
                            if best.is_none_or(|b| ncc > b.0) {
                                best = Some((ncc, asset, reversed, pos as f64, step));
                            }
                        }
                    }
                }
                if let Some((ncc, asset, reversed, pos, step)) = best {
                    // Offset against the candidate's own onset window (the one `matches` used), forward only.
                    let (rate, head) = &heads[asset];
                    let _ = (rate, head);
                    let ref_pos = candidates.iter().find(|c| c.asset == asset).map_or(0.0, |c| c.window_at as f64);
                    let offset = if reversed { pos * step } else { (pos - ref_pos) * step };
                    playback = json!({
                        "asset": label(asset), "ncc": (ncc * 1000.0).round() / 1000.0, "reversed": reversed,
                        "start_offset_frames": offset.round(), "from_match": base_lag.is_some(),
                    });
                }
            }
            // Attack shape: 2.5 ms bins over the first 50 ms from the onset, dB re the 0.1-0.4 s body.
            let onset = resid_onset(&r.mono, note_on);
            let env: Vec<f64> = match onset {
                Some(o) => {
                    let a = (o * 48_000.0) as usize;
                    let rmsdb = |from: usize, len: usize| {
                        let s = r.stereo.get(from.min(r.stereo.len())..(from + len).min(r.stereo.len())).unwrap_or(&[]);
                        db((s.iter().map(|f| f64::from(f[0] * f[0] + f[1] * f[1])).sum::<f64>() / s.len().max(1) as f64).sqrt())
                    };
                    let body = rmsdb(a + 4_800, 14_400);
                    (0..20).map(|b| ((rmsdb(a + b * 120, 120) - body) * 10.0).round() / 10.0).collect()
                }
                None => vec![],
            };
            engines.insert(
                r.name.clone(),
                json!({ "rms_db": (db(power.sqrt()) * 100.0).round() / 100.0, "peak_db": (db(f64::from(peak)) * 100.0).round() / 100.0,
                        "onset_ms": onset.map(|o| ((o - note_on) * 1000.0).round()), "env_db": env, "playback": playback, "matches": matches }),
            );
        }
        out_notes.push(json!({ "i": index, "key": key, "vel": vel, "t": t, "eligible": eligible, "engines": engines }));
        eprintln!("note {index}/{} key {key} vel {vel}", notes.len());
    }
    let report = json!({ "instrument": ins.name, "candidates": per_key.iter().map(|(k, v)| (k.to_string(), v.len())).collect::<HashMap<_, _>>(), "notes": out_notes });
    std::fs::write(output, serde_json::to_vec_pretty(&report)?)
}

/// `kontakt-keys INPUT.nki`: mapped key span, velocity splits and the keys with most zones, to choose probe keys.
pub fn keys(instrument: &Path) -> io::Result<()> {
    let k = sampler_kontakt::read(instrument).map_err(|e| io::Error::other(e.to_string()))?;
    let ins = &k.instrument;
    let attack: Vec<_> = ins.zones.iter().filter(|z| matches!(z.trigger, ir::Trigger::Attack)).collect();
    let (lo, hi) = attack.iter().fold((127u8, 0u8), |a, z| (a.0.min(z.keys.low), a.1.max(z.keys.high)));
    let mut vels: Vec<(u8, u8)> = attack.iter().map(|z| (z.velocities.low, z.velocities.high)).collect();
    vels.sort_unstable();
    vels.dedup();
    let mut per_key = [0usize; 128];
    for z in &attack {
        for key in z.keys.low..=z.keys.high.min(127) {
            per_key[usize::from(key)] += 1;
        }
    }
    let mapped: Vec<usize> = (0..128).filter(|&i| per_key[i] > 0).collect();
    println!("{}: {} zones ({} attack), {} groups, {} articulations, keys {lo}..{hi}", ins.name, ins.zones.len(), attack.len(), ins.groups.len(), ins.articulations.len());
    println!("velocity ranges: {vels:?}");
    println!("zones/key at low {} mid {} high {}", per_key[mapped.first().copied().unwrap_or(0)], per_key[mapped[mapped.len() / 2]], per_key[mapped.last().copied().unwrap_or(0)]);
    println!("articulations: {:?}", ins.articulations.iter().map(|a| (&a.name, &a.switch_keys)).collect::<Vec<_>>());
    Ok(())
}
