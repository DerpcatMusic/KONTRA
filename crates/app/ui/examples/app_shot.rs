//! The editor drawn headless on a specimen, state by state, clicked with
//! the pointer as a user would. Saves `<out>/<state>-<width>.png` and prints
//! one JSON line per state: what a frame costs when one is due, and how
//! many frames its motion runs before the editor is idle again (and stops
//! drawing). `cargo run --release -p kontra-ui --example app_shot -- <out>`

use std::error::Error;
use std::path::{Path, PathBuf};
use std::time::Instant;

use kontra_ui::app::{
    self, Action, App, Envelope, Filter, Format, Library, Part, PartTab, Pass, Sound, Stats, Zone,
};
use mui::prelude::{Buttons, Input, Point, PointerInput, Size, Ui};
use mui::vello;

struct Shot {
    ui: Ui,
    app: App,
    size: (u16, u16),
    out: PathBuf,
    actions: Vec<Action>,
}

impl Shot {
    /// One frame; its cost in µs and whether motion is still running.
    fn frame(&mut self, input: Input) -> Result<(u128, bool), Box<dyn Error>> {
        let t = Instant::now();
        let r = app::view(&mut self.ui, &mut self.app);
        self.actions.extend(r.changed);
        let size = Size::new(f64::from(self.size.0), f64::from(self.size.1));
        let f = self
            .ui
            .frame(r.el, Some(size), input, 1. / 60.)
            .map_err(|e| format!("{e:?}"))?;
        Ok((t.elapsed().as_micros(), f.animating))
    }

    /// Frames until nothing moves: the ones an action costs after it.
    fn settle(&mut self) -> Result<usize, Box<dyn Error>> {
        for n in 0..240 {
            if !self.frame(Input::default())?.1 {
                return Ok(n);
            }
        }
        Ok(240)
    }

    fn click(&mut self, id: &str) -> Result<(), Box<dyn Error>> {
        let at = self
            .ui
            .scene()
            .and_then(|s| s.surface(id))
            .map(|s| {
                Point::new(
                    s.frame.x + s.frame.size.width / 2.,
                    s.frame.y + s.frame.size.height / 2.,
                )
            })
            .ok_or_else(|| format!("no {id} on screen"))?;
        for buttons in [Buttons::PRIMARY, Buttons::default()] {
            let pointer = PointerInput {
                pos: Some(at),
                buttons,
                ..Default::default()
            };
            self.frame(Input {
                pointer,
                ..Default::default()
            })?;
        }
        self.settle()?;
        Ok(())
    }

    fn save(&mut self, state: &str) -> Result<(), Box<dyn Error>> {
        let settle = self.settle()?;
        let mut costs: Vec<u128> = (0..20)
            .map(|_| self.frame(Input::default()).map(|f| f.0))
            .collect::<Result<_, _>>()?;
        costs.sort_unstable();
        let (w, h) = self.size;
        let file = self.out.join(format!("{state}-{w}.png"));
        save(&file, w, h, &paint(&self.ui, w, h)?)?;
        let frame_us = costs.get(costs.len() / 2).copied().unwrap_or(0);
        println!(
            "{{\"state\":{state:?},\"width\":{w},\"frame_us\":{frame_us},\"settle_frames\":{settle},\"shot\":{file:?}}}"
        );
        Ok(())
    }
}

fn specimen() -> App {
    let library = |name: &str, format, presets: &[&str], starred| Library {
        name: name.into(),
        format,
        presets: presets.iter().map(|p| (*p).to_owned()).collect(),
        starred,
        art: Vec::new(),
    };
    App {
        libraries: vec![
            library("Afflatus Chapter II Brass", Format::Kontakt, &["Instruments/2 Horns KS", "Instruments/4 Horns KS", "Instruments/Solo Trumpet KS", "Presets/Warm/Tubas Slides"], true),
            library("ANALOG STRINGS", Format::Kontakt, &["ANALOG STRINGS"], false),
            library("Areia", Format::Kontakt, &["01 Areia - 16 Violins - Core Techniques", "02 Areia - 12 Violas"], false),
            library("Conflux", Format::Kontakt, &["Conflux"], true),
            library("Una Corda", Format::Kontakt, &["Una Corda Cotton", "Una Corda Felt"], false),
            library("Falcon Factory", Format::Uvi, &["Analog Pad", "Glass Keys"], false),
        ],
        recent: vec![(0, 0), (2, 0), (4, 0)],
        stats: Stats { cpu: 3., voices: 12, ram_mb: 412, samples_mb: 180, disk_mbs: 1.4, cache_mb: 128 },
        level: 0.4,
        report: vec![
            "Areia 16 Violins: the convolution reverb's impulse is not in the library\nThe library names Hall.wav in its reverb, and no file of that name ships with it; the dry signal plays.".into(),
            "2 Horns KS: script line 912 asks for an unknown effect's display\nget_engine_par_disp on slot 3 names no effect; Kontakt answers an empty text and so does KONTRA.".into(),
        ],
        folders: vec!["/data/main/Libraries/Kontakt".into(), "/data/main/Libraries/UVI".into()],
        ..App::default()
    }
}

