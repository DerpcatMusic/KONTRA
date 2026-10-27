//! Engine prepare: an IR instrument becomes a [`Plan`] off the audio thread,
//! loading every resident sample's preload into the store (05-STREAM.md §1).

use std::collections::HashMap;
use std::io;
use std::ops::Range;
use std::sync::Arc;

use kontra_ir as ir;
use kontra_stream::{BlobKey, LoadRequest, Sample, SampleFmt, Source, Store};
use kontra_voice::{Ahdsr, Loop, Region};

use crate::{Bus, Control, Crossfade, Group, Plan, RoundRobin, RrCounter, Trigger, Zone};

/// Opens a sample's audio as the store reads it: decoded to packed PCM.
/// Called from several loader threads at once.
pub type Open = dyn Fn(&ir::Sample) -> io::Result<(Arc<dyn Source>, SampleFmt)> + Sync;

/// Builds the plan for `instrument` at the engine `rate`. Samples that fail to
/// open drop their zones and are reported; the script is left to the caller.
pub fn prepare(
    instrument: &ir::Instrument,
    rate: u32,
    store: &mut Store,
    open: &Open,
) -> (Plan, Vec<ir::Diagnostic>) {
    let mut diagnostics = Vec::new();
    let loaded = load_samples(instrument, store, open, &mut diagnostics);
    let mut zones = Vec::new();
    let mut plan_switches: Option<(u8, u8)> = None;
    let mut polyphony = Vec::new();
    for (index, group) in instrument.groups.iter().enumerate() {
        let group_id = u32::try_from(index).unwrap_or(u32::MAX);
        if let Some(voices) = group.polyphony {
            polyphony.push((group_id, voices));
        }
        for rule in &group.start {
            if let ir::StartRule::Keyswitch { range, .. } = rule {
                let (lo, hi) = plan_switches.unwrap_or((*range.start(), *range.end()));
                plan_switches = Some((lo.min(*range.start()), hi.max(*range.end())));
            }
        }
        for z in &group.zones {
            let Some(Some(sample)) = loaded.get(z.sample) else {
                continue;
            };
            let Some(source) = instrument.samples.get(z.sample) else {
                continue;
            };
            zones.push(zone(group, group_id, z, source, sample));
        }
    }
    let mut plan = Plan::new(zones, usize::from(instrument.voices));
    plan.groups = instrument
        .groups
        .iter()
        .map(|g| Group {
            mods: g.mods.clone(),
            envelope: envelope(g.envelope, rate),
        })
        .collect();
    if !instrument.buses.is_empty() {
        plan.buses = instrument
            .buses
            .iter()
            .map(|b| Bus {
                name: b.name.clone(),
                gain: gain(b.volume_db),
                enabled: true,
            })
            .collect();
    }
    plan.polyphony = polyphony;
    plan.keyswitches = plan_switches.map(|(lo, hi)| lo..=hi);
    plan.default_keyswitch = plan.zones.iter().filter_map(|z| z.keyswitch).min();
    (plan, diagnostics)
}

