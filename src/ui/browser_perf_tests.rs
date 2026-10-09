//! Optional full installed-catalog probe: metadata only, no instrument or sample loading.
use super::*;
use super::tests::{Harness, center, pixels};
use std::collections::{BTreeMap, BTreeSet};

fn installed() -> Arc<SamplerParams> {
    let index = PathBuf::from(std::env::var_os("KONTRA_BROWSER_INDEX").expect("product index"));
    let cache: serde_json::Value = serde_json::from_slice(&std::fs::read(index).unwrap()).unwrap();
    assert_eq!(cache["version"], 1);
    let entries = cache["entries"].as_object().unwrap();
    let kontakt = Path::new("/mnt/MAIN_STORAGE/Libraries/Kontakt");
    let uvi = Path::new("/mnt/MAIN_STORAGE/Libraries/UVI");
    let mut products = BTreeMap::new();
    let mut libraries = BTreeMap::new();
    let mut files = BTreeSet::new();
    let mut snapshots = BTreeMap::<(PathBuf, String), Vec<PathBuf>>::new();
    let mut bases = BTreeMap::<(PathBuf, String), Vec<PathBuf>>::new();
    let mut indexed = BTreeSet::new();
    for (path, entry) in entries {
        let path = PathBuf::from(path);
        assert!(path.starts_with(kontakt) || path.starts_with(uvi));
        let stat = path.metadata().expect("indexed file still exists");
        assert_eq!(entry["stamp"][0].as_u64().unwrap(), stat.len(), "index size changed");
        assert_eq!(entry["stamp"][1].as_u64().unwrap() as u128,
            stat.modified().unwrap().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos(), "index mtime changed");
        let metadata = &entry["metadata"];
        let extension = path.extension().unwrap_or_default().to_string_lossy().to_ascii_lowercase();
        if ["nki", "nkm", "nksn", "ufs"].contains(&extension.as_str()) { indexed.insert(path.clone()); }
        let dir = if path.starts_with(kontakt) {
            kontakt.join(path.strip_prefix(kontakt).unwrap().components().next().unwrap())
        } else { path.clone() };
        if let Some(product) = metadata["Product"].as_array() {
            products.insert(dir.clone(), (product[0].as_str().unwrap().to_owned(), product[1].as_str().unwrap().to_owned()));
        }
        if extension == "nksn" {
            if let Some(name) = metadata["Snapshot"].as_str() { snapshots.entry((dir, name.into())).or_default().push(path); }
        } else if extension == "nki" || extension == "nkm" {
            libraries.entry(dir.clone()).or_insert_with(|| crate::library::Library { dir: dir.clone(), ..Default::default() });
            if let Some(name) = metadata["Instrument"].as_str() { bases.entry((dir, name.into())).or_default().push(path.clone()); }
            files.insert(path);
        } else if extension == "ufs" {
            let members = metadata["Bank"].as_array().expect("UVI members require the already populated product cache");
            libraries.entry(dir.clone()).or_insert_with(|| crate::library::Library { dir, ..Default::default() });
            files.extend(members.iter().map(|member| path.join(member.as_str().unwrap())));
        }
    }
    let mut live = BTreeSet::new();
    for root in [kontakt, uvi] {
        for entry in walkdir::WalkDir::new(root).follow_links(false).into_iter().filter_entry(|e|
            !(e.depth() > 0 && e.file_type().is_dir() && (e.file_name().to_string_lossy().eq_ignore_ascii_case("samples")
                || e.file_name().to_string_lossy().starts_with('.')))) {
            let entry = entry.unwrap();
            if entry.file_type().is_file() && entry.path().extension().is_some_and(|e|
                ["nki", "nkm", "nksn", "ufs"].iter().any(|wanted| e.eq_ignore_ascii_case(wanted))) {
                live.insert(entry.into_path());
            }
        }
    }
    assert_eq!(live.len(), indexed.len(), "new or removed catalog paths invalidate the fixture");
    assert!(live == indexed, "new or removed catalog paths invalidate the fixture");
    for library in libraries.values_mut() {
        let raw = library.dir.file_stem().unwrap().to_string_lossy();
        let (name, vendor) = crate::library::clean_name(&raw);
        (library.name, library.vendor) = products.get(&library.dir).cloned().unwrap_or((name, vendor));
        library.registered = products.contains_key(&library.dir);
        library.multis = files.iter().filter(|p| p.starts_with(&library.dir) && crate::library::is_multi(p)).count();
        library.instruments = files.iter().filter(|p| p.starts_with(&library.dir)).count() - library.multis;
    }
    let mut shelf = crate::library::Shelf::new(libraries.into_values().collect());
    for ((dir, name), paths) in snapshots {
        if let Some(base) = bases.get(&(dir, name.clone())).filter(|bases| bases.len() == 1) {
            shelf.snapshots.insert(base[0].clone(), crate::library::Snapshots { instrument: name, paths });
        }
    }
    let p = Arc::new(SamplerParams::new());
    let mut view = p.shared.view.lock().unwrap();
    view.shelf = Arc::new(shelf);
    view.files = Arc::new(files.into_iter().collect());
    view.scanned = p.shared.libraries.wanted();
    println!("INSTALLED libraries={} native_and_uvi={} matched_presets={}", view.shelf.libraries.len(), view.files.len(),
        view.shelf.snapshots.values().map(|s| s.paths.len()).sum::<usize>());
    drop(view);
    p
}

