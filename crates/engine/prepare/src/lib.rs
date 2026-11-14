//! Prepare: an IR instrument becomes an engine [`Plan`] off the audio thread,
//! loading every resident sample's preload into the store (05-STREAM.md §1).

use std::collections::HashMap;
use std::io;
use std::ops::{Range, RangeInclusive};
use std::path::Path;
use std::sync::Arc;

mod levels;

use kontra_ir as ir;
use kontra_stream::{
    BlobKey, LoadRequest, MAX_CHANNELS, Mix, Sample, SampleFmt, Source, Store, StreamClient,
};
use kontra_voice::{Ahdsr, Loop, PERIOD, Read, Region, Shape, Shaper, Side, Warp};

use kontra_engine::{
    Atom, Bus, Dormant, Group, Level, Plan, RoundRobin, RrCounter, Trigger, Zone, ZoneRules,
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
    let shared: Vec<Arc<ir::Sample>> = instrument.samples.iter().cloned().map(Arc::new).collect();
    let client = store.client();
    let mut zones = Vec::new();
    let mut switches: Vec<RangeInclusive<u8>> = Vec::new();
    let mut polyphony = Vec::new();
    let mut facts = Vec::new();
    // `detect_*` reads the samples' music information: measured here, once
    // per sample, only for instruments whose scripts ask.
    let mut group_rules = Vec::with_capacity(instrument.groups.len());
    let mut dormant = Vec::new();
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
        let rules = rules(group, &switches);
        for (member, z) in group.zones.iter().enumerate() {
            let Some(source) = instrument.samples.get(z.sample) else {
                continue;
            };
            // A script sees every saved zone, played or not (a purged group,
            // a sample that failed to open).
            facts.push(kontra_engine::ZoneFacts {
                id: z.id,
                name: kontra_engine::zone_name(&source.name).into(),
                group: group_id,
                keys: z.keys.clone(),
                velocities: z.velocities.clone(),
                root: z.root,
                start: z.start,
                end: z.end,
                start_range: z.start_mod,
                rate: source.rate,
                source: shared.get(z.sample).cloned(),
                loops: z.loops.clone(),
                user: false,
                volume: (z.volume_db * 100.0).round() as i32,
                pan: (z.pan * 1000.0).round() as i32,
                tune: (z.tune * 100.0).round() as i32,
                fades: [
                    i32::from(z.key_fade.0),
                    i32::from(z.key_fade.1),
                    i32::from(z.velocity_fade.0),
                    i32::from(z.velocity_fade.1),
                ],
                loop_pars: Default::default(),
            });
            // A purged group loads nothing: a script's un-purge builds it.
            if group.purged {
                continue;
            }
            let Some(Some(sample)) = loaded.get(z.sample) else {
                continue;
            };
            let mut zone = zone((group, group_id, &rules), z, source, sample, &client);
            zone.member = u16::try_from(member).unwrap_or(u16::MAX);
            zones.push(zone);
        }
        if group.purged && !group.zones.is_empty() {
            dormant.push(dormant_of(instrument, group, group_id));
        }
        group_rules.push(rules);
    }
    let mut plan = Plan::new(zones, usize::from(instrument.voices));
    plan.dormant = Arc::new(dormant);
    plan.zone_facts = facts;
    plan.groups = (instrument.groups.iter().zip(group_rules))
        .map(|(g, zones)| Group {
            name: g.name.clone(),
            level: Level {
                gain: gain(g.volume_db),
                pan: g.pan,
                tune: g.tune,
            },
            purged: g.purged,
            inserts: Vec::new(),
            mods: g.mods.clone(),
            envelope: envelope(g.envelope, rate),
            envelopes: g.envelopes.clone(),
            lfos: g.lfos.clone(),
            shapers: g.shapers.clone(),
            blends: g.blends.clone(),
            routes: g.routes.clone(),
            release_trigger: g.trigger == ir::Trigger::Release,
            bus: g.bus,
            wavetable: g.wavetable.map(read),
            speed: g.speed,
            zones,
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
                inserts: Vec::new(),
                sends: b.sends.clone(),
                to_main: b.to_main,
                enabled: true,
                mods: b.mods.clone(),
                blends: b.blends.clone(),
                lfos: b.lfos.clone(),
            })
            .collect();
    }
    plan.output = instrument.output;
    plan.route_pitch = instrument.route_pitch;
    plan.steal_whole_groups = instrument.steal_whole_groups;
    effects(&mut plan, instrument, rate, &mut diagnostics);
    plan.polyphony = polyphony;
    plan.voice_groups.clone_from(&instrument.voice_groups);
    plan.default_keyswitch = switches.iter().map(|r| *r.start()).min();
    plan.switches = switches;
    (plan, diagnostics)
}

