use super::*;

#[test]
fn a_transition_lands_between_the_two_fills_and_settles() {
    let mut ui = Ui::default();
    let tree = |on: bool| {
        block(40., 40.)
            .fill(if on { Role::Primary } else { Role::Field })
            .animate()
            .id("b")
    };
    let from = solid(
        &ui.frame(tree(false), None, Input::default(), 0.016)
            .unwrap(),
    );
    let snapshot = ui.scene_snapshot().unwrap();
    let mid = solid(&ui.frame(tree(true), None, Input::default(), 0.016).unwrap());
    assert_ne!(mid, from, "it left the old fill");
    let mut t = 0.0;
    let to = loop {
        let f = ui.frame(tree(true), None, Input::default(), 0.016).unwrap();
        t += 0.016;
        let paint = solid(&f);
        assert!(t < 2.0, "never settled");
        if !f.animating {
            break paint;
        }
    };
    assert_ne!(mid, to, "and the middle was not the end");
    let mut fresh = Ui::default();
    let want = solid(
        &fresh
            .frame(tree(true), None, Input::default(), 0.016)
            .unwrap(),
    );
    assert_eq!(to, want, "it settles on the declared fill");
    assert_eq!(
        snapshot.paint[0].paint, from,
        "the retained frame stays at rest"
    );
}

#[test]
fn a_transitioning_weld_morph_springs_between_its_declarations() {
    let tree = |p: f64| {
        mui_scene::weld![Weld::default().morph(p); block(20., 20.), block(20., 20.)]
            .animate_with(Spring::new(0.3, 0.6))
            .id("w")
    };
    let (pal, mut motion) = (Theme::DEFAULT.palette, None);
    let mut step = |p: f64| {
        let mut n = tree(p);
        let moving = transitions(&mut n, &mut motion, &pal, 0.016, false);
        (n.payload().extras().welding.unwrap().progress, moving)
    };
    assert_eq!(step(1.0), (1.0, false), "seeded, not flown in");
    let (mid, moving) = step(0.0);
    assert!(moving && 0.0 < mid && mid < 1.0, "{mid}");
    for _ in 0..200 {
        let (p, moving) = step(0.0);
        assert!((0.0..=1.0).contains(&p), "out of range: {p}");
        if !moving {
            assert_eq!(p, 0.0);
            return;
        }
    }
    panic!("never settled");
}

#[test]
fn retargeting_mid_flight_does_not_jump() {
    let mut ui = Ui::default();
    let tree = |on: bool| {
        block(40., 40.)
            .fill(if on { Role::Primary } else { Role::Field })
            .animate()
            .id("b")
    };
    let home = solid(
        &ui.frame(tree(false), None, Input::default(), 0.016)
            .unwrap(),
    );
    for _ in 0..3 {
        ui.frame(tree(true), None, Input::default(), 0.016).unwrap();
    }
    let before = solid(&ui.frame(tree(true), None, Input::default(), 0.016).unwrap());
    let after = solid(
        &ui.frame(tree(false), None, Input::default(), 0.016)
            .unwrap(),
    );
    assert_ne!(after, home, "a retarget carries velocity, it does not snap");
    assert_ne!(after, before, "and it keeps moving");
}

#[test]
fn a_tween_walks_to_its_target_and_never_back() {
    let mut ui = Ui::default();
    assert_eq!(ui.tween("cutoff", 0.0), 0.0, "it starts where it is told");
    let mut prev = 0.0;
    for _ in 0..180 {
        ui.frame(block(1., 1.), None, Input::default(), 0.016)
            .unwrap();
        let v = ui.tween("cutoff", 1.0);
        assert!(v >= prev, "went backwards: {v} after {prev}");
        assert!(v <= 1.0 + 1e-9, "overshot to {v}");
        prev = v;
    }
    assert!(prev > 0.99, "arrived: {prev}");
}

#[test]
fn a_bouncy_transition_never_undershoots_a_channel_below_zero() {
    let mut ui = Ui::default();
    let tree = |r: f64| {
        block(40., 40.)
            .fill(Role::Raised)
            .radius(r)
            .stroke(Role::Primary)
            .stroke_width(r / 4.)
            .animate_with(Spring::new(0.3, 0.6))
            .id("card")
    };
    ui.frame(tree(24.), None, PointerInput::default(), 0.016)
        .unwrap();
    for i in 0..120 {
        ui.frame(tree(0.), None, PointerInput::default(), 0.016)
            .unwrap_or_else(|e| panic!("frame {i} failed: {e:?}"));
    }
}

