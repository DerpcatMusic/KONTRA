//! Mechanical architectural guards (Wave 2A).
//!
//! Enforces structural invariants at test time so regressions fail CI:
//! 1. `ir::Instrument: PartialEq` compile-time assertion + zero `fn(` pointers
//!    or `fn_addr_eq` in `crates/model/ir/src`.
//! 2. Vendor-word ratchet across `crates/{engine,dsp,model}/*/src`.
//! 3. Architecture metric ratchets (`Source::` in `app/ui/src`, no parallel
//!    `EffectFamily`/`EffectSpec` taxonomy, `fn_addr_eq`
//!    workspace-wide).
//! 4. Per-module encoding equality and unique `ModelId`s across `kontakt-effects`.
//! 5. One routing path: only the audio graph runs instrument buses, and the
//!    engine always has a graph (docs/specs/effect-racks-on-audiograph.md).
//! 6. Cyclic `AudioGraph` errors are surfaced (never silently discarded) in both
//!    `kontra-prepare` and `Engine::try_new` / `Engine::graph_error`.
//! 6. UVI effects load through their one `UviModule` table: no second
//!    hand-written builder, and `format/uvi` production lines only fall.
#![allow(clippy::expect_used, clippy::indexing_slicing)]

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use kontra_engine::ir;
use kontra_engine::{Engine, Plan};
use kontra_stream::{Store, StoreConfig};

// Compile-time guard: `ir::Instrument` must remain `PartialEq` (no raw `fn` pointers).
const _: () = {
    const fn assert_partial_eq<T: PartialEq>() {}
    assert_partial_eq::<ir::Instrument>();
};

fn kontra_crates_dir() -> PathBuf {
    // CARGO_MANIFEST_DIR = `<repo>/plugins/kontra/crates/engine/engine`
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crates dir")
        .to_path_buf()
}

fn walk_rs(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let p = entry.path();
        if p.is_dir() {
            walk_rs(&p, out);
        } else if p.extension().is_some_and(|e| e == "rs") {
            out.push(p);
        }
    }
}

fn layer_rs_files(crates_dir: &Path, layer: &str) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let layer_dir = crates_dir.join(layer);
    let Ok(entries) = std::fs::read_dir(&layer_dir) else {
        return files;
    };
    for entry in entries.flatten() {
        let src = entry.path().join("src");
        if src.is_dir() {
            walk_rs(&src, &mut files);
        }
    }
    files
}

fn count_vendor_words(text: &str) -> usize {
    // Case-insensitive matches for: kontakt | ksp | uvi | falcon | $ni_ | engine_par
    let lower = text.to_ascii_lowercase();
    let bytes = lower.as_bytes();
    let needles: &[&[u8]] = &[
        b"kontakt",
        b"ksp",
        b"uvi",
        b"falcon",
        b"$ni_",
        b"engine_par",
    ];
    let mut count = 0usize;
    let mut i = 0usize;
    while i < bytes.len() {
        let mut matched = 0usize;
        for needle in needles {
            if bytes[i..].starts_with(needle) {
                matched = needle.len();
                break;
            }
        }
        if matched > 0 {
            count += 1;
            i += matched;
        } else {
            i += 1;
        }
    }
    count
}

#[test]
fn ir_has_zero_fn_pointers_and_zero_fn_addr_eq() {
    let ir_src = kontra_crates_dir().join("model/ir/src");
    let mut files = Vec::new();
    walk_rs(&ir_src, &mut files);
    assert!(!files.is_empty(), "expected ir source files");

    for path in files {
        let text = std::fs::read_to_string(&path).expect("read ir file");
        assert!(
            !text.contains("fn_addr_eq"),
            "{}: fn_addr_eq is forbidden in kontra-ir",
            path.display()
        );
        // Reject raw function pointer type syntax `fn(` in IR structs/enums.
        for (line_no, line) in text.lines().enumerate() {
            let trimmed = line.trim();
            if trimmed.starts_with("//") {
                continue;
            }
            // Check for `fn(` (not `fn name(`).
            let mut rest = trimmed;
            while let Some(pos) = rest.find("fn(") {
                let before_ok = pos == 0
                    || !rest.as_bytes()[pos - 1].is_ascii_alphanumeric()
                        && rest.as_bytes()[pos - 1] != b'_';
                assert!(
                    !before_ok,
                    "{}:{}: raw `fn(` pointer is forbidden in kontra-ir: {trimmed}",
                    path.display(),
                    line_no + 1
                );
                rest = &rest[pos + 3..];
            }
        }
    }
}

