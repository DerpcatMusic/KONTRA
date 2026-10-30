//! Layer 2 of `docs/self-test-loop.md`: drive a real instrument's page with no
//! window, and say which control each press lands on.
//!
//! For every visible control the page draws, it reads the control's rect from
//! the scene mui recorded when it drew it, presses the rect's centre through
//! the same path the editor uses (`input::read` and `Input`/`PointerInput`),
//! and checks the press selected that control and no other. A press two points
//! outside the rect must select nothing, so a "selects everything" hit test
//! cannot pass.
//!
//! `cargo run --release -p ui-drive -- [--scale <f>] [--out <dir>] <instrument>...`
//!
//! No display is opened: the page is drawn into mui's offscreen scene exactly
//! as `ui-shots` draws it, and painted with the CPU renderer only when `--out`
//! asks for the picture.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use kontra_engine::Engine;
use kontra_host::ui::lua::Page as _;
use kontra_host::ui::{Library, Live};
use kontra_stream::{Store, StoreConfig};
use kontra_ui::input::{self, Edit, Gesture, Set};
use kontra_ui::{Pictures, view};
use kontra_ui_model::{Interface, Kind, PageRef, Widget, WidgetRef};
use mui::prelude::{Buttons, Input, Mods, Point, PointerInput, Size as ViewSize, Ui};
use mui::vello;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut scale = 1.0f64;
    let mut out: Option<PathBuf> = None;
    let mut paths: Vec<PathBuf> = Vec::new();
    let mut args = std::env::args_os().skip(1);
    while let Some(arg) = args.next() {
        match arg.to_str() {
            Some("--scale") => {
                let value = args.next().ok_or("--scale needs a number")?;
                scale = value.to_string_lossy().parse()?;
            }
            Some("--out") => {
                out = Some(PathBuf::from(args.next().ok_or("--out needs a directory")?));
            }
            Some("-h" | "--help") => {
                usage();
                return Ok(());
            }
            _ => paths.push(PathBuf::from(arg)),
        }
    }
    if paths.is_empty() {
        usage();
        return Err("no instrument given".into());
    }
    if !(scale.is_finite() && scale > 0.) {
        return Err(format!("--scale must be a positive number, not {scale}").into());
    }
    if let Some(dir) = &out {
        std::fs::create_dir_all(dir)?;
    }
    let mut run = Run::default();
    for path in &paths {
        run.instrument(path, scale, out.as_deref())?;
    }
    run.report();
    if run.failed.is_empty() {
        Ok(())
    } else {
        std::process::exit(1);
    }
}

fn usage() {
    eprintln!(
        "usage: ui-drive [--scale <f>] [--out <dir>] <instrument>...\n\
         \n\
         Presses every visible control of an instrument's page at the centre of the rect\n\
         it was drawn in, headless, and fails when the press does not select that control\n\
         or when a press two points outside its rect selects it anyway."
    );
}

/// What the run found, across every instrument and page.
#[derive(Default)]
struct Run {
    pages: usize,
    checked: usize,
    skipped: usize,
    /// Controls whose click produced an `Edit` (a value, a cell, a menu).
    edited: usize,
    /// Controls the page paints underneath another one: unreachable in
    /// Kontakt too, so not a hit-test failure.
    covered: usize,
    failed: Vec<Fail>,
}

/// A control whose centre press did not select it, or whose outside press did.
struct Fail {
    page: String,
    widget: usize,
    source: String,
    name: String,
    rect: (f64, f64, f64, f64),
    point: Point,
    /// What the press selected instead; empty for nothing.
    selected: Vec<String>,
    why: &'static str,
}

