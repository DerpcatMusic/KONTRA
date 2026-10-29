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
        let mut cx = ProcessContext::new(&transport, RATE, BLOCK, &mut out_events);
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

/// Loads `path` into part 0 through the state, plays, saves, reloads into
/// a fresh plugin, plays again and saves again.
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
    }]);
    let t = std::time::Instant::now();
    let (live, mut live_dsp) = activate(&saved.write());
    let load = t.elapsed();
    let before = play(&live, &mut live_dsp);
    live.capture();
    let first = live.saved.read().unwrap().clone();
    let (again, mut again_dsp) = activate(&first);
    let after = play(&again, &mut again_dsp);
    again.capture();
    let second = again.saved.read().unwrap().clone();
    let mut wrong = Vec::new();
    if Saved::read(&first) != saved {
        wrong.push(format!("save differs from what loaded: {first}"));
    }
    if first != second {
        wrong.push(format!("re-save differs:\n{first}\n{second}"));
    }
    if before < 1e-4 {
        wrong.push(format!("silent: rms {before}"));
    }
    let db = 20.0 * (after / before.max(1e-9)).log10();
    if db.abs() > 1.0 {
        wrong.push(format!("reload plays {db:.2} dB from the live part"));
    }
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    let line = format!("{name}: loaded in {load:.1?}, rms {before:.4} → {after:.4}");
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
