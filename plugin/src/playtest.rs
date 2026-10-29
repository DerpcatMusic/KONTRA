//! Playability through the plugin's own process path: a performance of the
//! kinds that left notes stuck in a host (fast repeats, overlapping legato,
//! the sustain pedal, chords, note-offs beside controllers in one block,
//! velocity-0 note-ons, CLAP note-offs with a wildcard id), played on real
//! libraries. Every note must sound on time, every note must end, nothing
//! may sound late, and voices are stolen only where the plan sets a limit.

use std::path::{Path, PathBuf};

use kontra_host::state::{Saved, SavedPart};
use moose::core::{ExactEvent, ExactEventBody, ExactNoteAddress, ExactNoteKind};
use moose::prelude::*;

use super::tests::{BLOCK, KONTAKT, RATE, activate};
use crate::{Dsp, Kontra, KontraParams, Tick};

const UVI: &str = "/data/main/Libraries/UVI";

/// The longest a release tail may ring after a segment's last note-off.
const TAIL_S: f64 = 20.0;

/// One input event, typed MIDI or a host-native (CLAP) note.
#[derive(Clone, Copy)]
enum In {
    Midi(EventBody),
    Clap(ExactEventBody),
}

impl In {
    fn starts(self) -> bool {
        match self {
            In::Midi(EventBody::NoteOn { velocity, .. }) => velocity > 0,
            In::Clap(ExactEventBody::Note { kind, .. }) => kind == ExactNoteKind::On,
            _ => false,
        }
    }
}

fn on(key: u8) -> In {
    In::Midi(EventBody::NoteOn {
        group: 0,
        channel: 0,
        note: key,
        velocity: 100,
    })
}

fn off(key: u8) -> In {
    In::Midi(EventBody::NoteOff {
        group: 0,
        channel: 0,
        note: key,
        velocity: 0,
    })
}

fn cc(cc: u8, value: u8) -> In {
    In::Midi(EventBody::ControlChange {
        group: 0,
        channel: 0,
        cc,
        value,
    })
}

fn bend(value: u16) -> In {
    In::Midi(EventBody::PitchBend {
        group: 0,
        channel: 0,
        value,
    })
}

fn clap(kind: ExactNoteKind, key: u8, id: i32) -> In {
    In::Clap(ExactEventBody::Note {
        kind,
        address: ExactNoteAddress::from_raw_signed(0, 0, i16::from(key), id),
        velocity: 0.8,
    })
}

/// Seconds as frames.
fn at(s: f64) -> usize {
    (s * RATE) as usize
}

/// The performance, a segment at a time: (name, [(frame, event)]).
fn performance() -> Vec<(&'static str, Vec<(usize, In)>)> {
    let probe = vec![(137, on(60)), (at(0.6), off(60))];
    let mut repeats = Vec::new();
    for i in 0..16 {
        repeats.push((at(0.07 * f64::from(i)) + 11, on(62)));
        repeats.push((at(0.07 * f64::from(i) + 0.045) + 11, off(62)));
    }
    let mut legato = Vec::new();
    for (i, key) in [60, 62, 64, 65, 67].into_iter().enumerate() {
        let t = 0.3 * i as f64;
        legato.push((at(t), on(key)));
        legato.push((at(t + 0.35), off(key)));
    }
    let mut pedal = vec![(0, cc(64, 127))];
    for (i, key) in [60, 64, 67].into_iter().enumerate() {
        let t = 0.1 * (i + 1) as f64;
        pedal.push((at(t), on(key)));
        pedal.push((at(t + 0.15), off(key)));
    }
    pedal.push((at(1.2), cc(64, 0)));
    let keys = [48, 52, 55, 60, 64, 67, 72, 76];
    let mut chord: Vec<_> = keys
        .iter()
        .enumerate()
        .map(|(i, &k)| (i % 2 * 3, on(k)))
        .collect();
    chord.extend(keys.iter().map(|&k| (at(1.0), off(k))));
    // The note-off shares its frame with a controller and a bend.
    let same = vec![
        (0, on(64)),
        (at(0.5), cc(1, 64)),
        (at(0.5), bend(12_000)),
        (at(0.5), off(64)),
        (at(0.6), bend(8192)),
        (at(0.6), cc(1, 127)),
    ];
    let zero = vec![
        (0, on(65)),
        (
            at(0.4),
            In::Midi(EventBody::NoteOn {
                group: 0,
                channel: 0,
                note: 65,
                velocity: 0,
            }),
        ),
    ];
    let native = vec![
        (0, clap(ExactNoteKind::On, 67, 5)),
        (0, clap(ExactNoteKind::On, 69, 6)),
        (at(0.4), clap(ExactNoteKind::Off, 67, -1)),
        (at(0.4), off(69)),
    ];
    vec![
        ("probe", probe.clone()),
        ("fast repeats", repeats),
        ("overlapping legato", legato),
        ("sustain pedal", pedal),
        ("8-note chord", chord),
        ("note-off beside CC and bend", same),
        ("velocity-0 note-on", zero),
        ("CLAP note-off id -1", native),
        ("probe again", probe),
    ]
}

