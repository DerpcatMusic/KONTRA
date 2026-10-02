//! Actual-library MIDI lifecycle probe. Reports tails separately from held keys.
use anyhow::Result;
use kontakto::{
    articulate::{self, Articulate, In, Mode, Mpe, Route, Router, Zone},
    engine::{Engine, MAX_BLOCK, Rack},
    import,
};
use serde_json::{Value, json};
use std::{
    alloc::{GlobalAlloc, Layout, System},
    cell::Cell,
    path::Path,
    time::Instant,
};

struct CountAlloc;
thread_local! {
    static ARMED: Cell<bool> = const { Cell::new(false) };
    static HEAP: Cell<usize> = const { Cell::new(0) };
}
fn count() {
    if ARMED.with(Cell::get) {
        HEAP.with(|n| n.set(n.get() + 1));
    }
}
// SAFETY: every allocation operation is forwarded unchanged to System.
unsafe impl GlobalAlloc for CountAlloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        count();
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        count();
        unsafe { System.dealloc(ptr, layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        count();
        unsafe { System.realloc(ptr, layout, size) }
    }
}
#[global_allocator]
static ALLOCATOR: CountAlloc = CountAlloc;

#[derive(Default)]
struct Meter {
    blocks: u64,
    heap: usize,
    over_deadline: u64,
    wall_ms: f64,
    max_ms: f64,
    peak: f32,
    square: f64,
    samples: u64,
    frames: u64,
    nonfinite: u64,
}
impl Meter {
    fn measured<T>(&mut self, frames: usize, f: impl FnOnce() -> T) -> T {
        let before = HEAP.with(Cell::get);
        ARMED.with(|a| a.set(true));
        let started = Instant::now();
        let result = f();
        let ms = started.elapsed().as_secs_f64() * 1000.;
        ARMED.with(|a| a.set(false));
        self.heap += HEAP.with(Cell::get) - before;
        self.wall_ms += ms;
        self.max_ms = self.max_ms.max(ms);
        if frames > 0 {
            self.blocks += 1;
            self.over_deadline += u64::from(ms > frames as f64 / super::RATE * 1000.);
        }
        result
    }
}

fn snapshot(
    e: &Engine,
    phase: &str,
    meter: &Meter,
    start_samples: u64,
    start_square: f64,
) -> Value {
    let census = e.voice_census();
    let held: [usize; 16] =
        std::array::from_fn(|c| (0..128).filter(|&n| e.key_down(c as u8, n)).count());
    let mut phases = std::collections::BTreeMap::new();
    for v in &census {
        *phases.entry(format!("{:?}", v.phase)).or_insert(0usize) += 1;
    }
    let notes: Vec<_> = census.iter().map(|v| json!({
        "channel":v.channel, "note":v.note, "group":v.group, "phase":format!("{:?}",v.phase),
        "event":v.event.0,"physical_channel":v.input_channel,"owner":v.owner,"held":v.held,
        "released":v.released, "release_trigger":v.release_trigger,
        "gain":v.gain, "envelope":v.envelope, "level":v.gain * v.envelope,
    })).collect();
    json!({
        "phase":phase, "seconds":meter.frames as f64 / super::RATE,
        "held_keys_by_engine_channel":held, "pending_commands_writes_releases":e.pending_work(),
        "voices":census.len(), "unreleased_attack_voices":census.iter().filter(|v| !v.released && !v.release_trigger).count(),
        "release_trigger_voices":census.iter().filter(|v| v.release_trigger).count(), "envelope_phases":phases,
        "voice_notes":notes,
        "rms":((meter.square-start_square)/(meter.samples-start_samples).max(1) as f64).sqrt(),
        "sustain_cc":std::array::from_fn::<_,16,_>(|c|e.cc_state()[c][64]),
        "sostenuto_cc":std::array::from_fn::<_,16,_>(|c|e.cc_state()[c][66]),
    })
}