impl Run {
    fn instrument(
        &mut self,
        path: &Path,
        scale: f64,
        out: Option<&Path>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mut store = Store::new(StoreConfig::DEFAULT)?;
        let loaded = kontra_host::instrument::load(path, 48_000, &mut store)?;
        let mut engine = Engine::new(Arc::new(loaded.plan), store.client(), 48_000);
        // A KSP script's page: what `on init` built, then what the engine
        // says each control looks like now (ui-shots settles it the same way).
        for (slot, page) in loaded.pages.iter().enumerate() {
            let Some(page) = page else { continue };
            let s = u8::try_from(slot)?;
            let library = RefCell::new(Library::open(path));
            let meta = |p: &str| library.borrow_mut().meta(p);
            let now = engine
                .vm(s)
                .and_then(kontra_engine::Vm::own_page)
                .and_then(|p| p.downcast::<kontra_lua::Ui>().ok());
            let mut tree = kontra_host::ui::lua::tree(now.as_deref().unwrap_or(page), &meta);
            settle_script(&mut engine, s, &mut tree);
            let label = format!("{} — script {slot}", loaded.name);
            self.page(&label, tree, slot, scale, path, out)?;
        }
        // A library's own Interface face, when it has one.
        for (slot, face) in loaded.faces.iter().enumerate() {
            let Some(face) = face else { continue };
            let s = u8::try_from(slot)?;
            let library = RefCell::new(Library::open(path));
            let meta = |p: &str| library.borrow_mut().meta(p);
            let mut live = Live::new(face, s, &engine, &meta)?;
            let all = 0..live.tree.widgets.len();
            kontra_ui::layout::resolve(&mut live.tree, all);
            let mut block = vec![[0f32; 2]; 512];
            for _ in 0..10 {
                engine.process(&mut block, &[]);
                let changed = live.refresh(face, s, &engine, &meta);
                kontra_ui::layout::resolve(&mut live.tree, changed.widgets.iter().map(|w| w.0));
            }
            let label = format!("{} — face {slot}", loaded.name);
            self.page(&label, live.tree, slot, scale, path, out)?;
        }
        Ok(())
    }

    fn page(
        &mut self,
        label: &str,
        tree: Interface,
        face: usize,
        scale: f64,
        path: &Path,
        out: Option<&Path>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        self.pages += 1;
        let mut drive = Drive::new(tree, face, scale, path)?;
        drive.settle()?;
        let (w, h) = drive.size;
        println!("\n{label}, page 0: {w:.0}×{h:.0} at {scale}×");
        if let Some(dir) = out {
            let file = dir.join(format!("{}-page0.png", label.replace(['/', ' ', '—'], "_")));
            drive.save(&file)?;
            println!("  drawn to {}", file.display());
        }
        println!(
            "  {:>3}  {:<7} {:<24} {:<20} {:<20} {:<20} edit",
            "#", "source", "name", "drawn rect", "pressed", "selected"
        );
        let count = drive.tree.widgets.len();
        for n in 0..count {
            let wr = WidgetRef(n);
            if !drive.tree.visible(wr) {
                continue;
            }
            let Some(widget) = drive.tree.widgets.get(n) else {
                continue;
            };
            if !takes_input(widget) {
                continue;
            }
            let source = widget
                .source_id
                .map_or_else(|| "-".to_owned(), |s| s.to_string());
            let name = widget.name.clone();
            let Some(rect) = drive.rect(n) else {
                println!(
                    "  {n:>3}  {:<7} {:<24} {:<20} {:<20} {:<20} skip",
                    clip(&source, 7),
                    clip(&name, 24),
                    "-",
                    "-",
                    "not drawn",
                );
                self.skipped += 1;
                continue;
            };
            let point = Point::new(rect.0 + rect.2 / 2., rect.1 + rect.3 / 2.);
            if !drive.on_page(point) {
                println!(
                    "  {n:>3}  {:<7} {:<24} {:<20} {:<20} {:<20} skip",
                    clip(&source, 7),
                    clip(&name, 24),
                    drawn(rect),
                    at(point),
                    "off the page",
                );
                self.skipped += 1;
                continue;
            }
            self.checked += 1;
            let probe = drive.press(point)?;
            let selected = drive.describe(&probe.selected);
            let edit = probe
                .edits
                .first()
                .map_or_else(|| "-".to_owned(), edit_text);
            if !probe.edits.is_empty() {
                self.edited += 1;
            }
            let ok = probe.selected == [n] && probe.edits.iter().all(|e| e.widget.0 == n);
            // A control the page paints under another one cannot be clicked in
            // Kontakt either: the widget above takes the press there, and that
            // says nothing about the hit test. It is reported, not failed.
            let covered = drive.covering(n, point);
            let unreachable = covered.filter(|&m| probe.selected == [m]);
            let verdict = if ok {
                "ok"
            } else if unreachable.is_some() {
                "covered"
            } else {
                "FAIL"
            };
            if unreachable.is_some() {
                self.covered += 1;
            }
            if !ok && unreachable.is_none() {
                self.failed.push(Fail {
                    page: label.to_owned(),
                    widget: n,
                    source: source.clone(),
                    name: name.clone(),
                    rect,
                    point,
                    selected: probe
                        .selected
                        .iter()
                        .map(|&s| drive.describe(&[s]))
                        .collect(),
                    why: if probe.selected.is_empty() {
                        "the centre press selected nothing"
                    } else if probe.selected == [n] {
                        "the press selected it, but an Edit named another control"
                    } else {
                        "the centre press selected another control"
                    },
                });
            }
            // Two points outside the rect, inside the page: that press must
            // not select this control, or the hit test is selecting everything.
            if let Some(outside) = drive.outside(rect) {
                let miss = drive.press(outside)?;
                if miss.selected.contains(&n) || miss.edits.iter().any(|e| e.widget.0 == n) {
                    self.failed.push(Fail {
                        page: label.to_owned(),
                        widget: n,
                        source: source.clone(),
                        name: name.clone(),
                        rect,
                        point: outside,
                        selected: miss
                            .selected
                            .iter()
                            .map(|&s| drive.describe(&[s]))
                            .collect(),
                        why: "a press 2 points outside the rect selected it anyway",
                    });
                }
            }
            println!(
                "  {n:>3}  {:<7} {:<24} {:<20} {:<20} {:<20} {}  {verdict}{}",
                clip(&source, 7),
                clip(&name, 24),
                drawn(rect),
                at(point),
                clip(&selected, 20),
                edit,
                unreachable.map_or_else(String::new, |m| format!(
                    " (painted under {} {})",
                    drive.describe(&[m]),
                    drive.kind(m)
                )),
            );
        }
        Ok(())
    }