#[test]
fn a_transitioning_node_seeds_each_channel_from_its_own_declaration() {
    let mut ui = Ui::default();
    let stroked = || {
        block(40., 40.)
            .stroke(Role::Primary)
            .stroke_width(12.)
            .animate_with(Spring::DEFAULT)
            .id("n")
    };
    ui.frame(stroked(), None, PointerInput::default(), 0.016)
        .unwrap();
    let f = ui
        .frame(
            stroked().fill(Role::Primary),
            None,
            PointerInput::default(),
            0.016,
        )
        .unwrap();
    let Some(Paint::Solid(c)) = f.scene.paint.iter().find_map(|p| match &p.paint {
        Paint::Solid(c) => Some(Paint::Solid(*c)),
        _ => None,
    }) else {
        panic!("expected a solid fill");
    };
    assert!(
        c.lightness() <= 1.0 && c.alpha() <= 1.0,
        "fill sprang in from the stroke width: {c:?}"
    );
}

/// `.animate()` and `.on(..)` need no id: the tree path keys them.
#[test]
fn an_unnamed_node_transitions_and_takes_its_declared_states() {
    let mut ui = Ui::default();
    let tree = |on: bool| {
        row![
            block(40., 40.)
                .fill(if on { Role::Primary } else { Role::Field })
                .animate()
        ]
    };
    let from = solid(
        &ui.frame(tree(false), None, Input::default(), 0.016)
            .unwrap(),
    );
    let f = ui.frame(tree(true), None, Input::default(), 0.016).unwrap();
    assert!(f.animating, "it springs");
    let mid = solid(&f);
    let mut fresh = Ui::default();
    let to = solid(
        &fresh
            .frame(tree(true), None, Input::default(), 0.016)
            .unwrap(),
    );
    assert!(mid != from && mid != to, "between the two fills");

    let off = || {
        row![
            block(40., 40.)
                .fill(Role::Primary)
                .on(State::Disabled, |s| s.fill(Role::Field))
                .disabled()
        ]
    };
    let f = ui.frame(off(), None, Input::default(), 0.016).unwrap();
    assert_eq!(solid(&f), to_paint(Role::Field));
}
/// The corner radius the one rounded box in `f` paints with.
fn corner(f: &Frame) -> f64 {
    f.scene
        .paint
        .iter()
        .find_map(|p| p.rect.map(mui_geometry::RoundedRect::radius))
        .expect("a rounded rect")
}

/// `.on(State::Hover | State::Press)` needs no id: a node that declares
/// one is a pointer target by its tree path.
#[test]
fn an_unnamed_node_takes_its_hover_and_press_looks() {
    let tree = || {
        row![
            block(40., 40.)
                .fill(Role::Raised)
                .on(State::Hover, |s| s.radius(3.))
                .on(State::Press, |s| s.radius(5.))
        ]
    };
    let mut ui = Ui::default();
    let cold = corner(&ui.frame(tree(), None, Input::default(), 0.016).unwrap());
    let mut look = cold;
    for _ in 0..12 {
        look = corner(&ui.frame(tree(), None, at(10., 10., false), 0.016).unwrap());
    }
    assert!(
        cold != 3. && look == 3.,
        "hovered by its path: {cold} -> {look}"
    );
    for _ in 0..12 {
        look = corner(&ui.frame(tree(), None, at(10., 10., true), 0.016).unwrap());
    }
    assert_eq!(look, 5., "and pressed");
    assert_eq!(ui.interaction.held(), Some("/0"));
}

