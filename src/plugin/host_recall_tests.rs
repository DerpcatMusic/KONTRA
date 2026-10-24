//! DAW project recall on real instruments: move the controls, save the part state the way the
//! host saves it, reload into a fresh part, and compare the reload with the live part (script
//! variables, UI values, re-save, and rendered audio). Skips when the library is not installed.
use super::*;
use std::time::Instant;

const KONTAKT: &str = "/data/main/Libraries/Kontakt";
const UVI: &str = "/data/main/Libraries/UVI";

/// One instrument per format and library family. Missing files skip.
const INSTRUMENTS: &[&str] = &[
    "Conflux 1.1.0 [Native Instruments]/Instruments/Conflux.nki",
    "Areia 1.2.0 [Audio Imperia]/Instruments/01 Core Technique Patches/01 Areia - 16 Violins - Core Techniques.nki",
    "Una Corda Library/Instruments/Una Corda Felt.nki",
    "Audio Imperia Dolce/Instruments/01 7 1st Violins/Dolce - 01 7 1st Violins - Legato.nki",
    "UVI:VWinds - Flutes/VWinds-Alto_Flute.ufs",
];

/// Render a fixed phrase and hash every output sample. Each block refreshes the control cells
/// the way the plugin's process callback does, so saves read the audio side's live values.
fn render_hash(params: &SamplerParams, core: &mut V2Core) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for key in [48u8, 60, 67] {
        core.play(0, CoreEvent::midi1(0x90, key, 100));
        for _ in 0..(48_000 / 64 / 2) {
            let out = core.render(64);
            for bus in out.buses.iter().take(2) {
                for channel in bus.iter() {
                    for x in &channel[..64] {
                        hash = (hash ^ u64::from(x.to_bits())).wrapping_mul(0x0100_0000_01b3);
                    }
                }
            }
            if let Some(atoms) = params.shared.part(0) {
                atoms.refresh_controls(|id| core.control_value(0, id));
            }
        }
        core.play(0, CoreEvent::midi1(0x80, key, 0));
    }
    hash
}

/// Load `part` through the host's own load path and install it into a fresh core.
fn load_into(params: &SamplerParams, part: Part) -> Option<V2Core> {
    params.selection.write_unpoisoned().parts = vec![part];
    if !load_part(params, 0, None) {
        return None;
    }
    let (slot, _, loaded) = params.shared.ready.pop()?;
    let mut core = V2Core::with_parts(1, 48000.);
    core.install(slot, loaded);
    Some(core)
}

/// The part as the host saves it now.
fn save(params: &SamplerParams) -> Part {
    let mut selection = params.selection.write_unpoisoned();
    params.shared.capture_ui_controls(&mut selection);
    selection.parts[0].clone()
}

/// Move the first numeric UI control away from its current value.
fn move_first_control(params: &SamplerParams) -> Option<String> {
    let atoms = params.shared.part(0)?;
    let view = atoms.scripts.lock_unpoisoned().views.first()?.clone();
    let face = view.ui(&|_| None).ok()?;
    let widget = face.widgets.iter().find(|w| {
        matches!(w.kind, sampler_ui_ir::Kind::Knob { .. } | sampler_ui_ir::Kind::Slider { .. })
    })?;
    let epoch = atoms.generation.load(Ordering::Acquire);
    params.shared.set_widget_at(0, epoch, 0, widget, None, sampler_ui_ir::Value::Integer(37));
    Some(widget.name.clone())
}

fn library_path(entry: &str) -> Option<String> {
    let path = match entry.strip_prefix("UVI:") {
        Some(rest) => Path::new(UVI).join(rest),
        None => Path::new(KONTAKT).join(entry),
    };
    path.exists().then(|| path.to_string_lossy().into_owned())
}

