//! "Does it actually work" run for one instrument: load, every tab at two sizes,
//! notes, controls min→max with audio deltas, and presets. Driven by
//! tools/parity-run/run.py, which aggregates and diffs runs. Writes one
//! `result.json` (counts, metrics, widget ids, engine parameter names) and PNGs;
//! never audio, library scripts or authored values.
use super::*;
use crate::sound::{BlockInfo, MAX_BLOCK, event::Event};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

const RATE: f64 = 48000.;
const BANDS: usize = 40;
const FFT: usize = 16384;
const ENVELOPE_FRAME: usize = 480;

/// The metrics of one rendered note; the samples are dropped right after.
#[derive(Clone)]
struct Take {
    rms_db: f64,
    peak: f32,
    nan: usize,
    denormal: usize,
    clipped: usize,
    frames: usize,
    bands: Vec<f64>,
    /// RMS dB per 10 ms frame: the note's level over time (attack, decay, release).
    envelope: Vec<f64>,
    /// Groups of the voices sounding 0.1 s in.
    groups: Vec<u32>,
}

impl Take {
    fn of(samples: &[f32]) -> Self {
        let mut t = Take { rms_db: -200., peak: 0., nan: 0, denormal: 0, clipped: 0, frames: samples.len(), bands: vec![-200.; BANDS], groups: Vec::new(),
            envelope: samples.chunks(ENVELOPE_FRAME).map(|c| 10. * (c.iter().filter(|x| x.is_finite()).map(|&x| f64::from(x) * f64::from(x)).sum::<f64>() / c.len() as f64 + 1e-20).log10()).collect() };
        let mut sum = 0f64;
        for &x in samples {
            if !x.is_finite() {
                t.nan += 1;
                continue;
            }
            if x != 0. && x.abs() < f32::MIN_POSITIVE {
                t.denormal += 1;
            }
            if x.abs() >= 1. {
                t.clipped += 1;
            }
            t.peak = t.peak.max(x.abs());
            sum += f64::from(x) * f64::from(x);
        }
        t.rms_db = 10. * (sum / samples.len().max(1) as f64 + 1e-20).log10();
        // Log bands 40 Hz..20 kHz of a Hann window after the attack.
        let start = (0.05 * RATE) as usize;
        if samples.len() >= start + FFT {
            use realfft::RealFftPlanner;
            let fft = RealFftPlanner::<f32>::new().plan_fft_forward(FFT);
            let mut input: Vec<f32> = samples[start..start + FFT].iter().enumerate()
                .map(|(i, x)| if x.is_finite() { x * (0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / FFT as f32).cos()) } else { 0. })
                .collect();
            let mut out = fft.make_output_vec();
            fft.process(&mut input, &mut out).unwrap();
            let mut power = vec![0f64; BANDS];
            for (bin, c) in out.iter().enumerate().skip(1) {
                let hz = bin as f64 * RATE / FFT as f64;
                let band = ((hz / 40.).log2() / (500f64).log2() * BANDS as f64).floor();
                if (0. ..BANDS as f64).contains(&band) {
                    power[band as usize] += f64::from(c.norm_sqr());
                }
            }
            t.bands = power.iter().map(|p| 10. * (p + 1e-20).log10()).collect();
        }
        t
    }
    fn silent(&self) -> bool {
        self.rms_db < -90.
    }
    /// (|RMS change| dB, mean |band change| dB over bands within 60 dB of the
    /// loudest, mean |level-over-time change| dB over frames within 60 dB of the
    /// loudest). The last catches attack, decay and release changes the whole-note
    /// RMS averages away.
    fn delta(&self, other: &Take) -> (f64, f64, f64) {
        let live = |a: &[f64], b: &[f64]| {
            let top = a.iter().chain(b).cloned().fold(-200., f64::max);
            let live: Vec<f64> = a.iter().zip(b).filter(|(a, b)| a.max(**b) > top - 60.).map(|(a, b)| (a - b).abs()).collect();
            live.iter().sum::<f64>() / live.len().max(1) as f64
        };
        ((self.rms_db - other.rms_db).abs(), live(&self.bands, &other.bands), live(&self.envelope, &other.envelope))
    }
    fn json(&self) -> Value {
        json!({"rms_db": round(self.rms_db), "peak": round(f64::from(self.peak)), "nan": self.nan,
            "denormal": self.denormal, "clipped": self.clipped, "frames": self.frames})
    }
}

fn round(x: f64) -> f64 {
    (x * 100.).round() / 100.
}

