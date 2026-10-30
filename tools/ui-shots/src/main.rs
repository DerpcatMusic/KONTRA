//! Draws each instrument's Original view headless, the way the editor will,
//! and says what it cost: first frame, settled, paint, idle, RAM. Saves the
//! settled page as `<out>/<name>-<slot>.png`.
//! With `--app`, the page is also drawn where the user meets it, inside an
//! open part of the rack, and saved as `<out>/<name>-<slot>-app.png`.
//! `cargo run --release -p ui-shots -- [--press <control>] [--app] <out dir> <instrument>...`

use std::cell::RefCell;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use kontra_engine::Engine;
use kontra_host::ui::lua::Page as _;
use kontra_host::ui::{Library, Live};
use kontra_stream::{Store, StoreConfig};
use kontra_ui::{Pictures, view};
use kontra_ui_model::{Kind, PageRef, WidgetRef};
use mui::prelude::{Input, Size, Ui};
use mui::vello;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args: Vec<_> = std::env::args_os().skip(1).collect();
    // `--press <control>`: press that control before the pedal, so a page
    // the source shows on a press is measured too.
    let press = args.iter().position(|a| a == "--press").map(|i| {
        let name = args.get(i + 1).map(|n| n.to_string_lossy().into_owned());
        args.drain(i..(i + 2).min(args.len()));
        name.unwrap_or_default()
    });
    let app = args
        .iter()
        .position(|a| a == "--app")
        .map(|i| args.remove(i));
    let mut args = args.into_iter();
    let out = PathBuf::from(
        args.next()
            .ok_or("usage: ui-shots [--press <control>] <out dir> <instrument>...")?,
    );
    std::fs::create_dir_all(&out)?;
    for path in args.map(PathBuf::from) {
        let t = Instant::now();
        let mut store = Store::new(StoreConfig::DEFAULT)?;
        let loaded = kontra_host::instrument::load(&path, 48_000, &mut store)?;
        let mut engine = Engine::new(Arc::new(loaded.plan), store.client(), 48_000);
        let load = t.elapsed();
        let mut block = vec![[0f32; 2]; 512];
        for (slot, page) in loaded.pages.iter().enumerate() {
            if let Some(page) = page {
                lua_page(&out, &path, &loaded.name, page, slot as u8, &mut engine)?;
            }
        }
        for (slot, face) in loaded.faces.iter().enumerate() {
            let Some(face) = face else { continue };
            let slot = slot as u8;
            let t = Instant::now();
            let library = RefCell::new(Library::open(&path));
            let meta = |p: &str| library.borrow_mut().meta(p);
            let mut live = Live::new(face, slot, &engine, &meta)?;
            let all = 0..live.tree.widgets.len();
            kontra_ui::layout::resolve(&mut live.tree, all);
            // What a settled editor shows: listeners run in the first blocks.
            for _ in 0..10 {
                engine.process(&mut block, &[]);
                let changed = live.refresh(face, slot, &engine, &meta);
                kontra_ui::layout::resolve(&mut live.tree, changed.widgets.iter().map(|w| w.0));
            }
            let tree_ms = t.elapsed();
            let Some(p) = live.tree.pages.first() else {
                continue;
            };
            let (w, h) = (p.size.width.ceil() as u16, p.size.height.ceil() as u16);
            let start = Instant::now();
            // The worker opens the library itself: the first frame waits on nothing.
            let (reader, library) = (path.clone(), std::sync::Mutex::new(None));
            let mut pictures = Pictures::new(Box::new(move |p: &str| {
                let mut library = library.lock().ok()?;
                library
                    .get_or_insert_with(|| Library::open(&reader))
                    .read(p)
            }));
            let mut ui = kontra_ui::ui();
            let frame = |ui: &mut Ui, pictures: &mut Pictures, live: &Live| {
                let (el, built) = view::page(ui, 0, &live.tree, PageRef(0), pictures, (1., &[]));
                ui.frame(
                    el,
                    Some(Size::new(f64::from(w), f64::from(h))),
                    Input::default(),
                    1. / 60.,
                )
                .map(|_| built)
                .map_err(|e| format!("{e:?}"))
            };
            frame(&mut ui, &mut pictures, &live)?;
            let first = (start.elapsed(), pictures.pending());
            while pictures.pending() > 0 {
                if pictures.poll() {
                    frame(&mut ui, &mut pictures, &live)?;
                } else {
                    std::thread::sleep(Duration::from_micros(500));
                }
            }
            frame(&mut ui, &mut pictures, &live)?;
            let settled = start.elapsed();
            // An idle editor tick: the engine runs, the view asks what
            // changed and frames only when something did.
            let (mut idle, mut idle_built, mut idle_frames) = (Vec::new(), 0, 0);
            for _ in 0..60 {
                engine.process(&mut block, &[]);
                let t = Instant::now();
                let changed = live.refresh(face, slot, &engine, &meta);
                if pictures.poll() || !changed.is_empty() {
                    kontra_ui::layout::resolve(&mut live.tree, changed.widgets.iter().map(|w| w.0));
                    idle_built += frame(&mut ui, &mut pictures, &live)?;
                    idle_frames += 1;
                }
                idle.push(t.elapsed());
            }
            idle.sort();
            // The face as it loads, for the rack shot: the timing moves below
            // change what a library shows (ANALOG's knob shows its value).
            let untouched = live.tree.clone();
            // One shown knob moved: the widgets that frame builds.
            let knob = live.tree.widgets.iter().enumerate().find_map(|(n, w)| {
                let knob = matches!(w.kind, Kind::Knob { .. } | Kind::Slider { .. });
                (knob && live.tree.visible(WidgetRef(n)))
                    .then_some(w.source_id)
                    .flatten()
            });
            let mut moved = None;
            if let Some(id) = knob {
                engine.set_control(slot, id, 1);
                engine.process(&mut block, &[]);
                let changed = live.refresh(face, slot, &engine, &meta);
                kontra_ui::layout::resolve(&mut live.tree, changed.widgets.iter().map(|w| w.0));
                let mut built = frame(&mut ui, &mut pictures, &live)?;
                while pictures.pending() > 0 {
                    pictures.poll();
                    std::thread::sleep(Duration::from_micros(500));
                }
                built += frame(&mut ui, &mut pictures, &live)?;
                moved = Some(built);
            }
            let pressed = press
                .as_deref()
                .and_then(|p| live.tree.widgets.iter().find(|w| w.name == p)?.source_id);
            if let Some(id) = pressed {
                engine.set_control(slot, id, 1);
                for _ in 0..10 {
                    engine.process(&mut block, &[]);
                    let changed = live.refresh(face, slot, &engine, &meta);
                    kontra_ui::layout::resolve(&mut live.tree, changed.widgets.iter().map(|w| w.0));
                }
                frame(&mut ui, &mut pictures, &live)?;
                while pictures.pending() > 0 {
                    pictures.poll();
                    std::thread::sleep(Duration::from_micros(500));
                }
                frame(&mut ui, &mut pictures, &live)?;
            }
            // The sustain pedal down, then up: what a controller callback
            // lights (Una Corda's pedal light), and what that repaints.
            let mut pedal = [0; 2];
            for (built, v) in pedal.iter_mut().zip([127, 0]) {
                let cc = kontra_engine::Timed {
                    at: kontra_engine::SampleTime(0),
                    body: kontra_engine::Event::midi1(0xB0, 64, v),
                };
                engine.process(&mut block, &[cc]);
                for _ in 0..50 {
                    engine.process(&mut block, &[]);
                    let changed = live.refresh(face, slot, &engine, &meta);
                    if pictures.poll() || !changed.is_empty() {
                        kontra_ui::layout::resolve(
                            &mut live.tree,
                            changed.widgets.iter().map(|w| w.0),
                        );
                        *built += frame(&mut ui, &mut pictures, &live)?;
                    }
                }
                while pictures.pending() > 0 {
                    pictures.poll();
                    std::thread::sleep(Duration::from_micros(500));
                }
                *built += frame(&mut ui, &mut pictures, &live)?;
            }
            let t = Instant::now();
            let rgba = paint(&ui, w, h)?;
            let paint_ms = t.elapsed();
            let file = out.join(format!(
                "{}-{slot}.png",
                loaded.name.replace(['/', ' '], "_")
            ));
            save(&file, w, h, &rgba)?;
            let drawn = (0..live.tree.widgets.len())
                .filter(|&n| live.tree.visible(kontra_ui_model::WidgetRef(n)))
                .count();
            let knob_built = moved.map_or("null".into(), |m: usize| m.to_string());
            println!(
                "{{\"name\":{:?},\"slot\":{slot},\"widgets\":{},\"shown\":{drawn},\"pictures\":{},\"load_ms\":{:.0},\"tree_ms\":{:.1},\"first_frame_ms\":{:.2},\"first_pending\":{},\"settled_ms\":{:.1},\"paint_ms\":{:.1},\"idle_us\":{},\"idle_built\":{idle_built},\"idle_frames\":{idle_frames},\"knob_built\":{knob_built},\"pedal_built\":{pedal:?},\"images_mb\":{:.1},\"rss_mb\":{},\"peak_rss_mb\":{},\"shot\":{:?}}}",
                loaded.name,
                live.tree.widgets.len(),
                live.tree.assets.len(),
                ms(load),
                ms(tree_ms),
                ms(first.0),
                first.1,
                ms(settled),
                ms(paint_ms),
                idle.get(idle.len() / 2).map_or(0, Duration::as_micros),
                pictures.bytes() as f64 / 1e6,
                status("VmRSS:"),
                status("VmHWM:"),
                file,
            );
            if app.is_some() {
                let name = loaded.name.replace(['/', ' '], "_");
                let file = out.join(format!("{name}-{slot}-app.png"));
                let tree = if press.is_some() {
                    live.tree.clone()
                } else {
                    untouched
                };
                in_rack(&loaded.name, tree, pictures, &file)?;
            }
        }
    }
    Ok(())
}