/// Puts `instrument`'s effects into `plan`, whose groups and buses are
/// its own: every rack, the buses' levels, the slots' laws and the
/// convolution responses (read here). A build does it, and so does a
/// snapshot that saves other effects for the same instrument.
pub fn effects(
    plan: &mut Plan,
    instrument: &ir::Instrument,
    rate: u32,
    diagnostics: &mut Vec<ir::Diagnostic>,
) {
    for (g, from) in plan.groups.iter_mut().zip(&instrument.groups) {
        g.inserts.clone_from(&from.inserts);
    }
    for (b, from) in plan.buses.iter_mut().zip(&instrument.buses) {
        b.inserts.clone_from(&from.inserts);
        (b.gain, b.pan) = (gain(from.volume_db), from.pan);
    }
    plan.inserts.clone_from(&instrument.inserts);
    plan.sends.clone_from(&instrument.sends);
    plan.main.clone_from(&instrument.main);
    plan.slot_laws.clone_from(&instrument.slot_laws);
    plan.slot_models.clone_from(&instrument.slot_models);
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
}

/// `group` as a purge keeps it: its zones, the samples they play renumbered
/// from 0.
fn dormant_of(instrument: &ir::Instrument, group: &ir::Group, id: u32) -> Dormant {
    let mut source = group.clone();
    let mut samples = Vec::new();
    let mut local: HashMap<usize, usize> = HashMap::new();
    for z in &mut source.zones {
        let Some(s) = instrument.samples.get(z.sample) else {
            continue;
        };
        z.sample = *local.entry(z.sample).or_insert_with(|| {
            samples.push(s.clone());
            samples.len() - 1
        });
    }
    Dormant {
        group: id,
        source,
        samples,
    }
}

/// `plan` with purged group `group` loaded and its zones appended (indices of
/// the zones already there stay), the loads' diagnostics, and how many
/// samples it loaded. `None` when the group is not dormant. Samples open and
/// read on the calling thread: call it off the audio thread; with
/// [`Store::defer_heads`] on, the heads come after ([`Store::take_heads`]).
pub fn wake(
    plan: &Plan,
    group: u32,
    store: &mut Store,
    open: &Open<'_>,
) -> Option<(Plan, Vec<ir::Diagnostic>, usize)> {
    let d = plan.dormant.iter().find(|d| d.group == group)?;
    let mut source = d.source.clone();
    source.purged = false;
    let instrument = ir::Instrument {
        groups: vec![source],
        samples: d.samples.clone(),
        ..ir::Instrument::DEFAULT
    };
    let mut diagnostics = Vec::new();
    let loaded = load_samples(&instrument, store, open, &mut diagnostics);
    let client = store.client();
    let [source] = instrument.groups.as_slice() else {
        return None;
    };
    let rules = rules(source, &plan.switches);
    let mut out = plan.clone();
    for (member, z) in source.zones.iter().enumerate() {
        let (Some(from), Some(Some(sample))) =
            (instrument.samples.get(z.sample), loaded.get(z.sample))
        else {
            continue;
        };
        let mut zone = zone((source, group, &rules), z, from, sample, &client);
        zone.member = u16::try_from(member).unwrap_or(u16::MAX);
        out.zones.push(zone);
    }
    out.dormant = Arc::new(
        plan.dormant
            .iter()
            .filter(|d| d.group != group)
            .cloned()
            .collect(),
    );
    let count = loaded.iter().flatten().count();
    Some((out, diagnostics, count))
}