fn parts() -> Vec<Part> {
    let arts = [
        "Sustain · close and tree microphones, long release",
        "Staccato · short, all microphones",
        "Staccatissimo",
        "Marcato · accented attack with a longer tail",
        "Sforzando",
        "Swells · slow and fast, two layers",
        "Trills · half and whole step",
        "Rips",
        "Falls",
    ];
    vec![
        Part {
            name: "2 Horns KS".into(),
            library: "Afflatus Chapter II Brass".into(),
            channel: 1,
            output: 1,
            articulations: arts
                .iter()
                .zip(24..)
                .map(|(a, k)| ((*a).to_owned(), Some(k)))
                .collect(),
            keys: (36, 84),
            needs: vec!["A convolution impulse, Hall.wav, the library does not ship.".into()],
            info: vec![
                ("File".into(), "/data/main/Libraries/Kontakt/Afflatus Chapter II Brass/Instruments/2 Horns KS.nki".into()),
                ("Format".into(), "Kontakt 6 instrument".into()),
                ("Contents".into(), "1,204 zones · 38 groups · 1,204 samples · 2 buses".into()),
                ("Scripts".into(), "3 scripts · 212 controls · 9 articulations".into()),
            ],
            scripts: vec!["Afflatus Engine".into(), "Keyswitches".into(), "Legato".into()],
            groups: arts.iter().map(|a| (*a).to_owned()).take(4).collect(),
            zones: (0..4usize)
                .flat_map(|g| {
                    // Two velocity layers per group, each over three key splits.
                    (0..6u8).map(move |z| Zone {
                        group: g,
                        sample: format!("Horns_{g}_{}_{}.ncw", if z < 3 { "pp" } else { "ff" }, z % 3),
                        keys: (36 + 16 * (z % 3), 51 + 16 * (z % 3)),
                        velocities: if z < 3 { (1, 80) } else { (81, 127) },
                        root: 43 + 16 * (z % 3),
                        tune: f64::from(z) * 3. - 6.,
                        volume: -1.5,
                        ..Zone::default()
                    })
                })
                .collect(),
            sounds: (0..4u8)
                .map(|g| Sound {
                    envelope: Some(Envelope {
                        attack: 0.02 + 0.03 * f64::from(g),
                        hold: 0.,
                        decay: 0.6,
                        sustain: 0.7,
                        release: 0.45,
                    }),
                    filters: vec![
                        Filter { pass: Pass::Low, hz: 6000., q: 1.4, poles: 4, ..Filter::default() },
                        Filter { pass: Pass::Peak, hz: 400., q: 1., gain_db: -4., ..Filter::default() },
                        Filter { pass: Pass::HighShelf, hz: 9000., q: 0.7, gain_db: 3., ..Filter::default() },
                    ],
                    mods: vec![
                        ("Velocity".into(), "Volume".into(), "-12 dB".into()),
                        ("CC 1".into(), "Cutoff".into(), "+3 oct".into()),
                    ],
                    effects: vec!["Filter".into(), "EQ".into()],
                })
                .collect(),
            open: true,
            tab: PartTab::Articulations,
            level: vec![[0.3; 2]],
            ..Part::default()
        },
        Part {
            name: "01 Areia - 16 Violins - Core Techniques".into(),
            library: "Areia".into(),
            channel: 2,
            output: 2,
            volume: -3.5,
            pan: -0.3,
            keys: (55, 100),
            ..Part::default()
        },
        Part {
            name: "Una Corda Cotton".into(),
            library: "Una Corda".into(),
            output: 3,
            keys: (21, 108),
            solo: true,
            ..Part::default()
        },
    ]
}

fn main() -> Result<(), Box<dyn Error>> {
    let out = PathBuf::from(
        std::env::args_os()
            .nth(1)
            .ok_or("usage: app_shot <out dir>")?,
    );
    std::fs::create_dir_all(&out)?;
    for size in [(900, 600), (1180, 900)] {
        let mut s = Shot {
            ui: kontra_ui::ui(),
            app: specimen(),
            size,
            out: out.clone(),
            actions: Vec::new(),
        };
        let t = Instant::now();
        s.frame(Input::default())?;
        println!(
            "{{\"state\":\"first\",\"width\":{},\"first_frame_us\":{}}}",
            size.0,
            t.elapsed().as_micros()
        );
        s.save("empty")?;
        s.click("library-0")?;
        s.save("presets")?;
        s.click("preset-0")?;
        s.app.parts = parts();
        s.save("rack")?;
        for menu in [
            "part-0-channel",
            "part-0-output",
            "part-0-velocity",
            "part-0-needs",
            "part-0-more",
        ] {
            s.click(menu)?;
            s.save(menu.trim_start_matches("part-0-"))?;
            s.click(menu)?;
        }
        for tab in ["Info", "Scripts", "Mapping"] {
            s.click(&format!("part-0-tab-{tab}"))?;
            s.save(&tab.to_lowercase())?;
        }
        s.click("part-0-grid")?;
        s.save("mapping-picked")?;
        s.click("part-0-tab-Sound")?;
        s.save("sound")?;
        s.click("part-0-sound-Modulation")?;
        s.save("sound-modulation")?;
        s.click("part-0-tab-Articulations")?;
        s.click("tab-Mixer")?;
        s.save("mixer")?;
        s.click("tab-Report")?;
        s.click("report-0")?;
        s.save("report")?;
        s.click("tab-Rack")?;
        s.click("app-menu")?;
        s.save("app-menu")?;
        s.click("app-menu-0")?;
        s.save("settings")?;
        s.click("sheet-close")?;
        s.click("browser")?;
        s.save("browser-hidden")?;
        eprintln!("actions: {:?}", s.actions);
    }
    Ok(())
}

fn paint(ui: &Ui, w: u16, h: u16) -> Result<Vec<u8>, Box<dyn Error>> {
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

fn save(file: &Path, w: u16, h: u16, rgba: &[u8]) -> Result<(), Box<dyn Error>> {
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