/// Loads each sample a resident zone plays, pinning the union of its zones'
/// windows. Samples open on every core; the store reads their pages likewise.
fn load_samples(
    instrument: &ir::Instrument,
    store: &mut Store,
    open: &Open,
    diagnostics: &mut Vec<ir::Diagnostic>,
) -> Vec<Option<Arc<Sample>>> {
    let mut used: HashMap<usize, Vec<&ir::Zone>> = HashMap::new();
    for group in instrument.groups.iter().filter(|g| !g.purged) {
        for z in &group.zones {
            used.entry(z.sample).or_default().push(z);
        }
    }
    let used: Vec<_> = used.into_iter().collect();
    let opened = parallel(&used, |(index, _)| match instrument.samples.get(*index) {
        Some(sample) => open(sample),
        None => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("no sample {index}"),
        )),
    });
    let mut requests = Vec::new();
    let mut slots = Vec::new();
    for ((index, zones), result) in used.iter().zip(opened) {
        let name = instrument
            .samples
            .get(*index)
            .map_or("?", |s| s.name.as_str());
        let (source, fmt) = match result {
            Ok(opened) => opened,
            Err(e) => {
                diagnostics.push(broken("sample-open", format!("{name}: {e}")));
                continue;
            }
        };
        let Some(sample) = instrument.samples.get(*index) else {
            continue;
        };
        requests.push(LoadRequest {
            key: BlobKey {
                file: format!("{}#{}", sample.file.display(), sample.offset).into(),
                offset: sample.offset,
                fmt,
            },
            source,
            frames: sample.frames,
            pin: zones
                .iter()
                .map(|z| window(z, sample.frames, fmt, instrument.preload))
                .collect(),
        });
        slots.push(*index);
    }
    let mut loaded = vec![None; instrument.samples.len()];
    for (index, result) in slots.into_iter().zip(store.load_many(requests)) {
        match result {
            Ok(s) => {
                if let Some(slot) = loaded.get_mut(index) {
                    *slot = Some(s);
                }
            }
            Err(e) => diagnostics.push(broken("sample-load", format!("sample {index}: {e}"))),
        }
    }
    loaded
}

/// `f` over `items` on every core, results in order.
fn parallel<T: Sync, R: Send>(items: &[T], f: impl Fn(&T) -> R + Sync) -> Vec<R> {
    let workers = std::thread::available_parallelism().map_or(4, |n| n.get());
    let chunk = items.len().div_ceil(workers).max(1);
    std::thread::scope(|scope| {
        let handles: Vec<_> = items
            .chunks(chunk)
            .map(|part| scope.spawn(|| part.iter().map(&f).collect::<Vec<_>>()))
            .collect();
        handles
            .into_iter()
            .flat_map(|h| h.join().unwrap_or_default())
            .collect()
    })
}

/// Kontakt's preload window for one zone, in frames (05-STREAM.md §1).
fn window(z: &ir::Zone, frames: u64, fmt: SampleFmt, preload: u32) -> Range<u64> {
    const GRAIN: u64 = 6144;
    let frame = fmt.bytes_per_frame().max(1) as i64;
    let off = i64::try_from(z.start_mod).unwrap_or(i64::MAX / 4);
    let pos = i64::try_from(if z.reverse {
        frames.saturating_sub(z.end)
    } else {
        z.start
    })
    .unwrap_or(0);
    let d = pos - off;
    let mut base = i64::from(preload) * i64::from(fmt.channels);
    if d < 0 {
        base += frame * d;
    }
    let total = u64::try_from(base + off * frame * 2).unwrap_or(0);
    let head = total.div_ceil(GRAIN) * GRAIN / frame.unsigned_abs();
    let lo = u64::try_from(d).unwrap_or(0);
    let hi = lo + head;
    if z.reverse {
        frames.saturating_sub(hi.min(frames))..frames.saturating_sub(lo.min(frames))
    } else {
        lo.min(frames)..hi.min(frames)
    }
}