/// The plan with every user zone whose sample changed built as a playing
/// zone (`set_sample`; a zone keeps its place in the plan when its sample is
/// replaced), the loads' diagnostics. `None` when no user zone is due.
/// Samples open and read on the calling thread: off the audio thread.
pub fn user_zones(
    plan: &Plan,
    store: &mut Store,
    open: &Open<'_>,
) -> Option<(Plan, Vec<ir::Diagnostic>)> {
    let due: Vec<&kontra_engine::ZoneFacts> = (plan.user_facts().iter())
        .filter(|f| {
            f.source.as_ref().is_some_and(|s| {
                !(plan.user_loaded.iter()).any(|(id, l)| *id == f.id && Arc::ptr_eq(l, s))
            })
        })
        .collect();
    if due.is_empty() {
        return None;
    }
    let byte = |v: i32| u8::try_from(v).unwrap_or(u8::MAX);
    let zones: Vec<ir::Zone> = due
        .iter()
        .enumerate()
        .map(|(sample, f)| ir::Zone {
            id: f.id,
            sample,
            keys: f.keys.clone(),
            velocities: f.velocities.clone(),
            velocity_fade: (byte(f.fades[2]), byte(f.fades[3])),
            key_fade: (byte(f.fades[0]), byte(f.fades[1])),
            root: f.root,
            tune: f64::from(f.tune) / 100.0,
            volume_db: f.volume as f32 / 100.0,
            pan: f.pan as f32 / 1000.0,
            start: f.start,
            end: f.end,
            start_mod: 0,
            loops: Vec::new(),
            reverse: false,
            mix: Vec::new(),
            slot: 0,
        })
        .collect();
    let instrument = ir::Instrument {
        groups: vec![ir::Group {
            zones,
            ..ir::Group::DEFAULT
        }],
        samples: due
            .iter()
            .filter_map(|f| f.source.as_deref().cloned())
            .collect(),
        ..ir::Instrument::DEFAULT
    };
    let mut diagnostics = Vec::new();
    let loaded = load_samples(&instrument, store, open, &mut diagnostics);
    let client = store.client();
    let mut out = plan.clone();
    let [group] = instrument.groups.as_slice() else {
        return None;
    };
    for (i, (f, z)) in due.iter().zip(&group.zones).enumerate() {
        let (Some(from), Some(Some(sample))) = (instrument.samples.get(i), loaded.get(i)) else {
            continue;
        };
        let rules =
            (out.groups.get(f.group as usize)).map_or(ZoneRules::PLAIN, |g| g.zones.clone());
        let mut zone = zone(
            (&ir::Group::DEFAULT, f.group, &rules),
            z,
            from,
            sample,
            &client,
        );
        zone.region.loops = kontra_engine::user_loops(f);
        match out.zones.iter_mut().find(|p| p.id == f.id) {
            Some(slot) => *slot = zone,
            None => out.zones.push(zone),
        }
        out.user_loaded.retain(|(id, _)| *id != f.id);
        if let Some(s) = &f.source {
            out.user_loaded.push((f.id, Arc::clone(s)));
        }
    }
    Some((out, diagnostics))
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
    let mut used: Vec<_> = used.into_iter().collect();
    // The keys played most (middle C and two octaves either side) first, so
    // deferred heads reach them first.
    used.sort_by_key(|(_, zones)| {
        (zones.iter())
            .map(|(z, _)| {
                let (lo, hi) = (*z.keys.start(), *z.keys.end());
                lo.saturating_sub(72).max(48u8.saturating_sub(hi))
            })
            .min()
            .unwrap_or(u8::MAX)
    });
    // On fast storage a head of FAST_HEAD bytes per channel starts every
    // page on time; the law's own 60 KB is sized for spinning disks. Each
    // sample's file answers for itself: a library may keep archives on
    // several drives.
    let mut speeds: HashMap<std::path::PathBuf, bool> = HashMap::new();
    let mut fast = |s: &ir::Sample| {
        *(speeds.entry(s.file.clone())).or_insert_with(|| kontra_stream::fast_storage(&s.file))
    };
    let opened = parallel(&used, |(index, _)| match instrument.samples.get(*index) {
        Some(sample) => open(sample),
        None => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("no sample {index}"),
        )),
    });
    let mut requests = Vec::new();
    let mut slots = Vec::new();
    // A folder that moved fails every sample in it alike: one line per
    // folder, not per sample; any other failure is told once per file.
    let mut missing: std::collections::BTreeMap<&Path, (usize, &Path)> = Default::default();
    let mut told: std::collections::HashSet<(&Path, String)> = Default::default();
    for ((index, zones), result) in used.iter().zip(opened) {
        let name = instrument
            .samples
            .get(*index)
            .map_or("?", |s| s.name.as_str());
        let (source, fmt) = match result {
            Ok(opened) => opened,
            Err(e) => {
                match instrument.samples.get(*index) {
                    Some(s) if e.kind() == io::ErrorKind::NotFound => {
                        let dir = s.file.parent().unwrap_or(Path::new(""));
                        missing.entry(dir).or_insert((0, &s.file)).0 += 1;
                    }
                    Some(s) if told.insert((&s.file, e.to_string())) => {
                        diagnostics.push(broken(
                            "sample-open",
                            format!("{} cannot be read: {e}", s.file.display()),
                        ));
                    }
                    Some(_) => {}
                    None => diagnostics.push(broken("sample-open", format!("{name}: {e}"))),
                }
                continue;
            }
        };
        let Some(sample) = instrument.samples.get(*index) else {
            continue;
        };
        let fast = fast(sample);
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
    // A wavetable zone's table is built from its frames right here, so those
    // samples load whole now even when the store leaves heads for later.
    let tabled: Vec<usize> = (instrument.groups.iter().filter(|g| g.wavetable.is_some()))
        .flat_map(|g| g.zones.iter().map(|z| z.sample))
        .collect();
    let (now, later): (Vec<_>, Vec<_>) =
        (slots.into_iter().zip(requests)).partition(|(index, _)| tabled.contains(index));
    for (batch, whole) in [(now, true), (later, false)] {
        let (indices, batch): (Vec<_>, Vec<_>) = batch.into_iter().unzip();
        let results = if whole {
            store.load_many_now(batch)
        } else {
            store.load_many(batch)
        };
        for (index, result) in indices.into_iter().zip(results) {
            match result {
                Ok(s) => {
                    if let Some(slot) = loaded.get_mut(index) {
                        *slot = Some(s);
                    }
                }
                Err(e) => {
                    let (name, file) = instrument
                        .samples
                        .get(index)
                        .map_or((String::new(), Path::new("")), |s| {
                            (s.name.clone(), s.file.as_path())
                        });
                    if told.insert((file, e.to_string())) {
                        diagnostics.push(broken(
                            "sample-load",
                            format!("{} ({name}) is damaged: {e}", file.display()),
                        ));
                    }
                }
            }
        }
    }
    for (dir, (count, first)) in missing {
        // The first file is how the host searches for the folder.
        diagnostics.push(broken(
            "samples not found",
            format!(
                "{count} sample{} not found in {}\nfirst: {}",
                if count == 1 { "" } else { "s" },
                dir.display(),
                first.display()
            ),
        ));
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

/// A wavetable form as voices read it.
#[must_use]
pub fn warp(w: ir::Warp) -> Warp {
    let side = |s| match s {
        ir::Side::Minus => Side::Minus,
        ir::Side::Plus => Side::Plus,
        ir::Side::Both => Side::Both,
    };
    Warp {
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
    }
}

/// The voice's kind of mod oscillator.
#[must_use]
pub fn mod_kind(k: ir::ModOscKind) -> kontra_voice::ModKind {
    use kontra_voice::ModKind as V;
    match k {
        ir::ModOscKind::Off => V::Off,
        ir::ModOscKind::Fm => V::Fm,
        ir::ModOscKind::FmBy => V::FmBy,
        ir::ModOscKind::FmFeedback => V::FmFeedback,
        ir::ModOscKind::Pm => V::Pm,
        ir::ModOscKind::PmFeedback => V::PmFeedback,
        ir::ModOscKind::Ring => V::Ring,
        ir::ModOscKind::Mix => V::Mix,
        ir::ModOscKind::Blend => V::Blend,
    }
}

/// A group's wavetable as its voices read it.
#[must_use]
pub fn read(w: ir::Wavetable) -> Read {
    Read {
        position: w.position,
        phase: w.phase,
        phase_random: w.phase_random,
        gain: w.gain,
        inharmonic: kontra_voice::Inharmonic {
            on: w.inharmonic.on,
            amount: w.inharmonic.amount,
            range: w.inharmonic.range,
        },
        modosc: kontra_voice::ModOsc {
            kind: mod_kind(w.modosc.kind),
            amount: w.modosc.amount,
            tune: w.modosc.tune,
            tune_range: w.modosc.tune_range,
            fm_hz: w.modosc.fm_hz,
        },
        rate: 48_000.0,
        warps: w.warps.map(warp),
    }
}

/// A table plays its cycle at 440 Hz on A4, whatever the zone's root and
/// the sample's rate: (root, rate).
const TABLE_PITCH: (u8, u32) = (69, (440.0 * PERIOD) as u32);

/// `plan` with a table for every zone in a wavetable group that has none
/// (`set_zone_par` moved it there), built off the audio thread; `None` when
/// none lacks one. A sample not in memory stays without (plays raw). How a
/// table reads, and dropping it outside a wavetable group, is
/// `Plan::edit_zone`'s.
// ponytail: a zone that leaves a wavetable group drops its table and one
// that returns has it built again (~2 ms off audio); keep it aside if a
// library toggles zones faster.
#[must_use]
pub fn tables(plan: &Plan, client: &StreamClient) -> Option<Plan> {
    let read = |z: &Zone| {
        let group = plan.groups.get(z.group as usize)?;
        group.wavetable.filter(|_| z.region.table.is_none())
    };
    plan.zones.iter().any(|z| read(z).is_some()).then(|| {
        let mut out = plan.clone();
        for (z, read) in out.zones.iter_mut().zip(plan.zones.iter().map(read)) {
            let r = &z.region;
            let Some(table) =
                read.and_then(|w| levels::table(&r.sample, client, (r.start, r.end), w))
            else {
                continue;
            };
            z.region.table = Some(table);
            (z.root, z.sample_rate) = TABLE_PITCH;
        }
        out
    })
}

fn zone(
    (group, group_id, rules): (&ir::Group, u32, &ZoneRules),
    z: &ir::Zone,
    source: &ir::Sample,
    sample: &Arc<Sample>,
    client: &StreamClient,
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
    let (root, rate) = if region.table.is_some() {
        TABLE_PITCH
    } else {
        (z.root, source.rate)
    };
    let mut out = Zone::new(region, root, rate);
    out.start_mod = z.start_mod;
    out.slot = z.slot;
    out.id = z.id;
    out.keys = z.keys.clone();
    out.velocities = z.velocities.clone();
    out.fades = [z.velocity_fade, z.key_fade];
    out.range_crossfades();
    out.group = group_id;
    rules.apply(&mut out);
    // The group's own level, pan and tune are its plan group's.
    out.tune = z.tune;
    out.gain = gain(z.volume_db);
    out.pan = z.pan;
    out
}

/// What `group` gives each of its zones; `switches` are the plan's ranges.
fn rules(group: &ir::Group, switches: &[RangeInclusive<u8>]) -> ZoneRules {
    let mut slots: Vec<u16> = group.zones.iter().map(|z| z.slot).collect();
    slots.sort_unstable();
    slots.dedup();
    let trigger = match group.trigger {
        ir::Trigger::Attack => Trigger::Attack,
        ir::Trigger::Release => Trigger::Release,
        ir::Trigger::First => Trigger::First,
        ir::Trigger::Legato => Trigger::Legato,
    };
    let start = group
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
                // Each zone's position is its own (ZoneRules::apply).
                ir::StartRule::Cycle { random } => Atom::RoundRobin(RoundRobin {
                    counter: RrCounter::Group,
                    position: 0,
                    length: u8::try_from(slots.len()).unwrap_or(u8::MAX),
                    random: *random,
                }),
            };
            Some((*join, atom))
        })
        .collect();
    ZoneRules {
        start,
        slots,
        trigger,
        off_by: group.off_by.and_then(|g| u32::try_from(g).ok()),
        voice_group: group.voice_group.and_then(|g| u32::try_from(g).ok()),
        keytrack: group.keytrack,
        release_decay_db: group.release_decay_db,
        one_shot: group.one_shot,
    }
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
            id: 0,
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
            loops: Default::default(),
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