fn phase(
    rack: &mut Rack,
    routers: &mut [Router],
    meter: &mut Meter,
    name: &str,
    seconds: f64,
    events: &[In],
    realtime: bool,
) -> Value {
    let (start_samples, start_square) = (meter.samples, meter.square);
    let mut frames = (seconds * super::RATE).round() as u64;
    let (mut left, mut right) = ([0.; MAX_BLOCK], [0.; MAX_BLOCK]);
    let mut pace = kontakto::engine::Pace::start();
    let mut elapsed = 0u64;
    let mut last_frames = 0;
    while frames > 0 {
        if realtime {
            pace.until(std::time::Duration::from_secs_f64(
                elapsed as f64 / super::RATE,
            ));
        }
        let n = frames.min(MAX_BLOCK as u64) as usize;
        last_frames = n;
        meter.measured(n, || {
            rack.parts[0].begin_audio_block(n, 1, !realtime);
            if elapsed == 0 {
                for &event in events {
                    articulate::play(rack, routers, 0, event);
                }
            }
            let buses = rack.render(n);
            left[..n].copy_from_slice(&buses[0][0][..n]);
            right[..n].copy_from_slice(&buses[0][1][..n]);
        });
        for &x in left[..n].iter().chain(&right[..n]) {
            if !x.is_finite() {
                meter.nonfinite += 1;
                continue;
            }
            meter.peak = meter.peak.max(x.abs());
            meter.square += f64::from(x) * f64::from(x);
        }
        meter.samples += (n * 2) as u64;
        meter.frames += n as u64;
        elapsed += n as u64;
        frames -= n as u64;
    }
    let mut out = snapshot(&rack.parts[0], name, meter, start_samples, start_square);
    out["last_block_rms"] = json!((left[..last_frames].iter().chain(&right[..last_frames])
        .map(|&x| f64::from(x).powi(2)).sum::<f64>() / (last_frames * 2).max(1) as f64).sqrt());
    out
}

