use super::*;

/// A lead `lead` wide, then a memoised panel of two buttons, counting
/// its builds.
fn memo_tree(ui: &mut Ui, deps: u64, lead: f64, built: &mut usize) -> El {
    let panel = ui.memo("panel", deps, |_| {
        *built += 1;
        col([
            block(40., 40.)
                .fill(Role::Raised)
                .a11y(A11y::Button)
                .id("m.a"),
            block(40., 40.)
                .fill(Role::Raised)
                .a11y(A11y::Button)
                .id("m.b"),
        ])
        .id("m")
    });
    row([block(lead, 10.).fill(Role::Surface), panel]).align(Align::Start)
}
/// The same tree built plainly, for what a memo must paint like.
fn plain_tree(lead: f64) -> El {
    let mut t = memo_tree(&mut Ui::default(), 0, lead, &mut 0);
    t.children_mut()[1].payload_mut().extras_mut().memo = None;
    t
}
const ROOM: Option<Size> = Some(Size::new(300., 200.));

#[test]
fn a_ui_with_kept_memos_stays_send() {
    fn send<T: Send>() {}
    send::<Ui>();
}

#[test]
fn a_memo_builds_once_while_its_deps_hold_and_again_when_they_change() {
    let mut ui = Ui::default();
    let mut built = 0;
    for _ in 0..4 {
        let t = memo_tree(&mut ui, 1, 50., &mut built);
        ui.frame(t, ROOM, PointerInput::default(), 0.016).unwrap();
    }
    assert_eq!(built, 1);
    let t = memo_tree(&mut ui, 2, 50., &mut built);
    ui.frame(t, ROOM, PointerInput::default(), 0.016).unwrap();
    assert_eq!(built, 2, "new deps build it again");
    let t = memo_tree(&mut ui, 2, 50., &mut built);
    let f = ui.frame(t, ROOM, PointerInput::default(), 0.016).unwrap();
    assert_eq!(built, 2);
    assert!(
        f.scene.surface("m.b").is_some(),
        "the kept subtree is in the scene"
    );
}

/// Hover warms a button inside a memo on the same frames it would outside
/// one, and the memo rests again once the spring does.
#[test]
fn a_hovered_button_in_a_memo_is_restyled() {
    let (mut ui, mut plain) = (Ui::default(), Ui::default());
    let mut built = 0;
    let step = |ui: &mut Ui, plain: &mut Ui, p: PointerInput, built: &mut usize| {
        ui.anticipate(p.pos);
        let t = memo_tree(ui, 1, 50., built);
        let a = paint_color(&ui.frame(t, ROOM, p, 0.016).unwrap(), "m.a");
        let b = paint_color(
            &plain.frame(plain_tree(50.), ROOM, p, 0.016).unwrap(),
            "m.a",
        );
        (a, b)
    };
    let rest = step(&mut ui, &mut plain, PointerInput::default(), &mut built).0;
    step(&mut ui, &mut plain, PointerInput::default(), &mut built);
    // Told the pointer before the build, the memo warms on the frame the
    // hover lands, as a plain tree does.
    for _ in 0..60 {
        let (a, b) = step(&mut ui, &mut plain, at(70., 20., false), &mut built);
        assert_eq!(a, b, "the memo paints what a plain tree does");
    }
    let hot = step(&mut ui, &mut plain, at(70., 20., false), &mut built).0;
    assert_ne!(hot, rest, "hover warmed the fill");
    let settled = built;
    step(&mut ui, &mut plain, at(70., 20., false), &mut built);
    assert_eq!(built, settled, "a settled hover builds nothing");
    step(&mut ui, &mut plain, at(250., 150., false), &mut built);
    for _ in 0..60 {
        step(&mut ui, &mut plain, at(250., 150., false), &mut built);
    }
    assert_eq!(
        step(&mut ui, &mut plain, at(250., 150., false), &mut built).0,
        rest
    );
}

