//! The plugin driven the way a host drives it, without a host: state in,
//! reset, the control tick, audio blocks with notes, state out.

use std::path::{Path, PathBuf};

use kontra_host::state::{Saved, SavedPart};
use moose::prelude::*;

use crate::{Dsp, Kontra, KontraParams, Tick};

const RATE: f64 = 48_000.0;
const BLOCK: usize = 512;
const KONTAKT: &str = "/data/main/Libraries/Kontakt";

#[test]
fn plugin_contract() {
    moose_test::assert_valid_info::<crate::Plugin>();
    moose_test::assert_bus_config_instrument::<crate::Plugin>();
    moose_test::assert_state_round_trip::<crate::Plugin>();
    moose_test::assert_empty_state_no_crash::<crate::Plugin>();
    moose_test::assert_corrupt_state_no_crash::<crate::Plugin>();
}

/// A plugin as a host has it after loading `saved` and activating.
fn activate(saved: &str) -> (KontraParams, Dsp) {
    let params = KontraParams::new();
    *params.saved.write().unwrap() = saved.to_owned();
    params.restore();
    let mut dsp = Dsp::default();
    <Kontra as PluginLogic>::reset(&mut dsp, &params, &AudioConfig::new(RATE, BLOCK));
    Tick.run(&params);
    (params, dsp)
}

/// One block with `events`; the first output pair, interleaved.
fn block(params: &KontraParams, dsp: &mut Dsp, events: &[moose::prelude::Event]) -> Vec<f32> {
    let mut list = EventList::with_capacity(16);
    for &e in events {
        list.push(e);
    }
    let mut channels = vec![vec![0.0f32; BLOCK]; 2 * crate::OUTPUTS];
    {
        let mut outs: Vec<&mut [f32]> = channels.iter_mut().map(Vec::as_mut_slice).collect();
        let mut buffer = AudioBuffer::from_slices_checked(&[], &mut outs, BLOCK);
        let transport = TransportInfo::default();
        let mut out_events = EventList::with_capacity(16);
        // A bounce, as the host renders offline: every page read in time.
        let mut cx = ProcessContext::new(&transport, RATE, BLOCK, &mut out_events)
            .with_process_mode(ProcessMode::Offline);
        <Kontra as PluginLogic>::process(dsp, params, &mut buffer, &list, &mut cx);
    }
    channels[0]
        .iter()
        .zip(&channels[1])
        .flat_map(|(l, r)| [*l, *r])
        .collect()
}

fn note(on: bool, key: u8) -> moose::prelude::Event {
    let body = if on {
        EventBody::NoteOn {
            group: 0,
            channel: 0,
            note: key,
            velocity: 100,
        }
    } else {
        EventBody::NoteOff {
            group: 0,
            channel: 0,
            note: key,
            velocity: 0,
        }
    };
    moose::prelude::Event::new(0, body)
}

/// A second of one held note after a quarter second of quiet, as a player
/// gives a program time to settle; its RMS.
fn play(params: &KontraParams, dsp: &mut Dsp) -> f32 {
    let blocks = |s: f64| (s * RATE) as usize / BLOCK;
    for _ in 0..blocks(0.25) {
        block(params, dsp, &[]);
    }
    let mut audio = block(params, dsp, &[note(true, 60)]);
    for _ in 0..blocks(1.0) {
        audio.extend(block(params, dsp, &[]));
    }
    block(params, dsp, &[note(false, 60)]);
    (audio.iter().map(|x| x * x).sum::<f32>() / audio.len() as f32).sqrt()
}

fn library(path: &str) -> Option<PathBuf> {
    let path = Path::new(KONTAKT).join(path);
    path.exists().then_some(path)
}

/// Lets the commands sent so far reach the audio side and a capture of the
/// scripts' persistent values taken after them come back, as the control
/// tick does between blocks (a capture already on its way is read first).
fn settle(params: &KontraParams, dsp: &mut Dsp) {
    for _ in 0..2 {
        Tick.run(params);
        block(params, dsp, &[]);
    }
    Tick.run(params);
}