pub fn run(path: &Path, program: u32, snapshot: Option<&Path>, realtime: bool) -> Result<Value> {
    let started = Instant::now();
    let instrument = match snapshot {
        Some(snapshot) => import::read_snapshot(path, snapshot)?,
        None => import::read_program(path, program)?,
    };
    let (bank, scripts) = super::load(&instrument)?;
    let load_ms = started.elapsed().as_secs_f64() * 1000.;
    let load_issues = bank.issues.clone();
    let skipped_zones = bank.skipped_zones;
    let zone_skip_counts = bank.zone_skip_counts;
    let (sample_count, streamed_samples) = (bank.sample_count(), bank.streamed_samples());
    let mut rack = Rack::default();
    let e = &mut rack.parts[0];
    e.blocking_streams = !realtime;
    e.set_fx(kontakto::engine::effects(
        &instrument,
        scripts.0.as_deref(),
        super::RATE as f32,
    ));
    e.set_bank(Some(Box::new(bank)));
    let script_errors = scripts.1.clone();
    super::install_scripts(e, scripts);
    let found = kontakto::timing::found(&instrument, e);
    let path_text = path.to_string_lossy();
    let mut arts = Articulate::default();
    arts.sync(&path_text, &found);
    // Pick the covered musical key closest to middle C; named switches are excluded.
    let note = (0..=120u8)
        .filter(|n| !found.iter().any(|a| a.1 == Some(*n)))
        .filter(|n| {
            instrument
                .zones
                .iter()
                .any(|z| z.available && z.low_key <= *n && *n <= z.high_key)
        })
        .min_by_key(|n| n.abs_diff(60))
        .unwrap_or(60);
    let maximum_release = instrument
        .groups
        .iter()
        .filter_map(|g| g.volume_env.as_ref())
        .map(|e| e.release_ms as f64 / 1000.)
        .filter(|t| t.is_finite())
        .fold(0.3f64, f64::max);
    // A release reaches -60 dB at its stored time; -80 dB takes 4/3 of it.
    // This is an observation window, not a stuck-note verdict for script-defined tails.
    let tail_seconds = (maximum_release * 4. / 3. + 2.).max(8.);
    let mut routers: Vec<_> = (0..rack.parts.len()).map(|_| Router::default()).collect();
    let mut cases = Vec::new();
    for case in [
        "notes-pedals-stops",
        "channel-articulations",
        "mpe-lower",
        "mpe-upper",
    ] {
        rack.parts[0].reset(super::RATE);
        routers[0] = Router::default();
        let mpe = Mpe {
            zone: match case {
                "mpe-lower" => Zone::Lower,
                "mpe-upper" => Zone::Upper,
                _ => Zone::Off,
            },
            ..Mpe::default()
        };
        arts.mode = if case == "channel-articulations" {
            Mode::Channel
        } else {
            Mode::Keyswitch
        };
        routers[0].set_route(Route::new(&path_text, &arts, &mpe));
        if !arts.articulations.is_empty() {
            routers[0].select(0, 0, &mut rack.parts[0]);
        }
        let mut meter = Meter::default();
        let (underruns_before, dropped_before) =
            (rack.parts[0].underruns(), rack.parts[0].dropped_commands());
        let mut stages = Vec::new();
        let control_channel = if mpe.zone == Zone::Upper { 15 } else { 0 };
        let mut go = |name, seconds, events: &[In]| {
            stages.push(phase(
                &mut rack,
                &mut routers,
                &mut meter,
                name,
                seconds,
                events,
                realtime,
            ))
        };
        go(
            "controllers",
            0.05,
            &[
                In::Cc(control_channel, 1, 100),
                In::Cc(control_channel, 11, 127),
            ],
        );
        if case == "notes-pedals-stops" {
            go("same-pitch-first", 0.25, &[In::NoteOn(0, note, 100)]);
            go("same-pitch-overlap", 0.25, &[In::NoteOn(0, note, 90)]);
            go("same-pitch-off", 0.2, &[In::NoteOff(0, note)]);
            go(
                "sustain-down",
                0.25,
                &[In::Cc(0, 64, 127), In::NoteOn(0, note, 100)],
            );
            go("sustain-key-up", 0.25, &[In::NoteOff(0, note)]);
            go("sustain-up", 0.25, &[In::Cc(0, 64, 0)]);
            go(
                "sostenuto-capture",
                0.25,
                &[In::NoteOn(0, note, 100), In::Cc(0, 66, 127)],
            );
            go("sostenuto-later-key", 0.25, &[In::NoteOn(0, note + 2, 95)]);
            go(
                "sostenuto-keys-up",
                0.25,
                &[In::NoteOff(0, note), In::NoteOff(0, note + 2)],
            );
            go("sostenuto-up", 0.25, &[In::Cc(0, 66, 0)]);
            go(
                "all-notes-held-sustain",
                0.25,
                &[
                    In::Cc(0, 64, 127),
                    In::NoteOn(0, note, 100),
                    In::NoteOn(0, note + 4, 100),
                ],
            );
            go("all-notes-off", 0.25, &[In::Cc(0, 123, 0)]);
            go("all-notes-pedal-up", 0.25, &[In::Cc(0, 64, 0)]);
            go(
                "reset-controllers-pedal-and-held-key",
                0.25,
                &[
                    In::Cc(0, 64, 127),
                    In::NoteOn(0, note, 100),
                    In::NoteOff(0, note),
                    In::NoteOn(0, note + 2, 95),
                ],
            );
            go("reset-all-controllers", 0.25, &[In::Cc(0, 121, 0)]);
            go("reset-controllers-held-key-release", 0.25, &[In::NoteOff(0, note + 2)]);
            go("reset-controllers-expression-restore", 0.05, &[In::Cc(0, 1, 100), In::Cc(0, 11, 127)]);
            go("all-sound-note", 0.25, &[In::NoteOn(0, note, 100)]);
            go("all-sound-off", 0.1, &[In::Cc(0, 120, 0)]);
        } else if case == "channel-articulations" {
            go(
                "same-pitch-three-channels",
                0.5,
                &[
                    In::NoteOn(0, note, 100),
                    In::NoteOn(1, note, 95),
                    In::NoteOn(2, note, 90),
                ],
            );
            go("same-pitch-channel-two-off", 0.25, &[In::NoteOff(2, note)]);
            go(
                "same-pitch-channel-zero-retrigger",
                0.25,
                &[In::NoteOn(0, note, 85)],
            );
            go("same-pitch-channel-zero-off", 0.25, &[In::NoteOff(0, note)]);
            go("same-pitch-channel-one-off", 0.5, &[In::NoteOff(1, note)]);
            go(
                "same-pitch-reordered-ons",
                0.5,
                &[
                    In::NoteOn(2, note, 100),
                    In::NoteOn(0, note, 95),
                    In::NoteOn(1, note, 90),
                ],
            );
            go(
                "same-pitch-reordered-off-zero",
                0.25,
                &[In::NoteOff(0, note)],
            );
            go(
                "same-pitch-reordered-off-two",
                0.25,
                &[In::NoteOff(2, note)],
            );
            go("same-pitch-reordered-off-one", 0.5, &[In::NoteOff(1, note)]);
            go(
                "same-pitch-channel-sustain",
                0.25,
                &[
                    In::Cc(0, 64, 127),
                    In::NoteOn(1, note, 100),
                    In::NoteOn(2, note, 90),
                ],
            );
            go(
                "same-pitch-sustain-keys-up",
                0.25,
                &[In::NoteOff(1, note), In::NoteOff(2, note)],
            );
            go(
                "same-pitch-sustain-retrigger",
                0.25,
                &[In::NoteOn(1, note, 80)],
            );
            go(
                "same-pitch-sustain-release",
                0.5,
                &[In::NoteOff(1, note), In::Cc(0, 64, 0)],
            );
            go(
                "three-channel-chord",
                0.5,
                &[
                    In::NoteOn(0, note, 100),
                    In::NoteOn(1, note + 4, 100),
                    In::NoteOn(2, note + 7, 100),
                ],
            );
            go("channel-one-release", 0.25, &[In::NoteOff(1, note + 4)]);
            go("selective-channel-sound-off", 0.1, &[In::Cc(0, 120, 0)]);
            go(
                "surviving-channel-release",
                0.25,
                &[In::NoteOff(2, note + 7)],
            );
        } else {
            let (master, member, other) = if mpe.zone == Zone::Lower {
                (0, 1, 2)
            } else {
                (15, 14, 13)
            };
            go(
                "mpe-rpn",
                0.05,
                &[
                    In::Cc(master, 101, 0),
                    In::Cc(master, 100, 0),
                    In::Cc(master, 6, 2),
                    In::Cc(master, 38, 50),
                    In::Cc(member, 101, 0),
                    In::Cc(member, 100, 0),
                    In::Cc(member, 6, 12),
                    In::Cc(member, 38, 25),
                ],
            );
            go(
                "mpe-two-members",
                0.5,
                &[
                    In::Bend(master, 12288),
                    In::Bend(member, 10240),
                    In::Pressure(member, 80),
                    In::Cc(member, 74, 90),
                    In::NoteOn(member, note, 100),
                    In::NoteOn(other, note + 4, 100),
                ],
            );
            go(
                "mpe-master-sustain",
                0.25,
                &[In::Cc(master, 64, 127), In::NoteOff(member, note)],
            );
            go(
                "mpe-member-reuse",
                0.25,
                &[
                    In::Bend(member, 4096),
                    In::Pressure(member, 110),
                    In::NoteOn(member, note, 95),
                ],
            );
            go(
                "mpe-release",
                0.25,
                &[
                    In::NoteOff(member, note),
                    In::NoteOff(other, note + 4),
                    In::Cc(master, 64, 0),
                ],
            );
            go("mpe-master-all-notes-off", 0.1, &[In::Cc(master, 123, 0)]);
        }
        go("natural-release-observation", tail_seconds, &[]);
        let panic_note_channel = match mpe.zone {
            Zone::Upper => 14,
            Zone::Lower => 1,
            Zone::Off => 0,
        };
        go(
            "panic-pedal-and-note",
            0.25,
            &[
                In::Cc(control_channel, 64, 127),
                In::NoteOn(panic_note_channel, note, 100),
                In::Cc(control_channel, 66, 127),
            ],
        );
        drop(go);
        let panic_started = Instant::now();
        meter.measured(0, || rack.parts[0].panic());
        let panic_ms = panic_started.elapsed().as_secs_f64() * 1000.;
        stages.push(phase(
            &mut rack,
            &mut routers,
            &mut meter,
            "panic-after-declick",
            0.1,
            &[],
            realtime,
        ));
        // A new router mirrors the plugin's panic MIDI reset; a fresh note must recover.
        routers[0] = Router::default();
        stages.push(phase(
            &mut rack,
            &mut routers,
            &mut meter,
            "post-panic-fresh-note",
            0.25,
            &[
                In::Cc(0, 1, 100),
                In::Cc(0, 11, 127),
                In::NoteOn(0, note, 100),
            ],
            realtime,
        ));
        stages.push(phase(
            &mut rack,
            &mut routers,
            &mut meter,
            "post-panic-release",
            tail_seconds,
            &[In::NoteOff(0, note)],
            realtime,
        ));
        let diagnostics = rack.parts[0]
            .script()
            .map(|rt| rt.diagnostics())
            .unwrap_or_default();
        cases.push(json!({
            "case":case,"articulations_found":found.len(),"stages":stages,"panic_ms":panic_ms,
            "heap_operations_on_render_thread":meter.heap,"blocks":meter.blocks,
            "render_ms":meter.wall_ms,"max_block_or_event_ms":meter.max_ms,"blocks_exceeding_duration":meter.over_deadline,
            "peak":meter.peak,"rms":(meter.square/meter.samples.max(1) as f64).sqrt(),"nonfinite_samples":meter.nonfinite,
            "underruns":rack.parts[0].underruns()-underruns_before,"dropped_commands":rack.parts[0].dropped_commands()-dropped_before,"script_diagnostics":diagnostics,
        }));
    }
    Ok(json!({
        "build":serde_json::from_str::<Value>(kontakto::build_info::MANIFEST_JSON)?,
        "path":path,"program":program,"snapshot":snapshot,"name":instrument.name,"note":note,"load_ms":load_ms,
        "samples":sample_count,"streamed_samples":streamed_samples,"skipped_zones":skipped_zones,"zone_skip_counts":zone_skip_counts,"load_issues":load_issues,
        "script_errors":script_errors,"warnings":instrument.warnings,"realtime":realtime,
        "tail_observation_seconds":tail_seconds,"maximum_imported_release_seconds":maximum_release,
        "timing_scope":if realtime {"paced nonblocking rack render, including instrument FX, direct output and part mixing; host/GPU overhead excluded"} else {"offline blocking rack render, including instrument FX, direct output and part mixing; disk waits may exceed a realtime deadline"},
        "cases":cases,
    }))
}