/// Pushed along by what precedes it, a kept memo is not built again, and
/// it paints and is hit where it now stands.
#[test]
fn a_moved_memo_paints_and_hits_where_it_went() {
    let mut ui = Ui::default();
    let mut built = 0;
    for _ in 0..2 {
        let t = memo_tree(&mut ui, 1, 50., &mut built);
        ui.frame(t, ROOM, PointerInput::default(), 0.016).unwrap();
    }
    let snapshot = ui.scene_snapshot().unwrap();
    let previous_paint = snapshot.paint.clone();
    let t = memo_tree(&mut ui, 1, 90., &mut built);
    let f = ui.frame(t, ROOM, PointerInput::default(), 0.016).unwrap();
    assert_eq!(built, 1);
    let walked =
        mui_scene::resolve(&SceneSpec::new(plain_tree(90.)).offered(Size::new(300., 200.)))
            .unwrap();
    assert_eq!(f.scene.paint, walked.paint);
    assert_eq!(f.scene.surface("m.b").unwrap().frame.x, 90.);
    assert_eq!(snapshot.surface("m.b").unwrap().frame.x, 50.);
    assert_eq!(snapshot.paint, previous_paint);
    let t = memo_tree(&mut ui, 1, 90., &mut built);
    ui.frame(t, ROOM, at(110., 60., false), 0.016).unwrap();
    assert!(
        ui.get("m.b").hovered,
        "the moved button is hit where it stands"
    );
}

/// An outer memo built again keeps the inner one it nests; the inner
/// one's hover rebuilds both, since the outer one holds it.
#[test]
fn nested_memos_rebuild_only_what_changed() {
    let mut ui = Ui::default();
    let frame = |ui: &mut Ui, deps: u64, p: PointerInput, n: &mut (usize, usize)| {
        let t = ui.memo("outer", deps, |ui| {
            n.0 += 1;
            let kid = ui.memo("inner", 0, |_| {
                n.1 += 1;
                block(40., 40.)
                    .fill(Role::Raised)
                    .a11y(A11y::Button)
                    .id("in.b")
            });
            col([block(40., 40.).fill(Role::Raised).id("out.a"), kid])
        });
        ui.frame(row([t]), ROOM, p, 0.016)
            .unwrap()
            .scene
            .paint
            .len()
    };
    let mut n = (0, 0);
    let full = frame(&mut ui, 1, PointerInput::default(), &mut n);
    assert_eq!(frame(&mut ui, 1, PointerInput::default(), &mut n), full);
    assert_eq!(n, (1, 1));
    assert_eq!(frame(&mut ui, 2, PointerInput::default(), &mut n), full);
    assert_eq!(
        n,
        (2, 1),
        "the outer memo rebuilt around the kept inner one"
    );
    assert_eq!(frame(&mut ui, 2, PointerInput::default(), &mut n), full);
    frame(&mut ui, 2, at(20., 60., false), &mut n);
    frame(&mut ui, 2, at(20., 60., false), &mut n);
    assert!(n.0 > 2 && n.1 > 1, "the inner hover built both again");
    assert!(ui.get("in.b").hovered);
}

/// A canvas's label shapes once: the same (font, size, text) hands back the
/// same run, and another size is its own.
#[test]
fn a_text_run_is_shaped_once_and_kept() {
    let font = Font::new(epaint_default_fonts::HACK_REGULAR).unwrap();
    let mut ui = Ui::default();
    let a = ui.text_run(&font, "gain", 12.).unwrap();
    let b = ui.text_run(&font, "gain", 12.).unwrap();
    assert!(Arc::ptr_eq(&a, &b), "the run was shaped again");
    let big = ui.text_run(&font, "gain", 24.).unwrap();
    assert!(big.advance > a.advance);
}