    fn report(&self) {
        println!();
        if self.failed.is_empty() {
            println!(
                "PASS: {} controls checked on {} page(s), {} skipped; every control's centre \
                 selected itself and a press 2 points outside selected nothing. {} answered a \
                 click with an Edit (a knob or slider selects without changing, as Kontakt does). \
                 {} are painted under another control, so no click can reach them in Kontakt \
                 either; each is named in the table.",
                self.checked, self.pages, self.skipped, self.edited, self.covered,
            );
            return;
        }
        println!(
            "FAIL: {} of {} controls checked did not hit-test where they are drawn",
            self.failed.len(),
            self.checked
        );
        for f in &self.failed {
            println!(
                "  {} #{} {} (source {}) drawn {}, pressed {} — {}{}",
                f.page,
                f.widget,
                f.name,
                f.source,
                drawn(f.rect),
                at(f.point),
                f.why,
                if f.selected.is_empty() {
                    String::new()
                } else {
                    format!(" instead: {}", f.selected.join(", "))
                },
            );
        }
    }
}

/// A page drawn offscreen, driven with the editor's own pointer path.
struct Drive {
    ui: Ui,
    tree: Interface,
    pictures: Pictures,
    gesture: Gesture,
    face: usize,
    scale: f64,
    size: (f64, f64),
    /// Page 0's widgets back to front, and each one's place in that order:
    /// what is painted above what.
    order: Vec<usize>,
    rank: Vec<usize>,
}

/// One press at one point: which widget mui says it landed on, and the edits
/// the editor's reader made of it.
struct Probe {
    selected: Vec<usize>,
    edits: Vec<Edit>,
}

impl Drive {
    fn new(
        tree: Interface,
        face: usize,
        scale: f64,
        path: &Path,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let (w, h) = tree
            .pages
            .first()
            .map_or((0., 0.), |p| (p.size.width, p.size.height));
        if !(w > 0. && h > 0.) {
            return Err("the page has no size".into());
        }
        let (reader, library) = (path.to_path_buf(), std::sync::Mutex::new(None));
        let pictures = Pictures::new(Box::new(move |p: &str| {
            let mut library = library.lock().ok()?;
            library
                .get_or_insert_with(|| Library::open(&reader))
                .read(p)
        }));
        let order: Vec<usize> = tree.draw_order(PageRef(0)).iter().map(|w| w.0).collect();
        let mut rank = vec![usize::MAX; tree.widgets.len()];
        for (i, &n) in order.iter().enumerate() {
            if let Some(r) = rank.get_mut(n) {
                *r = i;
            }
        }
        Ok(Self {
            ui: kontra_ui::ui(),
            tree,
            pictures,
            gesture: Gesture::default(),
            face,
            scale,
            size: (w, h),
            order,
            rank,
        })
    }