/// What a block left: its mono output and the rack's stats after it.
#[derive(Default)]
struct Played {
    audio: Vec<f32>,
    voices: u32,
    stolen: u64,
    refused: u64,
}

fn block(params: &KontraParams, dsp: &mut Dsp, events: &[(usize, In)], last: &Played) -> Played {
    let mut list = EventList::with_capacity(64);
    for &(offset, e) in events {
        let offset = u32::try_from(offset).unwrap_or(0);
        match e {
            In::Midi(body) => list.push(moose::prelude::Event::new(offset, body)),
            In::Clap(body) => list.try_push_exact(ExactEvent::new(offset, body)).unwrap(),
        }
    }
    let mut channels = vec![vec![0.0f32; BLOCK]; 2 * crate::OUTPUTS];
    {
        let mut outs: Vec<&mut [f32]> = channels.iter_mut().map(Vec::as_mut_slice).collect();
        let mut buffer = AudioBuffer::from_slices_checked(&[], &mut outs, BLOCK);
        let transport = TransportInfo::default();
        let mut out_events = EventList::with_capacity(16);
        // Offline, so a slow debug build never starves a voice of data.
        let mut cx = ProcessContext::new(&transport, RATE, BLOCK, &mut out_events)
            .with_process_mode(ProcessMode::Offline);
        <Kontra as PluginLogic>::process(dsp, params, &mut buffer, &list, &mut cx);
    }
    let audio = channels[0]
        .iter()
        .zip(&channels[1])
        .map(|(l, r)| l + r)
        .collect();
    // The control tick may have taken this block's stats: keep the last.
    match crate::lock(&params.shared.rack)
        .as_ref()
        .and_then(|r| r.stats())
    {
        Some(s) => Played {
            audio,
            voices: s.voices,
            stolen: s.voices_stolen,
            refused: s.notes_refused + s.dropped,
        },
        None => Played { audio, ..*last },
    }
}

/// Whether the plan limits voices anywhere a steal could be fair.
fn limited(params: &KontraParams) -> (bool, usize) {
    let rack = crate::lock(&params.shared.rack);
    let plan = rack
        .as_ref()
        .and_then(|r| r.parts().first()?.as_ref()?.plan.clone());
    plan.map_or((true, 0), |p| {
        (
            !p.polyphony.is_empty() || !p.voice_groups.is_empty(),
            p.voices,
        )
    })
}

