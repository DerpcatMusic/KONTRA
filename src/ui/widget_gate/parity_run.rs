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
    /// Groups of the voices sounding 0.1 s in.
    groups: Vec<u32>,
}

impl Take {
    fn of(samples: &[f32]) -> Self {
        let mut t = Take { rms_db: -200., peak: 0., nan: 0, denormal: 0, clipped: 0, frames: samples.len(), bands: vec![-200.; BANDS], groups: Vec::new() };
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
    /// (|RMS change| dB, mean |band change| dB over bands within 60 dB of the loudest).
    fn delta(&self, other: &Take) -> (f64, f64) {
        let top = self.bands.iter().chain(&other.bands).cloned().fold(-200., f64::max);
        let live: Vec<f64> = self.bands.iter().zip(&other.bands)
            .filter(|(a, b)| a.max(**b) > top - 60.)
            .map(|(a, b)| (a - b).abs())
            .collect();
        ((self.rms_db - other.rms_db).abs(), live.iter().sum::<f64>() / live.len().max(1) as f64)
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
}
impl Noise {
    fn changed(&self, a: &Take, b: &Take) -> (bool, Value) {
        let (rms, spectral) = a.delta(b);
        let changed = !(a.silent() && b.silent()) && (rms > self.rms || spectral > self.spectral);
        (changed, json!({"rms_delta_db": round(rms), "spectral_delta_db": round(spectral)}))
    }
    /// What an audio comparison says about a control that moved.
    fn verdict(&self, a: &Take, b: &Take) -> (&'static str, Value) {
        let (changed, delta) = self.changed(a, b);
        let (rms, spectral) = a.delta(b);
        let verdict = if a.silent() && b.silent() { "both-takes-silent" } else if changed { "pass" }
            else if rms > 0.5 || spectral > 1.0 { "masked-by-noise" } else { "moves-but-silent-change" };
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
    /// Dynamics up as a player leaves them, then one note held and released.
    fn take(&mut self, key: u8, velocity: u8) -> Take {
        self.core.release_all_notes();
        let mut tail = Vec::new();
        self.render(0.3, &mut tail);
        let dynamics = self.params.shared.view.lock().unwrap().parts[0].report.as_ref()
            .map(|r| r.decoded.dynamics.clone()).unwrap_or_default();
        for (cc, _) in dynamics {
            self.core.play(0, Event::midi1(0xb0, cc, 100));
        }
        let mut samples = Vec::new();
        self.core.play(0, Event::midi1(0x90, key, velocity));
        self.render(0.1, &mut samples);
        let mut groups: Vec<u32> = self.core.voice_taps(0).iter().flatten().map(|t| t.group).collect();
        groups.sort();
        groups.dedup();
        self.render(0.5, &mut samples);
        self.core.play(0, Event::midi1(0x80, key, 0));
        self.render(0.2, &mut samples);
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
    /// Drag far up-right (max) or down-left (min) in steps, with the button held.
    fn sweep(&mut self, id: &str, at: Point, up: bool) -> (usize, Vec<String>) {
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
        self.pointer(Point::new(at.x - 1500. * sign, at.y + 1500. * sign), false);
        self.settle();
        self.observing = None;
        let _ = id;
        (self.submitted.len(), std::mem::take(&mut self.observed_changes).into_iter().collect())
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
    let (r, s) = a.delta(&b);
    let noise = Noise { rms: (3. * r).max(0.5), spectral: (3. * s).max(1.0) };
    let baseline = json!({"key": key, "rms_delta_db": round(r), "spectral_delta_db": round(s), "threshold_rms_db": round(noise.rms), "threshold_spectral_db": round(noise.spectral), "sounding_groups": a.groups.len()});

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
        let (sub_min, keys_min) = gate.sweep(&id, at, false);
        let low = gate.take(key, 100);
        let (sub_max, keys_max) = gate.sweep(&id, at, true);
        let high = gate.take(key, 100);
        let (audio_verdict, delta) = noise.verdict(&low, &high);
        let mut keys: Vec<String> = keys_min.into_iter().chain(keys_max).collect();
        keys.sort();
        keys.dedup();
        let verdict = if sub_min + sub_max == 0 { "ui-not-submitted" } else if keys.is_empty() { "engine-unchanged" }
            else { audio_verdict };
        let routes: Vec<String> = keys.iter().filter_map(|k| route_of(k)).collect();
        controls.push(json!({"id": id, "kind": kind, "verdict": verdict, "at": [at.x.round(), at.y.round()], "submitted": sub_min + sub_max,
            "engine_keys": keys.len(), "routes": routes, "audio": delta}));
    }
    // Host automation the instrument assigns.
    let mut host = Vec::new();
    let assigned: Vec<_> = instrument.as_ref().map(|i| i.script_automation.clone()).unwrap_or_default().into_iter()
        .filter(|a| matches!(a.source, sampler_ir::ScriptAutomationSource::HostParameter(_))).collect();
    let host_total = assigned.len();
    for a in assigned.iter().step_by((host_total / max_controls.max(1)).max(1)).take(max_controls) {
        let sampler_ir::ScriptAutomationSource::HostParameter(address) = a.source else { continue };
        let ok_low = gate.core.host_parameter(address, 0.);
        gate.settle();
        let low = gate.take(key, 100);
        let ok_high = gate.core.host_parameter(address, 1.);
        gate.settle();
        let high = gate.take(key, 100);
        let (audio, delta) = noise.verdict(&low, &high);
        host.push(json!({"address": address, "accepted": ok_low && ok_high, "slider": a.slider, "switch": a.switch,
            "verdict": if !(ok_low && ok_high) { "rejected" } else { audio }, "audio": delta}));
    }
    // A spread sample of engine parameters bound to controls.
    let mut engine = Vec::new();
    let audible: Vec<_> = bindings.iter().filter(|b| b.address.group < 0 || a.groups.contains(&(b.address.group as u32))).collect();
    let step = (audible.len() / max_engine.max(1)).max(1);
    for b in audible.into_iter().step_by(step).take(max_engine) {
        let id = sampler_ui_ir::ControlId(b.control.0);
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
        let (audio, delta) = noise.verdict(&low, &high);
        let moved = lo_v != hi_v;
        engine.push(json!({"route": format!("{} group {} slot {} generic {}", sampler_core::engine_parameter_name(b.address.parameter).unwrap_or("?"), b.address.group, b.address.slot, b.address.generic),
            "moved": moved, "verdict": if !moved { "engine-unchanged" } else { audio }, "audio": delta}));
    }
    for (name, rows) in [("controls-ui", &controls), ("controls-host", &host), ("controls-engine", &engine)] {
        let failed: Vec<&Value> = rows.iter().filter(|r| r["verdict"] != "pass" && r["verdict"] != "masked-by-noise").collect();
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
        "baseline": baseline, "controls": {"ui": controls, "ui_targets_total": total_targets, "host": host, "host_assigned_total": host_total, "engine": engine, "engine_bindings_total": bindings.len()},
        "presets": presets, "final_rss_mb": kb("VmRSS:") / 1024, "final_peak_rss_mb": kb("VmHWM:") / 1024});
    std::fs::write(out.join("result.json"), serde_json::to_string_pretty(&result).unwrap()).unwrap();
}
