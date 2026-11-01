//! Prepare: an IR instrument becomes an engine [`Plan`] off the audio thread,
//! loading every resident sample's preload into the store (05-STREAM.md §1).

use std::collections::HashMap;
use std::io;
use std::ops::{Range, RangeInclusive};
use std::sync::Arc;

mod levels;

use kontra_ir as ir;
use kontra_stream::{
    BlobKey, LoadRequest, MAX_CHANNELS, Mix, Sample, SampleFmt, Source, Store, StreamClient,
};
use kontra_voice::{Ahdsr, Loop, PERIOD, Read, Region, Shape, Shaper, Side, Warp};

use kontra_engine::{
    Atom, Bus, Control, Crossfade, Group, Level, Plan, RoundRobin, RrCounter, Trigger, Zone,
    response,
};

/// Opens a sample's audio as the store reads it: decoded to packed PCM.
/// Called from several loader threads at once.
pub type Open<'a> = dyn Fn(&ir::Sample) -> io::Result<(Arc<dyn Source>, SampleFmt)> + Sync + 'a;

/// Builds the plan for `instrument` at the engine `rate`. Samples that fail to
/// open drop their zones and are reported; the script is left to the caller.
pub fn prepare(
    instrument: &ir::Instrument,
    rate: u32,
    store: &mut Store,
    open: &Open<'_>,
) -> (Plan, Vec<ir::Diagnostic>) {
    let mut diagnostics = Vec::new();
    let loaded = load_samples(instrument, store, open, &mut diagnostics);
    let client = store.client();
    let mut zones = Vec::new();
    let mut switches: Vec<RangeInclusive<u8>> = Vec::new();
    let mut polyphony = Vec::new();
    for (index, group) in instrument.groups.iter().enumerate() {
        let group_id = u32::try_from(index).unwrap_or(u32::MAX);
        if let Some(voices) = group.polyphony {
            polyphony.push((group_id, voices));
        }
        for (_, rule) in &group.start {
            if let ir::StartRule::Keyswitch { range } = rule
                && !switches.contains(range)
            {
                switches.push(range.clone());
            }
        }
        for (member, z) in group.zones.iter().enumerate() {
            let Some(Some(sample)) = loaded.get(z.sample) else {
                continue;
            };
            let Some(source) = instrument.samples.get(z.sample) else {
                continue;
            };
            let mut zone = zone(group, group_id, z, source, sample, (&switches, &client));
            zone.member = u16::try_from(member).unwrap_or(u16::MAX);
            zones.push(zone);
        }
    }
    let mut plan = Plan::new(zones, usize::from(instrument.voices));
    plan.groups = instrument
        .groups
        .iter()
        .map(|g| Group {
            name: g.name.clone(),
            level: Level {
                gain: gain(g.volume_db),
                pan: g.pan,
                tune: g.tune,
            },
            purged: g.purged,
            inserts: g.inserts.clone(),
            mods: g.mods.clone(),
            envelope: envelope(g.envelope, rate),
            envelopes: g.envelopes.clone(),
            lfos: g.lfos.clone(),
            shapers: g.shapers.clone(),
            blends: g.blends.clone(),
            routes: g.routes.clone(),
            release_trigger: g.trigger == ir::Trigger::Release,
            bus: g.bus,
        })
        .collect();
    if !instrument.buses.is_empty() {
        plan.buses = instrument
            .buses
            .iter()
            .map(|b| Bus {
                name: b.name.clone(),
                gain: gain(b.volume_db),
                pan: b.pan,
                inserts: b.inserts.clone(),
                sends: b.sends.clone(),
                to_main: b.to_main,
                enabled: true,
                mods: b.mods.clone(),
                blends: b.blends.clone(),
            })
            .collect();
    }
    plan.inserts.clone_from(&instrument.inserts);
    plan.sends.clone_from(&instrument.sends);
    plan.main.clone_from(&instrument.main);
    plan.output = instrument.output;
    plan.route_pitch = instrument.route_pitch;
    plan.steal_whole_groups = instrument.steal_whole_groups;
    plan.slot_laws.clone_from(&instrument.slot_laws);
    plan.slot_shapes.clone_from(&instrument.slot_shapes);
    plan.impulses = instrument
        .impulses
        .iter()
        .map(|i| {
            match instrument
                .samples
                .get(i.sample)
                .ok_or_else(|| "no response sample".to_string())
                .and_then(|s| response(s, i, rate))
            {
                Ok(r) => Some(r),
                Err(e) => {
                    diagnostics.push(broken("impulse-open", e));
                    None
                }
            }
        })
        .collect();
    plan.impulse_laws = instrument
        .impulses
        .iter()
        .map(|i| {
            let name = instrument
                .samples
                .get(i.sample)
                .map_or("", |s| s.name.as_str());
            (kontra_engine::stem(name).to_lowercase(), *i)
        })
        .collect();
    plan.responses = instrument
        .responses
        .iter()
        .filter_map(|(name, i)| Some((name.clone(), instrument.samples.get(*i)?.clone())))
        .collect();
    // A slot's own response, so a script's knob can prepare it again.
    for ((name, _), i) in plan.impulse_laws.iter().zip(&instrument.impulses) {
        if !plan.responses.iter().any(|(n, _)| n == name)
            && let Some(s) = instrument.samples.get(i.sample)
        {
            plan.responses.push((name.clone(), s.clone()));
        }
    }
    plan.polyphony = polyphony;
    plan.voice_groups.clone_from(&instrument.voice_groups);
    plan.default_keyswitch = switches.iter().map(|r| *r.start()).min();
    plan.switches = switches;
    (plan, diagnostics)
}