/// A declared hover or press look lands on the frame the pointer arrives
/// and leaves on the frame it goes: it follows the state, not the hover
/// spring's rise, so a menu row does not light up frames behind the pointer.
#[test]
fn a_declared_look_follows_the_pointer_on_the_same_frame() {
    let tree = || {
        row![
            block(40., 40.)
                .fill(Role::Raised)
                .on(State::Hover, |s| s.radius(3.))
                .on(State::Press, |s| s.radius(5.)),
            block(40., 40.)
        ]
    };
    let mut ui = Ui::default();
    let look =
        |ui: &mut Ui, input: PointerInput| corner(&ui.frame(tree(), None, input, 0.016).unwrap());
    assert_ne!(look(&mut ui, PointerInput::default()), 3.);
    assert_eq!(look(&mut ui, at(10., 10., false)), 3., "hovered on arrival");
    assert_eq!(
        look(&mut ui, at(10., 10., true)),
        5.,
        "pressed on the press"
    );
    assert_eq!(look(&mut ui, at(10., 10., false)), 3., "released");
    assert_ne!(
        look(&mut ui, at(60., 10., false)),
        3.,
        "gone when it leaves"
    );
}

/// Which of `top` and `under`, stacked, a press on both takes -- and
/// whether a named field focused beforehand keeps the focus.
fn press_stack(under: &El, top: &El) -> (Option<String>, bool) {
    let tree = |under: &El, top: &El| {
        col![
            stack([under.clone(), top.clone()]),
            block(40., 20.).focusable().id("field"),
        ]
    };
    let mut ui = Ui::default();
    ui.frame(tree(under, top), None, Input::default(), 0.016)
        .unwrap();
    ui.focus("field");
    for down in [false, true] {
        ui.frame(tree(under, top), None, at(10., 10., down), 0.016)
            .unwrap();
    }
    (
        ui.interaction.held().map(str::to_owned),
        ui.focused("field"),
    )
}

/// An unnamed node with a pointer look routes exactly as the same node
/// with an id: over a named control it takes the press, under one it
/// does not. Plain decoration stays transparent to the pointer.
#[test]
fn an_unnamed_stateful_node_routes_like_a_named_one_and_decoration_not_at_all() {
    let control = || block(40., 40.).a11y(A11y::Button).id("c");
    let lit = || block(40., 40.).on(State::Hover, |s| s.radius(3.));
    let plain = || block(40., 40.).fill(Role::Raised);

    // Over the control.
    assert_eq!(
        press_stack(&control(), &lit()),
        (Some("/0/1".into()), false)
    );
    assert_eq!(
        press_stack(&control(), &lit().id("top")),
        (Some("top".into()), false)
    );
    // Under it.
    assert_eq!(press_stack(&lit(), &control()), (Some("c".into()), false));
    assert_eq!(
        press_stack(&lit().id("under"), &control()),
        (Some("c".into()), false)
    );
    // Decoration over the control is not there as far as the pointer is
    // concerned; with an id it would be.
    assert_eq!(press_stack(&control(), &plain()), (Some("c".into()), false));
    assert_eq!(
        press_stack(&control(), &plain().id("top")),
        (Some("top".into()), false)
    );
}

#[test]
fn reduced_motion_resolves_interaction_layout_paint_and_exits_without_idle_wakeups() {
    let mut ui = Ui::default();
    let tree = |changed: bool| {
        block(if changed { 80. } else { 40. }, 40.)
            .fill(if changed { Role::Primary } else { Role::Field })
            .radius(if changed { 15. } else { 2. })
            .on(State::Hover, |s| s.stroke(Role::Primary))
            .on(State::Press, |s| s.stroke_width(3.))
            .animate()
            .animate_layout()
            .appear(Appear::Scale(0.5))
            .morph(changed)
            .a11y(A11y::Button)
            .id("b")
    };
    ui.frame(tree(false), None, Input::default(), 0.016)
        .unwrap();
    assert!(
        ui.frame(tree(true), None, at(20., 20., true), 0.016)
            .unwrap()
            .animating
    );
    assert!(ui.set_motion_policy(MotionPolicy::Reduced));
    let f = ui.frame(tree(true), None, at(20., 20., true), 0.).unwrap();
    assert_eq!(f.scene.surface("b").unwrap().frame.size.width, 80.);
    assert_eq!(corner(&f), 15.);
    assert!(
        !f.animating,
        "no animation after reducing motion mid-flight"
    );
    assert_eq!(
        ui.state("b"),
        Interaction {
            hover: 1.,
            press: 1.
        }
    );
    assert!(
        ui.frame(tree(true), None, at(25., 20., true), 0.)
            .unwrap()
            .animating,
        "a moved drag sample still owes the tree its response"
    );
    for _ in 0..3 {
        let f = ui
            .frame(tree(true), None, at(25., 20., true), 0.016)
            .unwrap();
        assert!(!f.animating);
        assert_eq!(f.repaint_after, None);
    }
    ui.frame(block(1., 1.), None, Input::default(), 0.).unwrap();
    assert!(
        ui.ghosts.is_empty(),
        "exits do not fade under reduced motion"
    );
    assert!(
        !ui.frame(block(1., 1.), None, Input::default(), 0.)
            .unwrap()
            .animating
    );
}