/// The page inside an open part of the editor, 1180×900, saved to `file`
/// once its pictures are in.
fn in_rack(
    name: &str,
    tree: kontra_ui_model::Interface,
    pictures: Pictures,
    file: &std::path::Path,
) -> Result<(), Box<dyn std::error::Error>> {
    use kontra_ui::app::{self, App, Face, Part, PartTab};
    let mut app = App {
        parts: vec![Part {
            name: name.to_owned(),
            library: name.to_owned(),
            channel: 1,
            output: 1,
            keys: (21, 108),
            face: Some(Face {
                tree,
                pictures,
                gesture: kontra_ui::input::Gesture::default(),
            }),
            open: true,
            tab: PartTab::Interface,
            ..Part::default()
        }],
        width: 1180.,
        ..App::default()
    };
    let (w, h) = (1180u16, 900u16);
    let mut ui = kontra_ui::ui();
    let frame = |ui: &mut Ui, app: &mut App| -> Result<bool, Box<dyn std::error::Error>> {
        let r = app::view(ui, app);
        let f = ui
            .frame(
                r.el,
                Some(Size::new(f64::from(w), f64::from(h))),
                Input::default(),
                1. / 60.,
            )
            .map_err(|e| format!("{e:?}"))?;
        Ok(f.animating)
    };
    let pending = |app: &mut App| {
        app.parts
            .first_mut()
            .and_then(|p| p.face.as_mut())
            .map_or(0, |f| {
                f.pictures.poll();
                f.pictures.pending()
            })
    };
    for _ in 0..600 {
        let moving = frame(&mut ui, &mut app)?;
        if !moving && pending(&mut app) == 0 {
            break;
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    frame(&mut ui, &mut app)?;
    save(file, w, h, &paint(&ui, w, h)?)
}

/// A Lua script's page as the engine left it, `<name>-<slot>.png`; then its
/// first shown knob (or `UI_SHOTS_KNOB`) turned to the top,
/// `<name>-<slot>-knob.png`.
fn lua_page(
    out: &std::path::Path,
    path: &std::path::Path,
    name: &str,
    page: &kontra_lua::Ui,
    slot: u8,
    engine: &mut Engine,
) -> Result<(), Box<dyn std::error::Error>> {
    use kontra_host::ui::lua;
    let library = RefCell::new(Library::open(path));
    let meta = |p: &str| library.borrow_mut().meta(p);
    // As init left it, as the editor reads it.
    let now = (engine.vm(slot))
        .and_then(kontra_engine::Vm::own_page)
        .and_then(|p| p.downcast::<kontra_lua::Ui>().ok());
    let mut tree = lua::tree(now.as_deref().unwrap_or(page), &meta);
    let settle = |engine: &mut Engine, tree: &mut kontra_ui_model::Interface| {
        let mut block = vec![[0f32; 2]; 512];
        for _ in 0..10 {
            engine.process(&mut block, &[]);
        }
        let mut text = kontra_engine::Text::EMPTY;
        for n in 0..tree.widgets.len() {
            if let Some(look) = engine.widget(slot, n as u32) {
                let words = engine.widget_text(slot, n as u32, &mut text);
                lua::apply(tree, n, look, words.then(|| text.as_str()));
            }
        }
        kontra_ui::layout::resolve(tree, 0..tree.widgets.len());
    };
    settle(engine, &mut tree);
    let stem = name.replace(['/', ' '], "_");
    shot(&tree, path, &out.join(format!("{stem}-{slot}.png")))?;
    // `UI_SHOTS_KNOB=<name>` picks the knob; else the first shown one.
    let wanted = std::env::var("UI_SHOTS_KNOB").ok();
    // A shown knob or slider: its number, name and top.
    let knobs: Vec<(usize, String, f64)> = (tree.widgets.iter().enumerate())
        .filter(|&(n, _)| tree.visible(WidgetRef(n)))
        .filter_map(|(n, w)| match w.kind {
            Kind::Knob { range, .. } | Kind::Slider { range, .. } => {
                Some((n, w.name.clone(), range.max))
            }
            _ => None,
        })
        .collect();
    let names: Vec<&str> = knobs.iter().map(|k| k.1.as_str()).collect();
    eprintln!("{name}: shown knobs {names:?}");
    let knob = (knobs.iter()).find(|k| wanted.as_deref().is_none_or(|w| k.1 == w));
    let mut moved = String::from("null");
    if let Some((n, knob, top)) = knob {
        engine.set_widget(slot, *n as u32, *top);
        settle(engine, &mut tree);
        let now = tree.widgets.get(*n).and_then(|w| w.value.clone());
        moved = format!("{{\"knob\":{knob:?},\"to\":{top},\"now\":{now:?}}}");
        shot(&tree, path, &out.join(format!("{stem}-{slot}-knob.png")))?;
    }
    let shown = (0..tree.widgets.len())
        .filter(|&n| tree.visible(WidgetRef(n)))
        .count();
    println!(
        "{{\"name\":{name:?},\"slot\":{slot},\"lua\":true,\"widgets\":{},\"shown\":{shown},\"pictures\":{},\"moved\":{moved}}}",
        tree.widgets.len(),
        tree.assets.len(),
    );
    Ok(())
}

/// Draws page 0 of `tree` once its pictures are in, and saves it.
fn shot(
    tree: &kontra_ui_model::Interface,
    path: &std::path::Path,
    file: &std::path::Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let Some(p) = tree.pages.first() else {
        return Ok(());
    };
    let (w, h) = (p.size.width.ceil() as u16, p.size.height.ceil() as u16);
    let (reader, library) = (path.to_path_buf(), std::sync::Mutex::new(None));
    let mut pictures = Pictures::new(Box::new(move |p: &str| {
        let mut library = library.lock().ok()?;
        library
            .get_or_insert_with(|| Library::open(&reader))
            .read(p)
    }));
    let mut ui = kontra_ui::ui();
    let frame = |ui: &mut Ui, pictures: &mut Pictures| {
        let (el, _) = view::page(ui, 0, tree, PageRef(0), pictures, (1., &[]));
        ui.frame(
            el,
            Some(Size::new(f64::from(w), f64::from(h))),
            Input::default(),
            1. / 60.,
        )
        .map(|_| ())
        .map_err(|e| format!("{e:?}"))
    };
    frame(&mut ui, &mut pictures)?;
    while pictures.pending() > 0 {
        if !pictures.poll() {
            std::thread::sleep(Duration::from_micros(500));
        }
    }
    frame(&mut ui, &mut pictures)?;
    save(file, w, h, &paint(&ui, w, h)?)
}

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.
}

/// A `/proc/self/status` size line, in MB.
fn status(key: &str) -> u64 {
    std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|s| {
            let line = s.lines().find(|l| l.starts_with(key))?.to_owned();
            line.split_whitespace().nth(1)?.parse::<u64>().ok()
        })
        .map_or(0, |kb| kb / 1024)
}