/// Loads each sample a resident zone plays, pinning the union of its zones'
/// windows. Samples open on every core; the store reads their pages likewise.
fn load_samples(
    instrument: &ir::Instrument,
    store: &mut Store,
    open: &Open<'_>,
    diagnostics: &mut Vec<ir::Diagnostic>,
) -> Vec<Option<Arc<Sample>>> {
    let mut used: HashMap<usize, Vec<(&ir::Zone, Option<ir::Stream>)>> = HashMap::new();
    for group in instrument.groups.iter().filter(|g| !g.purged) {
        for z in &group.zones {
            used.entry(z.sample).or_default().push((z, group.stream));
        }
    }
    let used: Vec<_> = used.into_iter().collect();
    // On fast storage a head of FAST_HEAD bytes per channel starts every
    // page on time; the law's own 60 KB is sized for spinning disks.
    let fast = (used.first())
        .and_then(|(index, _)| instrument.samples.get(*index))
        .is_some_and(|s| kontra_stream::fast_storage(&s.file));
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
                .map(|&(z, stream)| match stream {
                    Some(stream) => window(z, sample.frames, fmt, head(stream, fast)),
                    None => 0..sample.frames,
                })
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
            // A worker's panic is the loader's: a short result would misalign
            // every later item with its input.
            .flat_map(|h| h.join().unwrap_or_else(|e| std::panic::resume_unwind(e)))
            .collect()
    })
}

/// Head bytes per channel on fast storage (05-STREAM.md §1): the chord
/// stress (render `KONTRA_CHORDS`, cold page cache) has no late page down to
/// 4 KiB on Areia and Vista; this keeps 3× that margin.
const FAST_HEAD: u32 = 12 << 10;

/// The law's stream settings with the head shortened on fast storage.
fn head(stream: ir::Stream, fast: bool) -> ir::Stream {
    if !fast {
        return stream;
    }
    ir::Stream {
        bytes_per_channel: stream.bytes_per_channel.min(FAST_HEAD),
        ..stream
    }
}