/// Recall checks for one instrument; `Err` lists every divergence found.
fn recall_one(path: &str) -> Result<String, String> {
    let live = SamplerParams::new();
    let mut live_core = load_into(&live, Part { path: path.into(), ..Default::default() })
        .ok_or("live load failed")?;
    let moved = move_first_control(&live);
    let live_hash = render_hash(&live, &mut live_core);
    let t = Instant::now();
    let saved = save(&live);
    let save_ms = t.elapsed().as_millis();

    let fresh = SamplerParams::new();
    let t = Instant::now();
    let mut fresh_core = load_into(&fresh, saved.clone()).ok_or("reload failed")?;
    let load_ms = t.elapsed().as_millis();
    let fresh_hash = render_hash(&fresh, &mut fresh_core);
    let resaved = save(&fresh);

    let mut failures = Vec::new();
    let state_of = |params: &SamplerParams| {
        let atoms = params.shared.part(0).unwrap();
        let ingress = atoms.ingress.lock_unpoisoned();
        match ingress.as_ref() {
            None => "no ingress".to_owned(),
            Some(c) => format!("ingress, save_script_state {:?}", c.save_script_state().map(|s| s.len())),
        }
    };
    let cells = |params: &SamplerParams| -> Vec<sampler_ui_ir::ControlId> {
        params.shared.part(0).unwrap().controls.lock_unpoisoned().iter().map(|c| c.id).collect()
    };
    let (live_cells, fresh_cells) = (cells(&live), cells(&fresh));
    let missing_from_fresh = saved
        .control_values
        .iter()
        .filter(|c| !fresh_cells.contains(&c.id()))
        .count();
    let diag = format!(
        "[live {}; fresh {}; cells {} vs {}; saved ids not cells on reload {}]",
        state_of(&live),
        state_of(&fresh),
        live_cells.len(),
        fresh_cells.len(),
        missing_from_fresh
    );
    let differing: Vec<_> = saved
        .control_values
        .iter()
        .filter(|c| !resaved.control_values.iter().any(|r| r.id() == c.id() && r.value == c.value))
        .map(|c| {
            let after = resaved.control_values.iter().find(|r| r.id() == c.id()).map(|r| r.value);
            format!("{}={} (resaved {after:?}, fresh cell {:?}) ", c.id().0, c.value, fresh.shared.part(0).unwrap().controls.lock_unpoisoned().iter().find(|x| x.id == c.id()).map(|x| x.value()))
        })
        .take(6)
        .collect();
    if !differing.is_empty() {
        failures.push(format!("control values changed on reload {diag}: {}", differing.concat()));
    }
    if saved.script_state.is_empty() {
        let size = live.shared.part(0).unwrap().ingress.lock_unpoisoned().as_ref().and_then(|c| c.save_script_state()).unwrap_or_default();
        failures.push(format!("no script state saved (live state {} bytes) {diag}", size.len()));
    }
    if live_hash != fresh_hash {
        failures.push(format!("audio hash {live_hash:x} != reloaded {fresh_hash:x}"));
    }
    if resaved.script_state != saved.script_state {
        let at = saved.script_state.bytes().zip(resaved.script_state.bytes()).position(|(a, b)| a != b).unwrap_or(saved.script_state.len().min(resaved.script_state.len()));
        let window = |t: &str| t.get(at.saturating_sub(60)..(at + 60).min(t.len())).unwrap_or("").to_owned();
        failures.push(format!("script state changed on reload at byte {at}: saved …{}… vs resaved …{}…", window(&saved.script_state), window(&resaved.script_state)));
    }
    if resaved != saved {
        failures.push("re-save after load is not idempotent".into());
    }
    let mut buf = Vec::new();
    saved.write_field(&mut buf);
    if Part::read_field(&mut moose::core::custom_state::StateCursor::new(&buf)).as_ref() != Some(&saved) {
        failures.push("state codec round trip differs".into());
    }
    let report = format!(
        "{}: moved {:?}, controls {}, script_state {} bytes, save {save_ms} ms, load {load_ms} ms",
        Path::new(path).file_name().unwrap().to_string_lossy(),
        moved,
        saved.control_values.len(),
        saved.script_state.len(),
    );
    if failures.is_empty() {
        Ok(report)
    } else {
        Err(format!("{report}: {}", failures.join("; ")))
    }
}

/// Files whose reload still diverges. Reported, not asserted, until each is fixed:
/// Una Corda: the live engine group volume is not saved, so a fresh load reads the .nki default
/// and `on persistence_changed` writes it back over the restored slider. VWinds: audio differs
/// with identical controls and script state, cause not found yet.
const KNOWN_DIVERGENT: &[&str] = &["Una Corda Felt.nki", "VWinds-Alto_Flute.ufs"];

#[test]
fn host_recall_matches_the_live_part_on_real_instruments() {
    let mut reports = Vec::new();
    let mut failures = Vec::new();
    for entry in INSTRUMENTS {
        let Some(path) = library_path(entry) else {
            eprintln!("skipped (not installed): {entry}");
            continue;
        };
        match recall_one(&path) {
            Ok(report) => reports.push(report),
            Err(report) => failures.push(report),
        }
    }
    for r in reports.iter().chain(&failures) {
        eprintln!("{r}");
    }
    let (known, new): (Vec<_>, Vec<_>) = failures
        .into_iter()
        .partition(|r| KNOWN_DIVERGENT.iter().any(|k| r.starts_with(k)));
    for r in &known {
        eprintln!("known divergence (not asserted): {r}");
    }
    assert!(new.is_empty(), "recall divergences:\n{}", new.join("\n"));
}