/// Measured by playing the same note twice: scripted randomness, round robin.
struct Noise {
    rms: f64,
    spectral: f64,
    envelope: f64,
}
impl Noise {
    fn of(a: &Take, b: &Take) -> Self {
        let (r, s, e) = a.delta(b);
        Noise { rms: (3. * r).max(0.5), spectral: (3. * s).max(1.0), envelope: (3. * e).max(0.5) }
    }
    fn changed(&self, a: &Take, b: &Take) -> (bool, Value) {
        let (rms, spectral, envelope) = a.delta(b);
        let changed = !(a.silent() && b.silent()) && (rms > self.rms || spectral > self.spectral || envelope > self.envelope);
        (changed, json!({"rms_delta_db": round(rms), "spectral_delta_db": round(spectral), "envelope_delta_db": round(envelope)}))
    }
    /// What an audio comparison says about a control that moved.
    fn verdict(&self, a: &Take, b: &Take) -> (&'static str, Value) {
        let (changed, delta) = self.changed(a, b);
        let (rms, spectral, envelope) = a.delta(b);
        let verdict = if a.silent() && b.silent() { "both-takes-silent" } else if changed { "pass" }
            else if rms > 0.5 || spectral > 1.0 || envelope > 0.5 { "masked-by-noise" } else { "moves-but-silent-change" };
        (verdict, delta)
    }
}

fn kb(key: &str) -> u64 {
    std::fs::read_to_string("/proc/self/status").unwrap_or_default().lines()
        .find_map(|l| l.strip_prefix(key)?.split_whitespace().next()?.parse().ok())
        .unwrap_or(0)
}

impl Gate {
    fn render(&mut self, seconds: f64, out: &mut Vec<f32>) {
        let frames = (seconds * RATE) as usize;
        let mut done = 0;
        let mut blocks = 0;
        while done < frames {
            let n = MAX_BLOCK.min(frames - done);
            self.core.begin_block(&BlockInfo { frames: n, offline: true, ..Default::default() });
            let r = self.core.render(n);
            let start = out.len();
            out.resize(start + n, 0.);
            for (bus, live) in r.buses.iter().zip(r.live) {
                if live {
                    for i in 0..n {
                        out[start + i] += bus[0][i] + bus[1][i];
                    }
                }
            }
            let _ = self.core.end_block(n, &mut |_| true);
            blocks += 1;
            if blocks % 32 == 0 {
                self.params.shared.widget_gate_readback(&mut self.core);
            }
            done += n;
        }
        self.params.shared.widget_gate_readback(&mut self.core);
    }
    /// Dynamics up as a player leaves them, then one note held 1 s and released.
    fn take(&mut self, key: u8, velocity: u8) -> Take {
        self.set_dynamics();
        self.take_as_is(key, velocity)
    }
    fn set_dynamics(&mut self) {
        let dynamics = self.params.shared.view.lock().unwrap().parts[0].report.as_ref()
            .map(|r| r.decoded.dynamics.clone()).unwrap_or_default();
        for (cc, _) in dynamics {
            self.core.play(0, Event::midi1(0xb0, cc, 100));
        }
    }
    /// One note with the controllers as they are: a control under test may own
    /// a dynamics CC (a mod wheel knob), which `take` would overwrite.
    fn take_as_is(&mut self, key: u8, velocity: u8) -> Take {
        self.core.release_all_notes();
        let mut tail = Vec::new();
        self.render(0.3, &mut tail);
        self.core.widget_gate_reseed(0);
        let mut samples = Vec::new();
        self.core.play(0, Event::midi1(0x90, key, velocity));
        // A stimulus after note-on: scripts reset per-note values at note start.
        for [s, d1, d2] in self.during.clone() {
            self.core.play(0, Event::midi1(s, d1, d2));
        }
        self.render(0.1, &mut samples);
        let mut groups: Vec<u32> = self.core.voice_taps(0).iter().flatten().map(|t| t.group).collect();
        groups.sort();
        groups.dedup();
        // Long enough for decay to reach sustain and release to fade: envelope
        // controls change nothing audible in a short note.
        self.render(0.9, &mut samples);
        self.core.play(0, Event::midi1(0x80, key, 0));
        self.render(0.8, &mut samples);
        Take { groups, ..Take::of(&samples) }
    }
    fn tab(&mut self, view: &str) -> bool {
        let id = format!("view-0-{view}");
        if self.editor.ui.scene().unwrap().surface(&id).is_none() {
            self.editor.press("header-0");
        }
        if self.editor.ui.scene().unwrap().surface(&id).is_none() {
            return false;
        }
        self.editor.press(&id);
        for _ in 0..10 {
            self.settle();
        }
        self.editor.settle_art();
        true
    }
    /// Drag far up-right (max) or down-left (min) in steps and play a note
    /// with the button still held: a spring-loaded control (a pitch wheel)
    /// returns to centre on release, so a note after release hears nothing.
    fn sweep(&mut self, at: Point, up: bool, key: u8) -> (usize, Vec<String>, Take) {
        self.phase = "parity-sweep";
        self.observing = Some(self.values());
        self.observed_changes.clear();
        self.submitted.clear();
        let sign = if up { -1. } else { 1. };
        self.pointer(at, true);
        for step in 1..=10 {
            let d = 150. * f64::from(step) * sign;
            self.pointer(Point::new(at.x - d, at.y + d), true);
        }
        let end = Point::new(at.x - 1500. * sign, at.y + 1500. * sign);
        for _ in 0..8 {
            self.pointer(end, true);
        }
        let take = self.take_as_is(key, 100);
        self.pointer(end, false);
        self.settle();
        self.observing = None;
        (self.submitted.len(), std::mem::take(&mut self.observed_changes).into_iter().collect(), take)
    }
    fn generation(&self) -> u64 {
        self.params.shared.view.lock().unwrap().parts[0].generation
    }
}

