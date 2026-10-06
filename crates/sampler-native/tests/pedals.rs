//! Pedal regression suite over installed Kontakt instruments, driven through the
//! real UMP ingress (or the MPE adapter) exactly as `render-kontakt` wires it:
//! `sampler_kontakt::load` -> `Runtime` -> `sampler_midi`. Every test skips when
//! `KONTRA_KONTAKT_LIBRARIES` is unset or its instrument is not installed.
//!
//! Timing is asserted on the engine's own sample clock (`release_context`), and
//! release-trigger firings are observed as new release-phase families per block.

use sampler_core::{FamilyId, Limits, NoteId, Outcome, Runtime, Stealing, Trigger};
use sampler_midi::{Applied, ApplyError, Ingress, Mpe, Packets, TimedPacket, Version, Zone};
use std::path::PathBuf;
use std::sync::Mutex;

const UNA_CORDA: &str = "Una Corda Library/Instruments/Una Corda Pure.nki";
const CELLOS: &str = "Performance Samples Vista/Instruments/Vista - 3 Cellos.nki";
const ANALOG: &str = "ANALOG STRINGS/Instruments/ANALOG STRINGS.nki";

const RATE: usize = 48000;
const BLOCK: usize = 64;
/// Longest tail allowed after the last event before a voice counts as stuck.
const TAIL: f64 = 60.0;

/// Loads are large; one instrument in memory at a time.
static SERIAL: Mutex<()> = Mutex::new(());

fn find(relative: &str) -> Option<PathBuf> {
    let Some(paths) = std::env::var_os("KONTRA_KONTAKT_LIBRARIES") else {
        eprintln!("skipped: KONTRA_KONTAKT_LIBRARIES is unset");
        return None;
    };
    let found = std::env::split_paths(&paths)
        .map(|root| root.join(relative))
        .find(|p| p.is_file());
    if found.is_none() {
        eprintln!("skipped: {relative} is not installed");
    }
    found
}