/// The cache keys on and shapes with the variation axes: an app that pins Martian
/// Mono's `wdth` below its default, and a cache that ignored it came out wide.
#[test]
fn text_runs_honour_axes() {
    let inter = Font::new(ttf_inter::REGULAR).unwrap();
    let runs = Ui::default().text_runs();
    let regular = runs.get(&inter, "SYNTH", 24., &[("wght", 400.)]).unwrap();
    let bold = runs.get(&inter, "SYNTH", 24., &[("wght", 700.)]).unwrap();
    assert!(bold.advance > regular.advance, "the wght axis was ignored");
    let again = runs.get(&inter, "SYNTH", 24., &[("wght", 400.)]).unwrap();
    assert!(Arc::ptr_eq(&regular, &again));
}

/// The path's baseline is y = 0 and its ink stays within ascent above and
/// descent below, so `.at(y + ascent)` places the top-left at y.
#[test]
fn text_run_baseline_is_ascent_below_the_top() {
    let font = Font::new(epaint_default_fonts::HACK_REGULAR).unwrap();
    let run = Ui::default()
        .text_runs()
        .get(&font, "Hg", 96., &[])
        .unwrap();
    let points = run.path.flatten(0.1, 250_000).unwrap().concat();
    let b = mui_geometry::bounds(points).unwrap();
    assert!(b.y0 < -0.5 * run.ascent && b.y0 >= -run.ascent, "{b:?}");
    assert!(b.y1 > 0. && b.y1 <= run.descent, "{b:?}");
}

/// A handle cloned into a canvas closure on another thread shares the cache,
/// and the cache stays bounded however many labels go through it.
#[test]
fn text_runs_handle_is_shared_and_bounded() {
    let font = Font::new(epaint_default_fonts::HACK_REGULAR).unwrap();
    let mut ui = Ui::default();
    let runs = ui.text_runs();
    let f = font.clone();
    let there = std::thread::spawn(move || runs.get(&f, "gain", 12., &[]).unwrap())
        .join()
        .unwrap();
    assert!(Arc::ptr_eq(
        &there,
        &ui.text_run(&font, "gain", 12.).unwrap()
    ));
    let runs = ui.text_runs();
    for i in 0..5000 {
        runs.get(&font, &i.to_string(), 12., &[]).unwrap();
        assert!(runs.len() <= 2048);
    }
}

