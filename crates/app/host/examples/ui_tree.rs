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
        let engine = Engine::new(Arc::new(loaded.plan), store.client(), 48_000);
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