fn at(seconds: f64) -> usize {
    (seconds * RATE as f64).round() as usize
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Msg {
    On(u8, u8, u8),
    Off(u8, u8),
    Cc(u8, u8, u8),
}
use Msg::*;

impl Msg {
    fn word(self) -> u32 {
        let (status, channel, a, b) = match self {
            On(c, k, v) => (9, c, k, v),
            Off(c, k) => (8, c, k, 64),
            Cc(c, n, v) => (11, c, n, v),
        };
        0x2000_0000 | status << 20 | u32::from(channel) << 16 | u32::from(a) << 8 | u32::from(b)
    }
}

#[derive(Clone, Copy)]
enum Port {
    Channel,
    /// Lower MPE zone with this many member channels.
    Mpe(u8),
}

#[derive(Clone, Copy)]
struct Setup {
    instrument: &'static str,
    keys: (u8, u8),
    scripts: bool,
    port: Port,
    voices: usize,
    block: usize,
    /// Replace the instrument's own scripts with this KSP.
    script: Option<&'static str>,
}
impl Setup {
    const fn new(instrument: &'static str, keys: (u8, u8)) -> Self {
        Self {
            instrument,
            keys,
            scripts: true,
            port: Port::Channel,
            voices: 1024,
            block: BLOCK,
            script: None,
        }
    }
}

#[derive(Debug, Default)]
struct Played {
    id: Option<NoteId>,
    key: u8,
    channel: u8,
    on: usize,
    key_at: Option<u64>,
    gate_at: Option<u64>,
    /// Block start frames at which new release-phase families appeared.
    fired: Vec<usize>,
    release_families: Vec<FamilyId>,
    release_trigger: Option<Trigger>,
}

#[derive(Debug)]
struct Run {
    out: Vec<[f32; 2]>,
    played: Vec<Played>,
    /// Message, frame and outcome for everything not Started/Released/Pedal.
    other: Vec<(usize, Msg, Result<Applied, ApplyError>)>,
    peak_voices: usize,
    idle_at: Option<usize>,
    last_event: usize,
    block: usize,
    bound_scripts: usize,
    /// Release-phase capacity reservations were all returned once idle.
    reserve_returned: bool,
    /// The instrument has release-phase (Kontakt release-trigger) zones.
    release_zones: bool,
    /// Live logical notes (input and script-generated) after each block.
    notes: Vec<usize>,
    /// Voices fading after being stolen, after each block.
    stolen: Vec<usize>,
    script_outcomes: Vec<String>,
}

impl Run {
    fn peak(&self) -> f32 {
        self.out.iter().flatten().fold(0f32, |p, x| p.max(x.abs()))
    }
    fn note(&self, key: u8, nth: usize) -> &Played {
        self.played
            .iter()
            .filter(|p| p.key == key)
            .nth(nth)
            .unwrap_or_else(|| panic!("note {key}#{nth} was not admitted"))
    }
    fn energy(&self, from: usize, to: usize) -> f64 {
        self.out[from.min(self.out.len())..to.min(self.out.len())]
            .iter()
            .flatten()
            .map(|x| f64::from(*x).powi(2))
            .sum()
    }
}

fn limits(plan: &sampler_core::Prepared, voices: usize) -> Limits {
    Limits::for_plan(plan, 64, voices)
}

/// `sampler_kontakt::load` with the instrument's scripts replaced by `source`.
fn with_script(
    path: &std::path::Path,
    options: &sampler_kontakt::Options,
    source: &str,
) -> sampler_kontakt::Loaded {
    let sampler_kontakt::Kontakt {
        mut instrument,
        locations,
        mut samples,
    } = sampler_kontakt::read(path).unwrap();
    instrument.behaviors = vec![sampler_ir::Behavior {
        name: "pedal test".into(),
        language: sampler_ir::Language::Ksp,
        source: source.into(),
        slot: None,
        state: Vec::new(),
        requires: Vec::new(),
    }];
    let (low, high) = (*options.keys.start(), *options.keys.end());
    let kept = instrument.retain_zones(|z| z.keys.low <= high && z.keys.high >= low);
    let pcm = kept
        .iter()
        .map(|&asset| {
            let decoded = samples.decode(&locations[asset]).unwrap();
            sampler_core::Pcm::new(decoded.rate, decoded.frames.into_boxed_slice()).unwrap()
        })
        .collect();
    let labels = kept
        .iter()
        .map(|&a| locations[a].display().to_string())
        .collect();
    let loaded = sampler_kontakt::finish(instrument, pcm, labels, options).unwrap();
    let failed: Vec<_> = loaded
        .instrument
        .unsupported
        .iter()
        .filter(|u| u.feature == "script")
        .collect();
    assert!(failed.is_empty(), "{failed:?}");
    loaded
}

/// Play `events` (seconds, message) and render until every note, family and
/// voice has retired, or `TAIL` seconds after the last event.
fn play(setup: Setup, events: &[(f64, Msg)]) -> Option<Run> {
    let path = find(setup.instrument)?;
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let options = sampler_kontakt::Options {
        keys: setup.keys.0..=setup.keys.1,
        // A substitute script always binds.
        scripts: setup.scripts || setup.script.is_some(),
        ..Default::default()
    };
    let loaded = match setup.script {
        None => sampler_kontakt::load(&path, &options, |_| {}).unwrap(),
        Some(source) => with_script(&path, &options, source),
    };
    assert_eq!(loaded.plan.sample_rate() as usize, RATE);
    let failed = loaded
        .instrument
        .unsupported
        .iter()
        .filter(|u| u.feature == "script")
        .count();
    let bound_scripts = if options.scripts {
        loaded.instrument.behaviors.len() - failed
    } else {
        0
    };
    let release_zones = loaded
        .instrument
        .zones
        .iter()
        .any(|z| z.trigger != sampler_ir::Trigger::Attack);
    let limits = limits(&loaded.plan, setup.voices);
    let mut rt = Runtime::new(loaded.plan, limits).unwrap();
    // As a host plays instruments: steal at capacity rather than reject.
    rt.set_voice_stealing(Some(Stealing::for_limits(RATE as u32, setup.voices)))
        .unwrap();
    let mut groups = [None; 16];
    groups[0] = Some(Version::Midi1);
    let mut ingress = Ingress::new(0, groups);
    let mut mpe = match setup.port {
        Port::Channel => None,
        Port::Mpe(members) => Some(Mpe::new(&rt, 0, 0, Zone::Lower, members, 64).unwrap()),
    };

    let mut events: Vec<(usize, Msg, u32)> =
        events.iter().map(|&(t, m)| (at(t), m, m.word())).collect();
    events.sort_by_key(|e| e.0); // Stable: equal instants keep script order.
    let words: Vec<[u32; 1]> = events.iter().map(|e| [e.2]).collect();
    let last_event = events.last().map_or(0, |e| e.0);
    let end = last_event + at(TAIL);

    let mut run = Run {
        out: Vec::new(),
        played: Vec::new(),
        other: Vec::new(),
        peak_voices: 0,
        idle_at: None,
        last_event,
        block: setup.block,
        bound_scripts,
        reserve_returned: false,
        release_zones,
        notes: Vec::new(),
        stolen: Vec::new(),
        script_outcomes: Vec::new(),
    };
    let mut buffer = vec![[0f32; 2]; setup.block];
    let mut next = 0;
    let mut begin = 0;
    while begin < end {
        let len = setup.block;
        let mut batch = Vec::new();
        let mut batch_events = Vec::new();
        while next < events.len() && events[next].0 < begin + len {
            let packet = Packets::new(&words[next]).next().unwrap().unwrap();
            batch.push(TimedPacket {
                offset: events[next].0 - begin,
                packet,
            });
            batch_events.push(events[next]);
            next += 1;
        }
        let mut results = Vec::new();
        match &mut mpe {
            None => ingress
                .render(&mut rt, &mut buffer, &batch, batch.len(), |i, r| {
                    results.push((i, r))
                })
                .unwrap(),
            Some(mpe) => {
                // The MPE adapter has no block driver; this mirrors Ingress::render.
                let mut offset = 0;
                for (i, event) in batch.iter().enumerate() {
                    rt.render(&mut buffer[offset..event.offset]).unwrap();
                    rt.render(&mut []).unwrap();
                    results.push((i, mpe.apply(&mut rt, event.packet)));
                    offset = event.offset;
                }
                rt.render(&mut buffer[offset..]).unwrap();
            }
        }
        for (i, result) in results {
            let (frame, msg, _) = batch_events[i];
            match (msg, result) {
                (On(channel, key, _), Ok(Applied::Started(id))) => run.played.push(Played {
                    id: Some(id),
                    key,
                    channel,
                    on: frame,
                    ..Default::default()
                }),
                (Off(..), Ok(Applied::Released { .. })) | (Cc(..), Ok(Applied::Pedal)) => {}
                (msg, result) => run.other.push((frame, msg, result)),
            }
        }
        // Observe every live note before its owner can retire.
        for played in &mut run.played {
            let Some(id) = played.id else { continue };
            let Ok(context) = rt.release_context(id) else {
                played.id = None;
                continue;
            };
            played.key_at = context.key.map(|k| k.at);
            played.gate_at = context.gate.map(|g| g.at);
            let mut fresh = false;
            for family in rt.note_families(id).unwrap() {
                let trigger = rt.family_trigger(family).unwrap();
                if trigger != Trigger::Attack && !played.release_families.contains(&family) {
                    played.release_families.push(family);
                    played.release_trigger = Some(trigger);
                    fresh = true;
                }
            }
            if fresh {
                played.fired.push(begin);
            }
        }
        rt.flush_behaviors(|_, _, outcome| {
            if !matches!(outcome, Outcome::Finished | Outcome::Cancelled) {
                run.script_outcomes.push(format!("{outcome:?}"));
            }
            true
        });
        rt.flush_ended(|_| true);
        run.notes.push(rt.note_count());
        run.stolen.push(rt.stolen_voices());
        run.peak_voices = run.peak_voices.max(rt.voice_count());
        run.out.extend_from_slice(&buffer);
        begin += len;
        let idle = rt.note_count() == 0 && rt.voice_count() == 0 && rt.family_count() == 0;
        if begin > last_event && idle {
            run.idle_at = Some(begin);
            run.reserve_returned = rt.release_reserve() == Default::default();
            for played in &mut run.played {
                played.id = played.id.filter(|&id| rt.note(id).is_ok());
            }
            break;
        }
    }
    assert_eq!(
        rt.nonfinite_frames(),
        0,
        "{}: non-finite summation",
        setup.instrument
    );
    Some(run)
}

/// Invariants every run must satisfy.
fn sane(run: &Run, name: &str) {
    assert!(
        run.out.iter().flatten().all(|x| x.is_finite()),
        "{name}: non-finite output"
    );
    let idle = run.idle_at.unwrap_or_else(|| {
        panic!(
            "{name}: stuck: notes/voices/families still alive {TAIL}s after the last event; {:?}",
            run.played
                .iter()
                .filter(|p| p.id.is_some())
                .map(|p| (p.key, p.channel, p.on, p.key_at, p.gate_at))
                .collect::<Vec<_>>()
        )
    });
    assert!(idle > run.last_event);
    assert!(run.reserve_returned, "{name}: release reservations leaked");
    for p in &run.played {
        assert!(p.id.is_none(), "{name}: note {} not retired", p.key);
        assert!(
            p.key_at.is_some(),
            "{name}: note {} never saw key-up",
            p.key
        );
        assert!(
            p.gate_at.is_some(),
            "{name}: note {} gate never closed",
            p.key
        );
        // Release triggers sound once per release event, never before key-up
        // and never after the gate closes.
        assert!(
            p.fired.len() <= 1,
            "{name}: note {} fired releases at {:?}",
            p.key,
            p.fired
        );
        if let Some(&fired) = p.fired.first() {
            let key = p.key_at.unwrap() as usize;
            let gate = p.gate_at.unwrap() as usize;
            assert!(
                fired / run.block >= key / run.block && fired / run.block <= gate / run.block,
                "{name}: note {} release at {fired} outside key-up {key}..gate {gate}",
                p.key
            );
        }
    }
    assert!(
        run.script_outcomes.is_empty(),
        "{name}: {:?}",
        run.script_outcomes
    );
}

/// The frame a release trigger is expected at, from the trigger's declared phase.
fn expected_release(p: &Played) -> usize {
    match p.release_trigger.unwrap() {
        Trigger::KeyRelease => p.key_at.unwrap() as usize,
        Trigger::GateRelease => p.gate_at.unwrap() as usize,
        Trigger::Attack => unreachable!(),
    }
}

fn assert_gate(run: &Run, name: &str, key: u8, nth: usize, key_up: f64, gate: f64) {
    let p = run.note(key, nth);
    assert_eq!(
        (p.key_at, p.gate_at),
        (Some(at(key_up) as u64), Some(at(gate) as u64)),
        "{name}: note {key}#{nth} key-up/gate frames"
    );
    if !p.fired.is_empty() {
        assert_eq!(
            p.fired,
            [expected_release(p) / run.block * run.block],
            "{name}: note {key}#{nth} release trigger timing"
        );
    }
}

/// Sustain holds released keys until pedal-up, and only then releases them.
fn sustain(setup: Setup) {
    sustain_run(setup);
}

fn sustain_run(setup: Setup) -> Option<Run> {
    let (a, b) = (setup.keys.0, setup.keys.0 + 4);
    let run = play(
        setup,
        &[
            (0.0, Cc(0, 64, 127)),
            (0.1, On(0, a, 100)),
            (0.1, On(0, b, 90)),
            (0.4, Off(0, a)),
            (0.4, Off(0, b)),
            (1.0, Cc(0, 64, 0)),
        ],
    )?;
    let name = "sustain";
    sane(&run, name);
    assert!(run.other.is_empty(), "{name}: {:?}", run.other);
    assert_gate(&run, name, a, 0, 0.4, 1.0);
    assert_gate(&run, name, b, 0, 0.4, 1.0);
    assert!(
        run.energy(at(0.5), at(1.0)) > 0.0,
        "{name}: silent while sustained"
    );
    Some(run)
}

/// Pedal pressed after the notes: a key released before the pedal is not
/// captured; one still held at pedal-down is.
fn pedal_after_notes(setup: Setup) {
    let (a, b) = (setup.keys.0, setup.keys.0 + 4);
    let Some(run) = play(
        setup,
        &[
            (0.0, On(0, a, 100)),
            (0.0, On(0, b, 100)),
            (0.2, Off(0, a)),
            (0.3, Cc(0, 64, 127)),
            (0.5, Off(0, b)),
            (1.0, Cc(0, 64, 0)),
        ],
    ) else {
        return;
    };
    let name = "pedal after notes";
    sane(&run, name);
    assert_gate(&run, name, a, 0, 0.2, 0.2);
    assert_gate(&run, name, b, 0, 0.5, 1.0);
}

/// Pedal lifted while keys are held: the gate follows the key; pressing it
/// again while a key is still down captures that key again.
fn pedal_up_while_held(setup: Setup) {
    let (a, b) = (setup.keys.0, setup.keys.0 + 4);
    let Some(run) = play(
        setup,
        &[
            (0.0, Cc(0, 64, 127)),
            (0.1, On(0, a, 100)),
            (0.1, On(0, b, 100)),
            (0.3, Cc(0, 64, 0)),
            (0.35, Off(0, a)),
            (0.4, Cc(0, 64, 127)),
            (0.6, Off(0, b)),
            (0.9, Cc(0, 64, 0)),
        ],
    ) else {
        return;
    };
    let name = "pedal up while held";
    sane(&run, name);
    assert_gate(&run, name, a, 0, 0.35, 0.35);
    assert_gate(&run, name, b, 0, 0.6, 0.9);
}

/// Half-pedal values: CC64 is a switch at 64; repeated down values change
/// nothing, and a slow lift releases at the first value below 64.
fn half_pedal(setup: Setup) {
    let (a, b, c) = (setup.keys.0, setup.keys.0 + 4, setup.keys.0 + 7);
    let mut events = vec![
        (0.0, Cc(0, 64, 63)),
        (0.1, On(0, a, 100)),
        (0.2, Off(0, a)),
        (0.3, Cc(0, 64, 64)),
        (0.35, On(0, b, 100)),
        (0.4, Off(0, b)),
        (0.5, Cc(0, 64, 100)),
        (0.55, Cc(0, 64, 64)),
        (0.6, Cc(0, 64, 40)),
        (0.7, Cc(0, 64, 127)),
        (0.75, On(0, c, 100)),
        (0.8, Off(0, c)),
    ];
    for (i, value) in [120, 100, 80, 70, 64, 63, 30, 0].into_iter().enumerate() {
        events.push((0.9 + 0.01 * i as f64, Cc(0, 64, value)));
    }
    let Some(run) = play(setup, &events) else {
        return;
    };
    let name = "half pedal";
    sane(&run, name);
    assert_gate(&run, name, a, 0, 0.2, 0.2);
    assert_gate(&run, name, b, 0, 0.4, 0.6);
    assert_gate(&run, name, c, 0, 0.8, 0.95);
}

/// Pedal and note messages at the same sample apply in arrival order, and a
/// lift-and-press within one sample still releases what the lift released.
fn same_instant(setup: Setup) {
    let (a, b, c) = (setup.keys.0, setup.keys.0 + 4, setup.keys.0 + 7);
    let Some(run) = play(
        setup,
        &[
            (0.1, On(0, a, 100)),
            (0.1, On(0, b, 100)),
            (0.2, Off(0, a)),
            (0.2, Cc(0, 64, 127)),
            (0.3, Off(0, b)),
            (0.5, Cc(0, 64, 0)),
            (0.5, On(0, a, 100)),
            (0.5, Cc(0, 64, 127)),
            (0.6, Off(0, a)),
            (0.8, Cc(0, 64, 0)),
            (0.9, Cc(0, 66, 127)),
            (0.9, On(0, c, 100)),
            (1.0, Off(0, c)),
            (1.1, Cc(0, 66, 0)),
        ],
    ) else {
        return;
    };
    let name = "same instant";
    sane(&run, name);
    assert_gate(&run, name, a, 0, 0.2, 0.2);
    assert_gate(&run, name, b, 0, 0.3, 0.5);
    assert_gate(&run, name, a, 1, 0.6, 0.8);
    assert_gate(&run, name, c, 0, 1.0, 1.0);
}

/// Sostenuto holds exactly the notes held at pedal-down.
fn sostenuto(setup: Setup) {
    let (a, b, c) = (setup.keys.0, setup.keys.0 + 4, setup.keys.0 + 7);
    let Some(run) = play(
        setup,
        &[
            (0.0, On(0, a, 100)),
            (0.0, On(0, c, 100)),
            (0.05, Off(0, c)),
            (0.1, Cc(0, 66, 127)),
            (0.2, On(0, b, 100)),
            // A repeated pedal-down value is not a new press: no recapture.
            (0.25, Cc(0, 66, 100)),
            (0.3, Off(0, a)),
            (0.4, Off(0, b)),
            // Re-struck while held by sostenuto: the new note is not captured.
            (0.5, On(0, a, 100)),
            (0.6, Off(0, a)),
            (1.0, Cc(0, 66, 0)),
        ],
    ) else {
        return;
    };
    let name = "sostenuto";
    sane(&run, name);
    assert_gate(&run, name, c, 0, 0.05, 0.05);
    assert_gate(&run, name, a, 0, 0.3, 1.0);
    assert_gate(&run, name, b, 0, 0.4, 0.4);
    assert_gate(&run, name, a, 1, 0.6, 0.6);
}

/// Both pedals: sustain holds everything until its lift, sostenuto keeps its
/// captured note beyond that.
fn sustain_and_sostenuto(setup: Setup) {
    let (a, b) = (setup.keys.0, setup.keys.0 + 4);
    let Some(run) = play(
        setup,
        &[
            (0.0, On(0, a, 100)),
            (0.1, Cc(0, 66, 127)),
            (0.2, Cc(0, 64, 127)),
            (0.25, On(0, b, 100)),
            (0.3, Off(0, a)),
            (0.3, Off(0, b)),
            (0.6, Cc(0, 64, 0)),
            (0.9, Cc(0, 66, 0)),
        ],
    ) else {
        return;
    };
    let name = "sustain and sostenuto";
    sane(&run, name);
    assert_gate(&run, name, a, 0, 0.3, 0.9);
    assert_gate(&run, name, b, 0, 0.3, 0.6);
}

/// Channel-mode messages under the sustain pedal. All Notes Off releases held
/// keys but the pedal still holds them; All Sound Off silences at once with no
/// release phase, before or after the late key-up. Reset All Controllers
/// lifts the pedal (RP-015), releasing the held notes before the CC64 lift.
fn channel_mode(setup: Setup) {
    let (a, b, c) = (setup.keys.0, setup.keys.0 + 4, setup.keys.0 + 7);
    let Some(run) = play(
        setup,
        &[
            (0.0, Cc(0, 64, 127)),
            (0.1, On(0, a, 100)),
            (0.1, On(0, b, 100)),
            (0.2, Off(0, a)),
            (0.3, Cc(0, 123, 0)),
            (0.4, Cc(0, 121, 0)),
            (0.6, Cc(0, 64, 0)),
            (0.8, On(0, c, 100)),
            (0.9, Cc(0, 120, 0)),
            (1.0, Off(0, c)),
        ],
    ) else {
        return;
    };
    let name = "channel mode";
    sane(&run, name);
    let other: Vec<_> = run.other.iter().map(|o| (o.0, o.1)).collect();
    assert_eq!(
        other,
        [
            (at(0.3), Cc(0, 123, 0)),
            (at(0.4), Cc(0, 121, 0)),
            (at(0.9), Cc(0, 120, 0))
        ]
    );
    assert_eq!(run.other[0].2, Ok(Applied::AllNotesOff { released: 1 }));
    // Reset All Controllers lifts the pedal (RP-015): the held notes go.
    assert_eq!(run.other[1].2, Ok(Applied::ResetControllers));
    assert!(matches!(run.other[2].2, Ok(Applied::AllSoundOff { stopped }) if stopped > 0));
    assert_gate(&run, name, a, 0, 0.2, 0.4);
    assert_gate(&run, name, b, 0, 0.3, 0.4);
    let p = run.note(c, 0);
    assert_eq!(
        (p.key_at, p.gate_at),
        (Some(at(1.0) as u64), Some(at(0.9) as u64))
    );
    assert!(
        p.fired.is_empty(),
        "{name}: release phase after All Sound Off"
    );
    assert_eq!(
        run.energy(at(0.9) + 1, at(1.0) + BLOCK),
        0.0,
        "{name}: sound after All Sound Off"
    );
}

/// CC67 (una corda) is an ordinary controller to the native core: it never
/// holds or releases notes. With no script bound nothing reads it, so the
/// render is identical to the same performance without it.
fn soft_pedal(setup: Setup) {
    let (a, b) = (setup.keys.0, setup.keys.0 + 4);
    let Some(plain) = sustain_run(setup) else {
        return;
    };
    let Some(run) = play(
        setup,
        &[
            (0.0, Cc(0, 64, 127)),
            (0.05, Cc(0, 67, 127)),
            (0.1, On(0, a, 100)),
            (0.1, On(0, b, 90)),
            (0.4, Off(0, a)),
            (0.4, Off(0, b)),
            (0.7, Cc(0, 67, 0)),
            (1.0, Cc(0, 64, 0)),
        ],
    ) else {
        return;
    };
    let name = "soft pedal";
    sane(&run, name);
    assert_eq!(
        run.other.iter().map(|o| o.2).collect::<Vec<_>>(),
        [Ok(Applied::Controller); 2]
    );
    assert_gate(&run, name, a, 0, 0.4, 1.0);
    assert_gate(&run, name, b, 0, 0.4, 1.0);
    if run.bound_scripts == 0 {
        assert!(
            run.out == plain.out,
            "{name}: CC67 changed an unscripted render"
        );
    }
}

/// A sustained key struck again keeps its first note (no same-key choke in
/// the native core) and both notes release at pedal-up, once each.
/// A held key struck twice without a note-off pairs note-offs first-in-first-out.
fn retrigger(setup: Setup) {
    let a = setup.keys.0;
    let b = a + 4;
    let Some(run) = play(
        setup,
        &[
            (0.0, Cc(0, 64, 127)),
            (0.1, On(0, a, 100)),
            (0.2, Off(0, a)),
            (0.3, On(0, a, 80)),
            (0.4, Off(0, a)),
            (0.8, Cc(0, 64, 0)),
            (1.0, On(0, b, 100)),
            (1.1, On(0, b, 70)),
            (1.2, Off(0, b)),
            (1.3, Off(0, b)),
            // A note-off with no held key is reported, not applied to anything.
            (1.4, Off(0, b)),
        ],
    ) else {
        return;
    };
    let name = "retrigger";
    sane(&run, name);
    assert_eq!(run.played.len(), 4);
    assert_gate(&run, name, a, 0, 0.2, 0.8);
    assert_gate(&run, name, a, 1, 0.4, 0.8);
    assert_gate(&run, name, b, 0, 1.2, 1.2);
    assert_gate(&run, name, b, 1, 1.3, 1.3);
    assert_eq!(
        run.other,
        [(
            at(1.4),
            Off(0, b),
            Err(ApplyError::Core(sampler_core::Error::StaleHandle))
        )]
    );
}

/// MPE lower zone, three members: manager pedals hold every member; member
/// pedals are ignored; a recycled member channel replays the same key while
/// the first is sustained, and each note keeps its own release.
fn mpe(setup: Setup) {
    let setup = Setup {
        port: Port::Mpe(3),
        ..setup
    };
    let (a, b, c) = (setup.keys.0, setup.keys.0 + 4, setup.keys.0 + 7);
    let Some(run) = play(
        setup,
        &[
            (0.0, On(1, a, 100)),
            (0.0, On(2, b, 100)),
            (0.0, On(3, c, 100)),
            (0.04, Cc(3, 64, 127)),
            (0.05, Off(3, c)),
            (0.1, Cc(0, 64, 127)),
            (0.2, Off(1, a)),
            (0.3, On(1, a, 90)),
            (0.4, Off(1, a)),
            (0.4, Off(2, b)),
            (0.5, Cc(2, 64, 0)),
            (0.8, Cc(0, 64, 0)),
            // Manager sostenuto captures the members held at pedal-down, not later ones.
            (1.0, On(2, a, 100)),
            (1.0, On(3, b, 100)),
            (1.1, Cc(0, 66, 127)),
            (1.2, Off(2, a)),
            (1.3, On(1, c, 100)),
            (1.4, Off(1, c)),
            (1.5, Off(3, b)),
            (1.6, Cc(3, 66, 0)),
            (1.8, Cc(0, 66, 0)),
        ],
    ) else {
        return;
    };
    let name = "mpe";
    sane(&run, name);
    assert_eq!(
        run.other
            .iter()
            .map(|o| (o.0, o.1, o.2))
            .collect::<Vec<_>>(),
        [
            (at(0.04), Cc(3, 64, 127), Ok(Applied::Ignored)),
            (at(0.5), Cc(2, 64, 0), Ok(Applied::Ignored)),
            (at(1.6), Cc(3, 66, 0), Ok(Applied::Ignored)),
        ]
    );
    let channels: Vec<_> = run.played.iter().map(|p| (p.channel, p.key)).collect();
    assert_eq!(
        channels,
        [(1, a), (2, b), (3, c), (1, a), (2, a), (3, b), (1, c)]
    );
    assert_gate(&run, name, c, 0, 0.05, 0.05);
    assert_gate(&run, name, a, 0, 0.2, 0.8);
    assert_gate(&run, name, b, 0, 0.4, 0.8);
    assert_gate(&run, name, a, 1, 0.4, 0.8);
    assert_gate(&run, name, a, 2, 1.2, 1.8);
    assert_gate(&run, name, c, 1, 1.4, 1.4);
    assert_gate(&run, name, b, 1, 1.5, 1.8);
}

/// A voice pool that fits two notes plus stealing headroom
/// ([`Stealing::for_limits`]): every note is admitted, later notes (and the
/// release triggers at pedal-up) steal the oldest released, then quietest,
/// voices, and each stolen voice fades out over the 10 ms steal fade rather
/// than being cut or rejecting the note.
fn voice_limit(setup: Setup) {
    let a = setup.keys.0;
    let Some(two) = play(
        setup,
        &[
            (0.0, Cc(0, 64, 127)),
            (0.0, On(0, a, 100)),
            (0.05, On(0, a + 2, 100)),
            (0.3, Off(0, a)),
            (0.3, Off(0, a + 2)),
            (0.6, Cc(0, 64, 0)),
        ],
    ) else {
        return;
    };
    sane(&two, "voice probe");
    // Polyphony (voices less a quarter of headroom) is 1.5 times what two
    // notes use under the pedal: four notes cannot all fit.
    let voices = 2 * two.peak_voices.max(2);
    let setup = Setup { voices, ..setup };
    let keys = [a, a + 2, a + 4, a + 7];
    let mut events = vec![(0.0, Cc(0, 64, 127))];
    for (i, &key) in keys.iter().enumerate() {
        events.push((0.05 * i as f64, On(0, key, 100)));
        events.push((0.3, Off(0, key)));
    }
    events.push((0.6, Cc(0, 64, 0)));
    let late = 0.6 + TAIL / 2.0;
    events.push((late, On(0, a, 100)));
    events.push((late + 0.2, Off(0, a)));
    let Some(run) = play(setup, &events) else {
        return;
    };
    let name = "voice limit";
    sane(&run, name);
    assert!(run.peak_voices <= voices);
    assert_eq!(run.other, [], "{name}: no admission is rejected");
    let admitted: Vec<u8> = run.played.iter().map(|p| p.key).collect();
    assert_eq!(admitted, [a, a + 2, a + 4, a + 7, a], "{name}: admitted keys");
    // Stolen voices fade: they outlive the stealing block, and every one is
    // gone within the fade (plus the block it started in and the next).
    let fade = Stealing::for_limits(RATE as u32, voices).fade as usize;
    assert!(
        run.stolen.iter().any(|&n| n > 0),
        "{name}: nothing was stolen at {voices} voices"
    );
    let longest = run
        .stolen
        .split(|&n| n == 0)
        .map(<[usize]>::len)
        .max()
        .unwrap_or(0);
    assert!(
        longest <= fade.div_ceil(run.block) + 2,
        "{name}: stolen voices faded for {longest} blocks"
    );
    assert_gate(&run, name, a + 7, 0, 0.3, 0.6);
    assert_gate(&run, name, a, 1, late + 0.2, late + 0.2);
}

/// Same input, same PCM: across runs, across host block partitions, and (with
/// no script bound) with scripts unbound.
fn determinism(setup: Setup) {
    let Some(first) = sustain_run(setup) else {
        return;
    };
    let Some(again) = sustain_run(setup) else {
        return;
    };
    assert!(again.out == first.out, "repeat render differs");
    let Some(split) = sustain_run(Setup { block: 61, ..setup }) else {
        return;
    };
    let n = first.out.len().min(split.out.len());
    assert!(
        split.out[..n] == first.out[..n],
        "block partition changes PCM"
    );
    let Some(unbound) = sustain_run(Setup {
        scripts: false,
        ..setup
    }) else {
        return;
    };
    if first.bound_scripts == 0 {
        assert!(unbound.out == first.out, "unbound scripts change PCM");
    }
}

fn record(name: &str, run: &Run) {
    eprintln!(
        "{name}: {} scripts bound, peak {:.3}, {} voices at most, idle {:.2}s after the last event",
        run.bound_scripts,
        run.peak(),
        run.peak_voices,
        (run.idle_at.unwrap() - run.last_event) as f64 / RATE as f64
    );
}

/// One test per scenario and instrument.
macro_rules! matrix {
    ($setup:expr; $($scenario:ident),*) => {
        $(#[test] fn $scenario() { super::$scenario($setup) })*
    };
}

/// Una Corda Pure, C4..G4: felt piano with scripted pedal noise, resonance,
/// repedalling and release noise ("MAIN", "RESONANCE", "RELEASE", "REPEDAL").
mod una_corda_pure {
    use super::*;
    // Its script owns the pedal (NO_SYS_SCRIPT_PEDAL): it ignores the host
    // notes and sustains its own, so the per-note gate expectations here do
    // not apply to the silent host notes. Script-driven pedal behaviour is
    // the sampler-ksp suite's; these scenarios exercise the native layers.
    const SETUP: Setup = Setup {
        scripts: false,
        ..Setup::new(UNA_CORDA, (60, 67))
    };

    #[test]
    fn profile() {
        let Some(plain) = sustain_run(SETUP) else {
            return;
        };
        record("Una Corda Pure", &plain);
        // Its release noise has no Kontakt release-trigger group: only the
        // "RELEASE" script plays it, so unscripted notes fire no release phase.
        assert!(!plain.release_zones);
        assert!(plain.peak() < 1.0, "peak {}", plain.peak());
    }

    matrix!(SETUP; sustain, pedal_after_notes, pedal_up_while_held, half_pedal, same_instant,
        sostenuto, sustain_and_sostenuto, soft_pedal, retrigger, channel_mode, mpe, voice_limit,
        determinism, scripted);
}

/// Vista 3 Cellos, C3..G3: sixteen Kontakt release-trigger groups (normal and
/// legato releases, four dynamics, two mic sets) and a legato script.
mod vista_3_cellos {
    use super::*;
    // Its legato script tracks held keys through %KEY_DOWN and
    // search(%KEY_DOWN, 1), which sampler-ksp does not maintain yet (reads 0,
    // -1): bound, it never plays. Exercise the native release-trigger path.
    const SETUP: Setup = Setup {
        scripts: false,
        ..Setup::new(CELLOS, (48, 55))
    };

    #[test]
    fn profile() {
        let Some(plain) = sustain_run(SETUP) else {
            return;
        };
        record("Vista 3 Cellos", &plain);
        // Kontakt release-trigger groups lower to GateRelease: one release
        // phase per released key, when the pedal lets the note go, not at
        // key-up (Kontakt's system pedal script holds the note-off).
        assert!(plain.release_zones);
        for p in &plain.played {
            assert_eq!(p.release_trigger, Some(Trigger::GateRelease));
            assert!(p.key_at < p.gate_at);
            assert_eq!(p.fired, [block_of(p.gate_at.unwrap() as usize)]);
        }
        assert!(plain.peak() < 1.5, "peak {}", plain.peak());
    }

    matrix!(SETUP; sustain, pedal_after_notes, pedal_up_while_held, half_pedal, same_instant,
        sostenuto, sustain_and_sostenuto, soft_pedal, retrigger, channel_mode, mpe, voice_limit,
        determinism, scripted);
}

/// ANALOG STRINGS, C4..G4: 480 groups with LFO-modulated layers that the
/// "Analog Strings" script selects between. Unscripted, every layer sounds,
/// so it runs the scenarios where LFO voices matter, with a large pool.
mod analog_strings {
    use super::*;
    // Bound, its scripts are silent (they rely on %KEY_DOWN, %CC_TOUCHED,
    // sort and computed find_mod, not maintained or executed by sampler-ksp
    // yet): exercise the native layers.
    const SETUP: Setup = Setup {
        voices: 4096,
        scripts: false,
        ..Setup::new(ANALOG, (60, 67))
    };

    #[test]
    fn profile() {
        let Some(plain) = sustain_run(SETUP) else {
            return;
        };
        record("ANALOG STRINGS", &plain);
        assert!(!plain.release_zones);
        // All 480 layers summed are far over full scale until the script binds.
        let ceiling = if plain.bound_scripts == 0 { 16.0 } else { 2.0 };
        assert!(plain.peak() < ceiling, "peak {}", plain.peak());
    }

    matrix!(SETUP; sostenuto, soft_pedal, retrigger, determinism);
}

/// A script that takes over the sustain pedal the way Vista's does: it
/// consumes CC64, holds note-offs while the pedal is down, and sends them at
/// pedal-up.
const SCRIPT_SUSTAIN: &str = "on init
    declare %ids[128]
    declare %held[128]
    declare $key
end on
on release
    if (%CC[64] > 63)
        ignore_event($EVENT_ID)
        %ids[$EVENT_NOTE] := $EVENT_ID
        %held[$EVENT_NOTE] := 1
    end if
end on
on controller
    if ($CC_NUM = 64)
        ignore_controller
        if (%CC[64] < 64)
            $key := 0
            while ($key < 128)
                if (%held[$key] = 1)
                    note_off(%ids[$key])
                    %held[$key] := 0
                end if
                $key := $key + 1
            end while
        end if
    end if
end on";

/// A script that swallows CC64 and does nothing with it: no sustain at all.
const SCRIPT_NO_PEDAL: &str = "on controller
    if ($CC_NUM = 64)
        ignore_controller
    end if
end on";

/// A script release noise in the style of Una Corda's: one fixed-length note
/// per key-up, leaving the pedal to the native core.
const SCRIPT_RELEASE_NOISE: &str = "on release
    play_note($EVENT_NOTE, 40, 0, 300000)
end on";

/// Scripts bound to the real instrument and consuming pedal/release events.
fn scripted(setup: Setup) {
    let events = [
        (0.0, Cc(0, 64, 127)),
        (0.1, On(0, setup.keys.0, 100)),
        (0.1, On(0, setup.keys.0 + 4, 90)),
        (0.4, Off(0, setup.keys.0)),
        (0.4, Off(0, setup.keys.0 + 4)),
        (1.0, Cc(0, 64, 0)),
    ];
    let (a, b) = (setup.keys.0, setup.keys.0 + 4);

    let Some(run) = play(
        Setup {
            script: Some(SCRIPT_SUSTAIN),
            ..setup
        },
        &events,
    ) else {
        return;
    };
    let name = "script sustain";
    sane(&run, name);
    assert_eq!(run.bound_scripts, 1);
    for key in [a, b] {
        let p = run.note(key, 0);
        assert_eq!(
            (p.key_at, p.gate_at),
            (Some(at(0.4) as u64), Some(at(1.0) as u64)),
            "{name}: note {key}"
        );
        // The ignored note-off is the release event only when the script
        // finally sends it: release triggers wait for pedal-up.
        let expected: &[usize] = if run.release_zones {
            &[block_of(at(1.0))]
        } else {
            &[]
        };
        assert_eq!(p.fired, expected, "{name}: note {key} release");
    }

    let Some(run) = play(
        Setup {
            script: Some(SCRIPT_NO_PEDAL),
            ..setup
        },
        &events,
    ) else {
        return;
    };
    let name = "script without pedal";
    sane(&run, name);
    assert_gate(&run, name, a, 0, 0.4, 0.4);
    assert_gate(&run, name, b, 0, 0.4, 0.4);

    let Some(run) = play(
        Setup {
            script: Some(SCRIPT_RELEASE_NOISE),
            ..setup
        },
        &events,
    ) else {
        return;
    };
    let name = "script release noise";
    sane(&run, name);
    assert_gate(&run, name, a, 0, 0.4, 1.0);
    assert_gate(&run, name, b, 0, 0.4, 1.0);
    // Two held inputs plus exactly one generated note per key-up.
    let notes = |t: f64| run.notes[at(t) / BLOCK];
    assert_eq!((notes(0.3), notes(0.45)), (2, 4), "{name}");
    assert_eq!(run.notes.iter().max(), Some(&4), "{name}");
    assert!(run.energy(at(0.4), at(0.7)) > 0.0);
}

fn block_of(frame: usize) -> usize {
    frame / BLOCK * BLOCK
}

