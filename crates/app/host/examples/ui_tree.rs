//! Loads instruments and builds each KSP script's UI tree after `on init`;
//! prints load time, tree build time and what the tree holds, per file.
//! `cargo run --release -p kontra-host --example ui_tree -- <file>...`

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Instant;

use kontra_engine::Engine;
use kontra_stream::{Store, StoreConfig};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    for path in std::env::args_os().skip(1) {
        let path = std::path::PathBuf::from(path);
        let t = Instant::now();
        let mut store = Store::new(StoreConfig::DEFAULT)?;
        let loaded = kontra_host::instrument::load(&path, 48_000, &mut store)?;
        let mut engine = Engine::new(Arc::new(loaded.plan), store.client(), 48_000);
        let init = t.elapsed();
        for (slot, face) in loaded.faces.iter().enumerate() {
            let Some(face) = face else { continue };
            let t = Instant::now();
            let tree = kontra_host::ui::interface(face, slot as u8, &engine, &|_| None);
            let built = t.elapsed();
            let tree = match tree {
                Ok(tree) => tree,
                Err(e) => {
                    println!("{}: slot {slot}: {e}", loaded.name);
                    continue;
                }
            };
            let visible = (0..tree.widgets.len())
                .filter(|&n| tree.visible(kontra_ui_model::WidgetRef(n)))
                .count();
            let mut gaps = BTreeMap::<&str, usize>::new();
            for u in &tree.unsupported {
                *gaps.entry(u.feature.as_str()).or_default() += 1;
            }
            let mut gaps: Vec<_> = gaps.into_iter().collect();
            gaps.sort_by_key(|g| std::cmp::Reverse(g.1));
            gaps.truncate(6);
            if std::env::var_os("KONTRA_UI_GAPS").is_some() {
                if let Ok(id) = std::env::var("KONTRA_UI_ID") {
                    for w in tree
                        .widgets
                        .iter()
                        .filter(|w| w.source_id.map(|s| s.to_string()) == Some(id.clone()))
                    {
                        println!("  widget {} {:?}", w.name, w.kind);
                    }
                }
                for u in tree.unsupported.iter().take(12) {
                    println!(
                        "  gap {:?} {} = {}",
                        u.widget
                            .and_then(|w| tree.widgets.get(w.0))
                            .map(|w| &w.name),
                        u.feature,
                        u.value
                    );
                }
            }
            // Idle frames, then one knob moved per frame.
            let mut live = kontra_host::ui::Live::new(face, slot as u8, &engine, &|_| None)?;
            let mut block = vec![[0f32; 2]; 512];
            let (mut idle, mut spent) = (0, std::time::Duration::ZERO);
            let mut last = None;
            for frame in 0..100 {
                engine.process(&mut block, &[]);
                let t = Instant::now();
                let changed = live.refresh(face, slot as u8, &engine, &|_| None);
                if frame >= 10 {
                    spent += t.elapsed();
                }
                if std::env::var_os("KONTRA_UI_IDLE").is_some() && !changed.is_empty() {
                    let names: Vec<_> = changed
                        .iter()
                        .filter_map(|w| live.tree.widgets.get(w.0))
                        .map(|w| (&w.name, &w.value, &w.text))
                        .take(8)
                        .collect();
                    println!("  idle rebuilt {names:?}");
                }
                if !changed.is_empty() {
                    last = Some(frame);
                }
                idle += changed.len();
            }
            let per_frame = spent / 90;
            let knob = tree.widgets.iter().find_map(|w| match w.kind {
                kontra_ui_model::Kind::Knob { .. } | kontra_ui_model::Kind::Slider { .. }
                    if !w.hidden =>
                {
                    w.source_id
                }
                _ => None,
            });
            let moved = knob.map(|id| {
                (1..=10)
                    .map(|v| {
                        engine.set_control(slot as u8, id, v);
                        engine.process(&mut block, &[]);
                        live.refresh(face, slot as u8, &engine, &|_| None).len()
                    })
                    .max()
                    .unwrap_or(0)
            });
            println!(
                "  refresh: {per_frame:?} per settled idle frame, {idle} idle rebuilds in 100 frames (last in frame {last:?}), one knob moved: {moved:?} widgets rebuilt per frame"
            );
            // The sustain pedal down, then up: what a controller callback lights.
            let pedal = [127, 0].map(|v| {
                let cc = kontra_engine::Timed {
                    at: kontra_engine::SampleTime(0),
                    body: kontra_engine::Event::midi1(0xB0, 64, v),
                };
                engine.process(&mut block, &[cc]);
                let mut changed = std::collections::BTreeSet::new();
                for _ in 0..50 {
                    engine.process(&mut block, &[]);
                    for w in live.refresh(face, slot as u8, &engine, &|_| None) {
                        changed.extend(live.tree.widgets.get(w.0).map(|w| w.name.clone()));
                    }
                }
                changed
            });
            println!("  pedal down rebuilt {:?}, up {:?}", pedal[0], pedal[1]);
            let page = tree.pages.first().map(|p| (p.size.width, p.size.height));
            println!(
                "{} slot {slot}: load+init {init:?}, tree {built:?}; {} widgets ({visible} shown), {} pictures, page {page:?}, perfview {}; gaps {gaps:?}",
                loaded.name,
                tree.widgets.len(),
                tree.assets.len(),
                tree.performance,
            );
        }
    }
    Ok(())
}