fn zone(
    group: &ir::Group,
    group_id: u32,
    z: &ir::Zone,
    source: &ir::Sample,
    sample: &Arc<Sample>,
) -> Zone {
    let region = Region {
        sample: Arc::clone(sample),
        start: z.start,
        end: z.end.min(source.frames),
        looped: z.looped.map(|l| Loop {
            start: l.start,
            end: l.end,
            crossfade: l.crossfade,
        }),
        reverse: z.reverse,
    };
    let mut out = Zone::new(region, z.root, source.rate);
    out.keys = z.keys.clone();
    out.velocities = z.velocities.clone();
    for (control, range, (fade_in, fade_out)) in [
        (Control::Velocity, &z.velocities, z.velocity_fade),
        (Control::Key, &z.keys, z.key_fade),
    ] {
        let (low, high) = (*range.start(), *range.end());
        if fade_in > 0 || fade_out > 0 {
            out.crossfades.push(Crossfade {
                control,
                fade_in: (fade_in > 0)
                    .then(|| (low.saturating_sub(1), low.saturating_add(fade_in))),
                fade_out: (fade_out > 0)
                    .then(|| (high.saturating_sub(fade_out), high.saturating_add(1))),
            });
        }
    }
    out.trigger = match group.trigger {
        ir::Trigger::Attack => Trigger::Attack,
        ir::Trigger::Release => Trigger::Release,
        ir::Trigger::First => Trigger::First,
        ir::Trigger::Legato => Trigger::Legato,
    };
    for rule in &group.start {
        match rule {
            ir::StartRule::Keyswitch { key, .. } => out.keyswitch = Some(*key),
            ir::StartRule::Controller { cc, range } => out.cc_ranges.push((*cc, range.clone())),
            ir::StartRule::RoundRobin {
                position,
                length,
                random,
            } => {
                out.round_robin = Some(RoundRobin {
                    counter: RrCounter::Global,
                    position: *position,
                    length: *length,
                    random: *random,
                });
            }
        }
    }
    out.group = group_id;
    out.off_by = group.off_by.and_then(|g| u32::try_from(g).ok());
    out.keytrack = group.keytrack;
    out.tune = group.tune + z.tune;
    out.gain = gain(group.volume_db + z.volume_db);
    out.pan = (group.pan + z.pan).clamp(-1.0, 1.0);
    out.release_decay_db = group.release_decay_db;
    out.one_shot = group.one_shot;
    out.bus = group.bus;
    out
}

fn envelope(e: ir::Envelope, rate: u32) -> Ahdsr {
    let frames = |seconds: f32| (f64::from(seconds.max(0.0)) * f64::from(rate)).round() as u32;
    Ahdsr {
        attack: frames(e.attack),
        hold: frames(e.hold),
        decay: frames(e.decay),
        sustain: e.sustain.clamp(0.0, 1.0),
        release: frames(e.release).max(1),
    }
}

fn gain(db: f32) -> f32 {
    10f32.powf(db / 20.0)
}

fn broken(feature: &str, detail: String) -> ir::Diagnostic {
    ir::Diagnostic {
        impact: ir::Impact::Broken,
        feature: feature.into(),
        detail,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kontra_stream::Encoding;

    const STEREO_24: SampleFmt = SampleFmt {
        encoding: Encoding::I24,
        channels: 2,
    };

    fn at(start: u64, start_mod: u64) -> ir::Zone {
        ir::Zone {
            sample: 0,
            keys: 0..=127,
            velocities: 1..=127,
            velocity_fade: (0, 0),
            key_fade: (0, 0),
            root: 60,
            tune: 0.0,
            volume_db: 0.0,
            pan: 0.0,
            start,
            end: 1_000_000,
            start_mod,
            looped: None,
            reverse: false,
        }
    }

    #[test]
    fn the_window_follows_kontakts_preload_law() {
        // 60 KB per channel, stereo 24-bit: 122,880 bytes = 20 grains → 20,480 frames.
        assert_eq!(window(&at(0, 0), 1_000_000, STEREO_24, 61_440), 0..20_480);
        // S.Mod 1000 from 5000: starts 1000 earlier, 2 × 1000 frames longer, rounded up to a grain.
        let w = window(&at(5_000, 1_000), 1_000_000, STEREO_24, 61_440);
        assert_eq!(w.start, 4_000);
        assert_eq!(
            w.end - w.start,
            (122_880u64 + 12_000).div_ceil(6144) * 6144 / 6
        );
        // A start inside S.Mod of the head has less room behind it.
        let near = window(&at(100, 1_000), 1_000_000, STEREO_24, 61_440);
        assert_eq!(near.start, 0);
        // A short sample clamps the window.
        assert_eq!(window(&at(0, 0), 500, STEREO_24, 61_440), 0..500);
    }
}