/// Fully inside the window or straddling its edge; partial overlaps of sibling
/// hit surfaces (neither contains the other) inside it.
fn layout(ui: &Ui, w: f64, h: f64) -> Value {
    type R = [f64; 4]; // x0 y0 x1 y1
    let scene = ui.scene().unwrap();
    let rects: Vec<(String, R)> = scene.surfaces()
        .filter(|s| !s.disabled && s.frame.size.width > 1. && s.frame.size.height > 1.)
        .map(|s| (s.key.to_string(), [s.frame.x, s.frame.y, s.frame.x + s.frame.size.width, s.frame.y + s.frame.size.height]))
        .collect();
    let contains = |a: &R, b: &R| b[0] >= a[0] - 1. && b[1] >= a[1] - 1. && b[2] <= a[2] + 1. && b[3] <= a[3] + 1.;
    // Leaves only: containers (scroll areas, panels) legitimately extend and nest.
    let rects: Vec<(String, R)> = rects.iter()
        .filter(|(k, a)| !rects.iter().any(|(j, b)| j != k && contains(a, b)))
        .cloned().collect();
    let inside = |r: &R| r[0] >= -1. && r[1] >= -1. && r[2] <= w + 1. && r[3] <= h + 1.;
    let visible = |r: &R| r[0] < w && r[1] < h && r[2] > 0. && r[3] > 0.;
    let clipped: Vec<&String> = rects.iter().filter(|(_, r)| visible(r) && !inside(r)).map(|(k, _)| k).collect();
    let within: Vec<&(String, R)> = rects.iter().filter(|(_, r)| inside(r)).collect();
    let area = |r: &R| (r[2] - r[0]) * (r[3] - r[1]);
    let (mut overlaps, mut anonymous) = (Vec::new(), 0);
    for (i, (ka, a)) in within.iter().map(|x| (&x.0, &x.1)).enumerate() {
        for (kb, b) in within[i + 1..].iter().map(|x| (&x.0, &x.1)) {
            let ix = a[2].min(b[2]) - a[0].max(b[0]);
            let iy = a[3].min(b[3]) - a[1].max(b[1]);
            if ix > 0. && iy > 0. && ix * iy > 0.25 * area(a).min(area(b)) && !contains(a, b) && !contains(b, a) {
                // Anonymous product layout keys ("/0/2/...") are informational only.
                if ka.starts_with('/') && kb.starts_with('/') { anonymous += 1 } else { overlaps.push(json!([ka, kb])) }
            }
        }
    }
    json!({"surfaces": rects.len(), "clipped": clipped.len(), "clipped_examples": clipped.iter().take(8).collect::<Vec<_>>(),
        "overlaps": overlaps.len(), "anonymous_overlaps": anonymous, "overlap_examples": overlaps.iter().take(8).collect::<Vec<_>>()})
}

/// Share of pixels that differ from the top-left one; ~0 is a blank view.
fn ink(rgba: &[u8]) -> f64 {
    let first = &rgba[..4];
    let differ = rgba.chunks_exact(4).filter(|p| p.iter().zip(first).any(|(a, b)| a.abs_diff(*b) > 8)).count();
    differ as f64 / (rgba.len() / 4).max(1) as f64
}

fn snapshots(nki: &Path) -> Vec<PathBuf> {
    let stem = nki.file_stem().unwrap_or_default();
    let mut found = Vec::new();
    let mut stack: Vec<PathBuf> = nki.ancestors().skip(1).take(3)
        .map(|a| a.join("Snapshots").join(stem)).filter(|p| p.is_dir()).take(1).collect();
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
            let p = entry.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().is_some_and(|e| e.eq_ignore_ascii_case("nksn")) {
                found.push(p);
            }
        }
    }
    found.sort();
    found
}