#[test]
fn motion_preference_changes_invalidate_memos_only_when_effective_policy_changes() {
    let mut ui = Ui::default();
    let mut builds = 0;
    let mut build = |ui: &mut Ui| {
        ui.memo("panel", (), |_| {
            builds += 1;
            block(20., 20.).fill(Role::Primary).animate()
        })
    };
    for _ in 0..2 {
        let tree = build(&mut ui);
        ui.frame(tree, None, Input::default(), 0.).unwrap();
    }
    assert!(ui.set_system_reduced_motion(true));
    let tree = build(&mut ui);
    ui.frame(tree, None, Input::default(), 0.).unwrap();
    assert!(!ui.set_system_reduced_motion(true));
    assert!(!ui.set_motion_policy(MotionPolicy::Reduced));
    assert!(
        !ui.set_system_reduced_motion(false),
        "override masks OS changes"
    );
    let tree = build(&mut ui);
    ui.frame(tree, None, Input::default(), 0.).unwrap();
    assert!(ui.set_motion_policy(MotionPolicy::System));
    let tree = build(&mut ui);
    ui.frame(tree, None, Input::default(), 0.).unwrap();
    assert_eq!(builds, 3, "one rebuild per effective preference change");
}

#[test]
fn reduced_ui_tweens_and_plays_snap_but_explicit_timeline_samples_keep_their_time() {
    let keys = Keys::new(0.).to(0.5, 1., Ease::Spring(Spring::new(0.4, 0.3)));
    let sampled = keys.at(0.2);
    assert_ne!(sampled, keys.target());
    let mut ui = Ui::default();
    ui.tween("gain", 0.);
    ui.play("intro", &keys);
    assert!(ui.set_motion_policy(MotionPolicy::Reduced));
    assert_eq!(ui.tween("gain", 1.), 1.);
    assert_eq!(ui.play("intro", &keys), 1.);
    assert!(
        !ui.frame(block(1., 1.), None, Input::default(), 0.)
            .unwrap()
            .animating
    );
    assert_eq!(
        keys.at(0.2),
        sampled,
        "explicit sampling does not use UI policy"
    );
    assert!(ui.set_motion_policy(MotionPolicy::Full));
    assert_eq!(ui.tween("gain", 1.), 1.);
    assert_eq!(
        ui.play("intro", &keys),
        1.,
        "completed entrances do not restart"
    );
    assert!(
        !ui.frame(block(1., 1.), None, Input::default(), 0.)
            .unwrap()
            .animating
    );
    ui.tween("gain", 0.);
    assert!(
        ui.frame(block(1., 1.), None, Input::default(), 0.016)
            .unwrap()
            .animating,
        "reenabling motion preserves spring tuning"
    );
}

#[test]
fn ui_plays_draw_the_endpoint_then_sleep_and_removed_plays_stop_requesting_frames() {
    let keys = Keys::new(0.).to(0.5, 1., Ease::Spring(Spring::new(0.4, 0.3)));
    let mut ui = Ui::default();
    let mut settled = false;
    for _ in 0..100 {
        let drawn = ui.play("intro", &keys);
        let f = ui
            .frame(block(1., 1.), None, Input::default(), 0.016)
            .unwrap();
        if !f.animating {
            assert_eq!(drawn, 1., "the final UI frame draws the endpoint");
            settled = true;
            break;
        }
    }
    assert!(settled, "play never stopped scheduling");
    ui.replay("intro");
    ui.play("intro", &keys);
    assert!(
        ui.frame(block(1., 1.), None, Input::default(), 0.016)
            .unwrap()
            .animating
    );
    assert!(
        !ui.frame(block(1., 1.), None, Input::default(), 0.016)
            .unwrap()
            .animating,
        "a play absent from the next build no longer schedules animation"
    );
}