#[test]
fn implicit_face_and_fallback_changes_rewalk_nested_paint_but_keep_memo_builds() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    fn label() -> El {
        text("Hi😀").text_size(18.).w(100.).h(32.).id("environment-label")
    }
    fn panel(walks: Arc<AtomicUsize>) -> El {
        let probe = canvas(move |_| {
            walks.fetch_add(1, Ordering::Relaxed);
            Vec::new()
        }).w(1.).h(1.);
        stack([label(), probe]).w(100.).h(32.)
    }
    fn plain(walks: Arc<AtomicUsize>) -> El {
        stack([panel(walks)]).w(100.).h(32.)
    }
    #[cfg(feature = "cpu")]
    fn pixels(ui: &Ui) -> Vec<u8> {
        use crate::vello::{self, vello_cpu::{Pixmap, RenderContext, Resources}};
        let mut ctx = RenderContext::new(100, 32);
        let mut resources = Resources::default();
        vello::paint(&mut vello::Cpu {
            ctx: &mut ctx, resources: &mut resources, cache: &mut vello::Cache::default(),
        }, ui.scene().unwrap(), vello::kurbo::Affine::IDENTITY).unwrap();
        ctx.flush();
        let mut pix = Pixmap::new(100, 32);
        ctx.render(&mut pix, &mut resources);
        pix.take_unpremultiplied().iter().flat_map(|p| [p.r, p.g, p.b, p.a]).collect()
    }
    let hack = Font::new(epaint_default_fonts::HACK_REGULAR).unwrap();
    let ubuntu = Font::new(epaint_default_fonts::UBUNTU_LIGHT).unwrap();
    let emoji = Font::new(epaint_default_fonts::NOTO_EMOJI_REGULAR).unwrap();
    let (mut retained, mut eager) = (Ui::default().font(hack.clone()), Ui::default().font(hack.clone()));
    let retained_walks = Arc::new(AtomicUsize::new(0));
    let eager_walks = Arc::new(AtomicUsize::new(0));
    let mut builds = (0, 0);
    #[cfg(feature = "cpu")]
    let mut before_face_swap = Vec::new();
    for step in 0..4 {
        if step == 1 {
            // The existing public fallback API appends; there is no remove/
            // reorder setter. Exercise a real selected fallback, not just ids.
            retained = retained.fallback_font(emoji.clone());
            eager = eager.fallback_font(emoji.clone());
        }
        for warm in 0..2 {
            let tree = retained.memo("environment-outer", 0, |ui| {
                builds.0 += 1;
                let child = ui.memo("environment-inner", 0, |_| {
                    builds.1 += 1;
                    panel(retained_walks.clone())
                });
                stack([child]).w(100.).h(32.)
            });
            // Also swap after the tree was built: heating closures in a setter
            // alone would be too late to reject this frame's old paint span.
            if warm == 0 && step >= 2 {
                let font = if step == 2 { &ubuntu } else { &hack };
                retained.set_font(Some(font.clone())); eager.set_font(Some(font.clone()));
            }
            let before = retained_walks.load(Ordering::Relaxed);
            let animating = retained.frame(tree, Some(Size::new(100., 32.)), Input::default(), 0.016).unwrap().animating;
            eager.frame(plain(eager_walks.clone()), Some(Size::new(100., 32.)), Input::default(), 0.016).unwrap();
            let context = format!("default face/fallback step={step}, warm={warm}");
            assert_eq!(retained_walks.load(Ordering::Relaxed) - before, usize::from(warm == 0),
                "{context}: each environment change walks paint once, then scene copying resumes");
            assert_eq!(eager_walks.load(Ordering::Relaxed), step * 2 + warm + 1, "{context}: eager paint walks every frame");
            assert_eq!(builds, (1, 1), "{context}: both closure levels stay retained");
            assert!(!animating, "{context}: no permanent environment wake");
            assert_eq!(retained.scene().unwrap().paint, eager.scene().unwrap().paint, "{context}: exact glyph/font paint");
            let frames = |ui: &Ui| ui.scene().unwrap().surfaces().map(|s| s.frame).collect::<Vec<_>>();
            assert_eq!(frames(&retained), frames(&eager), "{context}: all surface frames");
            let scene = retained.scene().unwrap();
            let run = scene.paint.iter().find(|p| p.key.as_str() == "environment-label").unwrap().text.as_ref().unwrap();
            assert_eq!(run.fonts[0].id(), if step == 2 { ubuntu.id() } else { hack.id() }, "{context}: current default face");
            assert!(!run.glyphs.is_empty(), "{context}: visible glyphs");
            if step > 0 {
                assert_eq!(run.fonts[1].id(), emoji.id(), "{context}: current fallback chain");
                assert!(run.glyphs.iter().any(|g| g.font == 1 && g.id != 0), "{context}: emoji really selects the fallback");
            }
            let spans = scene.memos_at("environment-label").collect::<Vec<_>>();
            assert_eq!(spans.len(), 2, "{context}: nested ancestors");
            assert!(spans.iter().all(|(_, reused)| *reused == (step > 0 || warm > 0)), "{context}: both memo trees stay retained");
            #[cfg(feature = "cpu")]
            {
                let actual = pixels(&retained);
                assert_eq!(actual, pixels(&eager), "{context}: strict CPU pixels");
                if step == 1 { before_face_swap = actual; }
                else if step == 2 { assert_ne!(actual, before_face_swap, "{context}: genuinely different face changes pixels"); }
            }
        }
    }
}