/// The script's engine-parameter writes during one control test.
struct Writes {
    applied: usize,
    /// Addresses a script wrote that no engine parameter is bound to: the write
    /// is dropped, so the control cannot change the sound.
    unbound: Vec<String>,
}
impl Writes {
    fn of(outcomes: &[sampler_core::EngineParameterOutcome]) -> Self {
        let mut unbound: Vec<String> = outcomes.iter().filter(|o| o.write && o.result.is_err())
            .map(|o| o.address.map_or_else(|| "unresolved address".into(), |a| format!("{} group {} slot {} generic {}",
                sampler_core::engine_parameter_name(a.parameter).unwrap_or("?"), a.group, a.slot, a.generic)))
            .collect();
        unbound.sort();
        unbound.dedup();
        Writes { applied: outcomes.iter().filter(|o| o.write && o.result.is_ok()).count(), unbound }
    }
    /// An audio verdict refined by what the script did: a write to an unbound
    /// address is an engine gap; no write at all (an unassigned macro) means the
    /// script itself leaves the sound alone in this state.
    fn verdict(&self, audio: &'static str, bound_route: bool) -> &'static str {
        match audio {
            "moves-but-silent-change" if !self.unbound.is_empty() => "unbound-engine-write",
            "moves-but-silent-change" if self.applied == 0 && !bound_route => "inert-in-state",
            _ => audio,
        }
    }
    fn json(&self) -> Value {
        json!({"applied": self.applied, "unbound": self.unbound})
    }
}

/// Make a parameter audible before it is swept, and say what it modulates:
/// decay and hold only sound with sustain below full, an attack curve only
/// over a long attack, a modulation depth only while its controller source is
/// up. Returns the context and the controls to restore afterwards.
fn stimulus(gate: &mut Gate, bindings: &[sampler_core::EngineParameterBinding], instrument: Option<&sampler_ir::Instrument>,
    b: &sampler_core::EngineParameterBinding) -> (Value, Vec<(sampler_ui_ir::ControlId, f64)>) {
    let name = sampler_core::engine_parameter_name(b.address.parameter).unwrap_or("?");
    let sibling = |p: &str| bindings.iter().find(|s| sampler_core::engine_parameter_name(s.address.parameter) == Some(p)
        && (s.address.group, s.address.slot, s.address.generic) == (b.address.group, b.address.slot, b.address.generic));
    let set = match name {
        "$ENGINE_PAR_DECAY" | "$ENGINE_PAR_HOLD" => sibling("$ENGINE_PAR_SUSTAIN").map(|s| (s.control, 0.)),
        // Half a second, in the envelope control's frames.
        "$ENGINE_PAR_ATK_CURVE" => sibling("$ENGINE_PAR_ATTACK").map(|s| (s.control, 0.5 * RATE)),
        _ => None,
    };
    let mut restore = Vec::new();
    let mut context = json!({});
    if let Some((control, v)) = set {
        let id = sampler_ui_ir::ControlId(control.0);
        if let Some(old) = gate.core.control_value(0, id) {
            restore.push((id, old));
        }
        gate.core.set_control(0, id, v);
        context["set"] = json!(format!("{} = {v}", sampler_core::engine_parameter_name(bindings.iter().find(|s| s.control == control).unwrap().address.parameter).unwrap_or("?")));
    }
    let variant = |debug: String| debug.split(['(', ' ', '{']).next().unwrap_or_default().to_string();
    let route = instrument.and_then(|i| i.source_indices.route_controls.iter()
        .find(|(_, depth, _)| sampler_core::lower::ir_control_id(&i.controls[depth.0].key) == b.control)
        .map(|(r, _, bypass)| (&i.routes[r.0], &i.modulators[i.routes[r.0].source.0].source,
            sampler_ui_ir::ControlId(sampler_core::lower::ir_control_id(&i.controls[bypass.0].key).0))));
    if let Some((route, source, bypass)) = route {
        context["source"] = json!(variant(format!("{source:?}")));
        context["target"] = json!(variant(format!("{:?}", route.target)));
        // A bypassed modulation slot is silent at any depth: enable it for the test.
        if let Some(old) = gate.core.control_value(0, bypass).filter(|v| *v >= 0.5) {
            restore.push((bypass, old));
            gate.core.set_control(0, bypass, 0.);
            context["unbypassed"] = json!(true);
        }
        // Up during each take, back to rest after (the caller resets dynamics
        // CCs to 100). ponytail: script sources are assumed aftertouch-driven,
        // as Conflux's are (on controller: AT -> EVENT_PAR_MOD_VALUE_ID 0);
        // map per library if another drives them from something else.
        let (up, rest) = match source {
            sampler_ir::ModulationSource::Controller(cc) => ([0xb0, *cc, 127], [0xb0, *cc, 0]),
            sampler_ir::ModulationSource::PitchBend => ([0xe0, 127, 127], [0xe0, 0, 64]),
            sampler_ir::ModulationSource::ChannelPressure | sampler_ir::ModulationSource::Script(_)
            | sampler_ir::ModulationSource::ScriptBipolar(_) => ([0xd0, 127, 0], [0xd0, 0, 0]),
            _ => ([0; 3], [0; 3]),
        };
        if up[0] != 0 {
            context["stimulus"] = json!(format!("{up:?}"));
            gate.during.push(up);
            gate.rest.push(rest);
        }
    }
    gate.settle();
    (context, restore)
}

/// What a changed gate key is: the instrument control's source key, or the
/// script variable's name (identifiers only, never values).
fn owner_of(gate: &Gate, instrument: Option<&sampler_ir::Instrument>, key: &str) -> String {
    let view = gate.params.shared.view.lock().unwrap();
    if let Some(id) = key.strip_prefix("control-").and_then(|n| n.parse::<u128>().ok()) {
        if let Some(c) = instrument.and_then(|i| i.controls.iter().find(|c| { sampler_core::lower::ir_control_id(&c.key).0 } == id)) {
            return format!("control {}", c.key);
        }
        return view.parts[0].interfaces.iter().flat_map(|f| &f.widgets)
            .find(|w| w.binding == ir::Binding::Control(ir::ControlId(id)))
            .map_or_else(|| format!("control {id:x}"), |w| format!("widget {}", w.name));
    }
    let id: Option<u128> = key.strip_prefix("typed-").and_then(|n| n.split('-').next()?.parse().ok());
    view.parts[0].interfaces.iter().flat_map(|f| &f.widgets)
        .find_map(|w| match &w.binding {
            ir::Binding::Variable { script, name } if Some({ sampler_ksp::derived_control_id(*script, name).0 }) == id => Some(format!("variable {script}:{name}")),
            _ => None,
        })
        .unwrap_or_else(|| key.to_string())
}

fn check(name: &str, pass: bool, detail: Value) -> Value {
    json!({"check": name, "pass": pass, "detail": detail})
}

#[test]
#[ignore = "installed instrument parity run; driven by tools/parity-run/run.py"]
fn parity_run() {
    let path = std::env::var("KONTRA_PARITY_PATH").expect("KONTRA_PARITY_PATH");
    let out = PathBuf::from(std::env::var_os("KONTRA_PARITY_OUT").expect("KONTRA_PARITY_OUT"));
    let env = |k: &str, d: usize| std::env::var(k).ok().and_then(|v| v.parse().ok()).unwrap_or(d);
    let (max_presets, max_controls, max_engine) = (env("KONTRA_PARITY_PRESETS", 8), env("KONTRA_PARITY_CONTROLS", 24), env("KONTRA_PARITY_ENGINE", 8));
    std::fs::create_dir_all(&out).unwrap();
    let mut checks = Vec::new();

    // 1. Load through the product load path.
    let mut selection = Selection::default();
    selection.parts = vec![Part { path: path.clone(), ..Default::default() }];
    let t0 = Instant::now();
    let mut gate = Gate::load(selection);
    for _ in 0..50 {
        gate.settle();
    }
    let load_ms = t0.elapsed().as_secs_f64() * 1e3;
    let peak_rss_kb = kb("VmHWM:");
    let (status, report, trace, instrument) = {
        let view = gate.params.shared.view.lock().unwrap();
        let p = &view.parts[0];
        (p.status.clone(), p.report.clone(), p.trace.clone(), p.instrument.clone())
    };
    let failure = load_failure(&status);
    let mut by_reason = BTreeMap::<String, usize>::new();
    let mut top = BTreeMap::<String, usize>::new();
    if let Some(r) = &report {
        for m in &r.missing {
            *by_reason.entry(format!("{:?}", m.reason)).or_default() += 1;
            *top.entry(format!("{:?}: {}", m.reason, m.feature)).or_default() += 1;
        }
    }
    let mut top: Vec<_> = top.into_iter().collect();
    top.sort_by(|a, b| b.1.cmp(&a.1));
    let unsupported = by_reason.get("Unknown").copied().unwrap_or(0);
    let partial = by_reason.get("NotModeled").copied().unwrap_or(0) + by_reason.get("UnknownLaw").copied().unwrap_or(0)
        + by_reason.get("InvalidValue").copied().unwrap_or(0);
    let load_faults = gate.faults();
    let replay_faults = report.as_ref().map_or(0, |r| r.faults.len());
    let load = json!({"ms": round(load_ms), "peak_rss_mb": peak_rss_kb / 1024, "status_failure": failure,
        "stages_ms": trace.as_ref().map(|t| t["stages_ms"].clone()), "trace_elapsed_ms": trace.as_ref().map(|t| t["elapsed_ms"].clone()),
        "decoded": report.as_ref().map(|r| json!({"format": r.decoded.format, "zones": r.decoded.zones, "groups": r.decoded.groups,
            "samples": r.decoded.samples, "scripts": r.decoded.scripts, "controls": r.decoded.controls})),
        "missing_by_reason": by_reason, "unsupported": unsupported, "partial": partial,
        "top_missing": top.iter().take(12).map(|(k, n)| json!([k, n])).collect::<Vec<_>>(),
        "script_faults": load_faults, "replay_faults": replay_faults,
        "uvi_fault_first": report.as_ref().and_then(|r| r.uvi_faults.first.as_ref().map(|c| format!("{c:?}"))),
        "runtime": report.as_ref().map(|r| serde_json::to_value(&r.runtime).unwrap()),
        "fault_events": gate.fault_events.iter().take(8).cloned().collect::<Vec<_>>()});
    checks.push(check("load", failure == "none" && report.is_some(), json!({"failure": failure})));
    checks.push(check("load-script-faults", load_faults == 0 && replay_faults == 0, json!({"faults": load_faults, "replay": replay_faults})));

    // 2. Every tab at two sizes.
    let mut ui = Vec::new();
    for (w, h) in [(900u16, 600u16), (1500, 1100)] {
        gate.editor.resize(Size::new(w.into(), h.into()));
        for view in ["Interface", "Articulations", "Mapping", "Sound", "Info", "Scripts"] {
            if !gate.tab(view) {
                ui.push(json!({"view": view, "width": w, "present": false}));
                continue;
            }
            let rgba = super::super::tests::pixels(&gate.editor.ui, w, h);
            let png = format!("{view}-{w}.png");
            moose::core::screenshot::save_png(&out.join(&png), &rgba, w.into(), h.into());
            let ink = ink(&rgba);
            let lay = layout(&gate.editor.ui, w.into(), h.into());
            let pass = ink > 0.02 && lay["clipped"] == 0 && lay["overlaps"] == 0;
            ui.push(json!({"view": view, "width": w, "present": true, "png": png, "ink": round(ink), "layout": lay, "pass": pass}));
        }
    }
    gate.editor.resize(Size::new(1500., 1100.));
    for row in ui.iter().filter(|r| r["present"] == true) {
        let name = format!("ui-{}-{}", row["view"].as_str().unwrap(), row["width"]);
        checks.push(check(&name, row["pass"].as_bool().unwrap_or(false), json!({"ink": row["ink"], "clipped": row["layout"]["clipped"], "overlaps": row["layout"]["overlaps"], "present": row["present"]})));
    }

    // 5 (first, it picks the key). Notes at a few keys and velocities.
    let mask = report.as_ref().map_or(0, |r| r.decoded.keys);
    let mapped: Vec<u8> = (0..128u8).filter(|k| mask >> k & 1 == 1).collect();
    let key = mapped.iter().copied().min_by_key(|k| k.abs_diff(60)).unwrap_or(60);
    let spread: Vec<u8> = if mapped.is_empty() { vec![48, 60, 72] } else {
        let mut v: Vec<u8> = [0.1, 0.4, 0.6, 0.9].iter().map(|f| mapped[((mapped.len() - 1) as f64 * f) as usize]).collect();
        v.dedup();
        v
    };
    let mut notes = Vec::new();
    for &k in &spread {
        for velocity in [40u8, 110] {
            let t = gate.take(k, velocity);
            let ok = !t.silent() && t.nan == 0 && t.clipped == 0 && t.denormal < t.frames / 100;
            notes.push(json!({"key": k, "velocity": velocity, "pass": ok, "take": t.json()}));
        }
    }
    let notes_pass = notes.iter().all(|n| n["pass"] == true);
    checks.push(check("notes", notes_pass, json!({"failed": notes.iter().filter(|n| n["pass"] != true).count(), "total": notes.len()})));

    // Same note twice: the threshold above per-take randomness.
    let (a, b) = (gate.take(key, 100), gate.take(key, 100));
    let (r, s, e) = a.delta(&b);
    let noise = Noise::of(&a, &b);
    let baseline = json!({"key": key, "rms_delta_db": round(r), "spectral_delta_db": round(s), "envelope_delta_db": round(e),
        "threshold_rms_db": round(noise.rms), "threshold_spectral_db": round(noise.spectral), "threshold_envelope_db": round(noise.envelope), "sounding_groups": a.groups.len()});

    // 4. Controls: performance-view widgets through real pointer drags.
    gate.tab("Interface");
    let bindings = gate.core.widget_gate_engine_bindings(0);
    let route_of = |key: &str| -> Option<String> {
        let id: u128 = key.strip_prefix("control-")?.parse().ok()?;
        bindings.iter().find(|b| b.control.0 == id).map(|b| format!("{} group {} slot {} generic {}",
            sampler_core::engine_parameter_name(b.address.parameter).unwrap_or("?"), b.address.group, b.address.slot, b.address.generic))
    };
    let mut controls = Vec::new();
    let targets: Vec<_> = gate.targets().into_iter().filter(|(_, k)| matches!(k.as_str(), "knob" | "slider" | "native-drag")).collect();
    let total_targets = targets.len();
    for (id, kind) in targets.into_iter().take(max_controls) {
        let Some(at) = gate.hit(&id) else {
            controls.push(json!({"id": id, "kind": kind, "verdict": "occluded"}));
            continue;
        };
        gate.engine_outcomes.clear();
        let first_fault = gate.fault_events.len();
        gate.faults();
        let (sub_min, keys_min, low) = gate.sweep(at, false, key);
        let (sub_max, keys_max, high) = gate.sweep(at, true, key);
        let faults = gate.faults();
        let fault = gate.fault_events.get(first_fault).map(|f| format!("{} {}", f["callback"], f["outcome"]));
        let writes = Writes::of(&std::mem::take(&mut gate.engine_outcomes));
        let (audio_verdict, delta) = noise.verdict(&low, &high);
        let mut keys: Vec<String> = keys_min.into_iter().chain(keys_max).collect();
        keys.sort();
        keys.dedup();
        let routes: Vec<String> = keys.iter().filter_map(|k| route_of(k)).collect();
        let verdict = if sub_min + sub_max == 0 { "ui-not-submitted" } else if keys.is_empty() { "engine-unchanged" }
            else if faults > 0 && audio_verdict == "moves-but-silent-change" { "script-fault" }
            else { writes.verdict(audio_verdict, !routes.is_empty()) };
        let owners: Vec<String> = keys.iter().map(|k| owner_of(&gate, instrument.as_deref(), k)).collect();
        controls.push(json!({"id": id, "kind": kind, "verdict": verdict, "at": [at.x.round(), at.y.round()], "submitted": sub_min + sub_max,
            "engine_keys": keys.len(), "owners": owners, "routes": routes, "engine_writes": writes.json(), "faults": faults, "first_fault": fault, "audio": delta}));
    }
    // Host automation the instrument assigns.
    let mut host = Vec::new();
    let assigned: Vec<_> = instrument.as_ref().map(|i| i.script_automation.clone()).unwrap_or_default().into_iter()
        .filter(|a| matches!(a.source, sampler_ir::ScriptAutomationSource::HostParameter(_))).collect();
    let host_total = assigned.len();
    for a in assigned.iter().step_by((host_total / max_controls.max(1)).max(1)).take(max_controls) {
        let sampler_ir::ScriptAutomationSource::HostParameter(address) = a.source else { continue };
        gate.engine_outcomes.clear();
        let ok_low = gate.core.host_parameter(address, 0.);
        gate.settle();
        let low = gate.take(key, 100);
        let ok_high = gate.core.host_parameter(address, 1.);
        gate.settle();
        let high = gate.take(key, 100);
        let writes = Writes::of(&std::mem::take(&mut gate.engine_outcomes));
        let (audio, delta) = noise.verdict(&low, &high);
        host.push(json!({"address": address, "accepted": ok_low && ok_high, "slider": a.slider, "switch": a.switch,
            "verdict": if !(ok_low && ok_high) { "rejected" } else { writes.verdict(audio, false) }, "engine_writes": writes.json(), "audio": delta}));
    }
    // A spread sample of engine parameters bound to controls.
    let mut engine = Vec::new();
    let audible: Vec<_> = bindings.iter().filter(|b| b.address.group < 0 || a.groups.contains(&(b.address.group as u32))).collect();
    let step = (audible.len() / max_engine.max(1)).max(1);
    for b in audible.into_iter().step_by(step).take(max_engine) {
        let id = sampler_ui_ir::ControlId(b.control.0);
        let (context, restore) = stimulus(&mut gate, &bindings, instrument.as_deref(), b);
        let before = gate.core.control_value(0, id);
        gate.core.set_control(0, id, -1e12);
        gate.settle();
        let lo_v = gate.core.control_value(0, id);
        let low = gate.take(key, 100);
        gate.core.set_control(0, id, 1e12);
        gate.settle();
        let hi_v = gate.core.control_value(0, id);
        let high = gate.take(key, 100);
        if let Some(v) = before {
            gate.core.set_control(0, id, v);
        }
        for (control, v) in restore {
            gate.core.set_control(0, control, v);
        }
        gate.during.clear();
        for [s, d1, d2] in std::mem::take(&mut gate.rest) {
            gate.core.play(0, Event::midi1(s, d1, d2));
        }
        gate.set_dynamics();
        let (audio, delta) = noise.verdict(&low, &high);
        let moved = lo_v != hi_v;
        engine.push(json!({"route": format!("{} group {} slot {} generic {}", sampler_core::engine_parameter_name(b.address.parameter).unwrap_or("?"), b.address.group, b.address.slot, b.address.generic),
            "moved": moved, "context": context, "verdict": if !moved { "engine-unchanged" } else { audio }, "audio": delta}));
    }
    // Performance inputs as MIDI, rest vs full, after note-on: tells a dead
    // wheel knob (its script only forwards set_controller) from an instrument
    // whose modulation slots ignore the wheel in this state.
    let mut midi = Vec::new();
    for (name, rest, up) in [("cc1", [0xb0, 1, 0], [0xb0, 1, 127]), ("pitch-bend", [0xe0, 0, 64], [0xe0, 0, 0]),
        ("aftertouch", [0xd0, 0, 0], [0xd0, 127, 0])] {
        gate.during = vec![rest];
        let low = gate.take_as_is(key, 100);
        gate.during = vec![up];
        let high = gate.take_as_is(key, 100);
        gate.during.clear();
        gate.core.play(0, Event::midi1(rest[0], rest[1], rest[2]));
        gate.set_dynamics();
        let (verdict, delta) = noise.verdict(&low, &high);
        midi.push(json!({"input": name, "verdict": verdict, "audio": delta}));
    }
    for (name, rows) in [("controls-ui", &controls), ("controls-host", &host), ("controls-engine", &engine)] {
        let failed: Vec<&Value> = rows.iter().filter(|r| !matches!(r["verdict"].as_str(), Some("pass" | "masked-by-noise" | "inert-in-state"))).collect();
        checks.push(check(name, failed.is_empty(), json!({"failed": failed.len(), "total": rows.len()})));
    }

    // 3. Presets: the first N snapshots and the last.
    let all = snapshots(Path::new(&path));
    let mut picked: Vec<&PathBuf> = all.iter().take(max_presets).collect();
    if let Some(last) = all.last().filter(|_| all.len() > max_presets) {
        picked.push(last);
    }
    let mut presets = Vec::new();
    let mut previous = gate.take(key, 100);
    for (n, preset) in picked.into_iter().enumerate() {
        let generation = gate.generation();
        gate.params.selection.write().unwrap().parts[0].select_snapshot(preset.to_string_lossy().into_owned());
        let t = Instant::now();
        Load.run(&gate.params);
        gate.params.shared.widget_gate_install(&mut gate.core);
        for _ in 0..20 {
            gate.settle();
        }
        let ms = t.elapsed().as_secs_f64() * 1e3;
        let failure = load_failure(&gate.params.shared.view.lock().unwrap().parts[0].status);
        let rebuilt = gate.generation() != generation;
        gate.tab("Interface");
        let rgba = super::super::tests::pixels(&gate.editor.ui, 1500, 1100);
        let png = format!("preset-{n:02}.png");
        moose::core::screenshot::save_png(&out.join(&png), &rgba, 1500, 1100);
        let faults = gate.faults();
        let take = gate.take(key, 100);
        let (audio, delta) = noise.changed(&previous, &take);
        let pass = failure == "none" && rebuilt && ink(&rgba) > 0.02 && audio && !take.silent() && faults == 0;
        presets.push(json!({"name": preset.file_stem().unwrap().to_string_lossy(), "png": png, "ms": round(ms), "failure": failure,
            "ui_rebuilt": rebuilt, "ink": round(ink(&rgba)), "faults": faults, "audio_changed": audio, "audio": delta, "take": take.json(), "silent": take.silent(), "pass": pass}));
        previous = take;
    }
    checks.push(check("presets", presets.iter().all(|p| p["pass"] == true),
        json!({"failed": presets.iter().filter(|p| p["pass"] != true).count(), "total": presets.len(), "available": all.len()})));

    let result = json!({"parity_run_schema": 1, "path": path, "checks": checks, "load": load, "ui": ui, "notes": notes,
        "baseline": baseline, "controls": {"ui": controls, "ui_targets_total": total_targets, "host": host, "host_assigned_total": host_total, "midi": midi, "engine": engine, "engine_bindings_total": bindings.len()},
        "presets": presets, "final_rss_mb": kb("VmRSS:") / 1024, "final_peak_rss_mb": kb("VmHWM:") / 1024});
    std::fs::write(out.join("result.json"), serde_json::to_string_pretty(&result).unwrap()).unwrap();
}
