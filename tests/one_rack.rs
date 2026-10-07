//! One rack through the host core: a Kontakt instrument, a UVI program, a
//! Kontakt multi program and a WAV, each on its own MIDI channel, two parts
//! layered on one channel, one MPE part, and the articulation migration on a
//! Kontakt part, all rendered together to the DAW pairs.
//!
//! Needs `KONTRA_KONTAKT_LIBRARIES` and `KONTRA_UVI_LIBRARIES`; with
//! `KONTRA_REQUIRE_LIBRARIES=1` a missing library fails instead of skipping.
use std::{
    alloc::{GlobalAlloc, Layout, System},
    cell::Cell,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use kontakto::sound::{
    BlockInfo, Core, CoreLoader, LoadRequest,
    event::{Event, HostNote, HostPattern},
    mix::Mix,
    v2::{V2Core, V2Loader},
};

struct Counting;
thread_local! {
    static COUNTING: Cell<bool> = const { Cell::new(false) };
    static CALLS: Cell<usize> = const { Cell::new(0) };
}
// SAFETY: forwards allocation and deallocation unchanged to System.
#[allow(unsafe_code, reason = "Test-only allocator forwards unchanged to System to count calls")]
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if COUNTING.get() {
            CALLS.set(CALLS.get() + 1);
        }
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        if COUNTING.get() {
            CALLS.set(CALLS.get() + 1);
        }
        unsafe { System.dealloc(ptr, layout) }
    }
}
#[global_allocator]
static GLOBAL: Counting = Counting;

/// Heap calls `f` makes on this thread.
fn heap_calls(f: impl FnOnce()) -> usize {
    let before = CALLS.get();
    COUNTING.set(true);
    f();
    COUNTING.set(false);
    CALLS.get() - before
}

const FRAMES: usize = 128;
const RATE: f64 = 48000.0;

fn library(var: &str, relative: &str) -> Option<PathBuf> {
    let roots = std::env::var_os(var).unwrap_or_default();
    let found = std::env::split_paths(&roots).map(|r| r.join(relative)).find(|p| p.is_file() || p.to_string_lossy().contains(".ufs"));
    if found.is_none() && std::env::var_os("KONTRA_REQUIRE_LIBRARIES").is_some() {
        panic!("{var} lacks {relative}");
    }
    found
}

fn sine(path: &Path, hz: f32) {
    let spec = hound::WavSpec { channels: 1, sample_rate: 48000, bits_per_sample: 16, sample_format: hound::SampleFormat::Int };
    let mut w = hound::WavWriter::create(path, spec).unwrap();
    for i in 0..48000 {
        w.write_sample(((i as f32 * hz / 48000.0 * std::f32::consts::TAU).sin() * 16000.0) as i16).unwrap();
    }
    w.finalize().unwrap();
}

struct Rack {
    core: V2Core,
    mix: Mix,
    names: Vec<String>,
    /// Per part: tree nodes below the root.
    tree_nodes: Vec<usize>,
}

impl Rack {
    fn add(&mut self, path: PathBuf, program: u32, mpe: bool, channel: i16, port: u8, output: u8, switching: u8) -> usize {
        let part = self.names.len();
        let request = LoadRequest { path: path.clone(), program, sample_rate: RATE, mpe, ..Default::default() };
        let loaded = V2Loader.prepare(&request, &mut |_| {}, &|| false).unwrap_or_else(|e| panic!("{}: {e:?}", path.display()));
        let c = &mut self.mix.parts[part];
        (c.channel, c.port, c.output, c.mpe, c.switching) = (channel, port, output, mpe, switching);
        self.core.set_mix(&self.mix);
        self.names.push(path.file_name().unwrap().to_string_lossy().into_owned());
        self.tree_nodes.push(loaded.tree.nodes.len() - 1);
        self.core.install(part, loaded.part);
        part
    }

    /// Render `blocks`, sleeping for streams and script threads; the loudest
    /// sample seen on each DAW pair.
    fn run(&mut self, blocks: usize, sleep: bool) -> [f32; 16] {
        let mut peak = [0.0f32; 16];
        for _ in 0..blocks {
            if sleep {
                std::thread::sleep(Duration::from_millis(2));
            }
            self.core.begin_block(&BlockInfo { frames: FRAMES, ..Default::default() });
            let r = self.core.render(FRAMES);
            for (b, p) in peak.iter_mut().enumerate() {
                if r.live[b] {
                    *p = r.buses[b].iter().flat_map(|c| c[..FRAMES].iter()).fold(*p, |m, x| m.max(x.abs()));
                }
            }
            self.core.end_block(FRAMES, &mut |_| true);
        }
        peak
    }