/// Turns part 0's first integer control one step, as the user would; the
/// variable and the value it was turned to.
fn move_control(params: &KontraParams) -> Option<(String, i64)> {
    let mut guard = crate::lock(&params.shared.rack);
    let rack = guard.as_mut()?;
    let script = &rack.parts().first()?.as_ref()?.script;
    let (slot, var, id, script_index) = script.vars.iter().enumerate().find_map(|(slot, v)| {
        let var = v
            .vars
            .iter()
            .find(|x| x.control.is_some() && x.len == 1 && !x.real)?;
        Some((slot, var.name.to_string(), var.control?, v.script))
    })?;
    let saved = rack.saved_scripts(0);
    let entry = saved
        .get(script_index)?
        .iter()
        .find(|e| e.split(' ').next() == Some(&*var))?;
    let now: i64 = entry.split(' ').nth(1)?.parse().ok()?;
    let to = if now > 0 { now - 1 } else { now + 1 };
    rack.set_control(0, u8::try_from(slot).ok()?, id, to);
    Some((var, to))
}

/// Loads `path` into part 0 through the state, plays, turns a control,
/// saves, reloads into a fresh plugin, plays again and saves again.
fn recall(path: PathBuf) -> Result<String, String> {
    let saved = Saved::new(vec![SavedPart {
        part: 0,
        path: path.clone(),
        channel: 0,
        output: 0,
        gain: 0.5,
        pan: 0.25,
        solo: false,
        mute: false,
        scripts: Vec::new(),
        delay_ms: None,
        slot_types: Vec::new(),
    }]);
    let t = std::time::Instant::now();
    let (live, mut live_dsp) = activate(&saved.write());
    let load = t.elapsed();
    let before = play(&live, &mut live_dsp);
    let missing = crate::lock(&live.shared.rack)
        .as_ref()
        .and_then(kontra_host::rack::Rack::stats)
        .map_or(0, |s| s.missing_frames);
    // A host's plain reset at the same rate keeps the part: it plays at once,
    // without a control tick to reload it.
    <Kontra as PluginLogic>::reset(&mut live_dsp, &live, &AudioConfig::new(RATE, BLOCK));
    let after_reset = play(&live, &mut live_dsp);
    settle(&live, &mut live_dsp);
    let moved = move_control(&live);
    settle(&live, &mut live_dsp);
    live.capture();
    let first = live.saved.read().unwrap().clone();
    let (again, mut again_dsp) = activate(&first);
    let after = play(&again, &mut again_dsp);
    settle(&again, &mut again_dsp);
    again.capture();
    let second = again.saved.read().unwrap().clone();
    let mut wrong = Vec::new();
    let mut read = Saved::read(&first);
    let scripts = read
        .parts
        .first_mut()
        .map(|p| std::mem::take(&mut p.scripts));
    let values: Vec<String> = scripts.unwrap_or_default().into_iter().flatten().collect();
    if read != saved {
        wrong.push(format!("the mix saved differs from what loaded: {first}"));
    }
    if first != second {
        let (a, b) = (Saved::read(&first), Saved::read(&second));
        let (a, b) = (&a.parts[0].scripts, &b.parts[0].scripts);
        let diff: Vec<_> = (a.iter().flatten())
            .zip(b.iter().flatten())
            .filter(|(x, y)| x != y)
            .take(5)
            .collect();
        wrong.push(format!("re-save after reload differs: {diff:?}"));
    }
    if after_reset < before * 0.5 {
        wrong.push(format!(
            "a reset at the same rate lost the part: rms {after_reset}"
        ));
    }
    if missing > 0 {
        wrong.push(format!("the bounce missed {missing} frames"));
    }
    if before < 1e-4 {
        wrong.push(format!("silent: rms {before}"));
    }
    let db = 20.0 * (after / before.max(1e-9)).log10();
    if db.abs() > 1.0 {
        wrong.push(format!("reload plays {db:.2} dB from the live part"));
    }
    let turned = match &moved {
        Some((var, to)) if values.contains(&format!("{var} {to}")) => format!("{var} → {to} kept"),
        Some((var, to)) => format!("{var} → {to} not kept (the script set it back)"),
        None => "no integer control".to_owned(),
    };
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    let line = format!(
        "{name}: loaded in {load:.1?}, rms {before:.4} → {after:.4}, {} persistent values, {turned}",
        values.len()
    );
    if wrong.is_empty() {
        Ok(line)
    } else {
        Err(format!("{line}\n  {}", wrong.join("\n  ")))
    }
}