fn paint(ui: &Ui, w: u16, h: u16) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let mut ctx = vello::vello_cpu::RenderContext::new(w, h);
    let mut resources = vello::vello_cpu::Resources::default();
    let scene = ui.scene().ok_or("no scene")?;
    let cpu = &mut vello::Cpu {
        ctx: &mut ctx,
        resources: &mut resources,
        cache: &mut vello::Cache::default(),
    };
    vello::paint(cpu, scene, vello::kurbo::Affine::IDENTITY).map_err(|e| format!("{e:?}"))?;
    ctx.flush();
    let mut pix = vello::vello_cpu::Pixmap::new(w, h);
    ctx.render(&mut pix, &mut resources);
    Ok(pix
        .take_unpremultiplied()
        .iter()
        .flat_map(|p| [p.r, p.g, p.b, p.a])
        .collect())
}

fn save(
    file: &std::path::Path,
    w: u16,
    h: u16,
    rgba: &[u8],
) -> Result<(), Box<dyn std::error::Error>> {
    let mut e = png::Encoder::new(
        std::io::BufWriter::new(std::fs::File::create(file)?),
        w.into(),
        h.into(),
    );
    e.set_color(png::ColorType::Rgba);
    e.set_depth(png::BitDepth::Eight);
    e.write_header()?.write_image_data(rgba)?;
    Ok(())
}