    fn on(&mut self, channel: u8, key: u8, id: i32, port: u8) {
        let note = HostNote { port, channel, key, id, clap: true };
        self.core.event(port, Event::NoteOn { note, velocity: 100.0 / 127.0, tune: 0.0 });
    }

    fn silence(&mut self) {
        self.core.event(0, Event::NoteOff(HostPattern { port: -1, channel: -1, key: -1, id: -1, clap: true }));
        self.core.event(1, Event::NoteOff(HostPattern { port: -1, channel: -1, key: -1, id: -1, clap: true }));
        self.core.panic();
        self.run(400, false);
    }
}

fn loud(peak: f32) -> bool {
    peak > 1e-4
}

#[test]
fn one_rack_plays_every_format_together() {
    let dir = tempfile::tempdir().unwrap();
    let (wav_a, wav_b, wav_mpe) = (dir.path().join("layer_a.wav"), dir.path().join("layer_b.wav"), dir.path().join("mpe.wav"));
    sine(&wav_a, 440.0);
    sine(&wav_b, 660.0);
    sine(&wav_mpe, 220.0);
    let nki = library("KONTRA_KONTAKT_LIBRARIES", "Una Corda Library/Instruments/Una Corda Pure.nki");
    let multi = library("KONTRA_KONTAKT_LIBRARIES", "Audio Imperia CHORUS/Multis/10 Chorus - Ensemble - Traditional Syllables.nkm");
    let uvi = library("KONTRA_UVI_LIBRARIES", "VWinds - Clarinets/VWinds-ContrabassClarinet_V2.ufs/Presets/Contrabass Clarinet.uvip");
    let (Some(nki), Some(multi), Some(uvi)) = (nki, multi, uvi) else {
        eprintln!("skipped: libraries are not installed");
        return;
    };
    let mut gaps: Vec<String> = Vec::new();
    let mut rack = Rack { core: V2Core::with_parts(8, RATE), mix: Mix::default(), names: Vec::new(), tree_nodes: Vec::new() };

    // Channel 0: Kontakt .nki. 1: UVI. 2: Kontakt multi program. 3: two WAVs layered.
    // Port 1: one MPE WAV. Channel 4: an articulated Kontakt instrument whose
    // switch keys migrate to velocity.
    let velocity = 0x80 | (sampler_ir::Driver::Velocity as u8) << 1;
    let part_nki = rack.add(nki, 0, false, 0, 0, 0, 0);
    let part_uvi = rack.add(uvi, 0, false, 1, 0, 1, 0);
    let part_multi = rack.add(multi, 0, false, 2, 0, 2, 0);
    let part_a = rack.add(wav_a, 0, false, 3, 0, 3, 0);
    let part_b = rack.add(wav_b, 0, false, 3, 0, 4, 0);
    let part_mpe = rack.add(wav_mpe, 0, true, 0, 1, 5, 0);
    let articulated = [
        "Pacific Ensemble Strings/Instruments/10 Cellos/Pacific - Ens Strings - 10 Cellos - Legato Sustains.nki",
        "Performance Samples Vista/Instruments/Vista - 3 Cellos.nki",
        "Afflatus Chapter II Brass/Instruments/3. Curated Ensembles/Barbarian Brass.nki",
    ]
    .iter()
    .find_map(|r| library("KONTRA_KONTAKT_LIBRARIES", r));
    let part_art = articulated.map(|p| rack.add(p, 0, false, 4, 0, 6, velocity));
    match part_art {
        Some(p) if rack.core.articulation(p).is_some() => {}
        Some(_) => gaps.push("articulation migration: no candidate Kontakt instrument is articulated through the native switching path".into()),
        None => gaps.push("no articulated Kontakt candidate is installed".into()),
    }
    rack.run(50, true);

    // Each part sounds on its own channel and pair, and only there.
    let probes = [(0u8, 60u8, part_nki, "kontakt nki"), (1, 36, part_uvi, "uvi"), (2, 40, part_multi, "kontakt multi")];
    let mut failures = Vec::new();
    for (channel, key, part, what) in probes {
        rack.silence();
        // A multi program's range is unknown: a spread of keys finds it.
        for (i, k) in (0..if part == part_multi { 6 } else { 1 }).map(|i| key + i * 5).enumerate() {
            rack.on(channel, k.min(120), 1 + i as i32, 0);
        }
        let peak = rack.run(300, true);
        if !loud(peak[part]) {
            failures.push(format!("{what} (part {part}, channel {channel}) is silent on pair {part}"));
        }
        for (bus, p) in peak.iter().enumerate().filter(|(b, _)| *b != part && *b < 6) {
            if loud(*p) {
                failures.push(format!("{what} leaked to pair {bus}"));
            }
        }
    }
    // Layering: one note on channel 3 sounds both WAV parts and nothing else.
    rack.silence();
    rack.on(3, 60, 1, 0);
    let peak = rack.run(50, true);
    for part in [part_a, part_b] {
        if !loud(peak[part]) {
            failures.push(format!("layer part {part} is silent"));
        }
    }
    if [0, 1, 2, 5].iter().any(|&b| loud(peak[b])) {
        failures.push(format!("channel 3 leaked: {peak:?}"));
    }
    // MPE: notes on member channels of port 1 sound the MPE part only.
    rack.silence();
    rack.on(1, 60, 1, 1);
    rack.on(2, 64, 2, 1);
    rack.core.event(1, Event::midi1(0xe1, 0, 96));
    let peak = rack.run(50, true);
    if !loud(peak[part_mpe]) {
        failures.push("mpe part is silent".into());
    }
    if (0..5).any(|b| loud(peak[b])) {
        failures.push(format!("port 1 leaked to port 0 parts: {peak:?}"));
    }
    // Articulation migration: velocities select articulations on the Kontakt part.
    if let Some(p) = part_art.filter(|&p| rack.core.articulation(p).is_some()) {
        rack.silence();
        let mut seen = std::collections::BTreeSet::new();
        for (id, v) in [(1, 8.0), (2, 64.0), (3, 120.0)] {
            let note = HostNote { port: 0, channel: 4, key: 48, id, clap: true };
            rack.core.event(0, Event::NoteOn { note, velocity: v / 127.0, tune: 0.0 });
            rack.run(2, false);
            seen.extend(rack.core.articulation(p));
        }
        if seen.len() < 2 {
            failures.push(format!("velocity migration selected only {seen:?}"));
        }
    }

    // Everything together: all channels, port 1 MPE, to the mix tree and pairs.
    rack.silence();
    for (i, (channel, key)) in [(0u8, 60u8), (1, 36), (2, 60), (3, 60), (4, 48)].into_iter().enumerate() {
        rack.on(channel, key, 10 + i as i32, 0);
    }
    rack.on(1, 60, 20, 1);
    rack.on(2, 67, 21, 1);
    let peak = rack.run(300, true);
    for part in 0..part_art.map_or(6, |p| p + 1) {
        if part == part_art.unwrap_or(usize::MAX) && gaps.iter().any(|g| g.contains("articulat")) {
            continue;
        }
        if !loud(peak[part]) {
            failures.push(format!("{} is silent in the full mix (pair {part})", rack.names[part]));
        }
    }
    // Mix tree: every part reports node peaks when it has nodes.
    for part in 0..rack.names.len() {
        let mut nodes = 0;
        let mut loud_nodes = 0;
        rack.core.take_node_peaks(part, &mut |_, p| {
            nodes += 1;
            loud_nodes += usize::from(p[0].max(p[1]) > 1e-4);
        });
        if rack.tree_nodes[part] > 0 && loud_nodes == 0 {
            gaps.push(format!("{}: {} tree nodes, none carried audio ({nodes} reported)", rack.names[part], rack.tree_nodes[part]));
        }
    }

    // Audio thread: no allocation while every part plays, and block time.
    let mut times = Vec::with_capacity(2000);
    let mut allocs = 0;
    for block in 0..2000 {
        if block % 500 == 0 {
            for (i, (channel, key)) in [(0u8, 60u8), (1, 36), (2, 60), (3, 60), (4, 48)].into_iter().enumerate() {
                rack.on(channel, key, 100 + block as i32 + i as i32, 0);
            }
        }
        let start = Instant::now();
        allocs += heap_calls(|| {
            rack.core.begin_block(&BlockInfo { frames: FRAMES, ..Default::default() });
            rack.core.render(FRAMES);
            rack.core.end_block(FRAMES, &mut |_| true);
        });
        times.push(start.elapsed());
        std::thread::sleep(Duration::from_micros(500));
    }
    times.sort();
    let p99 = times[times.len() * 99 / 100];
    let deadline = Duration::from_secs_f64(FRAMES as f64 / RATE);
    println!("block p50 {:?} p99 {:?} max {:?} deadline {deadline:?}; heap calls {allocs}", times[times.len() / 2], p99, times[times.len() - 1]);
    if allocs != 0 {
        failures.push(format!("{allocs} heap calls on the audio thread"));
    }
    if p99 > deadline {
        failures.push(format!("block p99 {p99:?} exceeds the deadline {deadline:?}"));
    }
    println!("GAPS: {gaps:#?}");
    assert!(failures.is_empty(), "{failures:#?}");
}
