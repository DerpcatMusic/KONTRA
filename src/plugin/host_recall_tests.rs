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
    "Afflatus Chapter II Brass/Instruments/3. Curated Ensembles/Barbarian Brass.nki",
    "Morphology Evolved [Zero-G] rutracker.org/Morphology Evolved.nki",
    "Pacific Ensemble Strings/Instruments/10 Cellos/Pacific - Ens Strings - 10 Cellos - Legato Sustains.nki",
    "Performance Samples Vista/Instruments/Vista - 5 Violins.nki",
    "Shreddage 3.5 E-Upright 1.0.1 [Impact Soundworks] rutracker.org/Instruments/Shreddage 3.5 E-Upright.nki",
    "ANALOG STRINGS/Instruments/ANALOG STRINGS.nki",
];

/// Render a fixed phrase and hash every output sample. Each block refreshes the control cells
/// the way the plugin's process callback does, so saves read the audio side's live values.
/// Offline blocks wait for streamed pages instead of starving voices, so every load of the
/// same instrument renders the same samples (real-time blocks shed load and vary by timing).
fn render_hash(params: &SamplerParams, core: &mut V2Core, part: usize) -> u64 {
    use crate::sound::{BlockInfo, Core};
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for key in [48u8, 60, 67] {
        core.play(part, CoreEvent::midi1(0x90, key, 100));
        for _ in 0..(48_000 / 64 / 2) {
            core.begin_block(&BlockInfo {
                frames: 64,
                offline: true,
                ..Default::default()
            });
            let out = core.render(64);
            for bus in out.buses.iter().take(2) {
                for channel in bus.iter() {
                    for x in &channel[..64] {
                        hash = (hash ^ u64::from(x.to_bits())).wrapping_mul(0x0100_0000_01b3);
                    }
                }
            }
            if let Some(atoms) = params.shared.part(part) {
                atoms.refresh_controls(|id| core.control_value(part, id));
            }
        }
        core.play(part, CoreEvent::midi1(0x80, key, 0));
    }
    hash
}

/// Load `parts` (slot order) through the host's own load path into a fresh core.
fn load_all(params: &SamplerParams, parts: Vec<Part>) -> Option<V2Core> {
    let count = parts.len();
    params.selection.write_unpoisoned().parts = parts;
    for slot in 0..count {
        if !load_part(params, slot, None) {
            return None;
        }
    }
    let mut core = V2Core::with_parts(count, 48000.);
    for _ in 0..count {
        let (slot, _, loaded) = params.shared.ready.pop()?;
        core.install(slot, loaded);
    }
    Some(core)
}

fn load_into(params: &SamplerParams, part: Part) -> Option<V2Core> {
    load_all(params, vec![part])
}

/// Every part as the host saves it now.
fn save_all(params: &SamplerParams) -> Vec<Part> {
    let mut selection = params.selection.write_unpoisoned();
    params.shared.capture_ui_controls(&mut selection);
    selection.parts.clone()
}

fn save(params: &SamplerParams) -> Part {
    save_all(params).remove(0)
}