#[test]
fn real_instruments_load_play_save_and_reload() {
    let instruments = [
        "ANALOG STRINGS/Instruments/ANALOG STRINGS.nki",
        "Una Corda Library/Instruments/Una Corda Felt.nki",
        "Shreddage 3.5 E-Upright 1.0.1 [Impact Soundworks] rutracker.org/Instruments/Shreddage 3.5 E-Upright.nki",
        "Conflux 1.1.0 [Native Instruments]/Instruments/Conflux.nki",
    ];
    let mut failed = Vec::new();
    for path in instruments.iter().filter_map(|p| library(p)) {
        match recall(path) {
            Ok(line) => println!("{line}"),
            Err(e) => failed.push(e),
        }
    }
    assert!(failed.is_empty(), "{}", failed.join("\n"));
}

/// One block with `events`; output pairs 0 and 1, each summed to mono.
fn pairs(params: &KontraParams, dsp: &mut Dsp, events: &[moose::prelude::Event]) -> [Vec<f32>; 2] {
    let mut list = EventList::with_capacity(16);
    for &e in events {
        list.push(e);
    }
    let mut channels = vec![vec![0.0f32; BLOCK]; 2 * crate::OUTPUTS];
    {
        let mut outs: Vec<&mut [f32]> = channels.iter_mut().map(Vec::as_mut_slice).collect();
        let mut buffer = AudioBuffer::from_slices_checked(&[], &mut outs, BLOCK);
        let transport = TransportInfo::default();
        let mut out_events = EventList::with_capacity(16);
        let mut cx = ProcessContext::new(&transport, RATE, BLOCK, &mut out_events)
            .with_process_mode(ProcessMode::Offline);
        <Kontra as PluginLogic>::process(dsp, params, &mut buffer, &list, &mut cx);
    }
    let mono = |n: usize| {
        channels[2 * n]
            .iter()
            .zip(&channels[2 * n + 1])
            .map(|(l, r)| l + r)
            .collect()
    };
    [mono(0), mono(1)]
}

