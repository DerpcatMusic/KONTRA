//! Installed-bank witness through the actual complete editor and shared painters.
use super::*;

fn complete_editor(path: &str, program: &str) {
    let bank = sampler_uvi::Bank::open(Path::new(path)).unwrap();
    let (xml, _) = bank.program(program).unwrap();
    let host = sampler_uvi::script::ScriptHost::new(
        &xml,
        bank.scripts(),
        sampler_uvi::script::Config::realtime(),
    )
    .unwrap();
    assert!(
        host.fault_counts().init.is_empty(),
        "initialization must complete: {:?}",
        host.fault_counts()
    );
    let face = host.interface();
    let widgets = face.widgets.len();
    let mut requested = std::collections::BTreeSet::new();
    if let Some(asset) = face.pages[0].background.image {
        requested.insert(asset.0);
    }
    for n in face
        .draw_order(sampler_ui_ir::PageRef(0))
        .into_iter()
        .filter(|&n| face.visible(n))
    {
        let widget = &face.widgets[n.0];
        requested.extend(widget.images.iter().map(|i| i.asset.0));
        if let Some(style) = widget.style.and_then(|s| face.styles.get(s.0)) {
            if let sampler_ui_ir::Font::File(a) | sampler_ui_ir::Font::Bitmap(a) = style.font {
                requested.insert(a.0);
            }
        }
    }
    let p = Arc::new(SamplerParams::new());
    p.shared.ensure_parts(1);
    *p.shared.part(0).unwrap().controls.lock().unwrap() = host
        .control_values()
        .into_iter()
        .map(|(id, value)| crate::plugin::ControlCell::loop_audit_new(id, value))
        .collect::<Vec<_>>()
        .into();
    p.selection
        .write()
        .unwrap()
        .parts
        .push(crate::plugin::Part {
            path: Path::new(path).join(program).to_string_lossy().into_owned(),
            view: 1,
            ..Default::default()
        });
    p.shared.view.lock().unwrap().parts[0].interfaces = vec![face].into();
    let mut h = tests::Harness::new(&p, 1180., 780.);
    h.ui.focus("tab-rack");
    let before = blake3::hash(&tests::pixels(&h.ui, 1180, 780));
    let start = std::time::Instant::now();
    let mut changed = start;
    let mut revision = pictures::revision();
    let initial_revision = revision;
    while start.elapsed() < Duration::from_secs(90) {
        h.idle(1);
        let now = pictures::revision();
        if now != revision {
            revision = now;
            changed = std::time::Instant::now();
        }
        if revision.saturating_sub(initial_revision) >= requested.len() as u64
            && changed.elapsed() > Duration::from_secs(1)
        {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let scene = h.ui.scene().unwrap();
    let frame = scene
        .surface("part-0-epoch-0-script-0-ir-view")
        .expect("whole editor includes authored view")
        .frame;
    assert!(frame.size.width > 0. && frame.size.height > 0.);
    let rgba = tests::pixels(&h.ui, 1180, 780);
    assert!(
        revision.saturating_sub(initial_revision) >= requested.len() as u64,
        "shared worker completes visible resource requests"
    );
    assert_ne!(
        blake3::hash(&rgba),
        before,
        "prepared artwork changes the complete editor's paint"
    );
    let colours: std::collections::HashSet<_> = rgba.chunks_exact(4).collect();
    assert!(colours.len() > 32, "complete editor paints pixels");
    assert_eq!(
        p.shared.view.lock().unwrap().parts[0].interfaces[0]
            .widgets
            .len(),
        widgets,
        "scene culling retains every authored widget"
    );
    eprintln!(
        "whole_editor_widgets={widgets} painted_colours={} resource_revision={revision}",
        colours.len()
    );
}

#[test]
#[ignore = "installed bank and approved KONTRA_UVI_READER required"]
fn complete_bartok_original_editor_fits_the_scene_and_paints() {
    complete_editor(
        "/mnt/MAIN_STORAGE/Libraries/UVI/UVI - Augmented Orchestra v1.1.2-R2R/Augmented Orchestra.ufs",
        "Presets/00 Orchestra/01 Strings/V Strings Bartok.uvip",
    );
}

#[test]
#[ignore = "installed bank and approved KONTRA_UVI_READER required"]
fn complete_clarinet_original_editor_fits_the_scene_and_paints() {
    complete_editor(
        "/mnt/MAIN_STORAGE/Libraries/UVI/VWinds - Clarinets/VWinds-AClarinet.ufs",
        "Presets/Clarinet A.uvip",
    );
}

#[test]
fn assistant_host_font_parses_on_request() {
    fn rss() -> u64 {
        std::fs::read_to_string("/proc/self/status").ok().and_then(|s|
            s.lines().find_map(|l| l.strip_prefix("VmRSS:").and_then(|v|v.split_whitespace().next()?.parse().ok()))
        ).unwrap_or(0)
    }
    let path=std::env::temp_dir().join("kontra-authored-host-font.uvip");
    let mut source=pictures::Source::of(&path);
    assert!(source.read_result("Unrelated-Regular.ttf").unwrap().is_none());
    let _ui=theme::ui();
    let before=rss();
    let asset=sampler_ui_ir::Asset {path:"Assistant-Regular.ttf".into(),kind:sampler_ui_ir::AssetKind::TrueTypeFont};
    let font=source.font(&asset).expect("requested Assistant Regular parses");
    assert_eq!(font.as_ref().len(),75500);
    eprintln!("HOST_FONT bytes={} rss_delta_kib={}",font.as_ref().len(),rss().saturating_sub(before));
}