#[test]
fn vendor_word_ratchet_in_engine_dsp_and_model() {
    let crates_dir = kontra_crates_dir();
    // Tight ceilings measured at Wave 2A; only allowed to move DOWN.
    let ceilings = [("engine", 275usize), ("dsp", 17usize), ("model", 176usize)];
    for (layer, max_allowed) in ceilings {
        let files = layer_rs_files(&crates_dir, layer);
        assert!(!files.is_empty(), "no source files found in {layer}");
        let total: usize = files
            .iter()
            .map(|p| {
                let text = std::fs::read_to_string(p).expect("read rs");
                count_vendor_words(&text)
            })
            .sum();
        assert!(
            total <= max_allowed,
            "vendor-word ratchet regressed in crates/{layer}/*/src: {total} > ceiling {max_allowed}"
        );
    }
}

#[test]
fn architecture_metric_ratchets_hold() {
    let crates_dir = kontra_crates_dir();

    // 1. Zero `Source::` format branching in `crates/app/ui/src`, plus
    // ratchet `Source::` and vendor words in `crates/model/ui-model/src`.
    let mut ui_files = Vec::new();
    walk_rs(&crates_dir.join("app/ui/src"), &mut ui_files);
    let ui_source_refs: usize = ui_files
        .iter()
        .map(|p| {
            std::fs::read_to_string(p)
                .expect("read ui")
                .matches("Source::")
                .count()
        })
        .sum();
    assert_eq!(
        ui_source_refs, 0,
        "Source:: branching leaked into app/ui/src"
    );

    let mut ui_model_files = Vec::new();
    walk_rs(&crates_dir.join("model/ui-model/src"), &mut ui_model_files);
    let ui_model_prod_source: usize = ui_model_files
        .iter()
        .map(|p| {
            let text = std::fs::read_to_string(p).expect("read ui-model");
            let prod = text.split("#[cfg(test)]").next().unwrap_or("");
            prod.matches("Source::").count()
        })
        .sum();
    assert_eq!(
        ui_model_prod_source, 0,
        "Source:: format branching leaked into production code of model/ui-model/src"
    );
    let ui_model_total_source: usize = ui_model_files
        .iter()
        .map(|p| {
            std::fs::read_to_string(p)
                .expect("read ui-model")
                .matches("Source::")
                .count()
        })
        .sum();
    assert!(
        ui_model_total_source <= 1,
        "Source:: ratchet regressed in model/ui-model/src: {ui_model_total_source} > 1"
    );
    let ui_model_vendor_words: usize = ui_model_files
        .iter()
        .map(|p| count_vendor_words(&std::fs::read_to_string(p).expect("read ui-model")))
        .sum();
    assert!(
        ui_model_vendor_words <= 143,
        "vendor-word ratchet regressed in model/ui-model/src: {ui_model_vendor_words} > 143"
    );

    // 2. Zero `fn_addr_eq` across all crate `src/` directories.
    let mut all_src = Vec::new();
    for group in ["app", "base", "dsp", "engine", "format", "model", "script"] {
        all_src.extend(layer_rs_files(&crates_dir, group));
    }
    let fn_addr_eq_count: usize = all_src
        .iter()
        .map(|p| {
            std::fs::read_to_string(p)
                .expect("read src")
                .matches("fn_addr_eq")
                .count()
        })
        .sum();
    assert_eq!(
        fn_addr_eq_count, 0,
        "fn_addr_eq must remain 0 across all crate src/ trees"
    );

    // 3. One effect encoding: `EffectKind`. The families are a measured
    // grouping (effect-family-null-report.md), not a second IR type.
    let taxonomy: Vec<_> = all_src
        .iter()
        .filter(|p| {
            let text = std::fs::read_to_string(p).expect("read src");
            ["EffectFamily", "EffectSpec"].iter().any(|name| {
                text.contains(&format!("enum {name}")) || text.contains(&format!("struct {name}"))
            })
        })
        .collect();
    assert!(
        taxonomy.is_empty(),
        "a parallel effect taxonomy came back in {taxonomy:?}; extend EffectKind instead"
    );

    // 4. Kontakt effects are mapped by one table (06-FORMATS.md §2): a row
    // per module in `format/kontakt/src/effects.rs` `MODULES`, not a match
    // on the module id, and no second per-id dispatch (`drive::effect`).
    let kontakt = crates_dir.join("format/kontakt/src");
    let read = |f: &str| std::fs::read_to_string(kontakt.join(f)).expect("read kontakt src");
    let effects = read("effects.rs");
    assert!(
        effects.contains("const MODULES: &[Fx]") && !effects.contains("match effect.id"),
        "Kontakt effects must map through the MODULES table, one row per module"
    );
    assert!(
        !read("drive.rs").contains("fn effect("),
        "drive::effect came back as a second dispatch on the module id; add a MODULES row"
    );
    // KSP knob bindings are typed Rust rows (audit KT-18), not a TSV parsed
    // from strings at load.
    assert!(
        !kontakt.join("../data/script_knobs.tsv").exists()
            && !read("script_knobs.rs").contains("fn parse_law"),
        "script_knobs.tsv and its string parser came back; add a row to script_knobs()"
    );

    // 5. Kontakt's program, group and zone `ignored_fields` are typed fast
    // paths (13-IR-VNEXT Phase 0): no Debug reflection outside tests.
    let translate = read("translate.rs");
    let production = translate.split("\nmod tests").next().unwrap_or_default();
    assert!(
        !production
            .lines()
            .any(|l| !l.trim_start().starts_with("//") && l.contains(".ignored_fields(")),
        "translate.rs reflects Debug text for ignored fields; destructure the record instead"
    );

    // 6. A library's interface has two looks, Original and KONTRA, drawn by
    // one renderer (14-UNIFIED-UI-IR): no third variant, no second look type,
    // no second page renderer.
    let model = std::fs::read_to_string(crates_dir.join("model/ui-model/src/lib.rs"))
        .expect("read ui-model");
    let looks = model
        .split("pub enum Presentation {")
        .nth(1)
        .and_then(|rest| rest.split('}').next())
        .expect("enum Presentation in ui-model");
    let variants: Vec<_> = looks
        .lines()
        .map(str::trim)
        .filter(|l| l.ends_with(',') && !l.starts_with("//") && !l.starts_with('#'))
        .collect();
    assert_eq!(
        variants,
        ["Original,", "Kontra,"],
        "a third look crept in; KONTRA draws the Original layout in its own controls"
    );
    let second: Vec<_> = all_src
        .iter()
        .filter(|p| {
            let text = std::fs::read_to_string(p).expect("read src");
            ["enum FaceLook", "fn vector_page", "fn kontra_page"]
                .iter()
                .any(|s| text.contains(s))
        })
        .collect();
    assert!(
        second.is_empty(),
        "a second look type or page renderer came back in {second:?}; use Presentation and view::page_as"
    );
}