#[test]
fn auto_align_reports_the_latest_part_and_holds_the_other() {
    let (Some(a), Some(b)) = (
        library("ANALOG STRINGS/Instruments/ANALOG STRINGS.nki"),
        library("Una Corda Library/Instruments/Una Corda Felt.nki"),
    ) else {
        return;
    };
    let part = |part: usize, path: PathBuf| SavedPart {
        part,
        path,
        channel: 0,
        output: u8::try_from(part).unwrap(),
        gain: 1.0,
        pan: 0.0,
        solo: false,
        mute: false,
        scripts: Vec::new(),
        delay_ms: None,
        slot_types: Vec::new(),
    };
    let saved = Saved::new(vec![part(0, a), part(1, b)]);
    let (params, mut dsp) = activate(&saved.write());
    let onsets = |params: &KontraParams, dsp: &mut Dsp| {
        for _ in 0..(RATE as usize / 4 / BLOCK) {
            pairs(params, dsp, &[]);
        }
        let mut out = [Vec::new(), Vec::new()];
        let mut events = vec![note(true, 60)];
        for _ in 0..(RATE as usize / BLOCK) {
            let [x, y] = pairs(params, dsp, &events);
            events.clear();
            out[0].extend(x);
            out[1].extend(y);
        }
        pairs(params, dsp, &[note(false, 60)]);
        // Long enough for a string release to die away.
        for _ in 0..(8 * RATE as usize / BLOCK) {
            pairs(params, dsp, &[]);
        }
        out.map(|x| kontra_align::onset_ms(&x, RATE, kontra_align::ONSET_DB).unwrap_or(f32::NAN))
    };
    let [a0, b0] = onsets(&params, &mut dsp);
    assert_eq!(
        <Kontra as PluginLogic>::latency(&dsp),
        0,
        "latency while off"
    );
    params.align.set_value(true);
    let t = std::time::Instant::now();
    Tick.run(&params);
    let measured = t.elapsed();
    pairs(&params, &mut dsp, &[]);
    let latency = <Kontra as PluginLogic>::latency(&dsp);
    let ms = latency as f32 * 1000.0 / RATE as f32;
    let [a1, b1] = onsets(&params, &mut dsp);
    println!(
        "off: onsets {a0:.1} / {b0:.1} ms; on: latency {ms:.1} ms, onsets {a1:.1} / {b1:.1} ms; measured in {measured:.1?}"
    );
    assert!(latency > 0, "no latency reported");
    assert!((a1 - b1).abs() < 10.0, "parts not aligned: {a1} vs {b1}");
    assert!(
        (a1.max(b1) - a0.max(b0)).abs() < 10.0,
        "the latest part was held"
    );
    // A reload keeps what was measured: same latency, no measuring.
    params.capture();
    let state = params.saved.read().unwrap().clone();
    let (again, mut again_dsp) = activate(&state);
    again.align.set_value(true);
    let t = std::time::Instant::now();
    Tick.run(&again);
    let remeasured = t.elapsed();
    pairs(&again, &mut again_dsp, &[]);
    assert_eq!(
        <Kontra as PluginLogic>::latency(&again_dsp),
        latency,
        "reload latency"
    );
    assert!(
        remeasured.as_secs_f32() < 1.0,
        "the reload measured again: {remeasured:?}"
    );
    params.align.set_value(false);
    Tick.run(&params);
    pairs(&params, &mut dsp, &[]);
    assert_eq!(
        <Kontra as PluginLogic>::latency(&dsp),
        0,
        "latency after off"
    );
}

#[test]
fn a_library_control_moved_in_the_editor_reaches_its_script() {
    let Some(path) = library("Una Corda Library/Instruments/Una Corda Felt.nki") else {
        return;
    };
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
        delay_ms: None,
        slot_types: Vec::new(),
    }]);
    let (params, mut dsp) = activate(&saved.write());
    settle(&params, &mut dsp);
    // The first shown widget of a persistent integer control, and its value.
    let found = {
        let guard = crate::lock(&params.shared.rack);
        let rack = guard.as_ref().unwrap();
        let part = rack.parts()[0].as_ref().unwrap();
        let view = part.view.as_ref().expect("Una Corda has an interface");
        let vars = &part.script.vars[usize::from(view.slot)];
        let saved = rack.saved_scripts(0);
        view.live
            .tree
            .widgets
            .iter()
            .enumerate()
            .find_map(|(n, w)| {
                let var = vars
                    .vars
                    .iter()
                    .find(|v| v.control == w.source_id && v.len == 1 && !v.real)?;
                let entry = saved
                    .get(vars.script)?
                    .iter()
                    .find(|e| e.split(' ').next() == Some(&*var.name))?;
                let now: i64 = entry.split(' ').nth(1)?.parse().ok()?;
                Some((
                    n,
                    var.name.to_string(),
                    if now > 0 { now - 1 } else { now + 1 },
                ))
            })
    };
    let (widget, var, to) = found.expect("a persistent control with a widget");
    params
        .shared
        .request(crate::Request::Ui(crate::editor::Edit {
            slot: Some(0),
            action: kontra_ui::app::Action::Control {
                part: 0,
                widget,
                value: to as f64,
            },
        }));
    settle(&params, &mut dsp);
    settle(&params, &mut dsp);
    params.capture();
    let state = Saved::read(&params.saved.read().unwrap());
    let values: Vec<&String> = state.parts[0].scripts.iter().flatten().collect();
    assert!(
        values.iter().any(|v| **v == format!("{var} {to}")),
        "{var} → {to} did not reach the script"
    );
    println!("widget {widget}: {var} → {to} kept");
}
