//! Playability through the plugin's own process path: a performance of the
//! kinds that left notes stuck in a host (fast repeats, overlapping legato,
//! the sustain pedal, chords, note-offs beside controllers in one block,
//! velocity-0 note-ons, CLAP note-offs with a wildcard id), played on real
//! libraries. Every note must sound on time, every note must end, nothing
//! may sound late, and voices are stolen only where the plan sets a limit.
//! Repeats, legato and chords play again under a slow and a fast sweep of the
//! wheel and expression: no clicks, no clipping, no jump no note explains.
//! Then every script control is moved under a held note ([`touch`]), and one
//! row per program goes to `$KONTRA_PLAYTEST_OUT` (JSON lines).

use std::path::{Path, PathBuf};

use kontra_host::state::{Saved, SavedPart};
use moose::core::{ExactAddress, ExactEvent, ExactEventBody, ExactNoteAddress, ExactNoteKind};
use moose::prelude::*;

use super::tests::{BLOCK, KONTAKT, RATE, activate};
use crate::{Dsp, Kontra, KontraParams, Tick};

mod touch;

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

    /// The same event `by` semitones up (a note; anything else as is).
    fn shifted(self, by: i8) -> In {
        let up = |note: u8| note.saturating_add_signed(by).min(127);
        match self {
            In::Midi(EventBody::NoteOn {
                group,
                channel,
                note,
                velocity,
            }) => In::Midi(EventBody::NoteOn {
                group,
                channel,
                note: up(note),
                velocity,
            }),
            In::Midi(EventBody::NoteOff {
                group,
                channel,
                note,
                velocity,
            }) => In::Midi(EventBody::NoteOff {
                group,
                channel,
                note: up(note),
                velocity,
            }),
            In::Clap(ExactEventBody::Note {
                kind,
                mut address,
                velocity,
            }) => {
                if let ExactAddress::Value(key) = address.key {
                    address.key = ExactAddress::Value(up(key));
                }
                In::Clap(ExactEventBody::Note {
                    kind,
                    address,
                    velocity,
                })
            }
            other => other,
        }
    }

    /// A note, or the sustain pedal, whose pedal-up plays the library's
    /// pedal noise (Una Corda) as a key plays a note.
    fn is_note(self) -> bool {
        matches!(
            self,
            In::Midi(
                EventBody::NoteOn { .. }
                    | EventBody::NoteOff { .. }
                    | EventBody::ControlChange { cc: 64, .. }
            ) | In::Clap(_)
        )
    }
}

/// The loudest 5 ms of high-frequency energy (the second difference, a
/// click's signature) over the 50 ms either side of it, in dB. A click
/// stands out from its neighbours; a wheel crossfading into brighter
/// layers does not (Kontakt's own Pacific chord sweep gains +10.9 dB over
/// the whole segment's median, lane level 2026-10-29). Quiet neighbours
/// count as the segment's median, so an attack out of silence reads as
/// loud as it is.
fn hf_burst_db(audio: &[f32]) -> f32 {
    let n = at(0.005);
    let frames: Vec<f32> = audio
        .windows(3)
        .map(|w| (w[2] - 2.0 * w[1] + w[0]).powi(2))
        .collect::<Vec<_>>()
        .chunks_exact(n)
        .map(|c| c.iter().sum())
        .collect();
    if frames.is_empty() {
        return 0.0;
    }
    let median = |v: &mut Vec<f32>| {
        v.sort_by(f32::total_cmp);
        v[v.len() / 2]
    };
    let floor = median(&mut frames.clone()).max(1e-20);
    let reach = 10;
    let loudest = (0..frames.len())
        .map(|i| {
            let around = i.saturating_sub(reach)..(i + reach + 1).min(frames.len());
            let mut near: Vec<f32> = around.filter(|&j| j != i).map(|j| frames[j]).collect();
            let local = if near.is_empty() {
                0.0
            } else {
                median(&mut near)
            };
            frames[i] / local.max(floor)
        })
        .fold(0.0f32, f32::max);
    10.0 * loudest.max(1e-20).log10()
}