    /// One frame the way the editor draws it: the reader first, against the
    /// rects the last frame recorded, then the page, then mui's hit test.
    fn frame(&mut self, input: Input) -> Result<Vec<Edit>, Box<dyn std::error::Error>> {
        let (edits, popup) = input::read(
            &mut self.ui,
            self.face,
            &self.tree,
            PageRef(0),
            self.scale,
            &mut self.gesture,
        );
        // The edits are reported, not applied: this harness asks where a press
        // landed, and applying one control's value would move the next probe's
        // starting point.
        let (page, _) = view::page(
            &mut self.ui,
            self.face,
            &self.tree,
            PageRef(0),
            &mut self.pictures,
            (self.scale, &[]),
        );
        let el = match popup {
            Some(popup) => mui::prelude::stack([page, popup]),
            None => page,
        };
        let (w, h) = self.size;
        self.ui
            .frame(
                el,
                Some(ViewSize::new(w * self.scale, h * self.scale)),
                input,
                1. / 60.,
            )
            .map_err(|e| format!("{e:?}"))?;
        Ok(edits)
    }

    /// Draws until the page's pictures are in (a page that never finishes
    /// loading is drawn as it is, not waited on forever).
    fn settle(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        self.frame(Input::default())?;
        for _ in 0..400 {
            if self.pictures.pending() == 0 {
                break;
            }
            if self.pictures.poll() {
                self.frame(Input::default())?;
            } else {
                std::thread::sleep(Duration::from_micros(500));
            }
        }
        self.frame(Input::default())?;
        self.frame(Input::default())?;
        Ok(())
    }

    /// The rect mui recorded when it drew widget `n`, in view points.
    fn rect(&self, n: usize) -> Option<(f64, f64, f64, f64)> {
        let id = input::surface(self.face, n);
        let s = self.ui.scene()?.surface(&id)?;
        Some((
            s.frame.x,
            s.frame.y,
            s.frame.size.width,
            s.frame.size.height,
        ))
    }

    fn on_page(&self, p: Point) -> bool {
        let (w, h) = self.size;
        (0. ..=w * self.scale).contains(&p.x) && (0. ..=h * self.scale).contains(&p.y)
    }

    /// Two points outside the drawn rect, at the middle of an edge, the first
    /// one still on the page.
    fn outside(&self, rect: (f64, f64, f64, f64)) -> Option<Point> {
        let (x, y, w, h) = rect;
        let (mid_x, mid_y) = (x + w / 2., y + h / 2.);
        [
            Point::new(x - 2., mid_y),
            Point::new(x + w + 2., mid_y),
            Point::new(mid_x, y - 2.),
            Point::new(mid_x, y + h + 2.),
        ]
        .into_iter()
        .find(|&p| self.on_page(p))
    }

    /// Down at `point`, a move held there, up, then a quiet frame — the
    /// editor's own sequence — and what the press selected.
    fn press(&mut self, point: Point) -> Result<Probe, Box<dyn std::error::Error>> {
        self.gesture = Gesture::default();
        self.settle()?;
        let mut edits = self.frame(pointer(point, true))?;
        // mui hit-tests the press against the last scene in the next frame:
        // the reader sees it on the move, and `ui.get` reports it now.
        let mut selected = self.pressed();
        edits.extend(self.frame(pointer(point, true))?);
        if selected.is_empty() {
            selected = self.pressed();
        }
        edits.extend(self.frame(pointer(point, false))?);
        edits.extend(self.frame(Input::default())?);
        // The capture is released by the quiet frame above, so the next probe
        // starts with nothing held.
        Ok(Probe { selected, edits })
    }

    /// The widget painted above `n` whose drawn rect holds `point`, if any:
    /// the press there belongs to it, in Kontakt as here.
    fn covering(&self, n: usize, point: Point) -> Option<usize> {
        let after = self.rank.get(n).copied().unwrap_or(usize::MAX);
        self.order
            .iter()
            .skip(after.saturating_add(1))
            .copied()
            .find(|&m| self.rect(m).is_some_and(|r| holds(r, point)))
    }