fn record(name: &str, action: impl FnOnce()) {
    browser::WORK.with(|n| n.set([0; 6]));
    let calls = crate::plugin::tests::allocations(action);
    println!("WORK stage={name} counters={:?} heap_calls={calls}", browser::WORK.with(|n| n.get()));
}

#[test]
#[ignore = "local installed metadata catalog; set KONTRA_BROWSER_INDEX and KONTRA_BROWSER_PERF_OUT"]
fn installed_browser_work() {
    let out = PathBuf::from(std::env::var_os("KONTRA_BROWSER_PERF_OUT").expect("receipt"));
    std::fs::create_dir_all(&out).unwrap();
    let p = installed();
    let mut harness = None;
    record("open", || harness = Some(Harness::new(&p, 1180., 760.)));
    let mut h = harness.unwrap();
    for provider in ["Kontakt", "UVI"] {
        if provider == "UVI" { record("provider", || h.press("bank-uvi")); }
        let view = p.shared.view.lock().unwrap();
        let library = view.shelf.libraries.iter().filter(|l| l.dir.starts_with(format!("/mnt/MAIN_STORAGE/Libraries/{provider}")))
            .max_by_key(|l| l.instruments + view.shelf.snapshots.iter().filter(|(base, _)| base.starts_with(&l.dir)).map(|(_, s)| s.paths.len()).sum::<usize>()).unwrap();
        let name = library.name.clone();
        drop(view);
        let id = (0..p.shared.view.lock().unwrap().shelf.libraries.len()).map(|n| format!("library-{n}"))
            .find(|id| h.ui.scene().unwrap().surface(id).is_some_and(|s| s.semantics.as_ref()
                .and_then(|semantics| semantics.label.as_deref()).is_some_and(|label| label.starts_with(&format!("{name},"))))).unwrap();
        record("select", || { h.press(&id); h.idle(30); });
        record("search", || {
            let at = center(&h.ui, &id);
            for down in [true, false] {
                h.tick(Input { pointer: PointerInput { pos: Some(at), buttons: if down { Buttons::PRIMARY } else { Buttons::default() }, ..Default::default() }, ..Default::default() });
            }
            h.idle(3);
            h.ui.focus("search");
            h.tick(Input { text: "a".into(), ..Default::default() });
            h.idle(3);
        });
        let list = if provider == "UVI" { "browser-list-uvi" } else { "browser-list" };
        let at = center(&h.ui, list);
        let mut costs = (0, 0);
        record("scroll100", || {
            for _ in 0..100 { let cost = h.tick_cost(Input { wheel: Vec2::new(0., 30.), pointer: PointerInput { pos: Some(at), ..Default::default() }, ..Default::default() }); costs.0 += cost.0; costs.1 += cost.1; }
        });
        println!("HEAP build={} resolve_frame={}", costs.0, costs.1);
        moose::core::screenshot::save_png(&out.join(format!("{provider}.png")), &pixels(&h.ui, 1180, 760), 1180, 760);
    }
}
