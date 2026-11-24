//! Mechanical architectural guards (Wave 2A).
//!
//! Enforces structural invariants at test time so regressions fail CI:
//! 1. `ir::Instrument: PartialEq` compile-time assertion + zero `fn(` pointers
//!    or `fn_addr_eq` in `crates/model/ir/src`.
//! 2. Vendor-word ratchet across `crates/{engine,dsp,model}/*/src`.
//! 3. Architecture metric ratchets (`Source::` in `app/ui/src`, `fn_addr_eq`
//!    workspace-wide).
//! 4. Per-module encoding equality and unique `ModelId`s across `kontakt-effects`.
//! 5. Cyclic `AudioGraph` errors are surfaced (never silently discarded) in both
//!    `kontra-prepare` and `Engine::try_new` / `Engine::graph_error`.
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
    let ceilings = [("engine", 291usize), ("dsp", 17usize), ("model", 176usize)];
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

    // 1. Zero `Source::` format branching in `crates/app/ui/src`.
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
}

#[test]
fn all_kontakt_effect_modules_register_unique_ids_and_roundtrip_saved() {
    use kontra_kontakt_effects::{
        bite, choral, dstortion, dynamics, flair, freak, module, phasis, psyche, skreamer, supergt,
        tape, vibrato,
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
    // Tape
    let mut tape_pub = Vec::new();
    for v in [0.4f32, 0.5, 0.7] {
        tape_pub.extend(v.to_le_bytes());
    }
    tape_pub.push(0);
    let tm = tape::model(&tape_pub).unwrap();
    assert_eq!(tm.build(&tm.saved), Some(tape::tape(0.4, 0.5, 0.7)));

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

    // Feedback Compressor
    let mut fb_pub = Vec::new();
    for v in [0.5f32, 0.3, 0.4, 0.5, 0.6, 1.0, 0.0] {
        fb_pub.extend(v.to_le_bytes());
    }
    fb_pub.extend([0u8, 1u8, 0u8]);
    let (fb_eff, _) = dynamics::effect(0x4c, 0x10, &fb_pub).unwrap();
    let fbm = dynamics::feedback_model(&fb_pub).unwrap();
    assert_eq!(fbm.build(&fbm.saved), Some(fb_eff));

    // Phasis, Flair, Choral, Bite, Freak, Psyche, Vibrato
    let pm = phasis::model();
    assert_eq!(pm.build(&pm.saved), Some(phasis::phasis(phasis::INIT)));
    let flm = flair::model();
    assert_eq!(flm.build(&flm.saved), Some(flair::flair(flair::INIT)));
    let chm = choral::model();
    assert_eq!(chm.build(&chm.saved), Some(choral::choral(choral::INIT)));
    let bm = bite::model(bite::FRESH);
    assert_eq!(bm.build(&bm.saved), Some(bite::bite(bite::FRESH)));
    let frm = freak::model(freak::FRESH);
    assert_eq!(frm.build(&frm.saved), Some(freak::freak(freak::FRESH)));
    let psm = psyche::model(psyche::INIT);
    assert_eq!(
        psm.build(&psm.saved),
        Some(ir::EffectKind::Shifter(psyche::shifter(psyche::INIT)))
    );
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
