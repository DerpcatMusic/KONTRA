//! Authored full-owner replacements through Load, Face and the real pointer path.
//! No installed library, plugin host, vendor script or artwork is involved.
use super::*;
use crate::{
    plugin::{
        Load, SamplerParams,
        ui_activity::{Activity, Count},
    },
    sound::{Core, v2::V2Core},
    ui::{native_runtime::Package, native_ui, tests::Harness},
};
use moose::prelude::BackgroundTask;
use std::{
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

#[allow(dead_code)]
#[path = "../../../crates/sampler-kontakt/tests/support/snapshot_reuse.rs"]
mod wire;

const SCRIPT: &str = r#"on init
 set_snapshot_type(0)
 make_perfview
 load_native_ui("main")
 declare ui_knob $value(0,1000,1)
 make_persistent($value)
 declare ui_button $action
 declare ui_knob $hits(0,1000,1)
end on
on ui_control($action)
 inc($hits)
end on"#;

const PRESENTATION: &str = r#"
local ui=require('native_ui')
local kontakt=require('kontakt')
local value=kontakt.connect_parameter('value')
local hits=kontakt.connect_parameter('hits')
local action=kontakt.connect_parameter('action')
return function()
    local get,set=ui.create_state(0)
    local caption=string.format('value=%d hits=%d local=%d',
        math.floor(value:value()*1000+0.5),math.floor(hits:value()*1000+0.5),get())
    local children={
        @ui.Image {file='photo.png',resizable=true}.frame(width=24,height=24),
        @ui.Text {text=caption}.font('fixture.ttf').font_size(12),
    }
    if value:value()>0.5 then
        children[#children+1]=@ui.Rectangle {color=ui.Color(230,80,20)}.frame(width=16,height=8)
    end
    return @ui.VStack {children[1],children[2],children[3]}
        .frame(width=180+math.floor(value:value()*100),height=80+math.floor(value:value()*40))
        .on_tap_gesture({complete=function()
            set(get()+1)
            action:set_value(1-action:value())
        end})
end
"#;

fn image(path: &Path, rgba: [u8; 4]) {
    let mut bytes = Vec::new();
    let mut encoder = png::Encoder::new(&mut bytes, 1, 1);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder
        .write_header()
        .unwrap()
        .write_image_data(&rgba)
        .unwrap();
    std::fs::write(path, bytes).unwrap();
}

struct Fixture {
    _directory: tempfile::TempDir,
    path: PathBuf,
    a: PathBuf,
    b: PathBuf,
}
fn fixture() -> Fixture {
    let directory = wire::directory();
    let path = directory.path().join("Authored.nki");
    std::fs::write(
        &path,
        wire::base_instrument("Authored", &[Some(SCRIPT), None, None, None, None]),
    )
    .unwrap();
    let root = directory.path().join("Resources/native_ui");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("main.nui"), PRESENTATION).unwrap();
    std::fs::write(root.join("fixture.ttf"), super::super::theme::NOTO_SANS).unwrap();
    image(&root.join("photo.png"), [40, 150, 220, 255]);
    let a = directory.path().join("A.nksn");
    let b = directory.path().join("B.nksn");
    for (snapshot, state) in [(&a, "$value 200"), (&b, "$value 800")] {
        std::fs::write(
            snapshot,
            wire::script_only_snapshot("Authored", "", &[&[state], &[], &[], &[], &[]]),
        )
        .unwrap();
    }
    Fixture {
        _directory: directory,
        path,
        a,
        b,
    }
}

fn load(p: &SamplerParams, core: &mut V2Core, snapshot: &Path) -> u64 {
    p.selection.write().unwrap().parts[0].snapshot = snapshot.to_string_lossy().into();
    Load.run(p);
    p.shared.widget_gate_install(core);
    let published = p.shared.view.lock().unwrap();
    let part = &published.parts[0];
    assert!(part.resource_revision.is_some(), "{}", part.status);
    assert_eq!(part.generation, core.epoch(0));
    assert!(
        part.interfaces.iter().any(|f| f.native_ui.is_some()),
        "{}",
        part.status
    );
    part.generation
}
fn audio(p: &SamplerParams, core: &mut V2Core) {
    core.render(64);
    p.shared.widget_gate_readback(core);
}
fn ready(p: &SamplerParams, core: &mut V2Core, h: &mut Harness, authored: Size) -> (u64, Arc<Package>) {
    let deadline = Instant::now() + Duration::from_secs(10);
    let setting = p.shared.libraries.settings().view_scale;
    loop {
        audio(p, core);
        h.idle(1);
        assert!(
            native_ui::gate_diagnostics().is_empty(),
            "{:?}",
            native_ui::gate_diagnostics()
        );
        if let Some((id, package)) = native_ui::gate_package(0)
            && package.complete()
            && package.images.get("photo.png").is_some()
            && layout_ready(&h.ui, authored, setting)
        {
            return (id, package);
        }
        assert!(
            Instant::now() < deadline,
            "authored NativeUI resources and measured rack layout did not settle: {:?}",
            native_ui::gate_targets(0).first().map(|(id, _)| routing(&h.ui, id))
        );
        std::thread::sleep(Duration::from_millis(1));
    }
}
fn layout_ready(ui: &Ui, authored: Size, setting: f32) -> bool {
    let scene = ui.scene().unwrap();
    let root = scene.surface(&target()).unwrap();
    let part = scene.surface("part-0").unwrap();
    let body = scene.surface("body-0").unwrap();
    let scale = scale_to_fit(fitting_room(ui), authored, setting);
    // Resource completion is not rack-layout completion. The rack reads the
    // previous body's natural height, then springs the part towards it. Until
    // that measurement agrees, its later footer/empty canvas can cover the
    // native root's center. Wait for the actual measurement and current fit,
    // never a frame count or a successful pointer retry. Incoming warm paints
    // are still asserted before ready() and compared with this settled layout.
    part.frame.size.height == (super::super::rack::SLIM + body.frame.size.height).round()
        && fitted_size_matches(root.frame.size, Size::new(authored.width * scale, authored.height * scale))
}
fn fitted_size_matches(actual: Size, expected: Size) -> bool {
    // Fitting computes 200 * (888 / 200) = 888.0000000000001, while
    // layout clamps to the available 888 points. Allow only arithmetic
    // roundoff; authored geometry and complete painted pixels stay asserted.
    let axis = |a: f64, b: f64| {
        a.is_finite() && b.is_finite()
            && (a - b).abs() <= 4. * f64::EPSILON * a.abs().max(b.abs()).max(1.)
    };
    axis(actual.width, expected.width) && axis(actual.height, expected.height)
}
#[track_caller]
fn caption(expected: &str) {
    let actual = native_ui::gate_caption_targets(0);
    assert!(
        actual.iter().any(|(text, _)| text == expected),
        "expected {expected:?}, got {actual:?}"
    );
}
fn target() -> String {
    native_ui::gate_targets(0)
        .into_iter()
        .find(|(_, kind)| *kind == "click")
        .unwrap()
        .0
}
fn pointer(h: &mut Harness, pos: Point, down: bool) {
    h.tick(Input {
        pointer: PointerInput {
            pos: Some(pos),
            buttons: if down {
                Buttons::PRIMARY
            } else {
                Buttons::default()
            },
            ..Default::default()
        },
        ..Default::default()
    });
}
fn routing(ui: &Ui, target: &str) -> String {
    let scene = ui.scene().unwrap();
    let surface = scene
        .surface(target)
        .map(|surface| (surface.frame, surface.clip, surface.disabled));
    let routed = scene
        .surfaces()
        .filter_map(|surface| {
            let response = ui.get(surface.key.as_str());
            (response.hovered || response.pressed || response.held || response.released)
                .then(|| (surface.key.as_str(), response))
        })
        .take(8)
        .collect::<Vec<_>>();
    let rack = ["rack-view", "part-0", "body-0", "stage-0", "face-0", "rack-empty"]
        .map(|id| (id, scene.surface(id).map(|s| (s.frame, s.clip))));
    format!("pointer={:?}, target={surface:?}, routed={routed:?}, rack={rack:?}", ui.pointer())
}
#[track_caller]
fn click(p: &SamplerParams, core: &mut V2Core, h: &mut Harness) {
    let id = target();
    let pos = super::super::tests::center(&h.ui, &id);
    // Move onto the currently displayed target before pressing. This uses
    // the real hit map, including clipping and any layout since resource
    // completion; a frame's bounding-box center alone is not a hit proof.
    pointer(h, pos, false);
    assert_eq!(target(), id, "hover must preserve the current native owner");
    assert!(h.ui.get(id.as_str()).hovered, "native target must receive the hover: {}", routing(&h.ui, &id));
    pointer(h, pos, true);
    let pressed = h.ui.get(id.as_str());
    assert!(pressed.pressed && pressed.held, "native target must capture the press: {}", routing(&h.ui, &id));
    pointer(h, pos, false);
    assert!(h.ui.get(id.as_str()).released, "native target must receive the release: {}", routing(&h.ui, &id));
    for _ in 0..5 {
        audio(p, core);
        h.idle(1);
    }
}
fn counts(package: &Package) -> (usize, usize, usize, usize) {
    let scan = package.images.scan();
    (scan.lookups, scan.encoded_bytes, scan.decodes, scan.fonts)
}
fn rendered_geometry(ui: &Ui, root: &str) -> Vec<[f64; 4]> {
    let scene = ui.scene().unwrap();
    assert!(
        scene.surface(root).is_some(),
        "current native target must be rendered"
    );
    let origin = scene.surface(root).unwrap().frame;
    let limit = scene.surfaces().count();
    scene
        .surfaces()
        .filter(|surface| {
            // Pure children have anonymous tree-path keys. Their nearest named
            // ancestor, rather than a string prefix, proves subtree membership.
            let mut ancestor = Some(&surface.key);
            for _ in 0..=limit {
                let Some(key) = ancestor else {
                    return false;
                };
                if key.as_str() == root {
                    return true;
                }
                ancestor = scene
                    .surface(key.as_str())
                    .and_then(|parent| parent.parent.as_ref());
            }
            panic!("rendered parent cycle");
        })
        .map(|surface| {
            let frame = surface.frame;
            [frame.x - origin.x, frame.y - origin.y, frame.size.width, frame.size.height]
        })
        .collect()
}
fn rendered_descendants(ui: &Ui, root: &str) -> usize {
    rendered_geometry(ui, root).len()
}
fn fitting_room(ui: &Ui) -> Size {
    Size::new(ui.scene().unwrap().surface("face-0").unwrap().frame.size.width, room_height(ui, 0, false))
}
fn assert_incoming_root_size(ui: &Ui, room: Size, authored: Size, setting: f32) {
    let scale = scale_to_fit(room, authored, setting);
    let actual = ui.scene().unwrap().surface(&target()).unwrap().frame.size;
    let expected = Size::new(authored.width * scale, authored.height * scale);
    assert!(fitted_size_matches(actual, expected),
        "the first paint must fit incoming bounds, not a default or previous graph: {actual:?} vs {expected:?}");
}

#[test]
fn full_generation_a_b_a_reuses_native_resources_with_fresh_sessions_and_pointer_callbacks() {
    let f = fixture();
    let mut params = SamplerParams::new();
    params.shared.ui_activity = Arc::new(Activity::new(true));
    let p = Arc::new(params);
    p.shared.view.lock().unwrap().scanned = p.shared.libraries.wanted();
    p.selection
        .write()
        .unwrap()
        .parts
        .push(crate::plugin::Part {
            path: f.path.to_string_lossy().into(),
            view: 1,
            ..Default::default()
        });
    let mut core = V2Core::with_parts(1, 48000.);
    native_ui::gate_clear();
    let a_epoch = load(&p, &mut core, &f.a);
    let mut h = Harness::new(&p, 1180., 780.);
    let (a_session, package) = ready(&p, &mut core, &mut h, Size::new(200., 88.));
    let authority = p.shared.view.lock().unwrap().parts[0]
        .resource_revision
        .clone();
    let io = counts(&package);
    assert_eq!(
        (io.0, io.2, io.3),
        (3, 1, 1),
        "source, font and image read once"
    );
    let font = package.font("fixture.ttf", false).unwrap().id();
    let warm_image = package.images.get("photo.png").unwrap();
    let a_nodes = rendered_descendants(&h.ui, &target());
    let a_geometry = rendered_geometry(&h.ui, &target());
    let setting = p.shared.libraries.settings().view_scale;
    assert_eq!(setting, 0., "this authored case must exercise automatic fitting");
    caption("value=200 hits=0 local=0");
    click(&p, &mut core, &mut h);
    caption("value=200 hits=1 local=1");
    let old_target = target();
    let old_pos = super::super::tests::center(&h.ui, &old_target);
    let old_widget = p.shared.view.lock().unwrap().parts[0].interfaces[0]
        .widgets
        .iter()
        .find(|w| w.name == "$action")
        .unwrap()
        .clone();
    pointer(&mut h, old_pos, true);

    let b_epoch = load(&p, &mut core, &f.b);
    assert!(b_epoch > a_epoch, "mode0 must replace the runtime");
    assert_eq!(
        p.shared.view.lock().unwrap().parts[0].resource_revision,
        authority
    );
    assert!(
        !p.shared
            .set_widget_at(0, a_epoch, 0, &old_widget, None, ir::Value::Integer(0)),
        "old control generation is refused"
    );
    let b_room = fitting_room(&h.ui);
    pointer(&mut h, old_pos, false);
    let b_first_geometry = rendered_geometry(&h.ui, &target());
    assert_incoming_root_size(&h.ui, b_room, Size::new(260., 112.), setting);
    let (b_session, b_package) = native_ui::gate_package(0)
        .expect("warm replacement paints immediately, without loading caption frames");
    assert_ne!(a_session, b_session, "new Session/Bridge/gesture namespace");
    assert!(
        Arc::ptr_eq(&package, &b_package),
        "complete immutable package survives full replacement"
    );
    assert!(h.ui.scene().unwrap().surface(&old_target).is_none());
    ready(&p, &mut core, &mut h, Size::new(260., 112.));
    caption("value=800 hits=0 local=0");
    assert_eq!(rendered_geometry(&h.ui, &target()), b_first_geometry,
        "every rendered native descendant must already have its settled geometry on first B paint");
    let b_nodes = rendered_descendants(&h.ui, &target());
    assert!(
        b_nodes > a_nodes,
        "the incoming graph's additional child is actually lowered"
    );
    assert_eq!(
        counts(&b_package),
        io,
        "no source/font/image lookup or decode repeated"
    );
    assert_eq!(b_package.font("fixture.ttf", false).unwrap().id(), font);
    assert!(Arc::ptr_eq(
        &warm_image,
        &b_package.images.get("photo.png").unwrap()
    ));
    click(&p, &mut core, &mut h);
    caption("value=800 hits=1 local=1");

    let again = load(&p, &mut core, &f.a);
    assert!(again > b_epoch);
    let a_room = fitting_room(&h.ui);
    h.idle(1);
    let a_first_geometry = rendered_geometry(&h.ui, &target());
    assert_incoming_root_size(&h.ui, a_room, Size::new(200., 88.), setting);
    let (again_session, again_package) = native_ui::gate_package(0).unwrap();
    assert_ne!(again_session, b_session);
    assert!(Arc::ptr_eq(&package, &again_package));
    ready(&p, &mut core, &mut h, Size::new(200., 88.));
    caption("value=200 hits=0 local=0");
    assert_eq!(rendered_geometry(&h.ui, &target()), a_first_geometry,
        "complete first return-A geometry equals its settled frame");
    assert_eq!(a_first_geometry, a_geometry,
        "the complete original A layout returns without borrowing B's dimensions");
    assert_eq!(counts(&again_package), io);
    click(&p, &mut core, &mut h);
    caption("value=200 hits=1 local=1");
    let activity = p.shared.ui_activity.snapshot().unwrap();
    assert_eq!(
        activity[Count::FaceNew as usize],
        3,
        "all three full generations reached production Face refresh"
    );

    // Equal filenames do not authorize retention after a namespace revision.
    // Keep every old Arc alive so allocator identity reuse cannot satisfy this.
    image(
        &f.path
            .parent()
            .unwrap()
            .join("Resources/native_ui/photo.png"),
        [230, 40, 20, 255],
    );
    load(&p, &mut core, &f.b);
    let (_, changed) = ready(&p, &mut core, &mut h, Size::new(260., 112.));
    assert!(!Arc::ptr_eq(&package, &changed));
    assert_ne!(changed.font("fixture.ttf", false).unwrap().id(), font);
    let changed_image = changed.images.get("photo.png").unwrap();
    assert!(!Arc::ptr_eq(&warm_image, &changed_image));
    assert_eq!(&changed_image.rgba[..4], &[230, 40, 20, 255]);
    assert_eq!(counts(&changed).0, 3);
    assert_eq!((counts(&changed).2, counts(&changed).3), (1, 1));
    caption("value=800 hits=0 local=0");
}

#[test]
fn pending_program_selection_keeps_the_committed_native_resource_owner() {
    let f = fixture();
    let mut params = SamplerParams::new();
    params.shared.ui_activity = Arc::new(Activity::new(true));
    let p = Arc::new(params);
    p.shared.view.lock().unwrap().scanned = p.shared.libraries.wanted();
    p.selection
        .write()
        .unwrap()
        .parts
        .push(crate::plugin::Part {
            path: f.path.to_string_lossy().into(),
            view: 1,
            ..Default::default()
        });
    let mut core = V2Core::with_parts(1, 48000.);
    native_ui::gate_clear();
    let epoch = load(&p, &mut core, &f.a);
    let mut h = Harness::new(&p, 1180., 780.);
    let (session, package) = ready(&p, &mut core, &mut h, Size::new(200., 88.));
    let before = p.shared.ui_activity.snapshot().unwrap()[Count::FaceNew as usize];
    let io = counts(&package);

    // A private load has changed the request, not the published program/epoch.
    // Program 1 is deliberately absent from this authored single-program NKI.
    p.selection.write().unwrap().parts[0].program = 1;
    p.shared.view.lock().unwrap().parts[0].loading = true;
    h.idle(3);
    let (current, retained) = native_ui::gate_package(0).unwrap();
    assert_eq!(current, session);
    assert!(Arc::ptr_eq(&package, &retained));
    assert_eq!(core.epoch(0), epoch);
    assert_eq!(
        p.shared.ui_activity.snapshot().unwrap()[Count::FaceNew as usize],
        before
    );
    assert_eq!(counts(&retained), io);
    caption("value=200 hits=0 local=0");
    click(&p, &mut core, &mut h);
    caption("value=200 hits=1 local=1");
    p.selection.write().unwrap().parts[0].program = 0;
    p.shared.view.lock().unwrap().parts[0].loading = false;
    h.idle(1);
    assert_eq!(native_ui::gate_package(0).unwrap().0, session);
    assert_eq!(
        p.shared.ui_activity.snapshot().unwrap()[Count::FaceNew as usize],
        before
    );
}

fn face(f: &Fixture, generation: u64, entry: &str) -> Face {
    let mut interface = ir::Interface {
        source: ir::Source::Ksp { slot: 0 },
        ..Default::default()
    };
    interface.pages.push(ir::Page {
        size: ir::Size {
            width: 220.,
            height: 96.,
        },
        ..Default::default()
    });
    interface.native_ui = Some(ir::NativeUi {
        entry: entry.into(),
    });
    Face::new_for_program(
        &f.path,
        generation,
        vec![interface].into(),
        0,
        Presentation::Bitmap,
        0,
    )
}

#[test]
fn incomplete_or_failed_native_state_cannot_supply_a_reusable_package() {
    native_ui::gate_clear();
    let f = fixture();
    let pending = face(&f, 1, "main");
    assert!(
        pending
            .native
            .as_ref()
            .unwrap()
            .reusable_package()
            .is_none(),
        "unreceived source/font preparation is not reusable"
    );
    let package = Arc::new(Package::load(&f.path).unwrap());
    let mut bad = native_ui::State::with_package(&f.path, 0, "absent", vec![], Some(package));
    let mut ui = super::super::theme::ui();
    let _ = bad.view(
        &mut ui,
        0,
        1.,
        &ir::Interface::default(),
        &Default::default(),
        &Default::default(),
    );
    assert!(
        bad.reusable_package().is_none(),
        "failed fresh session never authorizes resource reuse"
    );
    native_ui::gate_clear();
}

#[test]
fn changed_missing_or_foreign_resource_authority_never_reuses_native_package() {
    let f = fixture();
    let resources = sampler_kontakt::Resources::for_program(&f.path, 0).unwrap();
    let revision = Arc::new(
        crate::sound::resource_revision::ResourceRevision::capture(&f.path, 0, &resources).unwrap(),
    );
    let mut old = face(&f, 1, "main");
    old.resource_revision = Some(revision.clone());
    assert!(old.shares_resources(&f.path, 0, &Some(revision.clone())));
    assert!(!old.shares_resources(&f.path, 0, &None));
    assert!(!old.shares_resources(&f.path, 1, &Some(revision.clone())));
    assert!(!old.shares_resources(
        &f.path.with_file_name("other.nki"),
        0,
        &Some(revision.clone())
    ));
    let picture = f
        .path
        .parent()
        .unwrap()
        .join("Resources/native_ui/photo.png");
    image(&picture, [230, 40, 20, 255]);
    let resources = sampler_kontakt::Resources::for_program(&f.path, 0).unwrap();
    let changed = Arc::new(
        crate::sound::resource_revision::ResourceRevision::capture(&f.path, 0, &resources).unwrap(),
    );
    assert_ne!(revision, changed);
    assert!(!old.shares_resources(&f.path, 0, &Some(changed)));
    std::fs::remove_file(picture).unwrap();
    assert!(
        crate::sound::resource_revision::ResourceRevision::capture(&f.path, 0, &resources)
            .is_none()
    );
}
