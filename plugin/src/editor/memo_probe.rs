//! What the editor rebuilds on a real library: a preset swap, a snapshot
//! swap, folding a rack part and back, a tab switch and back, and closing
//! and reopening the editor. Counts picture decodes, library widgets built,
//! faces made and trees laid out, with the wall time until the face has
//! every picture again. Printed, not asserted (`--ignored --nocapture`).

use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use kontra_host::state::Saved;
use kontra_ui::app::PartTab;
use moose::mui::mui::prelude::{Input, Size as ViewSize};

use super::*;
use crate::tests::{activate, block, settle};

struct Counts {
    decodes: usize,
    built: usize,
    faces: usize,
    resolves: usize,
}

fn counts() -> Counts {
    Counts {
        decodes: kontra_ui::pictures::decodes(),
        built: kontra_ui::view::widgets_built(),
        faces: FACES.load(Ordering::Relaxed),
        resolves: RESOLVES.load(Ordering::Relaxed),
    }
}

/// The editor's state across frames, as [`super::view`] keeps it.
struct Editor {
    ui: moose::mui::mui::Ui,
    kept: Kept,
}

impl Editor {
    fn open(params: &Arc<KontraParams>) -> Self {
        Self {
            ui: kontra_ui::ui(),
            kept: Kept::take(params),
        }
    }

    fn frame(&mut self, params: &KontraParams) {
        let Kept {
            app, shown, mine, ..
        } = &mut self.kept;
        {
            let view = lock(&params.shared.view);
            view.fill(app, shown, mine);
        }
        let r = kontra_ui::app::view(&mut self.ui, app);
        let size = ViewSize::new(f64::from(SIZE.0), f64::from(SIZE.1));
        self.ui
            .frame(r.el, Some(size), Input::default(), 1. / 60.)
            .expect("laid out");
    }

    /// Frames until no picture is on its way (and one more to draw them).
    fn settle(&mut self, params: &KontraParams) {
        let until = Instant::now() + Duration::from_secs(20);
        loop {
            self.frame(params);
            let pending = (self.kept.app.parts.iter())
                .filter_map(|p| p.face.as_ref())
                .map(|f| f.pictures.pending())
                .sum::<usize>();
            if pending == 0 || Instant::now() > until {
                break;
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        self.frame(params);
    }
}

fn row(what: &str, before: &Counts, took: Duration) {
    let now = counts();
    println!(
        "| {what:<28} | {:>7} | {:>7} | {:>5} | {:>8} | {:>9.1} |",
        now.decodes - before.decodes,
        now.built - before.built,
        now.faces - before.faces,
        now.resolves - before.resolves,
        took.as_secs_f64() * 1e3,
    );
}

fn load(params: &KontraParams, dsp: &mut crate::Dsp, part: usize, path: PathBuf) {
    let was = lock(&params.shared.view)
        .parts
        .get(part)
        .map(|p| p.generation);
    params.shared.request(crate::Request::Load {
        part: Some(part),
        path,
    });
    let until = Instant::now() + Duration::from_secs(60);
    while Instant::now() < until {
        settle(params, dsp);
        block(params, dsp, &[]);
        let now = lock(&params.shared.view)
            .parts
            .get(part)
            .map(|p| p.generation);
        if now.is_some() && now != was {
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    settle(params, dsp);
}

#[test]
#[ignore = "real libraries; prints what each editor change rebuilds"]
fn what_the_editor_rebuilds_on_a_real_library() {
    let root = PathBuf::from(crate::tests::KONTAKT);
    let dolce = root.join("Audio Imperia Dolce/Instruments/01 7 1st Violins");
    let legato = dolce.join("Dolce - 01 7 1st Violins - Legato.nki");
    let sustained = dolce.join("Dolce - 02 7 1st Violins - Sustained.nki");
    let conflux = root.join("Conflux 1.1.0 [Native Instruments]/Snapshots/Conflux/Bass");
    if !legato.is_file() {
        eprintln!("skipped: no {}", legato.display());
        return;
    }
    let (params, mut dsp) = activate(&Saved::new(Vec::new()).write());
    let params = Arc::new(params);
    params.shared.open_editor();
    settle(&params, &mut dsp);
    load(&params, &mut dsp, 0, legato.clone());
    let mut ed = Editor::open(&params);
    let (c, t) = (counts(), Instant::now());
    ed.settle(&params);
    println!("| change (Dolce Legato)        | decodes | widgets | faces | resolves | settle ms |");
    println!("|---|---|---|---|---|---|");
    row("first open", &c, t.elapsed());

    let (c, t) = (counts(), Instant::now());
    ed.settle(&params);
    row("idle frame", &c, t.elapsed());

    for (what, path) in [
        ("preset swap: Sustained", &sustained),
        ("preset swap: back to Legato", &legato),
    ] {
        load(&params, &mut dsp, 0, path.clone());
        let (c, t) = (counts(), Instant::now());
        ed.settle(&params);
        row(what, &c, t.elapsed());
    }

    let fold = |ed: &mut Editor, open: bool| {
        if let Some(p) = ed.kept.app.parts.first_mut() {
            p.open = open;
        }
    };
    let (c, t) = (counts(), Instant::now());
    fold(&mut ed, false);
    ed.settle(&params);
    fold(&mut ed, true);
    ed.settle(&params);
    row("rack: fold part and back", &c, t.elapsed());

    let tab = |ed: &mut Editor, tab: PartTab| {
        if let Some(p) = ed.kept.app.parts.first_mut() {
            p.tab = tab;
        }
    };
    for other in [PartTab::Mapping, PartTab::Sound] {
        let (c, t) = (counts(), Instant::now());
        tab(&mut ed, other);
        ed.settle(&params);
        tab(&mut ed, PartTab::Interface);
        ed.settle(&params);
        row(&format!("tab: {other:?} and back"), &c, t.elapsed());
    }

    let (c, t) = (counts(), Instant::now());
    drop(ed);
    let mut ed = Editor::open(&params);
    ed.settle(&params);
    row("editor close and reopen", &c, t.elapsed());

    let snaps = ["Antarctica.nksn", "Big Foot.nksn"].map(|s| conflux.join(s));
    if snaps.iter().all(|s| s.is_file()) {
        load(&params, &mut dsp, 0, snaps[0].clone());
        ed.settle(&params);
        let (c, t) = (counts(), Instant::now());
        load(&params, &mut dsp, 0, snaps[1].clone());
        ed.settle(&params);
        row("Conflux snapshot swap", &c, t.elapsed());
    }
}