/// Plays the performance on `path`; what failed.
fn perform(path: PathBuf) -> Vec<String> {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let saved = Saved::new(vec![SavedPart {
        part: 0,
        path,
        channel: 0,
        output: 0,
        gain: 1.0,
        pan: 0.0,
        solo: false,
        mute: false,
        scripts: Vec::new(),
        delays: None,
        slot_types: Vec::new(),
        slot_responses: Vec::new(),
    }]);
    let started = std::time::Instant::now();
    let (params, mut dsp) = activate(&saved.write());
    eprintln!("{name}: loaded in {:.1?}", started.elapsed());
    params.align.set_value(true);
    Tick.run(&params);
    let mut failed = Vec::new();
    let mut fail = |what: String| failed.push(format!("{name}: {what}"));
    // The wheel and expression full, as a player starts.
    let mut last = Played::default();
    last = block(
        &params,
        &mut dsp,
        &[(0, cc(1, 127)), (0, cc(11, 127))],
        &last,
    );
    for _ in 0..at(0.5) / BLOCK {
        last = block(&params, &mut dsp, &[], &last);
    }
    Tick.run(&params);
    let latency_ms = <Kontra as PluginLogic>::latency(&dsp) as f32 * 1000.0 / RATE as f32;
    let (limited, pool) = limited(&params);
    let mut onsets = Vec::new();
    let mut blocks = 0usize;
    for (segment, events) in performance() {
        let (stolen, refused) = (last.stolen, last.refused);
        let end = events.iter().map(|e| e.0).max().unwrap_or(0) + 1;
        let starts: Vec<usize> = events
            .iter()
            .filter(|e| e.1.starts())
            .map(|e| e.0)
            .collect();
        let (mut audio, mut peaks, mut peak_voices) = (Vec::new(), Vec::new(), 0u32);
        let mut frame = 0;
        // The segment, then its tail until no voice is left.
        loop {
            let inside: Vec<_> = (events.iter())
                .filter(|e| (frame..frame + BLOCK).contains(&e.0))
                .map(|&(f, e)| (f - frame, e))
                .collect();
            last = block(&params, &mut dsp, &inside, &last);
            blocks += 1;
            if blocks % 10 == 0 {
                Tick.run(&params);
            }
            peaks.push(last.audio.iter().fold(0.0f32, |p, x| p.max(x.abs())));
            audio.extend_from_slice(&last.audio);
            peak_voices = peak_voices.max(last.voices);
            frame += BLOCK;
            if frame >= end && last.voices == 0 {
                break;
            }
            if frame > end + at(TAIL_S) {
                fail(format!(
                    "{segment}: {} voices still on {TAIL_S} s after the last note-off, at {:.1} dBFS",
                    last.voices,
                    20.0 * peaks.last().copied().unwrap_or(0.0).max(1e-9).log10()
                ));
                break;
            }
        }
        if starts.is_empty() {
            continue;
        }
        // Every note sounds: something plays after each note-on.
        let hold = at(f64::from(latency_ms) / 1000.0 + 0.05);
        let loudest = peaks.iter().copied().fold(0.0, f32::max);
        if loudest < 1e-4 {
            fail(format!("{segment}: silent"));
        }
        for &s in &starts {
            let after = audio
                .get(s..(s + hold + at(0.1)).min(audio.len()))
                .unwrap_or(&[]);
            if after.iter().all(|x| x.abs() < 1e-5) {
                fail(format!(
                    "{segment}: the note at {:.3} s never sounded",
                    s as f64 / RATE
                ));
            }
        }
        if segment.starts_with("probe") {
            let from = starts.first().copied().unwrap_or(0);
            let onset = kontra_align::onset_ms(&audio[from..], RATE, kontra_align::ONSET_DB);
            match onset {
                Some(ms) if ms <= latency_ms + 10.0 => onsets.push(ms),
                Some(ms) => fail(format!(
                    "{segment}: onset {ms:.1} ms past the {latency_ms:.1} ms hold"
                )),
                None => fail(format!("{segment}: no onset")),
            }
        }
        // No late bursts: a jump of 12 dB or more with no input near it.
        let inputs: Vec<usize> = events.iter().map(|e| e.0).collect();
        for (b, w) in peaks.windows(5).enumerate() {
            let (before, now) = (w[..4].iter().copied().fold(0.0, f32::max), w[4]);
            let start = (b + 4) * BLOCK;
            let near = inputs
                .iter()
                .any(|&i| i <= start + BLOCK && start <= i + hold);
            if now > 1e-3 && now > 4.0 * before && !near {
                fail(format!(
                    "{segment}: a burst at {:.3} s",
                    start as f64 / RATE
                ));
            }
        }
        if last.refused > refused {
            fail(format!(
                "{segment}: {} notes refused or events dropped",
                last.refused - refused
            ));
        }
        let steals = last.stolen - stolen;
        if steals > 0 && !limited && (peak_voices as usize) < pool {
            fail(format!(
                "{segment}: {steals} voices stolen at {peak_voices} of {pool}"
            ));
        }
        eprintln!(
            "{name}: {segment} ({:.1?}): peak {peak_voices} voices, {steals} stolen",
            started.elapsed()
        );
    }
    if let [first, .., again] = onsets[..]
        && (first - again).abs() > 10.0
    {
        fail(format!("probe onset moved: {first:.1} -> {again:.1} ms"));
    }
    println!("{name}: hold {latency_ms:.1} ms, probe onsets {onsets:?} ms");
    failed
}

/// The programs a build must play: polyphonic variables (CHORUS), pedal
/// and resonance (Una Corda), legato scripts (Areia), one Augmented
/// Orchestra and one VWinds program.
#[test]
#[ignore = "real libraries, minutes in debug: run with --ignored before offering a build"]
fn real_performances_sound_on_time_end_and_never_burst() {
    let programs = [
        Path::new(KONTAKT).join("Audio Imperia CHORUS/Instruments/01 Multi Patches/01 Chorus - Women - Traditional Articulations.nki"),
        Path::new(KONTAKT).join("Una Corda Library/Instruments/Una Corda Felt.nki"),
        Path::new(KONTAKT).join("Areia 1.2.0 [Audio Imperia]/Instruments/01 Core Technique Patches/01 Areia - 16 Violins - Core Techniques.nki"),
        Path::new(UVI).join("UVI - Augmented Orchestra v1.1.2-R2R/Augmented Orchestra.ufs/Presets/00 Orchestra/01 Strings/W Strings Harmonic.uvip"),
        Path::new(UVI).join("VWinds - Flutes/VWinds-C_Flute.ufs/Presets/Flute.uvip"),
    ];
    let mut failed = Vec::new();
    // KONTRA_PLAYTEST=<part of a path> plays just that program.
    let only = std::env::var("KONTRA_PLAYTEST").unwrap_or_default();
    for path in programs
        .into_iter()
        .filter(|p| p.to_string_lossy().contains(&only))
    {
        // A .ufs is a file; the program is inside it.
        if !path.ancestors().any(Path::is_file) {
            eprintln!("skipped: no {}", path.display());
            continue;
        }
        failed.extend(perform(path));
    }
    assert!(failed.is_empty(), "{}", failed.join("\n"));
}