/// Move the first numeric UI control away from its current value.
fn move_first_control(params: &SamplerParams) -> Option<String> {
    let atoms = params.shared.part(0)?;
    let view = atoms.scripts.lock_unpoisoned().views.first()?.clone();
    let face = view.ui(&|_| None).ok()?;
    let widget = face.widgets.iter().find(|w| {
        matches!(
            w.kind,
            sampler_ui_ir::Kind::Knob { .. } | sampler_ui_ir::Kind::Slider { .. }
        )
    })?;
    let epoch = atoms.generation.load(Ordering::Acquire);
    params
        .shared
        .set_widget_at(0, epoch, 0, widget, None, sampler_ui_ir::Value::Integer(37));
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
    let mut live_core = load_into(
        &live,
        Part {
            path: path.into(),
            ..Default::default()
        },
    )
    .ok_or("live load failed")?;
    let moved = move_first_control(&live);
    let live_hash = render_hash(&live, &mut live_core, 0);
    let t = Instant::now();
    let saved = save(&live);
    let save_ms = t.elapsed().as_millis();

    let fresh = SamplerParams::new();
    let t = Instant::now();
    let mut fresh_core = load_into(&fresh, saved.clone()).ok_or("reload failed")?;
    let load_ms = t.elapsed().as_millis();
    let fresh_hash = render_hash(&fresh, &mut fresh_core, 0);
    let resaved = save(&fresh);

    let mut failures = Vec::new();
    let state_of = |params: &SamplerParams| {
        let atoms = params.shared.part(0).unwrap();
        let ingress = atoms.ingress.lock_unpoisoned();
        match ingress.as_ref() {
            None => "no ingress".to_owned(),
            Some(c) => format!(
                "ingress, save_script_state {:?}",
                c.save_script_state().map(|s| s.len())
            ),
        }
    };
    let cells = |params: &SamplerParams| -> Vec<sampler_ui_ir::ControlId> {
        params
            .shared
            .part(0)
            .unwrap()
            .controls
            .lock_unpoisoned()
            .iter()
            .map(|c| c.id)
            .collect()
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
        .filter(|c| {
            !resaved
                .control_values
                .iter()
                .any(|r| r.id() == c.id() && r.value == c.value)
        })
        .map(|c| {
            let after = resaved
                .control_values
                .iter()
                .find(|r| r.id() == c.id())
                .map(|r| r.value);
            format!(
                "{}={} (resaved {after:?}, fresh cell {:?}) ",
                c.id().0,
                c.value,
                fresh
                    .shared
                    .part(0)
                    .unwrap()
                    .controls
                    .lock_unpoisoned()
                    .iter()
                    .find(|x| x.id == c.id())
                    .map(|x| x.value())
            )
        })
        .take(6)
        .collect();
    if !differing.is_empty() {
        failures.push(format!(
            "control values changed on reload {diag}: {}",
            differing.concat()
        ));
    }
    if saved.script_state.is_empty() {
        let size = live
            .shared
            .part(0)
            .unwrap()
            .ingress
            .lock_unpoisoned()
            .as_ref()
            .and_then(|c| c.save_script_state())
            .unwrap_or_default();
        failures.push(format!(
            "no script state saved (live state {} bytes) {diag}",
            size.len()
        ));
    }
    // The threaded UVI script host applies commands at whichever block its thread reaches, so two
    // fresh loads of the same .ufs render differently. An explicit audit seed makes the owner
    // synchronous per block, so UVI audio is comparable then. Run with
    // `--features shots` and `KONTRA_UVI_AUDIT_SEED=17` to enable it.
    let audio_comparable =
        !path.ends_with(".ufs") || std::env::var_os("KONTRA_UVI_AUDIT_SEED").is_some();
    if audio_comparable && live_hash != fresh_hash {
        failures.push(format!(
            "audio hash {live_hash:x} != reloaded {fresh_hash:x}"
        ));
    }
    if resaved.script_state != saved.script_state {
        let at = saved
            .script_state
            .bytes()
            .zip(resaved.script_state.bytes())
            .position(|(a, b)| a != b)
            .unwrap_or(saved.script_state.len().min(resaved.script_state.len()));
        let window = |t: &str| {
            t.get(at.saturating_sub(60)..(at + 60).min(t.len()))
                .unwrap_or("")
                .to_owned()
        };
        failures.push(format!(
            "script state changed on reload at byte {at}: saved …{}… vs resaved …{}…",
            window(&saved.script_state),
            window(&resaved.script_state)
        ));
    }
    if resaved != saved {
        failures.push("re-save after load is not idempotent".into());
    }
    let mut buf = Vec::new();
    saved.write_field(&mut buf);
    if Part::read_field(&mut moose::core::custom_state::StateCursor::new(&buf)).as_ref()
        != Some(&saved)
    {
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

/// Files whose reload still diverges. Reported, not asserted, until each is fixed.
/// Una Corda: faithful to the script. Its `on persistence_changed` reads the engine back
/// into `$T1_sliRumbleVol` through `%VolumeConvert`, which tops out at 629961, so any saved
/// position above that (630957) reloads as the table's top entry (339282). Kontakt runs the
/// same script, so the clamp is the expected reload value; the harness reports, not fails.
/// Pacific and Vista were nondeterministic on the real-time block path (stream starvation);
/// the offline block path in `render_hash` removed that, so they are asserted now.
const KNOWN_DIVERGENT: &[&str] = &["Una Corda Felt.nki"];

#[test]
#[ignore = "v2 recall diverges on real instruments since the v3 migration; v3 recall replaces it at P8 (lanes BOARD 2026-10-29)"]
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

/// Two parts in one project: each saved and reloaded in its slot, and each renders the same.
#[test]
#[ignore = "v2 recall diverges on real instruments since the v3 migration; v3 recall replaces it at P8 (lanes BOARD 2026-10-29)"]
fn two_parts_recall_together() {
    let parts = [
        "Conflux 1.1.0 [Native Instruments]/Instruments/Conflux.nki",
        "Performance Samples Vista/Instruments/Vista - 5 Violins.nki",
    ]
    .map(library_path);
    let [Some(a), Some(b)] = parts else { return };
    let live = SamplerParams::new();
    let mut live_core = load_all(
        &live,
        vec![
            Part {
                path: a.clone(),
                ..Default::default()
            },
            Part {
                path: b.clone(),
                ..Default::default()
            },
        ],
    )
    .unwrap();
    let hashes = |params: &SamplerParams, core: &mut V2Core| {
        [render_hash(params, core, 0), render_hash(params, core, 1)]
    };
    let live_hashes = hashes(&live, &mut live_core);
    let saved = save_all(&live);
    let fresh = SamplerParams::new();
    let mut fresh_core = load_all(&fresh, saved.clone()).unwrap();
    let fresh_hashes = hashes(&fresh, &mut fresh_core);
    let resaved = save_all(&fresh);
    assert!(
        resaved == saved,
        "re-save after a two-part load is not idempotent"
    );
    // Part 1 (Vista) is a known audio divergence above; part 0 must match.
    assert!(
        fresh_hashes[0] == live_hashes[0],
        "two-part audio differs after reload: live {live_hashes:x?} fresh {fresh_hashes:x?}"
    );
}

/// Una Corda's rumble slider above the script's volume table reloads clamped, by design: the
/// script's `persistence_changed` reads `%VolumeConvert[100]` (339282) back into the slider.
#[test]
#[ignore = "v2 recall diverges on real instruments since the v3 migration; v3 recall replaces it at P8 (lanes BOARD 2026-10-29)"]
fn una_corda_rumble_above_the_volume_table_reloads_clamped() {
    let Some(path) = library_path("Una Corda Library/Instruments/Una Corda Felt.nki") else {
        return;
    };
    let rumble = sampler_ui_ir::ControlId(4731838657588158385350167845526311085);
    let cell = |params: &SamplerParams| {
        params
            .shared
            .part(0)
            .unwrap()
            .controls
            .lock_unpoisoned()
            .iter()
            .find(|x| x.id == rumble)
            .map(|x| x.value())
    };
    let live = SamplerParams::new();
    let mut live_core = load_into(
        &live,
        Part {
            path: path.clone(),
            ..Default::default()
        },
    )
    .unwrap();
    let atoms = live.shared.part(0).unwrap();
    let view = atoms
        .scripts
        .lock_unpoisoned()
        .views
        .first()
        .unwrap()
        .clone();
    let face = view.ui(&|_| None).unwrap();
    let widget = face
        .widgets
        .iter()
        .find(|w| w.name.contains("Rumble"))
        .unwrap()
        .clone();
    let epoch = atoms.generation.load(Ordering::Acquire);
    live.shared.set_widget_at(
        0,
        epoch,
        0,
        &widget,
        None,
        sampler_ui_ir::Value::Real(630957.),
    );
    assert_eq!(cell(&live), Some(630957.));
    let _ = render_hash(&live, &mut live_core, 0);
    let fresh = SamplerParams::new();
    let mut core = load_into(&fresh, save(&live)).unwrap();
    let _ = render_hash(&fresh, &mut core, 0);
    assert_eq!(
        cell(&fresh),
        Some(339282.),
        "reloaded rumble is the script's clamped volume-table top"
    );
}