/// A streamed zone's resident window, in frames: its head from where it
/// may start, room for the start-offset range on both sides, rounded up to
/// the grain (05-STREAM.md §1).
fn window(z: &ir::Zone, frames: u64, fmt: SampleFmt, stream: ir::Stream) -> Range<u64> {
    let grain = u64::from(stream.grain.max(1));
    let frame = fmt.bytes_per_frame().max(1) as i64;
    let off = i64::try_from(z.start_mod).unwrap_or(i64::MAX / 4);
    let pos = i64::try_from(if z.reverse {
        frames.saturating_sub(z.end)
    } else {
        z.start
    })
    .unwrap_or(0);
    let d = pos - off;
    let mut base = i64::from(stream.bytes_per_channel) * i64::from(fmt.channels);
    if d < 0 {
        base += frame * d;
    }
    let total = u64::try_from(base + off * frame * 2).unwrap_or(0);
    let head = total.div_ceil(grain) * grain / frame.unsigned_abs();
    let lo = u64::try_from(d).unwrap_or(0);
    let hi = lo + head;
    if z.reverse {
        frames.saturating_sub(hi.min(frames))..frames.saturating_sub(lo.min(frames))
    } else {
        lo.min(frames)..hi.min(frames)
    }
}

/// A zone's channel mix as the voice reads it; channels past
/// [`MAX_CHANNELS`] are dropped.
fn mix(rows: &[[f32; 2]]) -> Option<Mix> {
    (!rows.is_empty()).then(|| {
        let mut mix = [[0.0; 2]; MAX_CHANNELS];
        mix.iter_mut().zip(rows).for_each(|(m, r)| *m = *r);
        mix
    })
}

/// A group's wavetable as its voices read it.
fn read(w: ir::Wavetable) -> Read {
    let side = |s| match s {
        ir::Side::Minus => Side::Minus,
        ir::Side::Plus => Side::Plus,
        ir::Side::Both => Side::Both,
    };
    let warp = |w: ir::Warp| Warp {
        shape: match w.shape {
            ir::WarpShape::Off => Shape::Off,
            ir::WarpShape::Sync => Shape::Sync,
            ir::WarpShape::Cosine => Shape::Cosine,
            ir::WarpShape::Arcsine => Shape::Arcsine,
            ir::WarpShape::CosineArcsine => Shape::CosineArcsine,
            ir::WarpShape::Pulse => Shape::Pulse,
            ir::WarpShape::Skew { reach, side: s } => Shape::Skew {
                reach,
                side: side(s),
            },
            ir::WarpShape::Flip => Shape::Flip,
            ir::WarpShape::Mirror => Shape::Mirror,
            ir::WarpShape::Quantize => Shape::Quantize,
            ir::WarpShape::Seesaw => Shape::Seesaw,
            ir::WarpShape::Exp => Shape::Exp,
            ir::WarpShape::Log => Shape::Log,
            ir::WarpShape::LogExp => Shape::LogExp,
            ir::WarpShape::Bend { side: s } => Shape::Bend { side: side(s) },
        },
        shaper: match w.shaper {
            ir::FormShaper::Off => Shaper::Off,
            ir::FormShaper::Edges => Shaper::Edges,
            ir::FormShaper::Window => Shaper::Window,
            ir::FormShaper::Saturate => Shaper::Saturate,
            ir::FormShaper::Fold => Shaper::Fold,
            ir::FormShaper::Blinds(n) => Shaper::Blinds(n),
            ir::FormShaper::Arch => Shaper::Arch,
            ir::FormShaper::Peak => Shaper::Peak,
            ir::FormShaper::Fall => Shaper::Fall,
            ir::FormShaper::Wrap => Shaper::Wrap,
        },
        amount: w.amount,
    };
    Read {
        position: w.position,
        phase: w.phase,
        warps: w.warps.map(warp),
    }
}

