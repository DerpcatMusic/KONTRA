//! Draws each instrument's Original view headless, the way the editor will,
//! and says what it cost: first frame, settled, paint, idle, RAM. Saves the
//! settled page as `<out>/<name>-<slot>.png`.
//! `cargo run --release -p ui-shots -- <out dir> <instrument>...`

use std::cell::RefCell;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use kontra_engine::Engine;
use kontra_host::ui::{Library, Live};
use kontra_stream::{Store, StoreConfig};
use kontra_ui::{Pictures, view};
use kontra_ui_model::{Kind, PageRef, WidgetRef};
use mui::prelude::{Input, Size, Ui};
use mui::vello;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args_os().skip(1);
    let out = PathBuf::from(
        args.next()
            .ok_or("usage: ui-shots <out dir> <instrument>...")?,
    );
    std::fs::create_dir_all(&out)?;
    for path in args.map(PathBuf::from) {
        let t = Instant::now();
        let mut store = Store::new(StoreConfig::DEFAULT)?;
        let loaded = kontra_host::instrument::load(&path, 48_000, &mut store)?;
        let mut engine = Engine::new(Arc::new(loaded.plan), store.client(), 48_000);
        let load = t.elapsed();
        let mut block = vec![[0f32; 2]; 512];
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
                let (el, built) = view::page(ui, &live.tree, PageRef(0), pictures, 1.);
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
            let mut idle = Vec::new();
            let mut idle_built = 0;
            for _ in 0..60 {
                let t = Instant::now();
                idle_built += frame(&mut ui, &mut pictures, &live)?;
                idle.push(t.elapsed());
            }
            idle.sort();
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
                    kontra_ui::layout::resolve(&mut live.tree, changed.widgets.iter().map(|w| w.0));
                    *built += frame(&mut ui, &mut pictures, &live)?;
                    pictures.poll();
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
                "{{\"name\":{:?},\"slot\":{slot},\"widgets\":{},\"shown\":{drawn},\"pictures\":{},\"load_ms\":{:.0},\"tree_ms\":{:.1},\"first_frame_ms\":{:.2},\"first_pending\":{},\"settled_ms\":{:.1},\"paint_ms\":{:.1},\"idle_us\":{},\"idle_built\":{idle_built},\"knob_built\":{knob_built},\"pedal_built\":{pedal:?},\"images_mb\":{:.1},\"rss_mb\":{},\"peak_rss_mb\":{},\"shot\":{:?}}}",
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
        }
    }
    Ok(())
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