    fn kind(&self, n: usize) -> &'static str {
        match self.tree.widgets.get(n).map(|w| &w.kind) {
            Some(Kind::Knob { .. }) => "knob",
            Some(Kind::Slider { .. }) => "slider",
            Some(Kind::ValueEdit { .. }) => "value edit",
            Some(Kind::Button { .. }) => "button",
            Some(Kind::Switch) => "switch",
            Some(Kind::Menu { .. }) => "menu",
            Some(Kind::Table { .. }) => "table",
            Some(Kind::TextEdit) => "text edit",
            Some(Kind::Label) => "label",
            Some(Kind::Image) => "image",
            Some(Kind::Panel) => "panel",
            _ => "widget",
        }
    }

    /// The widgets whose surface reports the press (or the capture) this
    /// frame, as mui resolved it.
    fn pressed(&self) -> Vec<usize> {
        (0..self.tree.widgets.len())
            .filter(|&n| {
                let r = self.ui.get(input::surface(self.face, n));
                r.pressed || r.held
            })
            .collect()
    }

    fn describe(&self, widgets: &[usize]) -> String {
        if widgets.is_empty() {
            return "nothing".to_owned();
        }
        widgets
            .iter()
            .filter_map(|&n| self.tree.widgets.get(n))
            .map(|w| w.name.clone())
            .collect::<Vec<_>>()
            .join(", ")
    }

    fn save(&self, file: &Path) -> Result<(), Box<dyn std::error::Error>> {
        let (w, h) = (
            (self.size.0 * self.scale).ceil() as u16,
            (self.size.1 * self.scale).ceil() as u16,
        );
        let mut ctx = vello::vello_cpu::RenderContext::new(w, h);
        let mut resources = vello::vello_cpu::Resources::default();
        let scene = self.ui.scene().ok_or("no scene")?;
        let cpu = &mut vello::Cpu {
            ctx: &mut ctx,
            resources: &mut resources,
            cache: &mut vello::Cache::default(),
        };
        vello::paint(cpu, scene, vello::kurbo::Affine::IDENTITY).map_err(|e| format!("{e:?}"))?;
        ctx.flush();
        let mut pix = vello::vello_cpu::Pixmap::new(w, h);
        ctx.render(&mut pix, &mut resources);
        let rgba: Vec<u8> = pix
            .take_unpremultiplied()
            .iter()
            .flat_map(|p| [p.r, p.g, p.b, p.a])
            .collect();
        let mut e = png::Encoder::new(
            std::io::BufWriter::new(std::fs::File::create(file)?),
            w.into(),
            h.into(),
        );
        e.set_color(png::ColorType::Rgba);
        e.set_depth(png::BitDepth::Eight);
        e.write_header()?.write_image_data(&rgba)?;
        Ok(())
    }
}

/// A widget the pointer can act on (the same set `input::read` reads).
fn takes_input(w: &Widget) -> bool {
    w.enabled
        && matches!(
            w.kind,
            Kind::Knob { .. }
                | Kind::Slider { .. }
                | Kind::ValueEdit { .. }
                | Kind::Button { .. }
                | Kind::Switch
                | Kind::Menu { .. }
                | Kind::Table { .. }
                | Kind::TextEdit
        )
}

/// A KSP script's page as the engine left it after `on init`, then with each
/// control's look and words applied (as `ui-shots` settles one).
fn settle_script(engine: &mut Engine, slot: u8, tree: &mut Interface) {
    let mut block = vec![[0f32; 2]; 512];
    for _ in 0..10 {
        engine.process(&mut block, &[]);
    }
    let mut text = kontra_engine::Text::EMPTY;
    for n in 0..tree.widgets.len() {
        let Ok(n32) = u32::try_from(n) else { continue };
        if let Some(look) = engine.widget(slot, n32) {
            let words = engine.widget_text(slot, n32, &mut text);
            kontra_host::ui::lua::apply(tree, n, look, words.then(|| text.as_str()));
        }
    }
    kontra_ui::layout::resolve(tree, 0..tree.widgets.len());
}

fn pointer(pos: Point, down: bool) -> Input {
    Input {
        pointer: PointerInput {
            pos: Some(pos),
            buttons: if down {
                Buttons::PRIMARY
            } else {
                Buttons::default()
            },
            mods: Mods::default(),
        },
        ..Default::default()
    }
}

fn edit_text(e: &Edit) -> String {
    let what = match &e.to {
        Set::Value(v) => format!("{v:.3}"),
        Set::Cell(k, v) => format!("cell {k} = {v:.3}"),
        Set::Text(t) => format!("text {t:?}"),
    };
    format!("{} = {what}", e.widget.0)
}

/// Whether a drawn rect holds a point.
fn holds(r: (f64, f64, f64, f64), p: Point) -> bool {
    p.x >= r.0 && p.x <= r.0 + r.2 && p.y >= r.1 && p.y <= r.1 + r.3
}

fn drawn(r: (f64, f64, f64, f64)) -> String {
    format!("{:.0}×{:.0} @({:.0},{:.0})", r.2, r.3, r.0, r.1)
}

fn at(p: Point) -> String {
    format!("({:.1},{:.1})", p.x, p.y)
}

fn clip(s: &str, n: usize) -> String {
    let mut out: String = s.chars().take(n).collect();
    if s.chars().count() > n {
        out.push('…');
    }
    out
}
