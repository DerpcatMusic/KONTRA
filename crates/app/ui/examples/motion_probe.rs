//! What each interaction costs the editor, through MUI's own frame
//! scheduler (`mui::host::Driver`) on a 60 Hz clock, with the CPU
//! presenter's damage-limited render after every frame the driver builds.
//! One JSON line per interaction: the frames built and the frames that drew
//! pixels in the 2 s after the input, the tick the last one ran on, and per
//! built frame the median build, layout and render µs, pixels and the layout
//! nodes measured or arranged again.
//!
//! The view does what KONTRA's editor does around `app::view`: a press,
//! release or action marks the model changed so the next frame shows it;
//! `PROBE_HEARTBEAT=0.25` adds the periodic repaint deadline the editor
//! kept before it polled; `PROBE_SHOT=<dir>` saves each settled state.
//! `cargo run -p kontra-ui --profile dsp-dev --example motion_probe`

use std::error::Error;
use std::time::{Duration, Instant};

use kontra_ui::app::{self, App, Format, Library};
use mui::Clipboard;
use mui::host::{Driver, Shared, View, Wheel};
use mui::input::{Button, Buttons, Mods};
use mui::prelude::{El, Input, Point, Ui};
use mui::vello::kurbo::Affine;
use mui::vello::software::Renderer;

const SIZE: (u32, u32) = (1180, 900);
const TICK: Duration = Duration::from_micros(16_667);
/// Ticks watched after an input: 2 s.
const WATCH: usize = 120;

struct NoClipboard;
impl Clipboard for NoClipboard {
    fn get(&mut self) -> Option<String> {
        None
    }
    fn set(&mut self, _: &str) {}
}

struct Editor {
    app: App,
    heartbeat: Option<f64>,
    buttons: Option<Buttons>,
    dirty: bool,
    build_ns: u64,
}

impl View for Editor {
    fn build(&mut self, ui: &mut Ui, _: &Input) -> El {
        let t = Instant::now();
        let r = app::view(ui, &mut self.app);
        if let Some(secs) = self.heartbeat {
            ui.repaint_in(secs);
        }
        let held = ui.pointer().buttons;
        if self.buttons.replace(held) != Some(held) || !r.changed.is_empty() {
            self.dirty = true;
        }
        self.build_ns += t.elapsed().as_nanos() as u64;
        r.el
    }
    fn changed(&mut self) -> bool {
        std::mem::take(&mut self.dirty)
    }
    fn request_resize(&mut self, _: u32, _: u32) -> bool {
        false
    }
}

#[derive(Default, Clone, Copy)]
struct Tick {
    built: bool,
    build: u64,
    layout: u64,
    render: u64,
    px: u64,
    nodes: u64,
}

struct Probe {
    driver: Driver,
    shared: Shared<Editor>,
    cpu: Renderer,
    now: Instant,
    /// Wheel notches still to land, one per tick.
    notches: usize,
}

impl Probe {
    fn tick(&mut self) -> Result<Tick, Box<dyn Error>> {
        self.now += TICK;
        if self.notches > 0 {
            self.notches -= 1;
            self.driver.wheel(Wheel::Lines(0., -3.), Mods::default());
        }
        self.shared.view.build_ns = 0;
        let t = Instant::now();
        let built = self.driver.advance(&mut self.shared, self.now);
        let advance = t.elapsed().as_nanos() as u64;
        if !built {
            return Ok(Tick::default());
        }
        let build = self.shared.view.build_ns;
        let stats = self.shared.ui.layout_stats();
        let t = Instant::now();
        let scene = self.shared.ui.scene().ok_or("no scene")?;
        let rects = self.cpu.render_damage(scene, Affine::IDENTITY)?;
        Ok(Tick {
            built,
            build: build / 1000,
            layout: advance.saturating_sub(build) / 1000,
            render: t.elapsed().as_nanos() as u64 / 1000,
            px: rects
                .iter()
                .map(|r| u64::from(r[2]) * u64::from(r[3]))
                .sum(),
            nodes: (stats.measured_nodes + stats.arranged_nodes) as u64,
        })
    }