#[test]
fn only_the_audio_graph_runs_instrument_buses() {
    let crates = kontra_crates_dir();
    let graph = crates.join("engine/racks/src/graph.rs");
    let callers: Vec<_> = layer_rs_files(&crates, "engine")
        .into_iter()
        .chain(layer_rs_files(&crates, "app"))
        .filter(|p| *p != graph)
        .filter(|p| {
            let text = std::fs::read_to_string(p).expect("read src");
            text.contains("bus::run(") || text.contains("fn mix_out(")
        })
        .collect();
    assert!(
        callers.is_empty(),
        "a second bus loop came back in {callers:?}; route through GraphRuntime instead"
    );
    let engine = std::fs::read_to_string(crates.join("engine/engine/src/lib.rs")).expect("read");
    assert!(
        !engine.contains("graph: Option<"),
        "the engine's graph is optional again: every plan routes through one"
    );
}

/// Code lines (not blank, not comments) of `src` before its trailing
/// `#[cfg(test)] mod … {`; `tests.rs` files count 0.
fn production_lines(src: &Path) -> usize {
    if src.file_name().is_some_and(|n| n == "tests.rs") {
        return 0;
    }
    let text = std::fs::read_to_string(src).expect("read src");
    let lines: Vec<&str> = text.lines().collect();
    let test_mod = lines
        .windows(2)
        .position(|w| w[0] == "#[cfg(test)]" && w[1].starts_with("mod ") && w[1].ends_with('{'));
    lines[..test_mod.unwrap_or(lines.len())]
        .iter()
        .filter(|l| {
            let l = l.trim();
            !l.is_empty() && !l.starts_with("//")
        })
        .count()
}

#[test]
fn uvi_effects_load_through_their_module_tables() {
    let mut files = Vec::new();
    walk_rs(&kontra_crates_dir().join("format/uvi/src"), &mut files);
    // The builders `UviModule` replaced (tools/migrations/uvi-mapping-tables.py).
    let builders = count_needles(
        &files,
        &[
            "fn dual_delay(",
            "fn spark_verb(",
            "fn flanger(",
            "fn phasor(",
            "fn wave_shaper(",
            "fn diode_clipper(",
            "fn shelves(",
            "fn maximizer(",
            "fn drive(",
        ],
    );
    assert_eq!(
        builders, 0,
        "a UVI effect grew a second builder beside its UviModule"
    );
    // 6,966 on b19c21ca3, 6,613 after the tables (06-FORMATS.md §3), 6,607
    // on c63dea078, 6,609 with the mono pan law, 6,619 with bank folder links (#258),
    // 6,620 with the performance-view preference (#336), 6,636 with the
    // tolerated missing bank sample (#352).
    let lines: usize = files.iter().map(|p| production_lines(p)).sum();
    assert!(
        lines <= 6_636,
        "format/uvi code lines rose to {lines}; ratchet is 6,636"
    );
}