fn zone(
    group: &ir::Group,
    group_id: u32,
    z: &ir::Zone,
    source: &ir::Sample,
    sample: &Arc<Sample>,
    (switches, client): (&[RangeInclusive<u8>], &StreamClient),
) -> Zone {
    let region = Region {
        sample: Arc::clone(sample),
        start: z.start,
        end: z.end.min(source.frames),
        loops: z
            .loops
            .iter()
            .map(|l| Loop {
                start: l.start,
                end: l.end,
                crossfade: l.crossfade,
                count: l.count,
                alternating: l.alternating,
                until_release: l.until_release,
            })
            .collect(),
        reverse: z.reverse,
        mix: mix(&z.mix),
        table: (group.wavetable).and_then(|w| {
            levels::table(sample, client, (z.start, z.end.min(source.frames)), read(w))
        }),
    };
    // A table plays its cycle at 440 Hz on A4, whatever the zone's root and
    // the sample's rate.
    let (root, rate) = if region.table.is_some() {
        (69, (440.0 * PERIOD) as u32)
    } else {
        (z.root, source.rate)
    };
    let mut out = Zone::new(region, root, rate);
    out.start_mod = z.start_mod;
    out.slot = z.slot;
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
    out.start = group
        .start
        .iter()
        .filter_map(|(join, rule)| {
            let atom = match rule {
                ir::StartRule::Keyswitch { range } => {
                    let i = switches.iter().position(|r| r == range)?;
                    Atom::Switch(u16::try_from(i).ok()?)
                }
                ir::StartRule::Controller { cc, range } => Atom::Cc(*cc, range.clone()),
                ir::StartRule::RoundRobin {
                    position,
                    length,
                    random,
                } => Atom::RoundRobin(RoundRobin {
                    counter: RrCounter::Global,
                    position: *position,
                    length: *length,
                    random: *random,
                }),
                ir::StartRule::Cycle { random } => {
                    let slots: std::collections::BTreeSet<u16> =
                        group.zones.iter().map(|z| z.slot).collect();
                    let count = |n: usize| u8::try_from(n).unwrap_or(u8::MAX);
                    Atom::RoundRobin(RoundRobin {
                        counter: RrCounter::Group,
                        position: count(slots.range(..z.slot).count()),
                        length: count(slots.len()),
                        random: *random,
                    })
                }
            };
            Some((*join, atom))
        })
        .collect();
    out.group = group_id;
    out.off_by = group.off_by.and_then(|g| u32::try_from(g).ok());
    out.voice_group = group.voice_group.and_then(|g| u32::try_from(g).ok());
    out.keytrack = group.keytrack;
    // The group's own level, pan and tune are its plan group's.
    out.tune = z.tune;
    out.gain = gain(z.volume_db);
    out.pan = z.pan;
    out.release_decay_db = group.release_decay_db;
    out.one_shot = group.one_shot;
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
        attack_shape: e.attack_shape,
        fall_shape: e.fall_shape,
    }
}

fn gain(db: f32) -> f32 {
    ir::common::DB_GAIN.eval_unclamped(db)
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
            mix: Vec::new(),
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
            loops: vec![],
            reverse: false,
            slot: 0,
        }
    }

    const DFD: ir::Stream = ir::Stream {
        bytes_per_channel: 61_440,
        grain: 6144,
    };

    #[test]
    fn the_window_follows_kontakts_preload_law() {
        // 60 KB per channel, stereo 24-bit: 122,880 bytes = 20 grains → 20,480 frames.
        assert_eq!(window(&at(0, 0), 1_000_000, STEREO_24, DFD), 0..20_480);
        // S.Mod 1000 from 5000: starts 1000 earlier, 2 × 1000 frames longer, rounded up to a grain.
        let w = window(&at(5_000, 1_000), 1_000_000, STEREO_24, DFD);
        assert_eq!(w.start, 4_000);
        assert_eq!(
            w.end - w.start,
            (122_880u64 + 12_000).div_ceil(6144) * 6144 / 6
        );
        // A start inside S.Mod of the head has less room behind it.
        let near = window(&at(100, 1_000), 1_000_000, STEREO_24, DFD);
        assert_eq!(near.start, 0);
        // A short sample clamps the window.
        assert_eq!(window(&at(0, 0), 500, STEREO_24, DFD), 0..500);
        // Fast storage: 12 KiB per channel, stereo 24-bit = 4 grains → 4096 frames.
        assert_eq!(
            window(&at(0, 0), 1_000_000, STEREO_24, head(DFD, true)),
            0..4096
        );
        assert_eq!(head(DFD, false), DFD);
    }
}