    fn watch(&mut self, name: &str, ticks: usize) -> Result<(), Box<dyn Error>> {
        let t: Vec<Tick> = (0..ticks).map(|_| self.tick()).collect::<Result<_, _>>()?;
        let built: Vec<&Tick> = t.iter().filter(|t| t.built).collect();
        let median = |f: fn(&Tick) -> u64| {
            let mut v: Vec<u64> = built.iter().map(|t| f(t)).collect();
            v.sort_unstable();
            v.get(v.len() / 2).copied().unwrap_or(0)
        };
        let last = t.iter().rposition(|t| t.built).map_or(0, |i| i + 1);
        let drew = t.iter().filter(|t| t.px > 0).count();
        let first = t.iter().position(|t| t.px > 0).map_or(0, |i| i + 1);
        let total: u64 = built.iter().map(|t| t.build + t.layout + t.render).sum();
        println!(
            "{{\"interaction\":{name:?},\"built\":{},\"drew\":{drew},\"first_drew_at_tick\":{first},\"settled_at_tick\":{last},\"build_us\":{},\"layout_us\":{},\"render_us\":{},\"px\":{},\"nodes\":{},\"total_us\":{total}}}",
            built.len(),
            median(|t| t.build),
            median(|t| t.layout),
            median(|t| t.render),
            median(|t| t.px),
            median(|t| t.nodes),
        );
        if let Some(dir) = std::env::var_os("PROBE_SHOT") {
            let file = std::path::Path::new(&dir).join(format!("{name}.png"));
            mui_headless::write_png(&file, SIZE.0, SIZE.1, &self.cpu.rgba8())?;
        }
        Ok(())
    }

    fn center(&self, id: &str) -> Result<Point, Box<dyn Error>> {
        let s = self.shared.ui.scene().and_then(|s| s.surface(id));
        let s = s.ok_or_else(|| format!("no {id} on screen"))?;
        Ok(Point::new(
            s.frame.x + s.frame.size.width / 2.,
            s.frame.y + s.frame.size.height / 2.,
        ))
    }

    fn hover(&mut self, name: &str, at: Point) -> Result<(), Box<dyn Error>> {
        self.driver.pointer_moved(at, Mods::default());
        self.watch(name, WATCH)
    }

    /// Press and release on `id`, one tick apart, as a quick click lands.
    fn click(&mut self, name: &str, id: &str) -> Result<(), Box<dyn Error>> {
        let at = self.center(id)?;
        self.driver.pointer_moved(at, Mods::default());
        self.tick()?;
        self.driver.button(Button::Primary, true, Mods::default());
        self.tick()?;
        self.driver.button(Button::Primary, false, Mods::default());
        self.watch(name, WATCH)
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    let libraries = (0..40)
        .map(|n| Library {
            name: format!("Library {n}"),
            format: if n % 5 == 0 {
                Format::Uvi
            } else {
                Format::Kontakt
            },
            presets: vec!["A".into(), "B".into()],
            ..Library::default()
        })
        .collect();
    let heartbeat = std::env::var("PROBE_HEARTBEAT")
        .ok()
        .and_then(|s| s.parse().ok());
    let mut p = Probe {
        driver: Driver::new(SIZE, 1.0, Box::new(NoClipboard)),
        shared: Shared {
            ui: kontra_ui::ui(),
            view: Editor {
                app: App {
                    libraries,
                    ..App::default()
                },
                heartbeat,
                buttons: None,
                dirty: false,
                build_ns: 0,
            },
        },
        cpu: Renderer::new(SIZE)?,
        now: Instant::now(),
        notches: 0,
    };
    let away = Point::new(f64::from(SIZE.0) * 0.6, f64::from(SIZE.1) * 0.5);
    p.driver.pointer_moved(away, Mods::default());
    p.watch("open", WATCH)?;
    let tile = p.center("library-0")?;
    p.hover("hover-tile", tile)?;
    let next = p.center("library-1")?;
    p.hover("hover-next-tile", next)?;
    p.hover("unhover-tile", away)?;
    p.click("menu-open", "app-menu")?;
    p.click("menu-close", "app-menu")?;
    p.hover("unhover-menu", away)?;
    let list = p.center("library-list")?;
    p.hover("enter-grid", list)?;
    p.notches = 10;
    p.watch("scroll-10-notches", WATCH)?;
    p.hover("leave-grid", away)?;
    p.watch("idle-10s", WATCH * 5)?;
    Ok(())
}