fn count_needles(files: &[PathBuf], needles: &[&str]) -> usize {
    files
        .iter()
        .map(|p| {
            let text = std::fs::read_to_string(p).expect("read src");
            needles
                .iter()
                .map(|n| text.matches(n).count())
                .sum::<usize>()
        })
        .sum()
}

#[test]
fn all_kontakt_effect_modules_register_unique_ids_and_roundtrip_saved() {
    use kontra_ir::EffectModel;
    use kontra_kontakt_effects::{
        amps, bite, cabinet, cat, choral, core_cell, cry_wah, delays, dstortion, dynamics, dyx,
        flair, freak, kolor, mangling, module, phasis, plate_reverb, psyche, ringmod, rotator,
        skreamer, skreamer_deluxe, stereo_tune, supergt, tape, twin_delay, vibrato,
    };

    module::register_all();
    let mut seen_ids = HashSet::new();
    for m in module::ALL {
        assert!(
            seen_ids.insert(m.id),
            "duplicate ModelId {:?} on module {}",
            m.id,
            m.name
        );
        assert!(
            m.id.0 < ir::ModelId::UVI_BASE,
            "Kontakt module {} ID {:?} outside Kontakt band",
            m.name,
            m.id
        );
        assert!(
            ir::model(m.id).is_some(),
            "module {} ({:?}) not registered",
            m.name,
            m.id
        );
    }

    // Per-module encoding equality: `model.build(&model.saved)` == load-time constructor.
    // Every module in `module::ALL` MUST be covered here so new modules cannot skip verification.
    let mut checked = HashSet::new();

    // Tape
    let mut tape_pub = Vec::new();
    for v in [0.4f32, 0.5, 0.7] {
        tape_pub.extend(v.to_le_bytes());
    }
    tape_pub.push(0);
    let tm = tape::model(&tape_pub).unwrap();
    assert_eq!(tm.build(&tm.saved), Some(tape::tape(0.4, 0.5, 0.7)));
    checked.insert(tm.id);

    // Skreamer
    let sk_pub: Vec<u8> = [0.5f32, 0.6, 0.4, 0.3, 0.8]
        .into_iter()
        .flat_map(f32::to_le_bytes)
        .collect();
    let sm = skreamer::model(&sk_pub).unwrap();
    assert_eq!(
        sm.build(&sm.saved),
        Some(skreamer::skreamer(0.5, 0.6, 0.4, 0.3, 0.8))
    );
    checked.insert(sm.id);

    // DStortion
    let ds_pub: Vec<u8> = [0.5f32, 0.4, 0.7, 0.5, 0.5, 0.6]
        .into_iter()
        .flat_map(f32::to_le_bytes)
        .collect();
    let dm = dstortion::model(&ds_pub).unwrap();
    assert_eq!(
        dm.build(&dm.saved),
        Some(dstortion::dstortion(0.5, 0.4, 0.7, 0.5, 0.5, 0.6))
    );
    checked.insert(dm.id);

    // Supercharger GT
    let mut sgt_pub = Vec::new();
    for v in [0.5f32, 0.3, 0.4, 0.5, 1.0, 0.3, 0.4] {
        sgt_pub.extend(v.to_le_bytes());
    }
    for i in [0i32, 1, 1, 1] {
        sgt_pub.extend(i.to_le_bytes());
    }
    sgt_pub.push(0);
    let sgt_k = supergt::saved(&sgt_pub).unwrap();
    let sgtm = supergt::model(&sgt_pub).unwrap();
    assert_eq!(sgtm.build(&sgtm.saved), Some(supergt::supergt(sgt_k)));
    checked.insert(sgtm.id);

    // Feedback Compressor
    let mut fb_pub = Vec::new();
    for v in [0.5f32, 0.3, 0.4, 0.5, 0.6, 1.0, 0.0] {
        fb_pub.extend(v.to_le_bytes());
    }
    fb_pub.extend([0u8, 1u8, 0u8]);
    let (fb_eff, _) = dynamics::effect(0x4c, 0x10, &fb_pub).unwrap();
    let fbm = dynamics::feedback_model(&fb_pub).unwrap();
    assert_eq!(fbm.build(&fbm.saved), Some(fb_eff));
    checked.insert(fbm.id);

    // Phasis, Flair, Choral, Bite, Freak, Psyche, Vibrato
    let pm = phasis::model();
    assert_eq!(pm.build(&pm.saved), Some(phasis::phasis(phasis::INIT)));
    checked.insert(pm.id);
    let flm = flair::model();
    assert_eq!(flm.build(&flm.saved), Some(flair::flair(flair::INIT)));
    checked.insert(flm.id);
    let chm = choral::model();
    assert_eq!(chm.build(&chm.saved), Some(choral::choral(choral::INIT)));
    checked.insert(chm.id);
    let bm = bite::model(bite::FRESH);
    assert_eq!(bm.build(&bm.saved), Some(bite::bite(bite::FRESH)));
    checked.insert(bm.id);
    let frm = freak::model(freak::FRESH);
    assert_eq!(frm.build(&frm.saved), Some(freak::freak(freak::FRESH)));
    checked.insert(frm.id);
    let psm = psyche::model(psyche::INIT);
    assert_eq!(
        psm.build(&psm.saved),
        Some(ir::EffectKind::Shifter(psyche::shifter(psyche::INIT)))
    );
    checked.insert(psm.id);
    let vib = vibrato::Vibrato {
        depth: 2,
        color: 1,
        blend: 0.5,
        rate: 0.4,
        width: 0.6,
        mix: 0.8,
    };
    let vm = vibrato::model(vib);
    assert_eq!(
        vm.build(&vm.saved),
        Some(ir::EffectKind::Scanner(vibrato::scanner(vib)))
    );
    checked.insert(vm.id);

    // Legacy Chorus & Flanger
    let ch_pub: Vec<u8> = [0.5f32, 1.5, 0.3, 0.0]
        .into_iter()
        .flat_map(f32::to_le_bytes)
        .collect();
    let ch_saved = delays::saved::<4>(0x51, &ch_pub).unwrap();
    let ch_mod = delays::CHORUS.knob_model(&ch_saved);
    assert_eq!(
        ch_mod.build(&ch_mod.saved),
        delays::legacy(0x11, 0x51, &ch_pub)
    );
    checked.insert(ch_mod.id);

    let fl_pub: Vec<u8> = [0.7f32, 0.6, 0.8, 0.4, 2.5, 0.0]
        .into_iter()
        .flat_map(f32::to_le_bytes)
        .collect();
    let fl_saved = delays::saved::<6>(0x51, &fl_pub).unwrap();
    let fl_mod = delays::FLANGER.knob_model(&fl_saved);
    assert_eq!(
        fl_mod.build(&fl_mod.saved),
        delays::legacy(0x12, 0x51, &fl_pub)
    );
    checked.insert(fl_mod.id);

    // Rotator & Mangling
    let rotm = rotator::model(rotator::INIT);
    assert_eq!(
        rotm.build(&rotm.saved),
        Some(ir::EffectKind::Rotary(rotator::rotary(rotator::INIT)))
    );
    checked.insert(rotm.id);

    let bmm = mangling::beat_masher::model(mangling::beat_masher::INIT);
    assert_eq!(
        bmm.build(&bmm.saved),
        Some(ir::EffectKind::BeatMasher(
            mangling::beat_masher::ir_effect(mangling::beat_masher::INIT)
        ))
    );
    checked.insert(bmm.id);

    let gtm = mangling::gater::model(mangling::gater::INIT);
    assert_eq!(
        gtm.build(&gtm.saved),
        Some(ir::EffectKind::Gater(mangling::gater::ir_effect(
            mangling::gater::INIT
        )))
    );
    checked.insert(gtm.id);

    let bsm = mangling::beat_slicer::model(mangling::beat_slicer::INIT);
    assert_eq!(
        bsm.build(&bsm.saved),
        Some(ir::EffectKind::BeatSlicer(
            mangling::beat_slicer::ir_effect(mangling::beat_slicer::INIT)
        ))
    );
    checked.insert(bsm.id);

    let rgm = mangling::reverse_grain::model(mangling::reverse_grain::INIT);
    assert_eq!(
        rgm.build(&rgm.saved),
        Some(ir::EffectKind::ReverseGrain(
            mangling::reverse_grain::ir_effect(mangling::reverse_grain::INIT)
        ))
    );
    checked.insert(rgm.id);

    let tsm = mangling::transpose_stretch::model(mangling::transpose_stretch::INIT);
    assert_eq!(
        tsm.build(&tsm.saved),
        Some(ir::EffectKind::TransposeStretch(
            mangling::transpose_stretch::ir_effect(mangling::transpose_stretch::INIT)
        ))
    );
    checked.insert(tsm.id);

    // Ring Modulator, Stereo Tune, Cry Wah, Twin Delay, Plate Reverb
    let rmm = ringmod::model(ringmod::INIT);
    assert_eq!(
        rmm.build(&rmm.saved),
        Some(ir::EffectKind::RingMod(ringmod::ringmod(ringmod::INIT)))
    );
    checked.insert(rmm.id);

    let stm = stereo_tune::model(stereo_tune::INIT);
    assert_eq!(
        stm.build(&stm.saved),
        Some(ir::EffectKind::StereoDetune(stereo_tune::detune(
            stereo_tune::INIT
        )))
    );
    checked.insert(stm.id);

    let cwm = cry_wah::model(cry_wah::INIT);
    assert_eq!(
        cwm.build(&cwm.saved),
        Some(ir::EffectKind::Wah(cry_wah::wah(cry_wah::INIT)))
    );
    checked.insert(cwm.id);

    let tdm = twin_delay::model(twin_delay::INIT);
    assert_eq!(
        tdm.build(&tdm.saved),
        Some(ir::EffectKind::DualDelay(twin_delay::delay(
            twin_delay::INIT
        )))
    );
    checked.insert(tdm.id);

    let prm = plate_reverb::model(plate_reverb::INIT);
    assert_eq!(
        prm.build(&prm.saved),
        Some(ir::EffectKind::Reverb(plate_reverb::reverb(
            plate_reverb::INIT
        )))
    );
    checked.insert(prm.id);

    // Amps, Cabinet, Cat, Skreamer Deluxe, Kolor, DYX, Core Cell
    let twm = amps::twang::model(amps::twang::INIT);
    assert_eq!(
        twm.build(&twm.saved),
        Some(ir::EffectKind::AmpStack(amps::twang::amp(
            amps::twang::INIT
        )))
    );
    checked.insert(twm.id);

    let cabm = cabinet::model(cabinet::INIT);
    assert_eq!(
        cabm.build(&cabm.saved),
        Some(ir::EffectKind::Cabinet(cabinet::cabinet(cabinet::INIT)))
    );
    checked.insert(cabm.id);

    let jmpm = amps::jump::model(amps::jump::INIT);
    assert_eq!(
        jmpm.build(&jmpm.saved),
        Some(ir::EffectKind::AmpStack(amps::jump::amp(amps::jump::INIT)))
    );
    checked.insert(jmpm.id);

    let v5m = amps::van51::model(amps::van51::INIT);
    assert_eq!(
        v5m.build(&v5m.saved),
        Some(ir::EffectKind::AmpStack(amps::van51::amp(
            amps::van51::INIT
        )))
    );
    checked.insert(v5m.id);

    let acm = amps::ac_box::model(amps::ac_box::INIT);
    assert_eq!(
        acm.build(&acm.saved),
        Some(ir::EffectKind::AmpStack(amps::ac_box::amp(
            amps::ac_box::INIT
        )))
    );
    checked.insert(acm.id);

    let hsm = amps::hot_solo::model(amps::hot_solo::INIT);
    assert_eq!(
        hsm.build(&hsm.saved),
        Some(ir::EffectKind::AmpStack(amps::hot_solo::amp(
            amps::hot_solo::INIT
        )))
    );
    checked.insert(hsm.id);

    let catm = cat::model(cat::INIT);
    assert_eq!(
        catm.build(&catm.saved),
        Some(ir::EffectKind::AmpStack(cat::amp(cat::INIT)))
    );
    checked.insert(catm.id);

    let bpm = amps::bass_pro::model(amps::bass_pro::INIT);
    assert_eq!(
        bpm.build(&bpm.saved),
        Some(ir::EffectKind::AmpStack(amps::bass_pro::amp(
            amps::bass_pro::INIT
        )))
    );
    checked.insert(bpm.id);

    let skdm = skreamer_deluxe::model(skreamer_deluxe::INIT);
    assert_eq!(
        skdm.build(&skdm.saved),
        Some(ir::EffectKind::AmpStack(skreamer_deluxe::amp(
            skreamer_deluxe::INIT
        )))
    );
    checked.insert(skdm.id);

    let kolm = kolor::model(kolor::INIT);
    assert_eq!(
        kolm.build(&kolm.saved),
        Some(ir::EffectKind::ColorSat(kolor::color_sat(kolor::INIT)))
    );
    checked.insert(kolm.id);

    let dyxm = dyx::model(dyx::INIT);
    assert_eq!(
        dyxm.build(&dyxm.saved),
        Some(ir::EffectKind::SpectralMorph(dyx::spectral_morph(
            dyx::INIT
        )))
    );
    checked.insert(dyxm.id);

    let ccm = core_cell::model(core_cell::INIT);
    assert_eq!(
        ccm.build(&ccm.saved),
        Some(core_cell::effect(core_cell::INIT))
    );
    checked.insert(ccm.id);

    assert_eq!(
        checked, seen_ids,
        "every module in kontakt_effects::module::ALL must be covered by roundtrip verification"
    );

    // Structural knob liveness guard: every SlotKnob written by a module's `rows()`
    // must exist in `module.knobs` and visibly affect `build_effect` when moved
    // across its law's range (prevents dead knobs like KT-12).
    let delay_rows = delays::rows();
    let ch_rows: Vec<_> = delay_rows
        .iter()
        .filter(|(n, ..)| n.starts_with("$ENGINE_PAR_CH_"))
        .cloned()
        .collect();
    let fl_rows: Vec<_> = delay_rows
        .iter()
        .filter(|(n, ..)| n.starts_with("$ENGINE_PAR_FL_"))
        .cloned()
        .collect();
    type Rows<'a> = &'a [(&'a str, ir::KnobTarget, ir::Law)];
    let modules_and_rows: &[(&module::Module, Rows, Vec<f32>)] = &[
        (&tape::MODULE, &tape::rows(), tm.saved.to_vec()),
        (&skreamer::MODULE, &skreamer::rows(), sm.saved.to_vec()),
        (&dstortion::MODULE, &dstortion::rows(), dm.saved.to_vec()),
        (&supergt::MODULE, &supergt::rows(), sgtm.saved.to_vec()),
        (
            &dynamics::FEEDBACK_MODULE,
            &dynamics::feedback_rows(),
            fbm.saved.to_vec(),
        ),
        (&phasis::MODULE, &phasis::rows(), pm.saved.to_vec()),
        (&flair::MODULE, &flair::rows(), flm.saved.to_vec()),
        (&choral::MODULE, &choral::rows(), chm.saved.to_vec()),
        (&bite::MODULE, &bite::rows(), bm.saved.to_vec()),
        (&freak::MODULE, &freak::rows(), frm.saved.to_vec()),
        (&psyche::MODULE, &psyche::rows(), psm.saved.to_vec()),
        (&vibrato::MODULE, &vibrato::rows(), vm.saved.to_vec()),
        (&delays::CHORUS, &ch_rows, ch_mod.saved.to_vec()),
        (&delays::FLANGER, &fl_rows, fl_mod.saved.to_vec()),
        (&rotator::MODULE, &rotator::rows(), rotm.saved.to_vec()),
        (
            &mangling::beat_masher::MODULE,
            &mangling::beat_masher::rows(),
            bmm.saved.to_vec(),
        ),
        (
            &mangling::gater::MODULE,
            &mangling::gater::rows(),
            gtm.saved.to_vec(),
        ),
        (
            &mangling::beat_slicer::MODULE,
            &mangling::beat_slicer::rows(),
            bsm.saved.to_vec(),
        ),
        (
            &mangling::reverse_grain::MODULE,
            &mangling::reverse_grain::rows(),
            rgm.saved.to_vec(),
        ),
        (
            &mangling::transpose_stretch::MODULE,
            &mangling::transpose_stretch::rows(),
            tsm.saved.to_vec(),
        ),
        (&ringmod::MODULE, &ringmod::rows(), rmm.saved.to_vec()),
        (
            &stereo_tune::MODULE,
            &stereo_tune::rows(),
            stm.saved.to_vec(),
        ),
        (&cry_wah::MODULE, &cry_wah::rows(), cwm.saved.to_vec()),
        (&twin_delay::MODULE, &twin_delay::rows(), tdm.saved.to_vec()),
        (
            &plate_reverb::MODULE,
            &plate_reverb::rows(),
            prm.saved.to_vec(),
        ),
        (
            &amps::twang::MODULE,
            &amps::twang::rows(),
            twm.saved.to_vec(),
        ),
        (&cabinet::MODULE, &cabinet::rows(), cabm.saved.to_vec()),
        (
            &amps::jump::MODULE,
            &amps::jump::rows(),
            jmpm.saved.to_vec(),
        ),
        (
            &amps::van51::MODULE,
            &amps::van51::rows(),
            v5m.saved.to_vec(),
        ),
        (
            &amps::ac_box::MODULE,
            &amps::ac_box::rows(),
            acm.saved.to_vec(),
        ),
        (
            &amps::hot_solo::MODULE,
            &amps::hot_solo::rows(),
            hsm.saved.to_vec(),
        ),
        (&cat::MODULE, &cat::rows(), catm.saved.to_vec()),
        (
            &amps::bass_pro::MODULE,
            &amps::bass_pro::rows(),
            bpm.saved.to_vec(),
        ),
        (
            &skreamer_deluxe::MODULE,
            &skreamer_deluxe::rows(),
            skdm.saved.to_vec(),
        ),
        (&kolor::MODULE, &kolor::rows(), kolm.saved.to_vec()),
        (&dyx::MODULE, &dyx::rows(), dyxm.saved.to_vec()),
        (&core_cell::MODULE, &core_cell::rows(), ccm.saved.to_vec()),
    ];
    let row_checked_ids: HashSet<_> = modules_and_rows.iter().map(|(m, ..)| m.id).collect();
    assert_eq!(
        row_checked_ids, seen_ids,
        "every module in kontakt_effects::module::ALL must have its rows() checked for knob liveness"
    );
    let base_effect = ir::Effect {
        kind: ir::EffectKind::Other,
        bypass: false,
        output: 1.0,
        dry: 0.0,
    };
    let mut dead = Vec::new();
    for (m, rows, base_params) in modules_and_rows {
        let base_built = m
            .build_effect(base_params, &base_effect)
            .expect("baseline params must build");
        for (par_name, target, law) in *rows {
            let ir::KnobTarget::Slot(knob) = *target else {
                continue;
            };
            let idx = m.knobs.iter().position(|k| *k == knob).unwrap_or_else(|| {
                panic!(
                    "module {} row {par_name} targets {knob:?}, which is absent from module.knobs",
                    m.name
                )
            });
            let mut changed = false;
            for x in [0.0f32, 1e-6, 5e-6, 9e-6, 0.25, 0.5, 0.75, 1.0] {
                let mut probe = base_params.clone();
                probe[idx] = law.eval(x);
                if m.build_effect(&probe, &base_effect)
                    .is_some_and(|e| e != base_built)
                {
                    changed = true;
                    break;
                }
            }
            if !changed {
                dead.push(format!("{} {par_name}", m.name));
            }
        }
    }
    // Ratchet: knobs a row writes that `build` never reads. This list may
    // only shrink; a new entry is a dead knob (like KT-12) to fix, not list.
    // Swept one at a time from the saved defaults, so a knob only another
    // mode reads (Freak's) shows here too.
    let known_dead: &[&str] = &[
        "Freak $ENGINE_PAR_FREAK_BP_FILTER",
        "Freak $ENGINE_PAR_FREAK_BP_FREQ",
        "Freak $ENGINE_PAR_FREAK_CARRIER",
        "Freak $ENGINE_PAR_FREAK_CONTOUR",
        "Freak $ENGINE_PAR_FREAK_DEMOD",
        "Freak $ENGINE_PAR_FREAK_RELEASE",
        "Freak $ENGINE_PAR_FREAK_TUNING",
        "Freak $ENGINE_PAR_FREAK_WIDTH",
        "Phasis $ENGINE_PAR_PHASIS_INVERT_MOD_MIX",
        "Phasis $ENGINE_PAR_PHASIS_RATE_UNIT",
        "Psyche Delay $ENGINE_PAR_PSYDL_DETUNE_STEREO",
        "Tape Saturator $ENGINE_PAR_TP_QUALITY",
    ];
    dead.sort();
    assert_eq!(
        dead, known_dead,
        "dead knobs changed: fix the new ones or shrink the list"
    );
}

#[test]
fn cyclic_audio_graph_surfaces_error_in_prepare_and_engine() {
    let mut graph = ir::AudioGraph::default();
    let a = graph.push_node(ir::GraphNodeKind::Mix {
        gain: 1.0,
        pan: 0.0,
    });
    let b = graph.push_node(ir::GraphNodeKind::Mix {
        gain: 1.0,
        pan: 0.0,
    });
    graph.connect(a, b, 1.0, true);
    graph.connect(b, a, 0.5, true);

    // 1. `kontra_prepare::prepare` surfaces `ir::Impact::Broken` diagnostic and refuses cyclic graph.
    let inst = ir::Instrument {
        graph: Some(graph.clone()),
        ..ir::Instrument::DEFAULT
    };
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let (prepared, diags) = kontra_prepare::prepare(&inst, 48_000, &mut store, &|_| {
        Err(std::io::Error::other("unused"))
    });
    assert!(
        diags
            .iter()
            .any(|d| d.impact == ir::Impact::Broken && d.feature == "ir.graph.cycle"),
        "expected Broken cycle diagnostic from prepare, got: {diags:?}"
    );
    assert!(prepared.graph.is_none());

    // 2. `Engine::try_new` returns Err and `Engine::new` records `graph_error()` (never silently ignored).
    let mut plan = Plan::new(vec![], 0);
    plan.graph = Some(graph);
    let err = Engine::try_new(Arc::new(plan.clone()), store.client(), 48_000)
        .err()
        .expect("try_new must fail on cyclic AudioGraph");
    assert!(err.contains("cycle"), "unexpected error: {err}");

    let engine = Engine::new(Arc::new(plan), store.client(), 48_000);
    assert!(
        engine.graph_error().is_some_and(|e| e.contains("cycle")),
        "Engine::new must expose cyclic graph error via graph_error()"
    );
}