#[test]
fn a_click_bursts_but_a_brightening_crossfade_does_not() {
    let len = at(1.0);
    let tone = |i: usize, f: f64| (std::f64::consts::TAU * f * i as f64 / RATE).sin();
    // The wheel from full to 0 and back: the bright layer fades out to the
    // dark one and back in (the old whole-segment metric read +12 dB).
    let crossfade: Vec<f32> = (0..len)
        .map(|i| {
            let x = (2.0 * i as f64 / len as f64 - 1.0).powi(2);
            (0.5 * ((1.0 - x) * tone(i, 220.0) + x * tone(i, 3520.0))) as f32
        })
        .collect();
    // The dark layer, with zipper steps in its level every 20 ms.
    let zipper: Vec<f32> = (0..len)
        .map(|i| (0.5 * tone(i, 220.0) * (1.0 - 0.1 * ((i / at(0.02)) % 2) as f64)) as f32)
        .collect();
    let plain: Vec<f32> = (0..len).map(|i| (0.5 * tone(i, 220.0)) as f32).collect();
    let base = hf_burst_db(&plain);
    assert!(
        hf_burst_db(&crossfade) - base < CLICK_DB,
        "{}",
        hf_burst_db(&crossfade) - base
    );
    assert!(
        hf_burst_db(&zipper) - base > CLICK_DB,
        "{}",
        hf_burst_db(&zipper) - base
    );
}

/// The most a sweep may add to its notes' high-frequency burst before it
/// counts as clicking (Afflatus before #1124: +17.7 dB; after: +2.3 dB).
const CLICK_DB: f32 = 10.0;

struct Segment {
    name: &'static str,
    voices: u32,
    stolen: u64,
    peak_db: f32,
    hf_db: f32,
    channel_db: [f32; 2],
}

