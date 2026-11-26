//! A drag on a real library page, frame by frame, as the editor runs it:
//! the snapshot fill, the view build, `Ui::frame`, and the rack tick that
//! carries the move to the script and copies its answer back. Printed, not
//! asserted (`--ignored --nocapture`).

use std::path::PathBuf;
use std::time::{Duration, Instant};

use kontra_host::state::{Saved, SavedPart};
use kontra_ui_model::Kind;
use moose::mui::mui::prelude::{Buttons, Input, Point, PointerInput, Size as ViewSize};

use super::*;
use crate::Tick;
use crate::tests::{activate, block, settle};

fn part(path: PathBuf) -> SavedPart {
    SavedPart {
        part: 0,
        path,
        channel: 0,
        output: 0,
        gain: 1.0,
        pan: 0.0,
        solo: false,
        mute: false,
        scripts: Vec::new(),
        delays: None,
        slot_types: Vec::new(),
        slot_responses: Vec::new(),
        preset: None,
        modified: false,
    }
}

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1e3
}

fn probe(name: &str, path: PathBuf, browser_hidden: bool) {
    if !path.ancestors().any(Path::is_file) {
        eprintln!("skipped: no {}", path.display());
        return;
    }
    let (params, mut dsp) = activate(&Saved::new(vec![part(path)]).write());
    // The player's own browser, as a real session lists it.
    *lock(&params.shared.browser) = Some(Browser::open(
        dirs::config_dir().as_deref(),
        dirs::cache_dir().as_deref(),
    ));
    params.shared.open_editor();
    settle(&params, &mut dsp);
    let (mut app, mut shown, mut mine) = (App::default(), Shown::default(), Mine::default());
    let mut ui = kontra_ui::ui();
    let size = ViewSize::new(f64::from(SIZE.0), f64::from(SIZE.1));
    let frame = |ui: &mut moose::mui::mui::Ui,
                 app: &mut App,
                 shown: &mut Shown,
                 mine: &mut Mine,
                 pointer: PointerInput| {
        let t0 = Instant::now();
        let load = {
            let view = lock(&params.shared.view);
            for (part, &slot) in app.parts.iter().zip(&shown.slots) {
                if part.face.as_ref().is_some_and(|f| f.gesture.held())
                    && let Some(&(generation, ..)) = shown.parts.get(&slot)
                {
                    mine.keep(slot, generation);
                }
            }
            let now = Instant::now();
            let released = mine.prune(&view.parts, view.applied, now);
            view.fill_with(app, shown, mine, &released, None);
            view.parts.first().map_or(0, |p| p.generation)
        };
        let t1 = Instant::now();
        // As the editor does: the build sees the input it answers.
        app.view.pointer = pointer.pos;
        let r = kontra_ui::app::view(ui, app);
        let t2 = Instant::now();
        let input = Input {
            pointer,
            ..Default::default()
        };
        ui.frame(r.el, Some(size), input, 1. / 60.)
            .expect("laid out");
        let t3 = Instant::now();
        let mut sent = 0;
        for action in r.changed {
            if matches!(action, Action::Control { .. }) {
                let seq = params.shared.request_ui(Edit {
                    slot: Some(0),
                    action: action.clone(),
                    seq: 0,
                });
                mine.note(0, load, seq, &action);
                sent += 1;
            }
        }
        (t1 - t0, t2 - t1, t3 - t2, ui.layout_stats(), sent)
    };
    app.view.browser_hidden = browser_hidden;
    let idle = PointerInput::default();
    for _ in 0..3 {
        frame(&mut ui, &mut app, &mut shown, &mut mine, idle);
    }
    // The first shown script knob or slider on the page.
    let found = (app.parts.first().and_then(|p| p.face.as_ref())).and_then(|f| {
        (f.tree.widgets.iter().enumerate()).find_map(|(n, w)| {
            let ranged = matches!(w.kind, Kind::Knob { .. } | Kind::Slider { .. });
            let id = kontra_ui::input::surface(0, n);
            let s = ui.scene()?.surface(&id)?;
            (ranged && w.source_id.is_some() && f.tree.visible(kontra_ui_model::WidgetRef(n)))
                .then(|| (n, w.name.clone(), s.frame))
        })
    });
    let Some((n, knob, at)) = found else {
        eprintln!("{name}: no knob on its page");
        return;
    };
    let widgets = app.parts[0]
        .face
        .as_ref()
        .map_or(0, |f| f.tree.widgets.len());
    let from = Point::new(at.x + at.size.width / 2., at.y + at.size.height / 2.);
    let held = |p: Point| PointerInput {
        pos: Some(p),
        buttons: Buttons::PRIMARY,
        ..Default::default()
    };
    frame(
        &mut ui,
        &mut app,
        &mut shown,
        &mut mine,
        PointerInput {
            pos: Some(from),
            ..Default::default()
        },
    );
    frame(&mut ui, &mut app, &mut shown, &mut mine, held(from));
    println!(
        "{name}: {knob} (widget {n} of {widgets}), {} libraries listed, browser {}",
        app.libraries.len(),
        if browser_hidden { "hidden" } else { "shown" }
    );
    println!(
        " move | fill ms | view ms | frame ms | measured | arranged | sent | tick ms | echo fill | echo view | echo frame | echo measured | shown"
    );
    let (mut sum_move, mut sum_echo) = (Duration::ZERO, Duration::ZERO);
    let moves = 20;
    let resolves_before = RESOLVES.load(Ordering::Relaxed);
    let built_before = kontra_ui::view::widgets_built();
    for k in 1..=moves {
        let p = Point::new(from.x, from.y - 3. * f64::from(k));
        let (fill, view, ui_frame, stats, sent) =
            frame(&mut ui, &mut app, &mut shown, &mut mine, held(p));
        let shown_now = |app: &App| {
            app.parts[0]
                .face
                .as_ref()
                .map_or(f64::NAN, |f| f.tree.widgets[n].number())
        };
        let t = Instant::now();
        Tick.run(&params);
        block(&params, &mut dsp, &[]);
        Tick.run(&params);
        let tick = t.elapsed();
        let (efill, eview, eframe, estats, _) =
            frame(&mut ui, &mut app, &mut shown, &mut mine, held(p));
        sum_move += fill + view + ui_frame;
        sum_echo += efill + eview + eframe;
        println!(
            " {k:4} | {:7.2} | {:7.2} | {:8.2} | {:8} | {:8} | {sent:4} | {:7.2} | {:9.2} | {:9.2} | {:10.2} | {:13} | {:.3}",
            ms(fill),
            ms(view),
            ms(ui_frame),
            stats.measured_nodes,
            stats.arranged_nodes,
            ms(tick),
            ms(efill),
            ms(eview),
            ms(eframe),
            estats.measured_nodes,
            shown_now(&app),
        );
    }
    let resolves_delta = RESOLVES.load(Ordering::Relaxed) - resolves_before;
    let built_delta = kontra_ui::view::widgets_built() - built_before;
    println!(
        "{name}: mean move frame {:.2} ms, mean echo frame {:.2} ms, full resolves={resolves_delta}, widgets_built={built_delta} ({:.1}/move)",
        ms(sum_move) / f64::from(moves),
        ms(sum_echo) / f64::from(moves),
        built_delta as f64 / f64::from(moves),
    );
    if let Some(tree) = lock(&params.shared.view).parts[0].tree.clone() {
        let t = Instant::now();
        let _ = resolved(&tree);
        println!(
            "{name}: one resolved() of the whole tree {:.2} ms",
            ms(t.elapsed())
        );
    }
}

#[test]
#[ignore = "real libraries; prints a drag's per-frame cost"]
fn a_drag_on_a_real_page_frame_by_frame() {
    for hidden in [false, true] {
        probe(
            "Analog Strings",
            PathBuf::from(crate::tests::KONTAKT)
                .join("ANALOG STRINGS/Instruments/ANALOG STRINGS.nki"),
            hidden,
        );
        probe(
            "Conflux",
            PathBuf::from(crate::tests::KONTAKT)
                .join("Conflux 1.1.0 [Native Instruments]/Instruments/Conflux.nki"),
            hidden,
        );
        probe(
            "VWinds",
            PathBuf::from("/data/main/Libraries/UVI")
                .join("VWinds - Clarinets/VWinds-BbClarinet.ufs/Presets/Bb Clarinet.uvip"),
            hidden,
        );
        probe(
            "Una Corda",
            PathBuf::from(crate::tests::KONTAKT)
                .join("Una Corda Library/Instruments/Una Corda Felt.nki"),
            hidden,
        );
    }
}
