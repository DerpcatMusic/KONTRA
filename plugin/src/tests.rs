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

/// v3 installs beside v2: its own name, CLAP id and VST3 id, so a v2
/// project never opens v3, nor the other way round.
#[test]
fn v3_has_its_own_identity() {
    let info = <crate::Plugin as moose::core::plugin::PluginRuntime>::info();
    assert_eq!(info.name, "KONTRA v3");
    assert_eq!(info.clap_id, "audio.matari.kontra-v3");
    assert_eq!(info.vst3_id, "audio.matari.kontra-v3");
    assert_eq!(&info.fourcc, b"Knt3");
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

/// A real library's instrument under [`KONTAKT`]; a test without it says
/// so and passes, as CI has no libraries.
fn library(path: &str) -> Option<PathBuf> {
    let path = Path::new(KONTAKT).join(path);
    if !path.exists() {
        eprintln!("skipped: no {}", path.display());
        return None;
    }
    Some(path)
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
            .find(|x| x.control.is_some() && x.len == 1 && !x.real && !x.text)?;
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
        delays: None,
        slot_types: Vec::new(),
        slot_responses: Vec::new(),
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
    // What the scripts did is theirs, not the mix: values, and the effect
    // types and responses they loaded after init (the re-save checks those).
    let scripts = read.parts.first_mut().map(|p| {
        p.slot_types.clear();
        p.slot_responses.clear();
        std::mem::take(&mut p.scripts)
    });
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
        delays: None,
        slot_types: Vec::new(),
        slot_responses: Vec::new(),
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
fn a_library_control_moved_in_the_editor_reaches_its_script_and_shows() {
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
        delays: None,
        slot_types: Vec::new(),
        slot_responses: Vec::new(),
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
        view.tree().widgets.iter().enumerate().find_map(|(n, w)| {
            let var = vars
                .vars
                .iter()
                .find(|v| v.control == w.source_id && v.len == 1 && !v.real && !v.text)?;
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
    // And the editor's tree shows it: read back from the running script.
    let (value, revision) = {
        let guard = crate::lock(&params.shared.rack);
        let view = guard.as_ref().unwrap().parts()[0]
            .as_ref()
            .unwrap()
            .view
            .as_ref()
            .unwrap();
        (view.tree().widgets[widget].value.clone(), view.revision)
    };
    assert_eq!(
        value,
        Some(kontra_ui_model::Value::Integer(to as i32)),
        "the tree shows {var}"
    );
    assert!(revision > 0, "the view never refreshed");
    println!("widget {widget}: {var} → {to} kept and shown (revision {revision})");
}

#[test]
fn a_preset_picked_in_the_browser_loads_and_plays() {
    if library("Una Corda Library/Instruments/Una Corda Felt.nki").is_none() {
        return;
    }
    let (params, mut dsp) = activate(&Saved::new(Vec::new()).write());
    params
        .shared
        .request(crate::Request::AddRoot(PathBuf::from(KONTAKT)));
    let t = std::time::Instant::now();
    let found = loop {
        Tick.run(&params);
        let guard = crate::lock(&params.shared.browser);
        let b = guard.as_ref().unwrap();
        if !b.scanning() && !b.catalog.libraries.is_empty() {
            let at = b.catalog.libraries.iter().enumerate().find_map(|(n, l)| {
                let p = l
                    .presets
                    .iter()
                    .position(|p| p.ends_with("Una Corda Felt.nki"))?;
                Some((n, p))
            });
            break (at, b.catalog.libraries.len());
        }
        drop(guard);
        assert!(t.elapsed().as_secs() < 300, "the scan never finished");
        std::thread::sleep(std::time::Duration::from_millis(50));
    };
    let scan = t.elapsed();
    let ((library, preset), libraries) = (found.0.expect("Una Corda in the catalog"), found.1);
    params
        .shared
        .request(crate::Request::Browse(kontra_ui::app::Action::Load {
            library,
            preset,
        }));
    Tick.run(&params);
    let name = crate::lock(&params.shared.rack).as_ref().unwrap().parts()[0]
        .as_ref()
        .map(|p| p.name.clone());
    let recent = crate::lock(&params.shared.browser)
        .as_ref()
        .unwrap()
        .recent();
    let rms = play(&params, &mut dsp);
    println!(
        "scan {scan:.1?}: {libraries} libraries; loaded {name:?}, rms {rms:.4}, recent {recent:?}"
    );
    assert!(name.is_some(), "nothing loaded");
    assert_eq!(recent, [(library, preset)]);
    assert!(rms > 1e-4, "silent: {rms}");
    // The part's arrows step through its library, and wrap.
    let path = |params: &KontraParams| {
        crate::lock(&params.shared.rack).as_ref().unwrap().parts()[0]
            .as_ref()
            .and_then(|p| p.path.clone())
    };
    let browse = |action| {
        params.shared.request(crate::Request::Browse(action));
        Tick.run(&params);
    };
    let (presets, felt) = {
        let b = crate::lock(&params.shared.browser);
        let presets = b.as_ref().unwrap().catalog.libraries[library]
            .presets
            .clone();
        let felt = presets[preset].clone();
        (presets, felt)
    };
    let count = presets.len();
    browse(kontra_ui::app::Action::Step { part: 0, by: 1 });
    assert_eq!(path(&params), Some(presets[(preset + 1) % count].clone()));
    browse(kontra_ui::app::Action::Step { part: 0, by: -1 });
    assert_eq!(path(&params), Some(felt.clone()), "back");
    browse(kontra_ui::app::Action::Switch { part: 0, preset: 0 });
    browse(kontra_ui::app::Action::Step { part: 0, by: -1 });
    assert_eq!(
        path(&params),
        presets.last().cloned(),
        "wraps below the first"
    );
    browse(kontra_ui::app::Action::Switch { part: 0, preset });
    let back = path(&params).unwrap();
    assert!(back.ends_with("Una Corda Felt.nki"), "{back:?}");
}

const SHREDDAGE: &str = "Shreddage 3.5 E-Upright 1.0.1 [Impact Soundworks] rutracker.org/Instruments/Shreddage 3.5 E-Upright.nki";

/// Sets Shreddage's console control `name` to `value`, as its UI would,
/// and lets the scripts and the effects they load settle.
fn console(params: &KontraParams, dsp: &mut Dsp, name: &str, value: i64) {
    {
        let mut guard = crate::lock(&params.shared.rack);
        let rack = guard.as_mut().unwrap();
        let part = rack.parts()[0].as_ref().unwrap();
        let (slot, id) = (part.faces.iter().enumerate())
            .find_map(|(slot, f)| Some((slot, f.as_ref()?.control_id(name)?)))
            .unwrap_or_else(|| panic!("no control {name}"));
        rack.set_control(0, u8::try_from(slot).unwrap(), id, value);
    }
    for _ in 0..40 {
        Tick.run(params);
        block(params, dsp, &[]);
    }
}

/// Shreddage with its second channel's first FX slot (slot index 8, `Bus(1)`
/// slot 0) set to EQ3BAND from its own console menu, the slot shown, and
/// that band's gain cut fully (a boost clips); the plugin. The first
/// channel (Neck) is saved at volume 0 (`%_channel_data` 1 1 0 …), so an
/// EQ there is silent in Kontakt too.
fn shreddage_eq() -> Option<(KontraParams, Dsp, f32)> {
    let path = library(SHREDDAGE)?;
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
    let (params, mut dsp) = activate(&saved.write());
    settle(&params, &mut dsp);
    let dry = band(&params, &mut dsp);
    // The menu's items hold unit ids; 1 is EQ3BAND (`%fx_unit_engine_par`).
    console(&params, &mut dsp, "$_ChannelFxSlotType8", 1);
    console(&params, &mut dsp, "$_ChannelFxSlotSelect8", 1);
    // Edit pane control 5 is the EQ's second band gain (1.45 kHz).
    console(&params, &mut dsp, "$EditPaneSlider5", 0);
    Some((params, dsp, dry))
}

/// Part 0's effect types loaded after init, as `[(rack, slot, type)]`.
/// The effect types part 0's scripts loaded after `on init`, as (rack,
/// slot, Kontakt type id, subtype).
fn slot_types(params: &KontraParams) -> String {
    let guard = crate::lock(&params.shared.rack);
    let part = guard.as_ref().and_then(|r| r.parts()[0].as_ref());
    let loaded = part.map(|p| {
        let types = p.plan.as_ref().map(|plan| &plan.slot_shapes.types);
        (p.slot_types.iter())
            .map(|&(at, slot, ty)| {
                let ty = types.and_then(|t| t.get(usize::from(ty)));
                (at, slot, ty.map(|t| (t.id, t.subtype)))
            })
            .collect::<Vec<_>>()
    });
    format!("{loaded:?}")
}

#[test]
fn shreddage_loads_an_eq_from_its_console_and_a_reload_keeps_it() {
    let Some((live, mut live_dsp, _)) = shreddage_eq() else {
        return;
    };
    // EQ3BAND is the EQ subtype 24.
    let eq = "Some([(Bus(1), 0, Some((24, true)))])";
    assert_eq!(slot_types(&live), eq, "the console's menu loads EQ3BAND");
    settle(&live, &mut live_dsp);
    live.capture();
    let first = live.saved.read().unwrap().clone();
    let (again, mut again_dsp) = activate(&first);
    settle(&again, &mut again_dsp);
    assert_eq!(
        slot_types(&again),
        eq,
        "a reload puts it back before on init"
    );
    again.capture();
    let second = again.saved.read().unwrap().clone();
    let units = |json: &str| {
        let s = Saved::read(json);
        (s.parts[0].scripts.iter().flatten())
            .find(|e| e.starts_with("%_rack_unit_id "))
            .cloned()
    };
    // Channel 1's first slot (index 8) holds unit 1, EQ3BAND.
    let unit = |u: &String| u.split(' ').nth(9) == Some("1");
    assert!(units(&first).is_some_and(|u| unit(&u)));
    assert_eq!(
        units(&first),
        units(&second),
        "the console's own record comes back"
    );
    assert_eq!(first, second, "re-save after reload is identical");
}

#[test]
fn shreddage_eq_band_gain_is_audible_and_survives_reload() {
    let Some((live, mut live_dsp, dry)) = shreddage_eq() else {
        return;
    };
    let wet = band(&live, &mut live_dsp);
    // C4 here −1.8 dB (C3 in the render example −3.1); the round robins
    // move it ±0.04 dB.
    assert!(
        dry - wet > 1.0,
        "band 2 fully cut moved {:.2} dB",
        wet - dry
    );
    live.capture();
    let (again, mut again_dsp) = activate(&live.saved.read().unwrap());
    settle(&again, &mut again_dsp);
    let reloaded = band(&again, &mut again_dsp);
    assert!(
        (reloaded - wet).abs() < 0.5,
        "{wet:.2} dB live, {reloaded:.2} reloaded"
    );
}

/// Six notes of C4 (past Shreddage's round robins): the energy between
/// about 1 and 2 kHz against all of it, in dB. A round robin moves it a
/// little; an EQ band there at full boost or cut moves it a lot.
fn band(params: &KontraParams, dsp: &mut Dsp) -> f32 {
    let blocks = |s: f64| (s * RATE) as usize / BLOCK;
    let k = |hz: f64| 1.0 - (-2.0 * std::f64::consts::PI * hz / RATE).exp();
    let (k1, k2) = (k(1000.0), k(2000.0));
    let (mut band, mut all, mut y1, mut y2) = (0.0f64, 0.0f64, 0.0f64, 0.0f64);
    for _ in 0..6 {
        let mut audio = block(params, dsp, &[note(true, 60)]);
        for _ in 0..blocks(0.75) {
            audio.extend(block(params, dsp, &[]));
        }
        block(params, dsp, &[note(false, 60)]);
        for _ in 0..blocks(0.5) {
            block(params, dsp, &[]);
        }
        for x in audio.chunks(2).map(|f| f64::from(f[0] + f[1])) {
            y1 += k1 * (x - y1);
            y2 += k2 * (x - y2);
            band += (y2 - y1) * (y2 - y1);
            all += x * x;
        }
    }
    (10.0 * (band / all.max(1e-12)).log10()) as f32
}

/// HOW-TO-TRY-V3's first run, headless: v2's folders and a UVI folder the
/// player adds are scanned, and a preset of each library loads, plays,
/// saves and reloads.
#[test]
#[ignore = "about 80 s in debug: run with --ignored before offering a build"]
fn a_first_run_lists_v2s_libraries_and_their_presets_play_and_recall() {
    const UVI: &str = "/data/main/Libraries/UVI";
    let v2 = dirs::config_dir().map(|c| c.join("kontra").join("settings.json"));
    let Some(v2) = v2.filter(|p| p.exists() && Path::new(UVI).exists()) else {
        eprintln!("skipped: no v2 settings or no {UVI}");
        return;
    };
    let dir = std::env::temp_dir().join(format!("kontra-first-run-{}", std::process::id()));
    // A run that died left its prefs: a first run starts without them.
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("kontra")).unwrap();
    std::fs::copy(&v2, dir.join("kontra").join("settings.json")).unwrap();
    let mut browser = kontra_host::browser::Browser::open(Some(&dir), Some(&dir));
    browser.add_root(UVI.into());
    let t = std::time::Instant::now();
    while browser.scanning() && t.elapsed().as_secs() < 300 {
        browser.poll();
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    println!("scanned in {:.1?}", t.elapsed());
    let mut failed = Vec::new();
    for name in ["Una Corda", "ANALOG STRINGS", "Augmented Orchestra"] {
        let libraries = &browser.catalog.libraries;
        let Some(n) = libraries.iter().position(|l| l.name.contains(name)) else {
            failed.push(format!("{name}: not listed"));
            continue;
        };
        println!("{name}: {} presets", libraries[n].presets.len());
        match browser.preset(n, 0).map(recall) {
            Some(Ok(line)) => println!("{line}"),
            Some(Err(e)) => failed.push(e),
            None => failed.push(format!("{name}: no presets")),
        }
    }
    std::fs::remove_dir_all(&dir).unwrap();
    assert!(failed.is_empty(), "{}", failed.join("\n"));
}

#[test]
fn a_librarys_cover_reads_from_its_resource_container() {
    let Some(nicnt) = library("Una Corda Library/Una Corda.nicnt") else {
        return;
    };
    // A second container adds nothing: the first reads the whole library's.
    let loose = nicnt.with_file_name("cover.png");
    let keys = crate::editor::art(&[nicnt.clone(), nicnt.with_extension("nkr"), loose.clone()]);
    assert_eq!(keys.len(), 3, "{keys:?}");
    assert_eq!(keys.last(), Some(&loose.to_string_lossy().into_owned()));
    let keys = &keys[..2];
    for key in keys {
        let png = crate::editor::cover(key);
        assert!(png.is_some_and(|b| b.starts_with(b"\x89PNG")), "no {key:?}");
    }
}

/// A host that hands back the state it saved (Bitwig: at creation, then
/// again as it activates) loads the instrument once, not again.
#[test]
fn the_same_state_restored_again_loads_nothing_twice() {
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
        delays: None,
        slot_types: Vec::new(),
        slot_responses: Vec::new(),
    }]);
    let (params, mut dsp) = activate(&saved.write());
    settle(&params, &mut dsp);
    let load = |params: &KontraParams| {
        let rack = crate::lock(&params.shared.rack);
        let part = rack
            .as_ref()
            .and_then(|r| r.parts().first()?.as_ref().map(|p| p.script.generation()));
        part.expect("Una Corda loaded")
    };
    let first = load(&params);
    params.capture();
    for _ in 0..2 {
        params.restore();
        settle(&params, &mut dsp);
        assert_eq!(load(&params), first, "the same state loaded the part again");
    }
}

/// An idle rack asks the editor for no frames: a tick that changes nothing
/// it shows leaves it clean, and one that does marks it.
#[test]
fn an_idle_tick_draws_no_frame() {
    use std::sync::atomic::Ordering;
    let (params, mut dsp) = activate("");
    settle(&params, &mut dsp);
    params.shared.dirty.store(false, Ordering::Release);
    for _ in 0..3 {
        Tick.run(&params);
        block(&params, &mut dsp, &[]);
    }
    assert!(
        !params.shared.dirty.load(Ordering::Acquire),
        "an idle tick asked for a frame"
    );
    params.shared.request(crate::Request::Load {
        part: None,
        path: PathBuf::from("/no/such.nki"),
    });
    Tick.run(&params);
    assert!(
        params.shared.dirty.load(Ordering::Acquire),
        "a report drew nothing"
    );
}

/// A UVI program's own page: a knob moved in the editor runs the script's
/// `changed`, the note plays louder or softer, and a reload keeps it.
fn uvi_knob_moves_the_sound_and_survives_reload(program: &str, knob: &str, to: f64) {
    let path = Path::new("/data/main/Libraries/UVI").join(program);
    // The program is a member of its bank, a file.
    if !path.ancestors().any(Path::is_file) {
        eprintln!("skipped: no {}", path.display());
        return;
    }
    let part = |path: PathBuf| SavedPart {
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
    };
    let (params, mut dsp) = activate(&Saved::new(vec![part(path)]).write());
    settle(&params, &mut dsp);
    let before = play(&params, &mut dsp);
    let shown = |params: &KontraParams| {
        let guard = crate::lock(&params.shared.rack);
        let part = guard.as_ref()?.parts()[0].as_ref()?;
        let tree = part.view.as_ref()?.tree();
        let n = tree.widgets.iter().position(|w| w.name == knob)?;
        Some((n, tree.widgets[n].value.clone()))
    };
    let (widget, _) = shown(&params).unwrap_or_else(|| panic!("{program}: no {knob}"));
    params
        .shared
        .request(crate::Request::Ui(crate::editor::Edit {
            slot: Some(0),
            action: kontra_ui::app::Action::Control {
                part: 0,
                widget,
                value: to,
            },
        }));
    settle(&params, &mut dsp);
    settle(&params, &mut dsp);
    let after = play(&params, &mut dsp);
    let db = |a: f32, b: f32| 20. * (b / a.max(1e-9)).log10();
    assert!(
        db(before, after).abs() > 1.,
        "{knob} → {to} moved {:.2} dB",
        db(before, after)
    );
    assert_eq!(
        shown(&params).and_then(|s| s.1),
        Some(kontra_ui_model::Value::Real(to))
    );
    params.capture();
    let saved = params.saved.read().unwrap().clone();
    let entry = format!("{knob}={to}");
    let state = Saved::read(&saved);
    assert!(
        state.parts[0].scripts.iter().flatten().any(|v| *v == entry),
        "{entry} was not saved"
    );
    let (again, mut again_dsp) = activate(&saved);
    settle(&again, &mut again_dsp);
    assert_eq!(
        shown(&again).and_then(|s| s.1),
        Some(kontra_ui_model::Value::Real(to))
    );
    // The live note was the program's second: round robins move on per note.
    play(&again, &mut again_dsp);
    let reloaded = play(&again, &mut again_dsp);
    assert!(
        db(after, reloaded).abs() < 0.5,
        "{after:.4} live, {reloaded:.4} reloaded"
    );
    println!(
        "{knob} → {to}: {:+.2} dB, kept on reload ({:+.2} dB)",
        db(before, after),
        db(after, reloaded)
    );
}

#[test]
fn a_vwinds_knob_moved_on_its_page_changes_the_sound_and_survives_reload() {
    uvi_knob_moves_the_sound_and_survives_reload(
        "VWinds - Clarinets/VWinds-BbClarinet.ufs/Presets/Bb Clarinet.uvip",
        "Air",
        127.,
    );
}

#[test]
#[ignore = "Augmented Orchestra takes about a minute to load in debug"]
fn an_augmented_orchestra_knob_moved_on_its_page_changes_the_sound_and_survives_reload() {
    uvi_knob_moves_the_sound_and_survives_reload(
        "UVI - Augmented Orchestra v1.1.2-R2R/Augmented Orchestra.ufs/Presets/00 Orchestra/01 Strings/V Strings Bartok.uvip",
        "PartVolume_0",
        0.25,
    );
}

#[test]
fn a_host_save_during_a_long_load_does_not_wait_for_it() {
    let (params, mut dsp) = activate(&Saved::new(Vec::new()).write());
    params
        .shared
        .request(crate::Request::Browse(kontra_ui::app::Action::Panic));
    Tick.run(&params);
    block(&params, &mut dsp, &[]);
    let before = params.saved.read().unwrap().clone();
    assert!(before.contains("parts"), "the tick left no state: {before}");
    // A tick loading a program holds the rack for seconds.
    let held = crate::lock(&params.shared.rack);
    let t = std::time::Instant::now();
    params.capture();
    assert!(
        t.elapsed() < std::time::Duration::from_millis(50),
        "{:?}",
        t.elapsed()
    );
    drop(held);
    assert_eq!(*params.saved.read().unwrap(), before);
}

/// A load asked for shows as loading until the tick carries it out, even
/// one that fails.
#[test]
fn a_load_shows_until_the_tick_is_done_with_it() {
    let (params, _dsp) = activate("");
    assert!(!params.shared.loading());
    params
        .shared
        .request(crate::Request::Browse(kontra_ui::app::Action::Replace {
            part: 0,
            library: 0,
            preset: 0,
        }));
    assert!(params.shared.loading(), "a replace asked for shows nothing");
    Tick.run(&params);
    assert!(!params.shared.loading(), "the tick left it loading");
}

/// Areia's timer hides "Host not connected" on its first tick, with the
/// transport stopped and nothing clicked; the audio thread never stalls on
/// the control tick's capture of its persistent cells (it scanned every
/// control per cell: about 9 s per capture, so no timer ran and a page
/// switch showed the panel).
#[test]
fn a_script_timer_runs_with_the_transport_stopped_and_no_block_stalls() {
    let Some(path) = library(
        "Areia 1.2.0 [Audio Imperia]/Instruments/01 Core Technique Patches/01 Areia - 16 Violins - Core Techniques.nki",
    ) else {
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
        delays: None,
        slot_types: Vec::new(),
        slot_responses: Vec::new(),
    }]);
    let (params, mut dsp) = activate(&saved.write());
    let panel = |params: &KontraParams| {
        let guard = crate::lock(&params.shared.rack);
        let part = guard.as_ref().unwrap().parts()[0].as_ref().unwrap();
        let t = part.view.as_ref().unwrap().tree();
        let n = (t.widgets.iter()).position(|w| &*w.name == "$label_no_audio_connected_text");
        t.visible(kontra_ui_model::WidgetRef(n.expect("Areia's panel")))
    };
    assert!(panel(&params), "init shows the panel until the timer runs");
    let mut slowest = std::time::Duration::ZERO;
    for _ in 0..10 {
        Tick.run(&params);
        let t = std::time::Instant::now();
        live_block(&params, &mut dsp);
        slowest = slowest.max(t.elapsed());
    }
    Tick.run(&params);
    assert!(!panel(&params), "the timer never hid the panel");
    assert!(slowest.as_secs_f64() < 2.0, "a live block took {slowest:?}");
}

/// One live block with no events and the transport stopped, as a host
/// plays a part nobody is playing.
fn live_block(params: &KontraParams, dsp: &mut Dsp) {
    let list = EventList::with_capacity(16);
    let mut channels = vec![vec![0.0f32; BLOCK]; 2 * crate::OUTPUTS];
    let mut outs: Vec<&mut [f32]> = channels.iter_mut().map(Vec::as_mut_slice).collect();
    let mut buffer = AudioBuffer::from_slices_checked(&[], &mut outs, BLOCK);
    let transport = TransportInfo::default();
    let mut out_events = EventList::with_capacity(16);
    let mut cx = ProcessContext::new(&transport, RATE, BLOCK, &mut out_events);
    <Kontra as PluginLogic>::process(dsp, params, &mut buffer, &list, &mut cx);
}