/// What a program's performance left: its failures and measurements by segment.
struct Report {
    name: String,
    load_s: f64,
    hold_ms: f32,
    failed: Vec<String>,
    segments: Vec<Segment>,
    ui: touch::Touched,
    diagnostics: (usize, usize, usize, Vec<String>),
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

/// `events` with the wheel (CC1) swept from full to 0 and back, and
/// expression (CC11) from full to 40 and back, over frames `from..to`.
fn wheel(mut events: Vec<(usize, In)>, from: usize, to: usize) -> Vec<(usize, In)> {
    const STEPS: usize = 64;
    for i in 0..=STEPS {
        let x = i as f64 / STEPS as f64;
        let depth = 1.0 - (2.0 * x - 1.0).abs();
        let frame = from + ((to - from) as f64 * x) as usize;
        events.push((frame, cc(1, (127.0 * (1.0 - depth)).round() as u8)));
        events.push((frame, cc(11, (127.0 - 87.0 * depth).round() as u8)));
    }
    // Stable: a note keeps its place before a controller on its frame.
    events.sort_by_key(|e| e.0);
    events
}

/// `events`, then the same under a slow (whole segment) and a fast (0.25 s,
/// mid-segment) sweep of the wheel and expression.
fn swept(
    name: &'static str,
    slow: &'static str,
    fast: &'static str,
    events: Vec<(usize, In)>,
) -> [(&'static str, Vec<(usize, In)>); 3] {
    let end = events.iter().map(|e| e.0).max().unwrap_or(0);
    let mid = end / 2;
    [
        (slow, wheel(events.clone(), 0, end)),
        (
            fast,
            wheel(
                events.clone(),
                mid.saturating_sub(at(0.125)),
                mid + at(0.125),
            ),
        ),
        (name, events),
    ]
}

/// The segment a swept one is compared with: the same notes, no sweep.
fn plain(segment: &str) -> Option<&str> {
    segment.split_once(" + wheel").map(|s| s.0)
}

#[test]
fn correlated_channels_below_full_scale_do_not_clip() {
    let channels = [vec![0.8_f32], vec![0.8_f32]];
    let peaks = output_peaks(&channels);
    assert!(
        !clips(peaks),
        "two channels at 0.8 must not clip: {peaks:?}"
    );
    assert!(clips(output_peaks(&[vec![1.01], vec![0.0]])));
}

/// The performance, a segment at a time: (name, [(frame, event)]), its
/// notes `by` semitones up.
fn performance(by: i8) -> Vec<(&'static str, Vec<(usize, In)>)> {
    let segments = performance_at_c4();
    (segments.into_iter())
        .map(|(name, events)| {
            (
                name,
                events
                    .into_iter()
                    .map(|(f, e)| (f, e.shifted(by)))
                    .collect(),
            )
        })
        .collect()
}

/// Plays silence until no voice is left and the effects' tail (a reverb) is
/// quiet, so it never reads as the next segment's onset; at most 5 s.
fn ring_out(params: &KontraParams, dsp: &mut Dsp, last: &mut Played) {
    for _ in 0..at(5.0) / BLOCK {
        if last.voices == 0 && last.audio.iter().all(|x| x.abs() < 1e-4) {
            return;
        }
        *last = block(params, dsp, &[], last);
    }
}

/// The octave shift that brings the scenario's repeated key (D4) where the
/// program plays: a bass's, a tuba's or a script's range may end below it.
/// Measured: a short note per octave, the first that starts a voice.
fn octave_shift(params: &KontraParams, dsp: &mut Dsp, last: &mut Played) -> i8 {
    for by in [0, -12, 12, -24, 24] {
        let key = 62u8.saturating_add_signed(by);
        *last = block(params, dsp, &[(0, on(key))], last);
        let mut sounded = last.voices > 0;
        for _ in 0..at(0.1) / BLOCK {
            *last = block(params, dsp, &[], last);
            sounded |= last.voices > 0;
        }
        *last = block(params, dsp, &[(0, off(key))], last);
        ring_out(params, dsp, last);
        if sounded {
            return by;
        }
    }
    0
}

fn performance_at_c4() -> Vec<(&'static str, Vec<(usize, In)>)> {
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
    let [repeats_slow, repeats_fast, repeats] = swept(
        "fast repeats",
        "fast repeats + wheel slow",
        "fast repeats + wheel fast",
        repeats,
    );
    let [legato_slow, legato_fast, legato] = swept(
        "overlapping legato",
        "overlapping legato + wheel slow",
        "overlapping legato + wheel fast",
        legato,
    );
    let [chord_slow, chord_fast, chord] = swept(
        "8-note chord",
        "8-note chord + wheel slow",
        "8-note chord + wheel fast",
        chord,
    );
    vec![
        ("probe", probe.clone()),
        repeats,
        repeats_slow,
        repeats_fast,
        legato,
        legato_slow,
        legato_fast,
        ("sustain pedal", pedal),
        chord,
        chord_slow,
        chord_fast,
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
    channel_peaks: [f32; 2],
    voices: u32,
    stolen: u64,
    refused: u64,
}

fn output_peaks(channels: &[Vec<f32>]) -> [f32; 2] {
    let mut peaks = [0.0f32; 2];
    for (peak, channel) in peaks.iter_mut().zip(channels) {
        *peak = channel.iter().fold(0.0f32, |p, x| p.max(x.abs()));
    }
    peaks
}

fn clips(peaks: [f32; 2]) -> bool {
    peaks.into_iter().any(|peak| peak > 1.0)
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
    let channel_peaks = output_peaks(&channels);
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
            channel_peaks,
            voices: s.voices,
            stolen: s.voices_stolen,
            refused: s.notes_refused + s.dropped,
        },
        None => Played {
            audio,
            channel_peaks,
            ..*last
        },
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

/// Plays the performance on `path`, then touches its controls.
fn perform(path: PathBuf) -> Report {
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
    let load_s = started.elapsed().as_secs_f64();
    eprintln!("{name}: loaded in {load_s:.1} s");
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
    let mut segments = Vec::new();
    let mut blocks = 0usize;
    let by = octave_shift(&params, &mut dsp, &mut last);
    for (segment, events) in performance(by) {
        let (stolen, refused) = (last.stolen, last.refused);
        let end = events.iter().map(|e| e.0).max().unwrap_or(0) + 1;
        let starts: Vec<usize> = events
            .iter()
            .filter(|e| e.1.starts())
            .map(|e| e.0)
            .collect();
        let (mut audio, mut peaks, mut peak_voices) = (Vec::new(), Vec::new(), 0u32);
        let mut channel_peaks = [0.0f32; 2];
        let mut frame = 0;
        // The segment, then its tail until no voice is left.
        loop {
            let inside: Vec<_> = (events.iter())
                .filter(|e| (frame..frame + BLOCK).contains(&e.0))
                .map(|&(f, e)| (f - frame, e))
                .collect();
            last = block(&params, &mut dsp, &inside, &last);
            blocks += 1;
            if blocks.is_multiple_of(10) {
                Tick.run(&params);
            }
            peaks.push(last.audio.iter().fold(0.0f32, |p, x| p.max(x.abs())));
            for (peak, value) in channel_peaks.iter_mut().zip(last.channel_peaks) {
                *peak = peak.max(value);
            }
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
        ring_out(&params, &mut dsp, &mut last);
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
        // No late bursts: a jump of 12 dB or more to within 20 dB of the
        // segment's loudest, with no note near it (a controller may move the
        // level, never jump it; a wind rising out of silence on the wheel,
        // VWinds -80 to -28 dBFS over ten blocks, is no burst).
        let inputs: Vec<usize> = (events.iter())
            .filter(|e| e.1.is_note())
            .map(|e| e.0)
            .collect();
        let loudest = peaks.iter().copied().fold(0.0f32, f32::max);
        for (b, w) in peaks.windows(5).enumerate() {
            let (before, now) = (w[..4].iter().copied().fold(0.0, f32::max), w[4]);
            let start = (b + 4) * BLOCK;
            let near = inputs
                .iter()
                .any(|&i| i <= start + BLOCK && start <= i + hold);
            if now > 1e-3 && now > 4.0 * before && now > 0.1 * loudest && !near {
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
        let channel_db = channel_peaks.map(|p| 20.0 * p.max(1e-9).log10());
        let loudest_db = channel_db.into_iter().fold(f32::NEG_INFINITY, f32::max);
        if clips(channel_peaks) {
            fail(format!("{segment}: clips at {loudest_db:+.1} dBFS"));
        }
        let hf = hf_burst_db(&audio);
        let base = plain(segment).and_then(|p| segments.iter().find(|s: &&Segment| s.name == p));
        if let Some(base) = base
            && hf - base.hf_db > CLICK_DB
        {
            fail(format!(
                "{segment}: clicks, HF burst {:+.1} dB over the same notes unswept",
                hf - base.hf_db
            ));
        }
        let steals = last.stolen - stolen;
        if steals > 0 && !limited && (peak_voices as usize) < pool {
            fail(format!(
                "{segment}: {steals} voices stolen at {peak_voices} of {pool}"
            ));
        }
        eprintln!(
            "{name}: {segment} ({:.1?}): peak {peak_voices} voices, {steals} stolen, {loudest_db:.1} dBFS, HF burst {hf:.1} dB",
            started.elapsed()
        );
        segments.push(Segment {
            name: segment,
            voices: peak_voices,
            stolen: steals,
            peak_db: loudest_db,
            hf_db: hf,
            channel_db,
        });
    }
    if let [first, .., again] = onsets[..]
        && (first - again).abs() > 10.0
    {
        fail(format!("probe onset moved: {first:.1} -> {again:.1} ms"));
    }
    let ui = touch::touch(&params, &mut dsp, &mut last, &mut fail);
    let diagnostics = touch::diagnostics(&params);
    println!(
        "{name}: hold {latency_ms:.1} ms, probe onsets {onsets:?} ms; touched {} of {} controls, {} reached the sound, {} clicked; diagnostics {} broken, {} audible",
        ui.touched,
        ui.shown,
        ui.reached,
        ui.clicks.len(),
        diagnostics.0,
        diagnostics.1
    );
    Report {
        name,
        load_s,
        hold_ms: latency_ms,
        failed,
        segments,
        ui,
        diagnostics,
    }
}

/// The ten programs the user plays: one per library (legato scripts,
/// polyphonic variables, pedal and resonance, wheel-driven dynamics, a big
/// script face, two UVI engines).
const PROGRAMS: [&str; 10] = [
    "Afflatus Chapter II Brass/Instruments/1. Ensembles/Single Instruments/3 Trumpets/3 Trumpets Legato.nki",
    "Pacific Ensemble Strings/Instruments/10 Cellos/Pacific - Ens Strings - 10 Cellos - Legato Sustains.nki",
    "Audio Imperia CHORUS/Instruments/01 Multi Patches/01 Chorus - Women - Traditional Articulations.nki",
    "Areia 1.2.0 [Audio Imperia]/Instruments/01 Core Technique Patches/01 Areia - 16 Violins - Core Techniques.nki",
    "Audio Imperia Dolce/Instruments/01 7 1st Violins/Dolce - 01 7 1st Violins - Legato.nki",
    "Una Corda Library/Instruments/Una Corda Felt.nki",
    "ANALOG STRINGS/Instruments/ANALOG STRINGS.nki",
    "Conflux 1.1.0 [Native Instruments]/Instruments/Conflux.nki",
    "UVI:VWinds - Clarinets/VWinds-AClarinet.ufs/Presets/Clarinet A.uvip",
    "UVI:UVI - Augmented Orchestra v1.1.2-R2R/Augmented Orchestra.ufs/Presets/00 Orchestra/01 Strings/W Strings Harmonic.uvip",
];

#[test]
#[ignore = "real libraries, minutes: run with --ignored (release) before offering a build"]
fn real_performances_sound_on_time_end_and_never_burst() {
    let mut failed = Vec::new();
    // KONTRA_PLAYTEST=<part of a path> plays just the programs it names.
    let only = std::env::var("KONTRA_PLAYTEST").unwrap_or_default();
    let out = std::env::var_os("KONTRA_PLAYTEST_OUT").map(PathBuf::from);
    // KONTRA_PLAYTEST_LIST=<file>: plays the programs it lists, one path a
    // line, instead of the user's ten; KONTRA_PLAYTEST_SHARD=i/n plays every
    // nth from the ith, so n processes share a corpus.
    let list = std::env::var_os("KONTRA_PLAYTEST_LIST")
        .map(|f| std::fs::read_to_string(f).expect("KONTRA_PLAYTEST_LIST"));
    let programs: Vec<&str> = match &list {
        Some(l) => l.lines().filter(|l| !l.is_empty()).collect(),
        None => PROGRAMS.to_vec(),
    };
    let (shard, shards) = std::env::var("KONTRA_PLAYTEST_SHARD")
        .ok()
        .and_then(|s| {
            let (i, n) = s.split_once('/')?;
            Some((i.parse::<usize>().ok()?, n.parse::<usize>().ok()?.max(1)))
        })
        .unwrap_or((0, 1));
    let programs = programs
        .into_iter()
        .filter(|p| p.contains(&only))
        .enumerate()
        .filter(|(n, _)| n % shards == shard)
        .map(|(_, p)| p);
    for program in programs {
        let path = match program.strip_prefix("UVI:") {
            Some(p) => Path::new(UVI).join(p),
            None => Path::new(KONTAKT).join(program),
        };
        // A .ufs is a file; the program is inside it.
        if !path.ancestors().any(Path::is_file) {
            eprintln!("skipped: no {}", path.display());
            continue;
        }
        // One program's panic is its fail, not the run's.
        let Ok(report) = std::panic::catch_unwind(|| perform(path.clone())) else {
            failed.push(format!("{}: panicked", path.display()));
            eprintln!("panicked: {}", path.display());
            continue;
        };
        if let Some(out) = &out {
            use std::io::Write;
            let line = touch::row(&path.to_string_lossy(), &report);
            let file = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(out);
            assert!(
                file.and_then(|mut f| writeln!(f, "{line}")).is_ok(),
                "cannot write {}",
                out.display()
            );
        }
        failed.extend(report.failed);
    }
    // A corpus run records its rows; the user's ten must pass.
    assert!(list.is_some() || failed.is_empty(), "{}", failed.join("\n"));
}
